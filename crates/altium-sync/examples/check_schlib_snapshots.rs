//! Real-file probe for complete typed snapshots, explicit scalar patches,
//! structural parameter/graphic/pin batches, part grow/shrink round-trips,
//! and library-level namespace transitions (aliases + component clone/remove).
//! Writes only temporary documents. This is not a user-facing saved-plan command.
use altium_format::SchLib;
use altium_format::schlib::{
    SchAliasOp, SchChildBatch, SchFootprintEdit, SchFootprintInit, SchGraphicEdit, SchGraphicInit,
    SchGraphicShape, SchLibComponentEdit, SchLibComponentSnapshot, SchLibEdit, SchLibSnapshot,
    SchNewComponent, SchOwnerGroup, SchParameterEdit, SchParameterInit, SchPinEdit, SchPinInit,
    SchPinPadMapInit,
};
use altium_format_types::{
    Color, ComponentKind, Coord, CoordPoint, LineStyle, PenWidth, PinElectricalType, RotationBy90,
};
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
            || result["structural_pin_insert_equal"] != true
            || result["structural_pin_reorder_equal"] != true
            || result["structural_pin_removal_equal"] != true
            || result["structural_pin_projection_equal"] != true
            || result["structural_all_pin_count_verbatim"] != true
            || result["structural_part_growth_equal"] != true
            || result["structural_part_shrink_equal"] != true
            || result["structural_part_report_correct"] != true
            || result["structural_part_restore_equal"] != true
            || result["structural_footprint_replace_equal"] != true
            || result["structural_footprint_probe_equal"] != true
            || result["structural_footprint_removal_equal"] != true
            || result["structural_footprint_projection_equal"] != true
            || result["structural_footprint_non_pcblib_invariant"] != true
            || result["namespace_alias_add_equal"] != true
            || result["namespace_alias_remove_equal"] != true
            || result["namespace_clone_equal"] != true
            || result["namespace_clone_projection_equal"] != true
            || result["namespace_restore_equal"] != true
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

    // Pin structural pass (increment C): per component, insert a probe pin
    // into part 1, reorder it, then remove it with authorization, asserting
    // pins() projection equality, native save/reopen typed equality, and a
    // verbatim stored AllPinCount at every stage.
    let pins = pin_structural_pass(SchLib::open(path)?.snapshot()?, temp.path())?;

    // Part structural pass (increment C): per component, append one empty
    // trailing part slot, save/reopen, then remove it again (empty expected
    // deletion list, memo-rule current_part_id adjustment asserted through
    // the typed report), save/reopen, restoring the original typed state.
    let parts = part_structural_pass(SchLib::open(path)?.snapshot()?, temp.path())?;

    // Namespace pass (increment D1): alias add/remove on component 0, then a
    // clone of component 0 under a >31-char name (SectionKeys storage path)
    // with projection equality, then authorized removal restoring the
    // original snapshot state exactly.
    let namespace = namespace_pass(SchLib::open(path)?.snapshot()?, temp.path())?;

    // Footprint structural pass (increment D2): per component, replace the
    // first footprint's description, add a probe footprint (with one pin-pad
    // map from an existing pin when possible), then remove it and restore the
    // description, asserting footprint_maps() projection equality, native
    // save/reopen typed equality, and the non-PCBLIB implementation count
    // invariant at every stage.
    let footprints = footprint_structural_pass(SchLib::open(path)?.snapshot()?, temp.path())?;

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
        "structural_pin_insert_equal": pins.insert_equal,
        "structural_pin_reorder_equal": pins.reorder_equal,
        "structural_pin_removal_equal": pins.removal_equal,
        "structural_pin_projection_equal": pins.projection_equal,
        "structural_all_pin_count_verbatim": pins.all_pin_count_verbatim,
        "structural_part_growth_equal": parts.growth_equal,
        "structural_part_shrink_equal": parts.shrink_equal,
        "structural_part_report_correct": parts.report_correct,
        "structural_part_restore_equal": parts.restore_equal,
        "structural_footprint_replace_equal": footprints.replace_equal,
        "structural_footprint_probe_equal": footprints.probe_equal,
        "structural_footprint_removal_equal": footprints.removal_equal,
        "structural_footprint_projection_equal": footprints.projection_equal,
        "structural_footprint_non_pcblib_invariant": footprints.non_pcblib_invariant,
        "structural_footprints_replaced": footprints.replaced_descriptions,
        "namespace_alias_add_equal": namespace.alias_add_equal,
        "namespace_alias_remove_equal": namespace.alias_remove_equal,
        "namespace_clone_equal": namespace.clone_equal,
        "namespace_clone_projection_equal": namespace.clone_projection_equal,
        "namespace_restore_equal": namespace.restore_equal,
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
            pins: None,
            footprints: None,
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
            pins: None,
            footprints: None,
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
            pins: None,
            footprints: None,
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

