//! Real-file probe for complete typed snapshots, explicit scalar patches, and
//! structural parameter/graphic batches.
//! Writes only temporary documents. This is not a user-facing saved-plan command.
use altium_format::SchLib;
use altium_format::schlib::{
    SchChildBatch, SchGraphicEdit, SchGraphicInit, SchGraphicShape, SchLibSnapshot, SchOwnerGroup,
    SchParameterEdit, SchParameterInit,
};
use altium_format_types::{Color, ComponentKind, Coord, CoordPoint, LineStyle, PenWidth};
use altium_spec_lang::{
    diagnostic::{Span, Spanned},
    intent::FieldIntent,
};
use altium_sync::schlib::{ComponentPropertiesIntent, SchLibPatch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut checked = 0;
    let mut failures = 0;
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let result = check(&path).map_err(|error| {
            std::io::Error::other(format!("checking snapshot '{}': {error}", path.display()))
        })?;
        if result["native_typed_noop_equal"] != true
            || result["native_typed_patch_equal"] != true
            || result["replan_empty"] != true
            || result["structural_insert_reorder_equal"] != true
            || result["structural_projection_equal"] != true
            || result["structural_graphic_reorder_equal"] != true
            || result["structural_removal_equal"] != true
        {
            failures += 1;
        }
        println!("{}", serde_json::to_string(&result)?);
        checked += 1;
    }
    if checked == 0 {
        return Err("usage: check_schlib_snapshots <library.SchLib> ...".into());
    }
    if failures != 0 {
        return Err(format!(
            "{failures} libraries changed complete typed state on native serialization"
        )
        .into());
    }
    Ok(())
}

fn check(path: &std::path::Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut doc = SchLib::open(path)?;
    let input = doc.snapshot()?;
    let materialized = input.materialize()?;
    if materialized.snapshot()? != input {
        return Err("typed materialization changed captured state".into());
    }
    let temp = tempfile::tempdir()?;
    let noop_path = temp.path().join("noop.SchLib");
    materialized.save(&noop_path)?;
    let noop_equal = SchLib::open(&noop_path)?.snapshot()? == input;

    let mut fields = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        fields.push((
            index,
            ComponentPropertiesIntent {
                description: authored(Some(format!("snapshot validation component {index}"))),
                designator: authored(Some(format!("X{index}?"))),
                component_kind: authored(if index % 2 == 0 {
                    ComponentKind::Jumper
                } else {
                    ComponentKind::StandardNoBom
                }),
                show_hidden_pins: authored(!component.show_hidden_pins()),
            },
        ));
    }
    let patch = SchLibPatch::component_properties(&input, fields.clone())?;
    let expected = patch.resulting_snapshot()?;
    patch.apply_in_memory(&mut doc)?;
    if doc.snapshot()? != expected {
        return Err("in-memory patch differs from its reviewed candidate".into());
    }
    let edited_path = temp.path().join("edited.SchLib");
    doc.save(&edited_path)?;
    let reopened = SchLib::open(&edited_path)?.snapshot()?;
    let replan_empty = SchLibPatch::component_properties(&reopened, fields)?.is_empty();

    // Structural pass: parameter/graphic insert, reorder, replace, and a
    // second authorized-removal batch, all through the public snapshot API,
    // starting from a fresh open of the unmodified input file.
    let structural = structural_pass(SchLib::open(path)?.snapshot()?, temp.path())?;

    Ok(serde_json::json!({
        "file": path,
        "components": input.component_count(),
        "replacements": patch.replacements().len(),
        "typed_materialize_equal": true,
        "in_memory_patch_equal": true,
        "native_typed_noop_equal": noop_equal,
        "native_typed_patch_equal": reopened == expected,
        "replan_empty": replan_empty,
        "properties": ["description", "designator", "component_kind", "show_hidden_pins"],
        "structural_replaced_colors": structural.replaced_colors,
        "structural_insert_reorder_equal": structural.insert_reorder_equal,
        "structural_projection_equal": structural.projection_equal,
        "structural_graphic_reorder_equal": structural.graphic_reorder_equal,
        "structural_removal_equal": structural.removal_equal,
    }))
}

struct StructuralOutcome {
    replaced_colors: usize,
    insert_reorder_equal: bool,
    projection_equal: bool,
    graphic_reorder_equal: bool,
    removal_equal: bool,
}

const PROBE_COLOR: Color = Color::new(0x0031_B257);

fn probe_rectangle() -> SchGraphicInit {
    SchGraphicInit {
        owner_part_id: 1,
        owner_part_display_mode: None,
        shape: SchGraphicShape::Rectangle {
            from: CoordPoint::zero(),
            to: CoordPoint {
                x: Coord::from_mils(120).expect("120 mils fits Coord"),
                y: Coord::from_mils(60).expect("60 mils fits Coord"),
            },
            line_width: PenWidth::Small,
            line_style: LineStyle::Solid,
            color: PROBE_COLOR,
            area_color: PROBE_COLOR,
            is_solid: false,
            transparent: false,
        },
    }
}

