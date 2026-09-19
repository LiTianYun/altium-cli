//! Real-file probe for complete typed snapshots, explicit scalar patches,
//! structural parameter/graphic/pin batches, part grow/shrink round-trips,
//! and library-level namespace transitions (aliases + component clone/remove).
//! Writes only temporary documents. This is not a user-facing saved-plan command.
// The single json! report object exceeds the default macro recursion depth.
#![recursion_limit = "256"]
use altium_format::SchLib;
use altium_format::schlib::{
    SchAliasOp, SchChildBatch, SchFootprintEdit, SchFootprintInit, SchGraphicDetail,
    SchGraphicEdit, SchGraphicInit, SchGraphicShape, SchLibComponentEdit, SchLibComponentSnapshot,
    SchLibEdit, SchLibSnapshot, SchNewComponent, SchOwnerGroup, SchParameterEdit, SchParameterInit,
    SchPinEdit, SchPinInit, SchPinPadMapInit,
};
use altium_format_types::{
    Color, ComponentKind, Coord, CoordPoint, LineStyle, PenWidth, PinElectricalType, RotationBy90,
};
use altium_spec_lang::{
    ast::{CollectionDelete, CollectionMode},
    diagnostic::{Span, Spanned},
    intent::{
        Collection, CollectionScope, ComponentIntent, FieldIntent, GraphicFields, GraphicIntent,
        GraphicKind, INTENT_SCHEMA_VERSION, ParameterIntent, PartIntent, PartNumber, PinIntent,
        SchLibIntent,
    },
};
use altium_sync::apply::{apply_saved_dump_plan, apply_saved_plan};
use altium_sync::plan::{
    ManifestRole, PlanArtifact, PlanArtifactContents, PlanPatches, PlanScope, PlanSourcePatch,
    SavedPlan, load_schlib_hashed,
};
use altium_sync::planner::{PLAN_EXIT_CHANGES, PLAN_EXIT_NO_CHANGES, plan_dump_schlib};
use altium_sync::report::{ReportCollection, ReportOp, render_plan_report, report};
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
            || result["intent_parameter_equal"] != true
            || result["intent_parameter_replan_empty"] != true
            || result["intent_create_equal"] != true
            || result["intent_create_replan_empty"] != true
            || result["intent_remove_equal"] != true
            || result["intent_restore_equal"] != true
            || result["intent_pin_rename_equal"] != true
            || result["intent_pin_rename_replan_empty"] != true
            || result["intent_pin_restore_equal"] != true
            || result["intent_part_graphic_equal"] != true
            || result["intent_part_graphic_replan_empty"] != true
            || result["intent_part_graphic_restore_equal"] != true
            || result["intent_part_growth_equal"] != true
            || result["intent_part_growth_replan_empty"] != true
            || result["intent_part_removal_equal"] != true
            || result["plan_payload_noop_equal"] != true
            || result["plan_roundtrip_equal"] != true
            || result["plan_deterministic"] != true
            || result["plan_full_circle_applied"] != true
            || result["plan_full_circle_replan_empty"] != true
            || result["plan_report_entries_exact"] != true
            || result["plan_target_drift_rejected"] != true
            || result["dump_create_applied"] != true
            || result["dump_create_converges"] != true
            || result["dump_noop_exit0"] != true
            || result["dump_edit_converges"] != true
            || result["dump_comment_preserved"] != true
            || result["dump_crlf_preserved"] != true
            || result["dump_report_entries_exact"] != true
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

    // Intent pass (increment E1): programmatic SchLibIntent elaboration —
    // (a) partial parameters probe on component 0 with replan-empty, then
    // (b) partial append of a new Blank component (one parameter, one alias)
    // and (c) a COMPLETE components intent built from the final library minus
    // the probe with delete: allow, removing exactly the probe and restoring
    // the pre-probe state; native save/reopen typed equality after each apply.
    let intent = intent_pass(SchLib::open(path)?, temp.path())?;

    // Intent E2 pass: positional partial pin rename on component 0 (restored
    // after), part-block graphic add on the first eligible multi-part
    // component (skips recorded — never silent), and a part growth + removal
    // round-trip via intent on component 0, each with apply, native
    // save/reopen typed equality, and replan-empty.
    let intent_e2 = intent_e2_pass(SchLib::open(path)?, temp.path())?;

    // Saved-plan pass (step-3 increment A): the §S7 hash-and-parse-same-
    // bytes load, the payload round-trip law over every component
    // (decode(encode(c)) installed via ReplaceWithState is a typed no-op),
    // and saved-plan lowering with strict-decode equality plus byte
    // determinism across two independent plannings.
    let plan = plan_pass(path, temp.path())?;

    // Dump-direction saved-plan pass (step-3 increment C): SourceCreate on
    // the first pass (plan -> serialize -> strict decode -> apply ->
    // recompile-and-elaborate to an EMPTY patch), a no-op second plan
    // (exit 0, zero edits), then a seeded comment + CRLF + one flipped
    // property value merged back through hash-bound structured edits with
    // byte-level comment/CRLF preservation and renewed convergence.
    let dump = dump_pass(path, temp.path())?;

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
        "intent_parameter_equal": intent.parameter_equal,
        "intent_parameter_replan_empty": intent.parameter_replan_empty,
        "intent_create_equal": intent.create_equal,
        "intent_create_replan_empty": intent.create_replan_empty,
        "intent_remove_equal": intent.remove_equal,
        "intent_restore_equal": intent.restore_equal,
        "intent_pin_rename_equal": intent_e2.pin_rename_equal,
        "intent_pin_rename_replan_empty": intent_e2.pin_rename_replan_empty,
        "intent_pin_restore_equal": intent_e2.pin_restore_equal,
        "intent_pin_rename_skipped": intent_e2.pin_rename_skipped,
        "intent_part_graphic_equal": intent_e2.part_graphic_equal,
        "intent_part_graphic_replan_empty": intent_e2.part_graphic_replan_empty,
        "intent_part_graphic_restore_equal": intent_e2.part_graphic_restore_equal,
        "intent_part_graphic_skipped": intent_e2.part_graphic_skipped,
        "intent_part_growth_equal": intent_e2.part_growth_equal,
        "intent_part_growth_replan_empty": intent_e2.part_growth_replan_empty,
        "intent_part_removal_equal": intent_e2.part_removal_equal,
        "plan_payload_noop_equal": plan.payload_noop_equal,
        "plan_roundtrip_equal": plan.roundtrip_equal,
        "plan_deterministic": plan.deterministic,
        "plan_full_circle_applied": plan.full_circle_applied,
        "plan_full_circle_replan_empty": plan.full_circle_replan_empty,
        "plan_report_entries_exact": plan.report_entries_exact,
        "plan_target_drift_rejected": plan.target_drift_rejected,
        "dump_create_applied": dump.create_applied,
        "dump_create_converges": dump.create_converges,
        "dump_generation_skips": dump.generation_skips,
        "dump_noop_exit0": dump.noop_exit0,
        "dump_edit_converges": dump.edit_converges,
        "dump_edits_applied": dump.edits_applied,
        "dump_comment_preserved": dump.comment_preserved,
        "dump_crlf_preserved": dump.crlf_preserved,
        "dump_report_entries_exact": dump.report_entries_exact,
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