struct PinStructuralOutcome {
    insert_equal: bool,
    reorder_equal: bool,
    removal_equal: bool,
    projection_equal: bool,
    all_pin_count_verbatim: bool,
}

fn probe_pin(index: usize) -> SchPinInit {
    SchPinInit {
        owner_part_id: 1,
        owner_part_display_mode: Some(0),
        designator: format!("STEP2C_{index}"),
        name: "STEP2C".to_owned(),
        electrical: PinElectricalType::Passive,
        length: Coord::from_mils(300).expect("300 mils fits Coord"),
        location: CoordPoint::zero(),
        orientation: RotationBy90::Rotate0,
        is_hidden: false,
        hidden_net_name: String::new(),
        swap_group: String::new(),
        part_swap_group: Some(String::new()),
        part_pin_swap_suffix: Some(String::new()),
        pair_swap_group: String::new(),
    }
}

/// Complete owned fingerprint of one projected pin view (every `SchPinView`
/// field, via its Debug representation).
fn pin_fingerprints(component: &SchLibComponentSnapshot) -> Vec<String> {
    component
        .pins()
        .iter()
        .map(|view| format!("{view:?}"))
        .collect()
}

fn pin_structural_pass(
    input: SchLibSnapshot,
    temp: &std::path::Path,
) -> Result<PinStructuralOutcome, Box<dyn std::error::Error>> {
    let mut all_pin_count_verbatim = true;
    let mut projection_equal = true;

    // Baselines from the immutable input.
    let mut original_fingerprints = Vec::new();
    let mut stored_counts = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        original_fingerprints.push(pin_fingerprints(&component));
        stored_counts.push(component.stored_all_pin_count());
    }

    // Stage 1: append the probe pin to part 1 of every component.
    let mut inserts = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        let count = component.pins().len();
        let edits: Vec<SchPinEdit> = (0..count)
            .map(SchPinEdit::Keep)
            .chain([SchPinEdit::New(probe_pin(index))])
            .collect();
        let edited = component.with_children(SchChildBatch {
            parameters: None,
            graphics: None,
            pins: Some(edits),
            footprints: None,
            allow_deletion: false,
        })?;
        inserts.push((index, edited));
    }
    let expected_inserted = input.replace_components(inserts)?;
    let inserted_path = temp.join("structural-pin-inserted.SchLib");
    expected_inserted.materialize()?.save(&inserted_path)?;
    let reopened_inserted = SchLib::open(&inserted_path)?.snapshot()?;
    let insert_equal = reopened_inserted == expected_inserted;
    for index in 0..reopened_inserted.component_count() {
        let component = reopened_inserted.component(index)?;
        let fingerprints = pin_fingerprints(&component);
        let (untouched, probe) = fingerprints.split_at(original_fingerprints[index].len());
        projection_equal &= untouched == original_fingerprints[index].as_slice();
        // The reopened probe must round-trip the submitted init exactly.
        let probe_ok = probe.len() == 1
            && component
                .pins()
                .last()
                .map(SchPinInit::from_view)
                .is_some_and(|init| init.is_ok_and(|init| init == probe_pin(index)));
        projection_equal &= probe_ok;
        all_pin_count_verbatim &= component.stored_all_pin_count() == stored_counts[index];
    }

    // Stage 2: reorder by swapping the probe (last member) with the last
    // original pin — physically adjacent stream slots, so the swap is safe on
    // any ownership graph. Components that had no pins keep their order.
    let mut reorders = Vec::new();
    let mut expected_swapped: Vec<Vec<String>> = Vec::new();
    for index in 0..reopened_inserted.component_count() {
        let component = reopened_inserted.component(index)?;
        let fingerprints = pin_fingerprints(&component);
        let count = fingerprints.len();
        let mut order: Vec<usize> = (0..count).collect();
        if count >= 2 {
            order.swap(count - 2, count - 1);
        }
        expected_swapped.push(order.iter().map(|&i| fingerprints[i].clone()).collect());
        let swapped = component.with_children(SchChildBatch {
            parameters: None,
            graphics: None,
            pins: Some(order.into_iter().map(SchPinEdit::Keep).collect()),
            footprints: None,
            allow_deletion: false,
        })?;
        reorders.push((index, swapped));
    }
    let expected_reordered = reopened_inserted.replace_components(reorders)?;
    let reordered_path = temp.join("structural-pin-reordered.SchLib");
    expected_reordered.materialize()?.save(&reordered_path)?;
    let reopened_reordered = SchLib::open(&reordered_path)?.snapshot()?;
    let reorder_equal = reopened_reordered == expected_reordered;
    for (index, expected) in expected_swapped.iter().enumerate() {
        let component = reopened_reordered.component(index)?;
        projection_equal &= pin_fingerprints(&component) == *expected;
        all_pin_count_verbatim &= component.stored_all_pin_count() == stored_counts[index];
    }

    // Stage 3: remove the probe with authorization, restoring the original
    // pins() projection of every component.
    let mut removals = Vec::new();
    for index in 0..reopened_reordered.component_count() {
        let component = reopened_reordered.component(index)?;
        let probe_designator = format!("STEP2C_{index}");
        let pins = component.pins();
        // After the swap the probe sits at count-2 (or 0 for a component that
        // had no original pins); verify before deleting.
        let count = pins.len();
        let probe_index = if count >= 2 { count - 2 } else { 0 };
        if pins[probe_index].designator != probe_designator {
            return Err(format!(
                "component {index}: pins()[{probe_index}] is not probe '{probe_designator}'"
            )
            .into());
        }
        let keeps: Vec<SchPinEdit> = (0..count)
            .filter(|i| *i != probe_index)
            .map(SchPinEdit::Keep)
            .collect();
        let removed = component.with_children(SchChildBatch {
            parameters: None,
            graphics: None,
            pins: Some(keeps),
            footprints: None,
            allow_deletion: true,
        })?;
        removals.push((index, removed));
    }
    let expected_removed = reopened_reordered.replace_components(removals)?;
    let removed_path = temp.join("structural-pin-removed.SchLib");
    expected_removed.materialize()?.save(&removed_path)?;
    let reopened_removed = SchLib::open(&removed_path)?.snapshot()?;
    let removal_equal = reopened_removed == expected_removed;
    for index in 0..reopened_removed.component_count() {
        let component = reopened_removed.component(index)?;
        // Removing the probe undoes the swap: the projection must equal the
        // original input's exactly.
        projection_equal &= pin_fingerprints(&component) == original_fingerprints[index];
        all_pin_count_verbatim &= component.stored_all_pin_count() == stored_counts[index];
    }

    Ok(PinStructuralOutcome {
        insert_equal,
        reorder_equal,
        removal_equal,
        projection_equal,
        all_pin_count_verbatim,
    })
}