fn set_shape_color(shape: &mut SchGraphicShape, value: Color) {
    match shape {
        SchGraphicShape::Line { color, .. }
        | SchGraphicShape::Rectangle { color, .. }
        | SchGraphicShape::RoundRectangle { color, .. }
        | SchGraphicShape::Arc { color, .. }
        | SchGraphicShape::EllipticalArc { color, .. }
        | SchGraphicShape::Ellipse { color, .. }
        | SchGraphicShape::Pie { color, .. }
        | SchGraphicShape::Polyline { color, .. }
        | SchGraphicShape::Polygon { color, .. }
        | SchGraphicShape::Bezier { color, .. }
        | SchGraphicShape::Image { color, .. }
        | SchGraphicShape::Label { color, .. }
        | SchGraphicShape::TextFrame { color, .. } => *color = value,
    }
}

fn structural_pass(
    input: SchLibSnapshot,
    temp: &std::path::Path,
) -> Result<StructuralOutcome, Box<dyn std::error::Error>> {
    let mut replaced_colors = 0usize;
    let mut replacements = Vec::new();
    let mut expected_projections = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        let parameters = component.parameters()?;
        let graphics = component.graphics();
        let probe_name = format!("STEP2B_{index}");

        // Reverse the parameters, then append the probe parameter.
        let mut parameter_edits: Vec<SchParameterEdit> = (0..parameters.len())
            .rev()
            .map(SchParameterEdit::Keep)
            .collect();
        parameter_edits.push(SchParameterEdit::New(SchParameterInit {
            name: probe_name.clone(),
            text: "step2 structural probe".to_owned(),
            is_hidden: true,
        }));
        let mut expected_parameters: Vec<(String, String, bool)> = parameters
            .iter()
            .rev()
            .map(|view| (view.name.to_owned(), view.text.to_owned(), view.is_hidden))
            .collect();
        expected_parameters.push((
            probe_name.clone(),
            "step2 structural probe".to_owned(),
            true,
        ));

        // Keep every graphic in place; recolor the first replaceable one
        // (remembering the submitted init and its index for a non-vacuous
        // post-reopen check); append the probe rectangle.
        let mut graphic_edits: Vec<SchGraphicEdit> = Vec::with_capacity(graphics.len() + 1);
        let mut recolored: Option<(usize, SchGraphicInit)> = None;
        for (graphic_index, view) in graphics.iter().enumerate() {
            if recolored.is_none() {
                if let Ok(mut init) = SchGraphicInit::from_view(view) {
                    set_shape_color(&mut init.shape, PROBE_COLOR);
                    graphic_edits.push(SchGraphicEdit::Replace(graphic_index, init.clone()));
                    recolored = Some((graphic_index, init));
                    continue;
                }
            }
            graphic_edits.push(SchGraphicEdit::Keep(graphic_index));
        }
        if recolored.is_some() {
            replaced_colors += 1;
        }
        graphic_edits.push(SchGraphicEdit::New(probe_rectangle()));
        let mut expected_graphic_kinds: Vec<&'static str> = graphics
            .iter()
            .map(|view| view.detail.kind_name())
            .collect();
        expected_graphic_kinds.push("Rectangle");

        let edited = component.with_children(SchChildBatch {
            parameters: Some(parameter_edits),
            graphics: Some(graphic_edits),
            allow_deletion: false,
        })?;
        expected_projections.push((expected_parameters, expected_graphic_kinds, recolored));
        replacements.push((index, edited));
    }

    let expected = input.replace_components(replacements)?;
    let inserted_path = temp.join("structural-inserted.SchLib");
    expected.materialize()?.save(&inserted_path)?;
    let reopened = SchLib::open(&inserted_path)?.snapshot()?;
    let insert_reorder_equal = reopened == expected;

    // Projection equality: the reopened views must equal the requested final
    // sequences exactly, and the recolored Replace must have landed at its
    // exact index (compared init-for-init, never satisfiable by the appended
    // probe rectangle).
    let mut projection_equal = true;
    for (index, (expected_parameters, expected_graphic_kinds, recolored)) in
        expected_projections.iter().enumerate()
    {
        let component = reopened.component(index)?;
        let parameters: Vec<(String, String, bool)> = component
            .parameters()?
            .iter()
            .map(|view| (view.name.to_owned(), view.text.to_owned(), view.is_hidden))
            .collect();
        let graphics = component.graphics();
        let kinds: Vec<&'static str> = graphics
            .iter()
            .map(|view| view.detail.kind_name())
            .collect();
        let probe = graphics.last().expect("appended probe rectangle");
        projection_equal &= parameters == *expected_parameters
            && kinds == *expected_graphic_kinds
            && probe.owner_group == SchOwnerGroup::Part(1)
            && probe.owner_part_display_mode == 0
            && probe.unique_id.is_some_and(|id| !id.is_empty());
        if let Some((replaced_index, submitted)) = recolored {
            projection_equal &= SchGraphicInit::from_view(&graphics[*replaced_index])
                .is_ok_and(|reopened_init| reopened_init == *submitted);
        }
    }

    // Second batch: reorder the graphics of every component by swapping the
    // probe rectangle (last member) with the last original member — the two
    // occupy adjacent stream slots, so the swap is safe on any ownership
    // graph — and verify the swapped projection after save/reopen.
    let mut reorders = Vec::new();
    let mut expected_swapped_kinds = Vec::new();
    for index in 0..reopened.component_count() {
        let component = reopened.component(index)?;
        let graphics = component.graphics();
        let count = graphics.len();
        let mut order: Vec<usize> = (0..count).collect();
        if count >= 2 {
            order.swap(count - 2, count - 1);
        }
        let kinds: Vec<&'static str> = order
            .iter()
            .map(|&i| graphics[i].detail.kind_name())
            .collect();
        expected_swapped_kinds.push(kinds);
        let swapped = component.with_children(SchChildBatch {
            parameters: None,
            graphics: Some(order.into_iter().map(SchGraphicEdit::Keep).collect()),
            allow_deletion: false,
        })?;
        reorders.push((index, swapped));
    }
    let expected_reordered = reopened.replace_components(reorders)?;
    let reordered_path = temp.join("structural-reordered.SchLib");
    expected_reordered.materialize()?.save(&reordered_path)?;
    let reopened_reordered = SchLib::open(&reordered_path)?.snapshot()?;
    let mut graphic_reorder_equal = reopened_reordered == expected_reordered;
    for (index, expected_kinds) in expected_swapped_kinds.iter().enumerate() {
        let component = reopened_reordered.component(index)?;
        let kinds: Vec<&'static str> = component
            .graphics()
            .iter()
            .map(|view| view.detail.kind_name())
            .collect();
        graphic_reorder_equal &= kinds == *expected_kinds;
    }

    // Third batch: remove the probe parameter AND the probe rectangle again,
    // with authorization, restoring the original graphics kind sequences.
    let mut removals = Vec::new();
    for index in 0..reopened_reordered.component_count() {
        let component = reopened_reordered.component(index)?;
        let probe_name = format!("STEP2B_{index}");
        let parameters = component.parameters()?;
        let keeps: Vec<SchParameterEdit> = parameters
            .iter()
            .enumerate()
            .filter(|(_, view)| view.name != probe_name)
            .map(|(parameter_index, _)| SchParameterEdit::Keep(parameter_index))
            .collect();
        if keeps.len() + 1 != parameters.len() {
            return Err(format!(
                "component {index}: probe parameter '{probe_name}' not found exactly once"
            )
            .into());
        }
        // After the swap the probe rectangle sits at index count-2 (or 0 for a
        // component that had no original graphics); verify before deleting.
        let graphics = component.graphics();
        let count = graphics.len();
        let probe_index = if count >= 2 { count - 2 } else { 0 };
        let probe_init = SchGraphicInit::from_view(&graphics[probe_index])
            .map_err(|error| format!("component {index}: probe rectangle view: {error}"))?;
        if probe_init.shape != probe_rectangle().shape || probe_init.owner_part_id != 1 {
            return Err(format!(
                "component {index}: graphics()[{probe_index}] is not the probe rectangle"
            )
            .into());
        }
        let graphic_keeps: Vec<SchGraphicEdit> = (0..count)
            .filter(|i| *i != probe_index)
            .map(SchGraphicEdit::Keep)
            .collect();
        let removed = component.with_children(SchChildBatch {
            parameters: Some(keeps),
            graphics: Some(graphic_keeps),
            allow_deletion: true,
        })?;
        removals.push((index, removed));
    }
    let expected_removed = reopened_reordered.replace_components(removals)?;
    let removed_path = temp.join("structural-removed.SchLib");
    expected_removed.materialize()?.save(&removed_path)?;
    let reopened_removed = SchLib::open(&removed_path)?.snapshot()?;
    let mut removal_equal = reopened_removed == expected_removed;
    for index in 0..reopened_removed.component_count() {
        let component = reopened_removed.component(index)?;
        removal_equal &= component
            .parameters()?
            .iter()
            .all(|view| !view.name.starts_with("STEP2B_"));
        // Removing the probe undoes the swap: the kind sequence must equal
        // the original input's (expected kinds minus the appended probe).
        let (_, expected_graphic_kinds, _) = &expected_projections[index];
        let original_kinds = &expected_graphic_kinds[..expected_graphic_kinds.len() - 1];
        let kinds: Vec<&'static str> = component
            .graphics()
            .iter()
            .map(|view| view.detail.kind_name())
            .collect();
        removal_equal &= kinds == original_kinds;
    }

    Ok(StructuralOutcome {
        replaced_colors,
        insert_reorder_equal,
        projection_equal,
        graphic_reorder_equal,
        removal_equal,
    })
}

fn authored<T>(value: T) -> FieldIntent<T> {
    FieldIntent::Set(Spanned::new(value, Span::new(0, 0)))
}