struct PlanOutcome {
    payload_noop_equal: bool,
    roundtrip_equal: bool,
    deterministic: bool,
    full_circle_applied: bool,
    full_circle_replan_empty: bool,
    report_entries_exact: bool,
    /// The target FILE drifted (one byte appended) after planning and the
    /// manifest content-hash precondition rejected the apply. This is an
    /// input-drift check, not a plan-file tamper: plan-payload tampering
    /// (valid-but-different before-state, contract-breaking fields) is
    /// covered by the fixture-free apply unit tests.
    target_drift_rejected: bool,
}

fn plan_pass(
    path: &std::path::Path,
    temp: &std::path::Path,
) -> Result<PlanOutcome, Box<dyn std::error::Error>> {
    // §S7: read once, hash and parse the same buffer.
    let loaded = load_schlib_hashed(path)?;
    let input = loaded.library.snapshot()?;

    // Round-trip law: every component's encoded state decodes and installs
    // as a typed no-op through ReplaceWithState (one batch edit).
    let slots = (0..input.component_count())
        .map(|index| {
            let payload = input.component(index)?.encode_state();
            let decoded = serde_json::from_str(&serde_json::to_string(&payload)?)?;
            Ok(SchLibComponentEdit::ReplaceWithState(index, decoded))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let reinstalled = input.with_library_edit(SchLibEdit {
        components: Some(slots),
        ..Default::default()
    })?;
    let payload_noop_equal = reinstalled == input;

    // Lowering + strict decode + determinism over a real content patch.
    let build_patch = || {
        SchLibPatch::descriptions(
            &input,
            (0..input.component_count()).map(|index| {
                (
                    index,
                    FieldIntent::Set(Spanned::new(
                        Some(format!("step3 plan probe {index}")),
                        Span::new(0, 0),
                    )),
                )
            }),
        )
    };
    let artifacts = || {
        vec![
            PlanArtifact {
                role: ManifestRole::TargetDocument,
                declared_path: path.to_owned(),
                contents: PlanArtifactContents::Bytes(loaded.bytes.clone()),
            },
            PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: temp.join("plan-out.SchLib"),
                contents: PlanArtifactContents::ExpectedAbsent,
            },
        ]
    };
    let plan_path = temp.join("plan.json");
    let plan = SavedPlan::compile_from_schlib_patch(
        &build_patch()?,
        PlanScope::default(),
        artifacts(),
        Some(&plan_path),
        Vec::new(),
    )?;
    let bytes = plan.to_json_bytes()?;
    let roundtrip_equal = SavedPlan::from_json_bytes(&bytes)? == plan;
    let second = SavedPlan::compile_from_schlib_patch(
        &build_patch()?,
        PlanScope::default(),
        artifacts(),
        Some(&plan_path),
        Vec::new(),
    )?;
    // No components are created, so two plannings over identical inputs
    // must be byte-identical (the determinism gate's strongest form).
    let deterministic = second.to_json_bytes()? == bytes && plan.to_json_bytes()? == bytes;

    // Full-circle saved-plan stage (step-3 increment B): plan a
    // programmatic intent against a COPY of the real library, serialize the
    // plan to disk, re-read + strict-decode it, execute it through
    // apply_saved_plan (manifest verification, address resolution,
    // before-equality, execution, save, reopen typed equality), then verify
    // replan-empty and exact report entry counts; finally append one byte
    // to the target file (input DRIFT, not plan tampering) and assert the
    // manifest content-hash rejection.
    let circle_target = temp.join("plan-circle.SchLib");
    std::fs::copy(path, &circle_target)?;
    let circle_loaded = load_schlib_hashed(&circle_target)?;
    let circle_input = circle_loaded.library.snapshot()?;
    let circle_fields: Vec<(usize, FieldIntent<Option<String>>)> = (0..circle_input
        .component_count())
        .map(|index| {
            (
                index,
                FieldIntent::Set(Spanned::new(
                    Some(format!("step3 full circle {index}")),
                    Span::new(0, 0),
                )),
            )
        })
        .collect();
    let circle_patch = SchLibPatch::descriptions(&circle_input, circle_fields.clone())?;
    let expected = circle_patch.resulting_snapshot()?;
    let circle_out = temp.join("plan-circle-out.SchLib");
    let circle_plan_path = temp.join("plan-circle.json");
    let circle_plan = SavedPlan::compile_from_schlib_patch(
        &circle_patch,
        PlanScope::default(),
        vec![
            PlanArtifact {
                role: ManifestRole::TargetDocument,
                declared_path: circle_target.clone(),
                contents: PlanArtifactContents::Bytes(circle_loaded.bytes.clone()),
            },
            PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: circle_out.clone(),
                contents: PlanArtifactContents::ExpectedAbsent,
            },
        ],
        Some(&circle_plan_path),
        Vec::new(),
    )?;
    std::fs::write(&circle_plan_path, circle_plan.to_json_bytes()?)?;
    let decoded = SavedPlan::from_json_bytes(&std::fs::read(&circle_plan_path)?)?;
    let applied = apply_saved_plan(&decoded)?;
    let written = SchLib::open(&circle_out)?.snapshot()?;
    let full_circle_applied = applied.replaced == circle_input.component_count()
        && applied.created == 0
        && applied.deleted == 0
        && written == expected;
    let full_circle_replan_empty = SchLibPatch::descriptions(&written, circle_fields)?.is_empty();

    // Exact report derivation: one component-level replace entry per
    // component, each carrying the description before/after pair.
    let derived = report(&decoded)?;
    let report_entries_exact = derived.entries.len() == circle_input.component_count()
        && derived.entries.iter().all(|entry| {
            entry.op == ReportOp::Replace
                && entry.address.collection == ReportCollection::Components
                && entry
                    .fields
                    .iter()
                    .any(|field| field.name == "description" && field.after.is_some())
        })
        && derived.allocated_ids.is_empty()
        && !render_plan_report(&derived).is_empty();

    // Tamper one byte of the target file: a fresh plan against the intact
    // copy must be rejected by the manifest hash precondition.
    let tamper_out = temp.join("plan-tamper-out.SchLib");
    let tamper_plan = SavedPlan::compile_from_schlib_patch(
        &SchLibPatch::descriptions(
            &circle_input,
            [(
                0,
                FieldIntent::Set(Spanned::new(
                    Some("step3 tamper probe".to_owned()),
                    Span::new(0, 0),
                )),
            )],
        )?,
        PlanScope::default(),
        vec![
            PlanArtifact {
                role: ManifestRole::TargetDocument,
                declared_path: circle_target.clone(),
                contents: PlanArtifactContents::Bytes(circle_loaded.bytes.clone()),
            },
            PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: tamper_out,
                contents: PlanArtifactContents::ExpectedAbsent,
            },
        ],
        None,
        Vec::new(),
    )?;
    let mut tampered_bytes = circle_loaded.bytes.clone();
    tampered_bytes.push(0);
    std::fs::write(&circle_target, &tampered_bytes)?;
    let target_drift_rejected = match apply_saved_plan(&tamper_plan) {
        Err(error) => error.to_string().contains("content hash mismatch"),
        Ok(_) => false,
    };

    Ok(PlanOutcome {
        payload_noop_equal,
        roundtrip_equal,
        deterministic,
        full_circle_applied,
        full_circle_replan_empty,
        report_entries_exact,
        target_drift_rejected,
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

struct IntentOutcome {
    parameter_equal: bool,
    parameter_replan_empty: bool,
    create_equal: bool,
    create_replan_empty: bool,
    remove_equal: bool,
    restore_equal: bool,
}

const INTENT_PARAMETER: &str = "STEP2E";
const INTENT_PROBE: &str = "STEP2E_PROBE";
const INTENT_PROBE_PARAMETER: &str = "STEP2E_P";
const INTENT_PROBE_ALIAS: &str = "STEP2E_ALIAS";

fn spanned<T>(value: T) -> Spanned<T> {
    Spanned::new(value, Span::new(0, 0))
}

fn blank_component_intent(name: &str) -> ComponentIntent {
    ComponentIntent {
        name: spanned(name.to_owned()),
        annotation: None,
        designator: FieldIntent::Absent,
        description: FieldIntent::Absent,
        component_kind: FieldIntent::Absent,
        part_count: FieldIntent::Absent,
        show_hidden_pins: FieldIntent::Absent,
        pins: Collection::default(),
        parameters: Collection::default(),
        aliases: Collection::default(),
        footprints: Collection::default(),
        graphics: Collection::default(),
        parts: Collection::default(),
    }
}

fn intent_of(scope: CollectionScope, components: Vec<ComponentIntent>) -> SchLibIntent {
    SchLibIntent {
        schema_version: INTENT_SCHEMA_VERSION,
        components: Collection {
            scope,
            items: components,
        },
    }
}

fn intent_pass(
    mut doc: SchLib,
    temp: &std::path::Path,
) -> Result<IntentOutcome, Box<dyn std::error::Error>> {
    let input = doc.snapshot()?;
    let component_0 = input.component(0)?.lib_reference().to_owned();

    // Stage (a): partial components intent setting parameter STEP2E on
    // component 0 (partial parameters: matched slots update, this unmatched
    // name appends).
    let mut with_parameter = blank_component_intent(&component_0);
    with_parameter.parameters = Collection {
        scope: CollectionScope::default(),
        items: vec![ParameterIntent {
            name: spanned(INTENT_PARAMETER.to_owned()),
            text: authored("intent pass probe".to_owned()),
            is_hidden: authored(true),
        }],
    };
    let parameter_intent = intent_of(CollectionScope::default(), vec![with_parameter]);
    let patch = SchLibPatch::from_intent(&input, &parameter_intent)?;
    let expected = patch.resulting_snapshot()?;
    patch.apply_in_memory(&mut doc)?;
    let mut parameter_equal = doc.snapshot()? == expected;
    parameter_equal &= expected
        .component(0)?
        .parameters()?
        .iter()
        .any(|view| view.name == INTENT_PARAMETER && view.text == "intent pass probe");
    let parameter_path = temp.join("intent-parameter.SchLib");
    doc.save(&parameter_path)?;
    parameter_equal &= SchLib::open(&parameter_path)?.snapshot()? == expected;
    let parameter_replan_empty =
        SchLibPatch::from_intent(&doc.snapshot()?, &parameter_intent)?.is_empty();

    // Stage (b): partial append of a NEW component (Blank, one parameter,
    // one alias).
    let state_after_parameter = doc.snapshot()?;
    let mut probe = blank_component_intent(INTENT_PROBE);
    probe.parameters = Collection {
        scope: CollectionScope::default(),
        items: vec![ParameterIntent {
            name: spanned(INTENT_PROBE_PARAMETER.to_owned()),
            text: authored("created by the intent pass".to_owned()),
            is_hidden: FieldIntent::Absent,
        }],
    };
    probe.aliases = Collection {
        scope: CollectionScope::default(),
        items: vec![spanned(INTENT_PROBE_ALIAS.to_owned())],
    };
    let create_intent = intent_of(CollectionScope::default(), vec![probe]);
    let create_patch = SchLibPatch::from_intent(&state_after_parameter, &create_intent)?;
    let expected_created = create_patch.resulting_snapshot()?;
    create_patch.apply_in_memory(&mut doc)?;
    let mut create_equal = doc.snapshot()? == expected_created;
    create_equal &=
        expected_created.component_count() == state_after_parameter.component_count() + 1;
    {
        let created = expected_created.component(expected_created.component_count() - 1)?;
        create_equal &= created.lib_reference() == INTENT_PROBE;
        create_equal &= created
            .parameters()?
            .iter()
            .any(|view| view.name == INTENT_PROBE_PARAMETER);
        create_equal &= created.aliases().any(|alias| alias == INTENT_PROBE_ALIAS);
    }
    let created_path = temp.join("intent-created.SchLib");
    doc.save(&created_path)?;
    create_equal &= SchLib::open(&created_path)?.snapshot()? == expected_created;
    let create_replan_empty =
        SchLibPatch::from_intent(&doc.snapshot()?, &create_intent)?.is_empty();

    // Stage (c): a COMPLETE components intent constructed from the final
    // library minus the probe, with delete: allow — removes exactly the
    // probe (its alias released with the aggregate) and restores stage (a)'s
    // state.
    let state_with_probe = doc.snapshot()?;
    let survivors: Vec<ComponentIntent> = state_with_probe
        .component_lib_references()
        .filter(|name| *name != INTENT_PROBE)
        .map(blank_component_intent)
        .collect();
    let remove_intent = intent_of(
        CollectionScope {
            mode: CollectionMode::Complete,
            delete: CollectionDelete::Allow,
            span: Some(Span::new(0, 0)),
        },
        survivors,
    );
    let remove_patch = SchLibPatch::from_intent(&state_with_probe, &remove_intent)?;
    let mut remove_equal = remove_patch.removed_components().len() == 1
        && remove_patch.removed_components()[0].lib_reference == INTENT_PROBE
        && remove_patch.replacements().is_empty();
    let expected_removed = remove_patch.resulting_snapshot()?;
    remove_patch.apply_in_memory(&mut doc)?;
    remove_equal &= doc.snapshot()? == expected_removed;
    let removed_path = temp.join("intent-removed.SchLib");
    doc.save(&removed_path)?;
    remove_equal &= SchLib::open(&removed_path)?.snapshot()? == expected_removed;
    let restore_equal = expected_removed == state_after_parameter;

    Ok(IntentOutcome {
        parameter_equal,
        parameter_replan_empty,
        create_equal,
        create_replan_empty,
        remove_equal,
        restore_equal,
    })
}

struct IntentE2Outcome {
    pin_rename_equal: bool,
    pin_rename_replan_empty: bool,
    pin_restore_equal: bool,
    pin_rename_skipped: usize,
    part_graphic_equal: bool,
    part_graphic_replan_empty: bool,
    part_graphic_restore_equal: bool,
    part_graphic_skipped: usize,
    part_growth_equal: bool,
    part_growth_replan_empty: bool,
    part_removal_equal: bool,
}

fn e2_pin_item(designator: &str) -> PinIntent {
    PinIntent {
        placement: None,
        designator: spanned(designator.to_owned()),
        name: FieldIntent::Absent,
        electrical: FieldIntent::Absent,
        length: FieldIntent::Absent,
        location: FieldIntent::Absent,
        location_x: FieldIntent::Absent,
        location_y: FieldIntent::Absent,
        orientation: FieldIntent::Absent,
        is_hidden: FieldIntent::Absent,
        hidden_net_name: FieldIntent::Absent,
        swap_group: FieldIntent::Absent,
        part_swap_group: FieldIntent::Absent,
        pair_swap_group: FieldIntent::Absent,
    }
}
fn e2_part_item(number: i32) -> PartIntent {
    PartIntent {
        number: spanned(PartNumber::new(number).expect("positive part number")),
        part_swap_group: FieldIntent::Absent,
        pins: Collection::default(),
        graphics: Collection::default(),
    }
}
fn e2_graphic_item(kind: GraphicKind) -> GraphicIntent {
    GraphicIntent {
        auto_size: None,
        kind: spanned(kind),
        fields: GraphicFields::default(),
    }
}
fn e2_items<T>(items: Vec<T>) -> Collection<T> {
    Collection {
        scope: CollectionScope::default(),
        items,
    }
}

/// Maps a projected graphic detail back to its authorable intent kind;
/// `None` for intent-unrepresentable kinds (Symbol/Hyperlink/other).
fn intent_kind_of(detail: &SchGraphicDetail<'_>) -> Option<GraphicKind> {
    Some(match detail {
        SchGraphicDetail::Line { .. } => GraphicKind::Line,
        SchGraphicDetail::Rectangle { .. } => GraphicKind::Rectangle,
        SchGraphicDetail::RoundRectangle { .. } => GraphicKind::RoundRectangle,
        SchGraphicDetail::Arc { .. } => GraphicKind::Arc,
        SchGraphicDetail::EllipticalArc { .. } => GraphicKind::EllipticalArc,
        SchGraphicDetail::Ellipse { .. } => GraphicKind::Ellipse,
        SchGraphicDetail::Pie { .. } => GraphicKind::Pie,
        SchGraphicDetail::Polyline { .. } => GraphicKind::Polyline,
        SchGraphicDetail::Polygon { .. } => GraphicKind::Polygon,
        SchGraphicDetail::Bezier { .. } => GraphicKind::Bezier,
        SchGraphicDetail::Image { .. } => GraphicKind::Image,
        SchGraphicDetail::Label { .. } => GraphicKind::Label,
        SchGraphicDetail::TextFrame { .. } => GraphicKind::TextFrame,
        SchGraphicDetail::Symbol | SchGraphicDetail::Hyperlink | SchGraphicDetail::Other { .. } => {
            return None;
        }
    })
}

fn intent_e2_pass(
    mut doc: SchLib,
    temp: &std::path::Path,
) -> Result<IntentE2Outcome, Box<dyn std::error::Error>> {
    let mut outcome = IntentE2Outcome {
        pin_rename_equal: true,
        pin_rename_replan_empty: true,
        pin_restore_equal: true,
        pin_rename_skipped: 0,
        part_graphic_equal: true,
        part_graphic_replan_empty: true,
        part_graphic_restore_equal: true,
        part_graphic_skipped: 0,
        part_growth_equal: true,
        part_growth_replan_empty: true,
        part_removal_equal: true,
    };

    // Stage (a): positional partial rename of component 0's FIRST pin — its
    // full-projection index 0 is necessarily filtered index 0 of its own
    // owner group — through the matching scope (component scope for a
    // shared pin on a multi-part component, a part block for Part(n)).
    // A shared pin on a single-part component is unaddressable by the memo
    // §1 mapping (component scope means Part(1) there); recorded as skipped.
    let input = doc.snapshot()?;
    let pin_scope_target: Option<Option<i32>> = {
        let component = input.component(0)?;
        let effective = component.effective_part_count()?;
        let pins = component.pins();
        match pins.first().map(|view| view.owner_group) {
            None => None,
            Some(SchOwnerGroup::SharedPins) if effective >= 2 => Some(None),
            Some(SchOwnerGroup::Part(n)) => Some(Some(n)),
            Some(_) => None,
        }
    };
    match pin_scope_target {
        None => outcome.pin_rename_skipped = 1,
        Some(part) => {
            let component_name = input.component(0)?.lib_reference().to_owned();
            let (designator, original_name) = {
                let component = input.component(0)?;
                let pins = component.pins();
                (pins[0].designator.to_owned(), pins[0].name.to_owned())
            };
            let build = |name: &str| {
                let mut item = e2_pin_item(&designator);
                item.name = authored(name.to_owned());
                let mut component = blank_component_intent(&component_name);
                match part {
                    None => component.pins = e2_items(vec![item]),
                    Some(number) => {
                        let mut part_item = e2_part_item(number);
                        part_item.pins = e2_items(vec![item]);
                        component.parts = e2_items(vec![part_item]);
                    }
                }
                intent_of(CollectionScope::default(), vec![component])
            };
            let renamed = format!("{original_name}_STEP2E2");
            let rename_intent = build(&renamed);
            let patch = SchLibPatch::from_intent(&input, &rename_intent)?;
            let expected = patch.resulting_snapshot()?;
            patch.apply_in_memory(&mut doc)?;
            outcome.pin_rename_equal &= doc.snapshot()? == expected;
            outcome.pin_rename_equal &= expected.component(0)?.pins()[0].name == renamed;
            let renamed_path = temp.join("intent-e2-pin-rename.SchLib");
            doc.save(&renamed_path)?;
            outcome.pin_rename_equal &= SchLib::open(&renamed_path)?.snapshot()? == expected;
            outcome.pin_rename_replan_empty =
                SchLibPatch::from_intent(&doc.snapshot()?, &rename_intent)?.is_empty();

            let restore_intent = build(&original_name);
            let patch = SchLibPatch::from_intent(&doc.snapshot()?, &restore_intent)?;
            patch.apply_in_memory(&mut doc)?;
            outcome.pin_restore_equal = doc.snapshot()? == input;
            let restored_path = temp.join("intent-e2-pin-restore.SchLib");
            doc.save(&restored_path)?;
            outcome.pin_restore_equal &= SchLib::open(&restored_path)?.snapshot()? == input;
        }
    }

    // Stage (b): part-block graphic add on the first multi-part component
    // whose Part(2) graphics group is fully intent-representable (a partial
    // scope must mirror-address every existing member to append, and a
    // complete scope removes the probe afterwards); none present is
    // recorded, never silently skipped.
    let state = doc.snapshot()?;
    let mut selection: Option<(usize, Vec<GraphicKind>)> = None;
    for index in 0..state.component_count() {
        let component = state.component(index)?;
        if component.effective_part_count()? < 2 {
            continue;
        }
        let mirrors: Option<Vec<GraphicKind>> = component
            .graphics()
            .iter()
            .filter(|view| matches!(view.owner_group, SchOwnerGroup::Part(2)))
            .map(|view| intent_kind_of(&view.detail))
            .collect();
        if let Some(kinds) = mirrors {
            selection = Some((index, kinds));
            break;
        }
    }
    match selection {
        None => outcome.part_graphic_skipped = 1,
        Some((index, mirror_kinds)) => {
            let component_name = state.component(index)?.lib_reference().to_owned();
            let graphics_before = state.component(index)?.graphics().len();
            let probe = {
                let mut probe = e2_graphic_item(GraphicKind::Rectangle);
                probe.fields.to = authored(CoordPoint {
                    x: Coord::from_mils(120).expect("120 mil fits Coord"),
                    y: Coord::from_mils(60).expect("60 mil fits Coord"),
                });
                probe
            };
            let add_intent = {
                let mut items: Vec<GraphicIntent> =
                    mirror_kinds.iter().copied().map(e2_graphic_item).collect();
                items.push(probe.clone());
                let mut part_item = e2_part_item(2);
                part_item.graphics = e2_items(items);
                let mut component = blank_component_intent(&component_name);
                component.parts = e2_items(vec![part_item]);
                intent_of(CollectionScope::default(), vec![component])
            };
            let patch = SchLibPatch::from_intent(&state, &add_intent)?;
            let expected = patch.resulting_snapshot()?;
            patch.apply_in_memory(&mut doc)?;
            outcome.part_graphic_equal &= doc.snapshot()? == expected;
            {
                let component = expected.component(index)?;
                let graphics = component.graphics();
                outcome.part_graphic_equal &= graphics.len() == graphics_before + 1;
                let appended = graphics.last().expect("appended probe");
                outcome.part_graphic_equal &=
                    matches!(appended.owner_group, SchOwnerGroup::Part(2))
                        && matches!(appended.detail, SchGraphicDetail::Rectangle { .. });
            }
            let added_path = temp.join("intent-e2-part-graphic.SchLib");
            doc.save(&added_path)?;
            outcome.part_graphic_equal &= SchLib::open(&added_path)?.snapshot()? == expected;
            outcome.part_graphic_replan_empty =
                SchLibPatch::from_intent(&doc.snapshot()?, &add_intent)?.is_empty();

            let remove_intent = {
                let mut part_item = e2_part_item(2);
                part_item.graphics = Collection {
                    scope: CollectionScope {
                        mode: CollectionMode::Complete,
                        delete: CollectionDelete::Allow,
                        span: Some(Span::new(0, 0)),
                    },
                    items: mirror_kinds.iter().copied().map(e2_graphic_item).collect(),
                };
                let mut component = blank_component_intent(&component_name);
                component.parts = e2_items(vec![part_item]);
                intent_of(CollectionScope::default(), vec![component])
            };
            let patch = SchLibPatch::from_intent(&doc.snapshot()?, &remove_intent)?;
            patch.apply_in_memory(&mut doc)?;
            outcome.part_graphic_restore_equal = doc.snapshot()? == state;
            let removed_path = temp.join("intent-e2-part-graphic-removed.SchLib");
            doc.save(&removed_path)?;
            outcome.part_graphic_restore_equal &= SchLib::open(&removed_path)?.snapshot()? == state;
        }
    }

    // Stage (c): part growth + removal round-trip via intent on component 0
    // — a new trailing part slot carrying one probe pin, then a complete
    // parts scope with delete: allow removing it again, the deletion and any
    // current_part_id adjustment surfaced through patch inspection.
    let state = doc.snapshot()?;
    let component_name = state.component(0)?.lib_reference().to_owned();
    let effective = state.component(0)?.effective_part_count()?;
    let new_slot = effective + 1;
    let growth_intent = {
        let mut probe = e2_pin_item("STE2");
        probe.name = authored("STEP2E2_GROWTH".to_owned());
        let mut part_item = e2_part_item(new_slot);
        part_item.pins = e2_items(vec![probe]);
        let mut component = blank_component_intent(&component_name);
        component.parts = e2_items(vec![part_item]);
        intent_of(CollectionScope::default(), vec![component])
    };
    let patch = SchLibPatch::from_intent(&state, &growth_intent)?;
    let expected = patch.resulting_snapshot()?;
    patch.apply_in_memory(&mut doc)?;
    outcome.part_growth_equal &= doc.snapshot()? == expected;
    {
        let component = expected.component(0)?;
        outcome.part_growth_equal &= component.effective_part_count()? == new_slot;
        let pins = component.pins();
        let probe = pins.last().expect("appended probe pin");
        outcome.part_growth_equal &= probe.designator == "STE2"
            && matches!(probe.owner_group, SchOwnerGroup::Part(n) if n == new_slot);
    }
    let grown_path = temp.join("intent-e2-part-growth.SchLib");
    doc.save(&grown_path)?;
    outcome.part_growth_equal &= SchLib::open(&grown_path)?.snapshot()? == expected;
    outcome.part_growth_replan_empty =
        SchLibPatch::from_intent(&doc.snapshot()?, &growth_intent)?.is_empty();

    let removal_intent = {
        let mut component = blank_component_intent(&component_name);
        component.parts = Collection {
            scope: CollectionScope {
                mode: CollectionMode::Complete,
                delete: CollectionDelete::Allow,
                span: Some(Span::new(0, 0)),
            },
            items: (1..=effective).map(e2_part_item).collect(),
        };
        intent_of(CollectionScope::default(), vec![component])
    };
    let patch = SchLibPatch::from_intent(&doc.snapshot()?, &removal_intent)?;
    outcome.part_removal_equal &= patch.parts_shrinks().len() == 1;
    let mut reported_adjustment = None;
    if let Some(report) = patch.parts_shrinks().first() {
        outcome.part_removal_equal &= report.component == component_name
            && report.removed_parts == [new_slot]
            && report.deleted_children.len() == 1
            && report
                .deleted_children
                .iter()
                .all(|child| child.owner_part_id == new_slot);
        reported_adjustment = report.current_part_id_adjustment;
    }
    let expected = patch.resulting_snapshot()?;
    patch.apply_in_memory(&mut doc)?;
    outcome.part_removal_equal &= doc.snapshot()? == expected;
    let removed_path = temp.join("intent-e2-part-removed.SchLib");
    doc.save(&removed_path)?;
    outcome.part_removal_equal &= SchLib::open(&removed_path)?.snapshot()? == expected;
    match reported_adjustment {
        // The native memo-§3-rule-4 decrement fires on any slot removal
        // while the stored current part is > 1, so the round trip is exact
        // only for current-part-1 components; otherwise the reported
        // adjustment itself is asserted (documented native rule).
        None => outcome.part_removal_equal &= expected == state,
        Some(adjustment) => {
            outcome.part_removal_equal &=
                adjustment.before > 1 && adjustment.after == adjustment.before - 1;
        }
    }
    Ok(outcome)
}

// ── Dump-direction saved-plan pass (step-3 increment C) ─────────────────────

struct DumpOutcome {
    create_applied: bool,
    create_converges: bool,
    generation_skips: usize,
    noop_exit0: bool,
    edit_converges: bool,
    edits_applied: usize,
    comment_preserved: bool,
    crlf_preserved: bool,
    report_entries_exact: bool,
}

/// The directional convergence law over real files: the on-disk source
/// compiles (captured imports) and elaborates against the on-disk library to
/// an EMPTY patch.
fn dump_converges(
    spec_path: &std::path::Path,
    library_path: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let graph = altium_spec_lang::import::capture_imports(spec_path)
        .map_err(|error| format!("capturing '{}': {error}", spec_path.display()))?;
    let compiled = altium_spec_lang::intent::compile_captured_schlib(&graph)
        .map_err(|error| format!("compiling '{}': {error}", spec_path.display()))?;
    let input = SchLib::open(library_path)?.snapshot()?;
    let patch = SchLibPatch::from_intent(&input, &compiled.root)
        .map_err(|error| format!("elaborating '{}': {error}", spec_path.display()))?;
    Ok(patch.is_empty())
}

fn dump_pass(
    path: &std::path::Path,
    temp: &std::path::Path,
) -> Result<DumpOutcome, Box<dyn std::error::Error>> {
    let library_path = temp.join("dump-target.SchLib");
    std::fs::copy(path, &library_path)?;
    let spec_path = temp.join("dump-target.schlib-spec");

    // Pass 1: SourceCreate — plan, serialize, strict-decode, apply.
    let created = plan_dump_schlib(
        &library_path,
        &spec_path,
        Some(&temp.join("dump-plan.json")),
    )
    .map_err(|error| format!("dump planning (create): {error}"))?;
    let generation_skips = created.plan.diagnostics.len();
    if created.exit_code != PLAN_EXIT_CHANGES
        || !matches!(
            created.plan.patches,
            PlanPatches::Dump {
                source: PlanSourcePatch::Create { .. },
                ..
            }
        )
    {
        return Err("first dump plan must be a SourceCreate with changes".into());
    }
    let decoded = SavedPlan::from_json_bytes(&created.plan.to_json_bytes()?)?;
    let library_bytes = std::fs::read(&library_path)?;
    let applied =
        apply_saved_dump_plan(&decoded).map_err(|error| format!("dump apply (create): {error}"))?;
    let create_applied =
        applied.created && spec_path.exists() && std::fs::read(&library_path)? == library_bytes;
    let create_converges = dump_converges(&spec_path, &library_path)?;

    // Pass 2: no-op — the created source already matches.
    let noop = plan_dump_schlib(&library_path, &spec_path, None)
        .map_err(|error| format!("dump planning (no-op): {error}"))?;
    let noop_edits_empty = matches!(
        &noop.plan.patches,
        PlanPatches::Dump {
            source: PlanSourcePatch::Edit { edits, .. },
            ..
        } if edits.is_empty()
    );
    let noop_exit0 = noop.exit_code == PLAN_EXIT_NO_CHANGES && noop_edits_empty;

    // Pass 3: seed a comment, convert to CRLF, and flip one boolean
    // property; the dump edit must correct the value while preserving the
    // seeded comment and the CRLF style byte-for-byte.
    let text = std::fs::read_to_string(&spec_path)?;
    let (flip_from, flip_to) = if text.contains("show_hidden_pins: false") {
        ("show_hidden_pins: false", "show_hidden_pins: true")
    } else {
        ("show_hidden_pins: true", "show_hidden_pins: false")
    };
    if !text.contains(flip_from) {
        return Err("generated source carries no show_hidden_pins property".into());
    }
    let tampered = text
        .replacen(flip_from, flip_to, 1)
        .replacen("component ", "// seeded dump comment\ncomponent ", 1)
        .replace('\n', "\r\n");
    std::fs::write(&spec_path, &tampered)?;
    let edit_plan = plan_dump_schlib(&library_path, &spec_path, None)
        .map_err(|error| format!("dump planning (edit): {error}"))?;
    if edit_plan.exit_code != PLAN_EXIT_CHANGES {
        return Err("edit-bearing dump plan must classify as changes".into());
    }
    let edit_count = match &edit_plan.plan.patches {
        PlanPatches::Dump {
            source: PlanSourcePatch::Edit { edits, .. },
            bundles,
            ..
        } => edits.len() + bundles.len(),
        _ => return Err("edit pass must produce an Edit patch".into()),
    };
    // Exact report: one entry per persisted edit plus one per staged bundle
    // needing a write, all addressed to the spec/bundle artifacts.
    let derived = report(&edit_plan.plan)?;
    let report_entries_exact = derived.entries.len() == edit_count
        && derived
            .entries
            .iter()
            .all(|entry| entry.address.artifact.is_some())
        && !render_plan_report(&derived).is_empty();
    let decoded = SavedPlan::from_json_bytes(&edit_plan.plan.to_json_bytes()?)?;
    let applied =
        apply_saved_dump_plan(&decoded).map_err(|error| format!("dump apply (edit): {error}"))?;
    let edits_applied = applied.edits_applied;
    let merged = std::fs::read_to_string(&spec_path)?;
    let comment_preserved =
        merged.contains("// seeded dump comment\r\n") && merged.contains(flip_from);
    let crlf_preserved = !merged.replace("\r\n", "").contains('\n');
    let edit_converges = dump_converges(&spec_path, &library_path)?;

    Ok(DumpOutcome {
        create_applied,
        create_converges,
        generation_skips,
        noop_exit0,
        edit_converges,
        edits_applied,
        comment_preserved,
        crlf_preserved,
        report_entries_exact,
    })
}