struct PartStructuralOutcome {
    growth_equal: bool,
    shrink_equal: bool,
    report_correct: bool,
    restore_equal: bool,
}

/// Kind-name sequence of the graphics projection (order-sensitive).
fn graphic_kinds(component: &SchLibComponentSnapshot) -> Vec<&'static str> {
    component
        .graphics()
        .iter()
        .map(|view| view.detail.kind_name())
        .collect()
}

fn part_structural_pass(
    input: SchLibSnapshot,
    temp: &std::path::Path,
) -> Result<PartStructuralOutcome, Box<dyn std::error::Error>> {
    let mut report_correct = true;
    let mut restore_equal = true;

    // Baselines from the immutable input.
    let mut original_effective = Vec::new();
    let mut original_pin_fingerprints = Vec::new();
    let mut original_graphic_kinds = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        original_effective.push(component.effective_part_count()?);
        original_pin_fingerprints.push(pin_fingerprints(&component));
        original_graphic_kinds.push(graphic_kinds(&component));
    }

    // Stage 1: append one empty trailing part slot to every component.
    // Growth changes only the two count fields, so every child projection
    // must stay index-identical to the input's.
    let mut grows = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        let grown = component.with_parts_appended(1)?;
        report_correct &= grown.effective_part_count()? == original_effective[index] + 1
            && pin_fingerprints(&grown) == original_pin_fingerprints[index]
            && graphic_kinds(&grown) == original_graphic_kinds[index];
        grows.push((index, grown));
    }
    let expected_grown = input.replace_components(grows)?;
    let grown_path = temp.join("structural-part-grown.SchLib");
    expected_grown.materialize()?.save(&grown_path)?;
    let reopened_grown = SchLib::open(&grown_path)?.snapshot()?;
    let growth_equal = reopened_grown == expected_grown;

    // Stage 2: remove the appended trailing slot again. The new slot owns no
    // children, so the typed report must list zero deletions (no
    // authorization needed) and the memo §3 rule 4 current_part_id decrement
    // (fires only when the stored current part is > 1, on ANY slot removal)
    // is asserted through the report.
    let mut shrinks = Vec::new();
    let mut adjusted = Vec::new();
    for index in 0..reopened_grown.component_count() {
        let component = reopened_grown.component(index)?;
        let removed_slot = original_effective[index] + 1;
        let shrink = component.with_parts_removed(&[removed_slot], false)?;
        report_correct &= shrink.removed_parts == [removed_slot]
            && shrink.deleted_children.is_empty()
            && match shrink.current_part_id_adjustment {
                None => true,
                Some(adjustment) => {
                    adjustment.before > 1 && adjustment.after == adjustment.before - 1
                }
            };
        adjusted.push(shrink.current_part_id_adjustment.is_some());
        shrinks.push((index, shrink.component));
    }
    let expected_shrunk = reopened_grown.replace_components(shrinks)?;
    let shrunk_path = temp.join("structural-part-shrunk.SchLib");
    expected_shrunk.materialize()?.save(&shrunk_path)?;
    let reopened_shrunk = SchLib::open(&shrunk_path)?.snapshot()?;
    let shrink_equal = reopened_shrunk == expected_shrunk;

    // Restore check: wherever no current_part_id adjustment was reported the
    // grow+shrink cycle must hand back the original typed component exactly;
    // an adjusted component (stored current part > 1) differs in exactly
    // that reported field, so it is held to count and projection equality.
    for index in 0..reopened_shrunk.component_count() {
        let component = reopened_shrunk.component(index)?;
        restore_equal &= component.effective_part_count()? == original_effective[index]
            && pin_fingerprints(&component) == original_pin_fingerprints[index]
            && graphic_kinds(&component) == original_graphic_kinds[index];
        if !adjusted[index] {
            restore_equal &= component == input.component(index)?;
        }
    }

    Ok(PartStructuralOutcome {
        growth_equal,
        shrink_equal,
        report_correct,
        restore_equal,
    })
}

struct FootprintStructuralOutcome {
    replace_equal: bool,
    probe_equal: bool,
    removal_equal: bool,
    projection_equal: bool,
    non_pcblib_invariant: bool,
    replaced_descriptions: usize,
}

const FOOTPRINT_PROBE_NAME: &str = "PROBE_STEP2D2";
const FOOTPRINT_PROBE_DESCRIPTION: &str = "STEP2D2 replaced description";

/// (model_name, description, is_current, pin→pads) tuples of the footprint
/// projection plus the preserved non-PCBLIB implementation count.
type FootprintFingerprint = (
    Vec<(String, String, bool, Vec<(String, Vec<String>)>)>,
    usize,
);

fn footprint_fingerprint(
    component: &SchLibComponentSnapshot,
) -> Result<FootprintFingerprint, Box<dyn std::error::Error>> {
    let maps = component.footprint_maps()?;
    Ok((
        maps.footprints
            .iter()
            .map(|footprint| {
                (
                    footprint.model_name.to_owned(),
                    footprint.description.to_owned(),
                    footprint.is_current,
                    footprint
                        .pin_pad_maps
                        .iter()
                        .map(|map| (map.pin.to_owned(), map.pads.to_vec()))
                        .collect(),
                )
            })
            .collect(),
        maps.preserved_non_pcblib_implementations,
    ))
}

fn footprint_structural_pass(
    input: SchLibSnapshot,
    temp: &std::path::Path,
) -> Result<FootprintStructuralOutcome, Box<dyn std::error::Error>> {
    let mut projection_equal = true;
    let mut non_pcblib_invariant = true;
    let mut replaced_descriptions = 0usize;

    // Baselines from the immutable input. Every component participates: the
    // batch API passes wild current-states (e.g. STM32F4's keyless ISCURRENT
    // ⇒ zero currents) through verbatim as long as no current value is
    // written, so value-preserving Replaces and non-current probe News work
    // on every stored shape.
    let mut baselines = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        baselines.push(footprint_fingerprint(&component)?);
    }

    // Stage 1: replace the first footprint's description (pin_pad_maps left
    // unaddressed — the definers must survive verbatim; is_current keeps the
    // stored value).
    let mut replacements = Vec::new();
    for index in 0..input.component_count() {
        if baselines[index].0.is_empty() {
            continue;
        }
        let component = input.component(index)?;
        let maps = component.footprint_maps()?;
        let first = &maps.footprints[0];
        let mut edits: Vec<SchFootprintEdit> = (0..maps.footprints.len())
            .map(SchFootprintEdit::Keep)
            .collect();
        edits[0] = SchFootprintEdit::Replace(
            0,
            SchFootprintInit {
                model_name: first.model_name.to_owned(),
                description: FOOTPRINT_PROBE_DESCRIPTION.to_owned(),
                is_current: first.is_current,
                pin_pad_maps: None,
            },
        );
        let edited = component.with_children(SchChildBatch {
            parameters: None,
            graphics: None,
            pins: None,
            footprints: Some(edits),
            allow_deletion: false,
        })?;
        replaced_descriptions += 1;
        replacements.push((index, edited));
    }
    let expected_replaced = input.replace_components(replacements)?;
    let replaced_path = temp.join("structural-footprint-replaced.SchLib");
    expected_replaced.materialize()?.save(&replaced_path)?;
    let reopened_replaced = SchLib::open(&replaced_path)?.snapshot()?;
    let replace_equal = reopened_replaced == expected_replaced;
    for index in 0..reopened_replaced.component_count() {
        let component = reopened_replaced.component(index)?;
        let fingerprint = footprint_fingerprint(&component)?;
        non_pcblib_invariant &= fingerprint.1 == baselines[index].1;
        if baselines[index].0.is_empty() {
            projection_equal &= fingerprint == baselines[index];
        } else {
            let mut expected = baselines[index].clone();
            expected.0[0].1 = FOOTPRINT_PROBE_DESCRIPTION.to_owned();
            projection_equal &= fingerprint == expected;
        }
    }

    // Stage 2: add the probe footprint to every component, with one pin-pad
    // map from an existing pin when the component has pins.
    let mut inserts = Vec::new();
    for index in 0..reopened_replaced.component_count() {
        let component = reopened_replaced.component(index)?;
        let maps = component.footprint_maps()?;
        let pin_pad_maps = match component.pins().first() {
            Some(pin) if !pin.designator.is_empty() => vec![SchPinPadMapInit {
                pin: pin.designator.to_owned(),
                pads: vec!["STEP2D2_PAD".to_owned()],
            }],
            _ => Vec::new(),
        };
        let edits: Vec<SchFootprintEdit> = (0..maps.footprints.len())
            .map(SchFootprintEdit::Keep)
            .chain([SchFootprintEdit::New(SchFootprintInit {
                model_name: FOOTPRINT_PROBE_NAME.to_owned(),
                description: "step2 D2 probe".to_owned(),
                // The sole footprint must be the current one; alongside an
                // existing current the probe stays non-current.
                is_current: maps.footprints.is_empty(),
                pin_pad_maps: Some(pin_pad_maps),
            })])
            .collect();
        let edited = component.with_children(SchChildBatch {
            parameters: None,
            graphics: None,
            pins: None,
            footprints: Some(edits),
            allow_deletion: false,
        })?;
        inserts.push((index, edited));
    }
    let expected_probed = reopened_replaced.replace_components(inserts)?;
    let probed_path = temp.join("structural-footprint-probed.SchLib");
    expected_probed.materialize()?.save(&probed_path)?;
    let reopened_probed = SchLib::open(&probed_path)?.snapshot()?;
    let probe_equal = reopened_probed == expected_probed;
    for index in 0..reopened_probed.component_count() {
        let component = reopened_probed.component(index)?;
        let fingerprint = footprint_fingerprint(&component)?;
        non_pcblib_invariant &= fingerprint.1 == baselines[index].1;
        let probe = fingerprint.0.last();
        projection_equal &= fingerprint.0.len() == baselines[index].0.len() + 1
            && probe.is_some_and(|(name, _, current, _)| {
                name == FOOTPRINT_PROBE_NAME && *current == baselines[index].0.is_empty()
            });
    }

    // Stage 3: remove the probe (authorized) and restore the description; the
    // final snapshot must equal the original input exactly.
    let mut removals = Vec::new();
    for index in 0..reopened_probed.component_count() {
        let component = reopened_probed.component(index)?;
        let maps = component.footprint_maps()?;
        let count = maps.footprints.len();
        if maps.footprints[count - 1].model_name != FOOTPRINT_PROBE_NAME {
            return Err(format!(
                "component {index}: footprints[{}] is not the probe footprint",
                count - 1
            )
            .into());
        }
        let mut edits: Vec<SchFootprintEdit> = (0..count - 1).map(SchFootprintEdit::Keep).collect();
        if !baselines[index].0.is_empty() {
            let original = &baselines[index].0[0];
            edits[0] = SchFootprintEdit::Replace(
                0,
                SchFootprintInit {
                    model_name: original.0.to_owned(),
                    description: original.1.to_owned(),
                    is_current: original.2,
                    pin_pad_maps: None,
                },
            );
        }
        let edited = component.with_children(SchChildBatch {
            parameters: None,
            graphics: None,
            pins: None,
            footprints: Some(edits),
            allow_deletion: true,
        })?;
        removals.push((index, edited));
    }
    let expected_removed = reopened_probed.replace_components(removals)?;
    let removed_path = temp.join("structural-footprint-removed.SchLib");
    expected_removed.materialize()?.save(&removed_path)?;
    let reopened_removed = SchLib::open(&removed_path)?.snapshot()?;
    let mut removal_equal = reopened_removed == expected_removed;
    removal_equal &= reopened_removed == input;
    for index in 0..reopened_removed.component_count() {
        let component = reopened_removed.component(index)?;
        let fingerprint = footprint_fingerprint(&component)?;
        projection_equal &= fingerprint == baselines[index];
        non_pcblib_invariant &= fingerprint.1 == baselines[index].1;
    }

    Ok(FootprintStructuralOutcome {
        replace_equal,
        probe_equal,
        removal_equal,
        projection_equal,
        non_pcblib_invariant,
        replaced_descriptions,
    })
}

struct NamespaceOutcome {
    alias_add_equal: bool,
    alias_remove_equal: bool,
    clone_equal: bool,
    clone_projection_equal: bool,
    restore_equal: bool,
}

const NAMESPACE_ALIAS: &str = "STEP2D_0";
/// 42 characters — beyond the 31-character CFB storage-name limit, so the
/// save path must route it through SectionKeys.
const NAMESPACE_CLONE: &str = "STEP2D_CLONE_WITH_A_VERY_LONG_STORAGE_NAME";

/// Complete owned fingerprint of a component's public typed projection
/// surface, deliberately excluding lib reference, component UniqueId (not
/// projected), and aliases — the axes a clone legitimately differs on.
fn component_fingerprint(
    component: &SchLibComponentSnapshot,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut fingerprint = vec![
        format!("description={:?}", component.description()),
        format!("designator={:?}", component.designator()?),
        format!("kind={:?}", component.component_kind()),
        format!("hidden_pins={:?}", component.show_hidden_pins()),
        format!("effective_parts={:?}", component.effective_part_count()?),
        format!("all_pin_count={}", component.stored_all_pin_count()),
        format!("parts={:?}", component.parts()?),
        format!("footprints={:?}", component.footprint_maps()?),
    ];
    fingerprint.extend(
        component
            .parameters()?
            .iter()
            .map(|view| format!("param {view:?}")),
    );
    fingerprint.extend(component.pins().iter().map(|view| format!("pin {view:?}")));
    fingerprint.extend(
        component
            .graphics()
            .iter()
            .map(|view| format!("graphic {view:?}")),
    );
    Ok(fingerprint)
}

fn namespace_pass(
    input: SchLibSnapshot,
    temp: &std::path::Path,
) -> Result<NamespaceOutcome, Box<dyn std::error::Error>> {
    let canonical = input.component(0)?.lib_reference().to_owned();

    // Stage 1: add alias STEP2D_0 to component 0 (declared transition; the
    // three alias representations are validated together at save/reopen).
    let with_alias = input.with_library_edit(SchLibEdit {
        alias_ops: vec![SchAliasOp::Add {
            component: canonical.clone(),
            alias: NAMESPACE_ALIAS.to_owned(),
        }],
        ..Default::default()
    })?;
    let alias_path = temp.join("namespace-alias.SchLib");
    with_alias.materialize()?.save(&alias_path)?;
    let reopened_alias = SchLib::open(&alias_path)?.snapshot()?;
    let alias_add_equal = reopened_alias == with_alias
        && reopened_alias
            .component(0)?
            .aliases()
            .any(|alias| alias == NAMESPACE_ALIAS);

    // Stage 2: remove the alias again; the typed state must restore the
    // original input snapshot exactly.
    let without_alias = reopened_alias.with_library_edit(SchLibEdit {
        alias_ops: vec![SchAliasOp::Remove {
            component: canonical.clone(),
            alias: NAMESPACE_ALIAS.to_owned(),
        }],
        ..Default::default()
    })?;
    let alias_removed_path = temp.join("namespace-alias-removed.SchLib");
    without_alias.materialize()?.save(&alias_removed_path)?;
    let restored_alias = SchLib::open(&alias_removed_path)?.snapshot()?;
    let alias_remove_equal = restored_alias == without_alias && restored_alias == input;

    // Stage 3: clone component 0 under a fresh >31-char name.
    let component_count = restored_alias.component_count();
    let mut clone_slots: Vec<SchLibComponentEdit> = (0..component_count)
        .map(SchLibComponentEdit::Keep)
        .collect();
    clone_slots.push(SchLibComponentEdit::New(SchNewComponent::CloneOf {
        source: 0,
        lib_reference: NAMESPACE_CLONE.to_owned(),
    }));
    let cloned = restored_alias.with_library_edit(SchLibEdit {
        components: Some(clone_slots),
        ..Default::default()
    })?;
    let clone_path = temp.join("namespace-clone.SchLib");
    cloned.materialize()?.save(&clone_path)?;
    let reopened_clone = SchLib::open(&clone_path)?.snapshot()?;
    let clone_equal = reopened_clone == cloned;
    let clone = reopened_clone.component(component_count)?;
    let source = reopened_clone.component(0)?;
    let clone_projection_equal = clone.lib_reference() == NAMESPACE_CLONE
        && clone.aliases().len() == 0
        && component_fingerprint(&clone)? == component_fingerprint(&source)?;

    // Stage 4: authorized removal of the clone restores the original
    // snapshot state exactly.
    let removed = reopened_clone.with_library_edit(SchLibEdit {
        components: Some(
            (0..component_count)
                .map(SchLibComponentEdit::Keep)
                .collect(),
        ),
        allow_component_removal: true,
        ..Default::default()
    })?;
    let removed_path = temp.join("namespace-clone-removed.SchLib");
    removed.materialize()?.save(&removed_path)?;
    let restored = SchLib::open(&removed_path)?.snapshot()?;
    let restore_equal = restored == removed && restored == input;

    Ok(NamespaceOutcome {
        alias_add_equal,
        alias_remove_equal,
        clone_equal,
        clone_projection_equal,
        restore_equal,
    })
}

fn authored<T>(value: T) -> FieldIntent<T> {
    FieldIntent::Set(Spanned::new(value, Span::new(0, 0)))
}
