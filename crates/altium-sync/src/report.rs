//! Exact plan reporting (step-3 increment B; design authority
//! `/tmp/step3/design.md` §3; NEXT.md "Reporting").
//!
//! [`report`] is a pure function of a [`SavedPlan`]: it derives every entry
//! from the embedded patches and typed before/after payloads — never stored
//! in the plan, never an execution input, re-renderable at review and at
//! apply. Child-collection detail is computed through the SAME typed views
//! the planner used ([`ComponentStatePayload::component_view`]): keyed
//! collections pair by key (with occurrence ordinals for duplicates) under
//! the planner's own equivalence — parameters by the native
//! case-insensitive fold (`parameter_key_fold`, so a case-only respelling
//! is a Replace with a `name` field diff, never delete + create), aliases
//! and footprint model names exactly —, positional collections pair by
//! index, `parts_shrink`
//! reports surface their deleted-children lists, and deletes are ALWAYS
//! expanded entries (old index, before values, no new index) — never
//! counts-only. There are NO heuristics: this is the exact indexed diff;
//! display pairing/grouping heuristics are step 9 and, because the report is
//! a derived display, can never change patches or after-states.
//!
//! Field values are typed serializations of the projection views
//! (`serde_json::Value` built from the views' `Serialize` impls), replacing
//! the Debug-string `scalar_changes()` surface for reporting.
//!
//! [`render_plan_report`] is a plain-text renderer for tests and example
//! drivers; no CLI is wired in step 3.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::plan::{
    ManifestRole, PlanComponentPatch, PlanDirection, PlanDomain, PlanError, PlanLibraryOp,
    PlanPartsShrink, PlanPatches, PlanPrecondition, SavedPlan,
};
use altium_format::api::{ComponentStatePayload, SchLibComponentSnapshot};

/// Longest edit-fragment summary rendered inline, in bytes. The cut is moved
/// back to the nearest character boundary, never applied to a raw byte index.
const SUMMARY_BYTE_BUDGET: usize = 120;

/// Collection label of one change entry. Extends the plan schema's
/// collection vocabulary with the library-level namespaces (fonts, image
/// assets) so every executable change has a labeled home. Display-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportCollection {
    /// A dump-direction source file (created or structurally edited).
    Source,
    Components,
    Parts,
    Pins,
    Parameters,
    Aliases,
    Footprints,
    Graphics,
    Maps,
    Fonts,
    ImageAssets,
}

impl ReportCollection {
    pub fn name(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Components => "components",
            Self::Parts => "parts",
            Self::Pins => "pins",
            Self::Parameters => "parameters",
            Self::Aliases => "aliases",
            Self::Footprints => "footprints",
            Self::Graphics => "graphics",
            Self::Maps => "maps",
            Self::Fonts => "fonts",
            Self::ImageAssets => "image_assets",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportOp {
    Create,
    Replace,
    Delete,
}

impl ReportOp {
    pub fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Delete => "delete",
        }
    }
}

/// One field-level before/after pair. `before` alone is a delete expansion,
/// `after` alone a create expansion, both a changed value.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    pub name: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// Where one change entry lives: the component (`None` for library-level
/// namespaces), the owner path inside it where applicable (a footprint for
/// pin-pad maps), the collection, and BOTH retained indices (NEXT.md
/// Reporting): the old index in the before projection and the new index in
/// the after projection.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeAddress {
    /// The written artifact this change belongs to (declared path of the
    /// manifest entry it will be written to). The increment-B compile slice
    /// omitted this dimension (one write artifact); dump plans write several
    /// files, so every entry now names its artifact (review finding B-3).
    pub artifact: Option<String>,
    pub component: Option<String>,
    pub owner_path: Option<String>,
    pub collection: ReportCollection,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
}

/// One expanded executable change.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeEntry {
    pub address: ChangeAddress,
    pub op: ReportOp,
    pub fields: Vec<FieldChange>,
}

/// One manifest artifact as the report presents it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportArtifact {
    pub role: ManifestRole,
    pub declared_path: String,
    pub canonical_path: String,
    /// `"hash"` or `"absent"`.
    pub precondition: &'static str,
    /// The recorded content hash (hex) for hash preconditions — the memo §3
    /// "plan hashes summary" (review finding B-3).
    pub hash: Option<String>,
    pub will_write: bool,
}

/// One summary row: counts per (artifact, collection, op) — the memo §3
/// key, restored for the dump slice (review finding B-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanReportSummaryRow {
    pub artifact: Option<String>,
    pub collection: ReportCollection,
    pub op: ReportOp,
    pub count: usize,
}

/// Native creation ids of one created component, read from its embedded
/// after-state (native output data, not synchronization bindings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocatedIds {
    pub lib_reference: String,
    pub unique_ids: Vec<String>,
}

/// The derived exact report.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanReport {
    pub domain: PlanDomain,
    pub direction: PlanDirection,
    pub schema_version: u32,
    pub state_format_version: u32,
    pub artifacts: Vec<ReportArtifact>,
    pub entries: Vec<ChangeEntry>,
    pub summary: Vec<PlanReportSummaryRow>,
    pub allocated_ids: Vec<AllocatedIds>,
    pub diagnostics: Vec<String>,
}

fn json_error(context: &str) -> impl FnOnce(serde_json::Error) -> PlanError + '_ {
    move |source| PlanError::Json {
        context: context.to_owned(),
        source,
    }
}

fn format_error(context: String) -> impl FnOnce(altium_format::AltiumFormatError) -> PlanError {
    move |source| PlanError::Format { context, source }
}

/// The typed scalar field projection of one component state.
fn scalar_fields(view: &SchLibComponentSnapshot) -> Result<Vec<(&'static str, Value)>, PlanError> {
    let name = view.lib_reference().to_owned();
    let context = format!("projecting scalar fields of component '{name}'");
    Ok(vec![
        (
            "description",
            serde_json::to_value(view.description()).map_err(json_error(&context))?,
        ),
        (
            "designator",
            serde_json::to_value(view.designator().map_err(format_error(context.clone()))?)
                .map_err(json_error(&context))?,
        ),
        (
            "component_kind",
            serde_json::to_value(view.component_kind()).map_err(json_error(&context))?,
        ),
        ("show_hidden_pins", Value::Bool(view.show_hidden_pins())),
        (
            "effective_part_count",
            serde_json::to_value(
                view.effective_part_count()
                    .map_err(format_error(context.clone()))?,
            )
            .map_err(json_error(&context))?,
        ),
    ])
}

/// Serialized child projections of one component state, through the same
/// typed views the planner used. Keyed collections carry their pairing key;
/// positional ones pair by index.
struct ChildProjections {
    /// (exact name, serialized view) — paired by case-folded name +
    /// occurrence (`parameter_key_fold`, the planner's equivalence); the
    /// exact name stays in the serialized view so a respelling surfaces as
    /// a `name` field diff.
    parameters: Vec<(String, Value)>,
    /// alias values — keyed by value + occurrence.
    aliases: Vec<String>,
    /// (model_name, serialized view without maps, serialized maps) — keyed
    /// by model name + occurrence; maps pair positionally per footprint.
    footprints: Vec<(String, Value, Vec<Value>)>,
    /// positional.
    pins: Vec<Value>,
    /// positional.
    graphics: Vec<Value>,
    /// (part number, serialized view) — keyed by part number.
    parts: Vec<(i32, Value)>,
}

fn child_projections(view: &SchLibComponentSnapshot) -> Result<ChildProjections, PlanError> {
    let name = view.lib_reference().to_owned();
    let context = format!("projecting children of component '{name}'");
    let parameters = view
        .parameters()
        .map_err(format_error(context.clone()))?
        .iter()
        .map(|parameter| {
            Ok((
                parameter.name.to_owned(),
                serde_json::to_value(parameter).map_err(json_error(&context))?,
            ))
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let aliases = view.aliases().map(str::to_owned).collect();
    let footprints = view
        .footprint_maps()
        .map_err(format_error(context.clone()))?
        .footprints
        .iter()
        .map(|footprint| {
            let maps = footprint
                .pin_pad_maps
                .iter()
                .map(|map| serde_json::to_value(map).map_err(json_error(&context)))
                .collect::<Result<Vec<_>, PlanError>>()?;
            let mut value = serde_json::to_value(footprint).map_err(json_error(&context))?;
            if let Value::Object(object) = &mut value {
                object.remove("pin_pad_maps");
            }
            Ok((footprint.model_name.to_owned(), value, maps))
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let pins = view
        .pins()
        .iter()
        .map(|pin| serde_json::to_value(pin).map_err(json_error(&context)))
        .collect::<Result<Vec<_>, PlanError>>()?;
    let graphics = view
        .graphics()
        .iter()
        .map(|graphic| serde_json::to_value(graphic).map_err(json_error(&context)))
        .collect::<Result<Vec<_>, PlanError>>()?;
    let parts = view
        .parts()
        .map_err(format_error(context.clone()))?
        .iter()
        .map(|part| {
            Ok((
                part.number,
                serde_json::to_value(part).map_err(json_error(&context))?,
            ))
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    Ok(ChildProjections {
        parameters,
        aliases,
        footprints,
        pins,
        graphics,
        parts,
    })
}

/// Per-key field diff of two serialized views. Non-object values diff as a
/// single `"value"` field.
fn diff_values(before: &Value, after: &Value) -> Vec<FieldChange> {
    match (before, after) {
        (Value::Object(before_map), Value::Object(after_map)) => {
            let mut fields = Vec::new();
            for (key, before_value) in before_map {
                match after_map.get(key) {
                    Some(after_value) if after_value == before_value => {}
                    Some(after_value) => fields.push(FieldChange {
                        name: key.clone(),
                        before: Some(before_value.clone()),
                        after: Some(after_value.clone()),
                    }),
                    None => fields.push(FieldChange {
                        name: key.clone(),
                        before: Some(before_value.clone()),
                        after: None,
                    }),
                }
            }
            for (key, after_value) in after_map {
                if !before_map.contains_key(key) {
                    fields.push(FieldChange {
                        name: key.clone(),
                        before: None,
                        after: Some(after_value.clone()),
                    });
                }
            }
            fields
        }
        _ => {
            if before == after {
                Vec::new()
            } else {
                vec![FieldChange {
                    name: "value".to_owned(),
                    before: Some(before.clone()),
                    after: Some(after.clone()),
                }]
            }
        }
    }
}

/// Expands every field of one serialized view on one side only.
fn one_sided_fields(value: &Value, is_before: bool) -> Vec<FieldChange> {
    let side = |v: Value| {
        if is_before {
            (Some(v), None)
        } else {
            (None, Some(v))
        }
    };
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, v)| {
                let (before, after) = side(v.clone());
                FieldChange {
                    name: key.clone(),
                    before,
                    after,
                }
            })
            .collect(),
        other => {
            let (before, after) = side(other.clone());
            vec![FieldChange {
                name: "value".to_owned(),
                before,
                after,
            }]
        }
    }
}

/// Occurrence-qualified key: pairs the k-th occurrence of a key on the
/// before side with the k-th occurrence on the after side (exact keyed
/// pairing over possibly duplicated keys, no heuristics).
fn occurrence_keys<K: Clone + Eq + std::hash::Hash>(keys: &[K]) -> Vec<(K, usize)> {
    let mut seen: std::collections::HashMap<K, usize> = std::collections::HashMap::new();
    keys.iter()
        .map(|key| {
            let ordinal = seen.entry(key.clone()).or_insert(0);
            let qualified = (key.clone(), *ordinal);
            *ordinal += 1;
            qualified
        })
        .collect()
}

/// Emits exact keyed-diff entries for one collection: matched keys diff
/// field-by-field, unmatched before-keys expand as deletes, unmatched
/// after-keys as creates.
#[allow(clippy::too_many_arguments)]
fn keyed_entries<K: Clone + Eq + std::hash::Hash + Ord>(
    entries: &mut Vec<ChangeEntry>,
    component: &str,
    owner_path: Option<&str>,
    collection: ReportCollection,
    before_keys: &[K],
    before_values: &[Value],
    after_keys: &[K],
    after_values: &[Value],
) {
    let before_qualified = occurrence_keys(before_keys);
    let after_qualified = occurrence_keys(after_keys);
    let after_index: BTreeMap<&(K, usize), usize> = after_qualified
        .iter()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect();
    let before_index: BTreeMap<&(K, usize), usize> = before_qualified
        .iter()
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect();
    let address = |old_index, new_index| ChangeAddress {
        artifact: None,
        component: Some(component.to_owned()),
        owner_path: owner_path.map(str::to_owned),
        collection,
        old_index,
        new_index,
    };
    for (old_index, key) in before_qualified.iter().enumerate() {
        match after_index.get(key) {
            Some(&new_index) => {
                let fields = diff_values(&before_values[old_index], &after_values[new_index]);
                if !fields.is_empty() || old_index != new_index {
                    entries.push(ChangeEntry {
                        address: address(Some(old_index), Some(new_index)),
                        op: ReportOp::Replace,
                        fields,
                    });
                }
            }
            None => entries.push(ChangeEntry {
                address: address(Some(old_index), None),
                op: ReportOp::Delete,
                fields: one_sided_fields(&before_values[old_index], true),
            }),
        }
    }
    for (new_index, key) in after_qualified.iter().enumerate() {
        if !before_index.contains_key(key) {
            entries.push(ChangeEntry {
                address: address(None, Some(new_index)),
                op: ReportOp::Create,
                fields: one_sided_fields(&after_values[new_index], false),
            });
        }
    }
}

/// Emits exact positional-diff entries for one collection: index N pairs
/// with index N; tail surplus expands as deletes/creates.
fn positional_entries(
    entries: &mut Vec<ChangeEntry>,
    component: &str,
    owner_path: Option<&str>,
    collection: ReportCollection,
    before_values: &[Value],
    after_values: &[Value],
) {
    let address = |old_index, new_index| ChangeAddress {
        artifact: None,
        component: Some(component.to_owned()),
        owner_path: owner_path.map(str::to_owned),
        collection,
        old_index,
        new_index,
    };
    let shared = before_values.len().min(after_values.len());
    for index in 0..shared {
        let fields = diff_values(&before_values[index], &after_values[index]);
        if !fields.is_empty() {
            entries.push(ChangeEntry {
                address: address(Some(index), Some(index)),
                op: ReportOp::Replace,
                fields,
            });
        }
    }
    for (index, value) in before_values.iter().enumerate().skip(shared) {
        entries.push(ChangeEntry {
            address: address(Some(index), None),
            op: ReportOp::Delete,
            fields: one_sided_fields(value, true),
        });
    }
    for (index, value) in after_values.iter().enumerate().skip(shared) {
        entries.push(ChangeEntry {
            address: address(None, Some(index)),
            op: ReportOp::Create,
            fields: one_sided_fields(value, false),
        });
    }
}

/// Expands every child of one component state on one side (create or delete
/// expansion of a whole component).
fn one_sided_component_children(
    entries: &mut Vec<ChangeEntry>,
    component: &str,
    projections: &ChildProjections,
    op: ReportOp,
) {
    let is_before = op == ReportOp::Delete;
    let index_pair = |index: usize| {
        if is_before {
            (Some(index), None)
        } else {
            (None, Some(index))
        }
    };
    let mut push =
        |collection: ReportCollection, owner_path: Option<String>, index: usize, value: &Value| {
            let (old_index, new_index) = index_pair(index);
            entries.push(ChangeEntry {
                address: ChangeAddress {
                    artifact: None,
                    component: Some(component.to_owned()),
                    owner_path,
                    collection,
                    old_index,
                    new_index,
                },
                op,
                fields: one_sided_fields(value, is_before),
            });
        };
    for (index, (_, value)) in projections.parts.iter().enumerate() {
        push(ReportCollection::Parts, None, index, value);
    }
    for (index, (_, value)) in projections.parameters.iter().enumerate() {
        push(ReportCollection::Parameters, None, index, value);
    }
    for (index, alias) in projections.aliases.iter().enumerate() {
        push(
            ReportCollection::Aliases,
            None,
            index,
            &Value::String(alias.clone()),
        );
    }
    for (index, value) in projections.pins.iter().enumerate() {
        push(ReportCollection::Pins, None, index, value);
    }
    for (index, value) in projections.graphics.iter().enumerate() {
        push(ReportCollection::Graphics, None, index, value);
    }
    for (index, (model, value, maps)) in projections.footprints.iter().enumerate() {
        push(ReportCollection::Footprints, None, index, value);
        for (map_index, map) in maps.iter().enumerate() {
            push(
                ReportCollection::Maps,
                Some(format!("footprint '{model}'")),
                map_index,
                map,
            );
        }
    }
}

/// Exact child-collection diff between two component states of one
/// component (keyed collections by key, positional by index).
fn component_child_entries(
    entries: &mut Vec<ChangeEntry>,
    component: &str,
    before: &ChildProjections,
    after: &ChildProjections,
) {
    let part_keys = |parts: &[(i32, Value)]| parts.iter().map(|(n, _)| *n).collect::<Vec<_>>();
    let part_values =
        |parts: &[(i32, Value)]| parts.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>();
    keyed_entries(
        entries,
        component,
        None,
        ReportCollection::Parts,
        &part_keys(&before.parts),
        &part_values(&before.parts),
        &part_keys(&after.parts),
        &part_values(&after.parts),
    );
    // Parameters pair by the planner's native equivalence (the ordinal
    // case-insensitive `parameter_key_fold` used at elaboration), never by
    // exact spelling: a case-only respelling is one Replace entry whose
    // `name` field diff shows the respelling, not a delete + create.
    let parameter_keys = |items: &[(String, Value)]| {
        items
            .iter()
            .map(|(k, _)| crate::schlib::parameter_key_fold(k))
            .collect::<Vec<_>>()
    };
    let named_values =
        |items: &[(String, Value)]| items.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>();
    keyed_entries(
        entries,
        component,
        None,
        ReportCollection::Parameters,
        &parameter_keys(&before.parameters),
        &named_values(&before.parameters),
        &parameter_keys(&after.parameters),
        &named_values(&after.parameters),
    );
    keyed_entries(
        entries,
        component,
        None,
        ReportCollection::Aliases,
        &before.aliases,
        &before
            .aliases
            .iter()
            .map(|alias| Value::String(alias.clone()))
            .collect::<Vec<_>>(),
        &after.aliases,
        &after
            .aliases
            .iter()
            .map(|alias| Value::String(alias.clone()))
            .collect::<Vec<_>>(),
    );
    positional_entries(
        entries,
        component,
        None,
        ReportCollection::Pins,
        &before.pins,
        &after.pins,
    );
    positional_entries(
        entries,
        component,
        None,
        ReportCollection::Graphics,
        &before.graphics,
        &after.graphics,
    );
    // Footprints pair by model name (+ occurrence); each pair's pin-pad
    // maps pair positionally under that footprint owner.
    let footprint_keys = |items: &[(String, Value, Vec<Value>)]| {
        items.iter().map(|(k, _, _)| k.clone()).collect::<Vec<_>>()
    };
    let footprint_values = |items: &[(String, Value, Vec<Value>)]| {
        items.iter().map(|(_, v, _)| v.clone()).collect::<Vec<_>>()
    };
    keyed_entries(
        entries,
        component,
        None,
        ReportCollection::Footprints,
        &footprint_keys(&before.footprints),
        &footprint_values(&before.footprints),
        &footprint_keys(&after.footprints),
        &footprint_values(&after.footprints),
    );
    let before_qualified = occurrence_keys(&footprint_keys(&before.footprints));
    let after_qualified = occurrence_keys(&footprint_keys(&after.footprints));
    for (before_position, key) in before_qualified.iter().enumerate() {
        if let Some(after_position) = after_qualified.iter().position(|other| other == key) {
            positional_entries(
                entries,
                component,
                Some(&format!("footprint '{}'", key.0)),
                ReportCollection::Maps,
                &before.footprints[before_position].2,
                &after.footprints[after_position].2,
            );
        }
        // A footprint present on one side only already expanded its delete/
        // create through keyed_entries; its maps expand with it below.
    }
    for (before_position, key) in before_qualified.iter().enumerate() {
        if !after_qualified.contains(key) {
            for (map_index, map) in before.footprints[before_position].2.iter().enumerate() {
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(component.to_owned()),
                        owner_path: Some(format!("footprint '{}'", key.0)),
                        collection: ReportCollection::Maps,
                        old_index: Some(map_index),
                        new_index: None,
                    },
                    op: ReportOp::Delete,
                    fields: one_sided_fields(map, true),
                });
            }
        }
    }
    for (after_position, key) in after_qualified.iter().enumerate() {
        if !before_qualified.contains(key) {
            for (map_index, map) in after.footprints[after_position].2.iter().enumerate() {
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(component.to_owned()),
                        owner_path: Some(format!("footprint '{}'", key.0)),
                        collection: ReportCollection::Maps,
                        old_index: None,
                        new_index: Some(map_index),
                    },
                    op: ReportOp::Create,
                    fields: one_sided_fields(map, false),
                });
            }
        }
    }
}

/// Deleted-children expansion of a parts shrink (never counts-only).
fn parts_shrink_entries(
    entries: &mut Vec<ChangeEntry>,
    component: &str,
    shrink: &PlanPartsShrink,
) -> Result<(), PlanError> {
    let context = format!("reporting parts shrink of component '{component}'");
    for child in &shrink.deleted_children {
        entries.push(ChangeEntry {
            address: ChangeAddress {
                artifact: None,
                component: Some(component.to_owned()),
                owner_path: Some(format!("part {}", child.owner_part_id)),
                collection: ReportCollection::Parts,
                old_index: Some(child.child_index),
                new_index: None,
            },
            op: ReportOp::Delete,
            fields: vec![
                FieldChange {
                    name: "kind".to_owned(),
                    before: Some(serde_json::to_value(child.kind).map_err(json_error(&context))?),
                    after: None,
                },
                FieldChange {
                    name: "owner_part_id".to_owned(),
                    before: Some(Value::from(child.owner_part_id)),
                    after: None,
                },
            ],
        });
    }
    Ok(())
}

/// Collects every non-empty `unique_id` string of a payload's typed
/// serialization, in serialization order (the same field-name convention the
/// determinism normalizer `mask_unique_ids` uses).
fn collect_unique_ids(payload: &ComponentStatePayload) -> Result<Vec<String>, PlanError> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, entry) in map {
                    if key == "unique_id"
                        && let Value::String(id) = entry
                    {
                        if !id.is_empty() {
                            out.push(id.clone());
                        }
                    } else {
                        walk(entry, out);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }
    let value = serde_json::to_value(payload).map_err(json_error(
        "serializing an after-state for allocated-id listing",
    ))?;
    let mut ids = Vec::new();
    walk(&value, &mut ids);
    Ok(ids)
}

/// Derives the exact report from a saved plan. Pure: reads no files,
/// executes nothing.
pub fn report(plan: &SavedPlan) -> Result<PlanReport, PlanError> {
    let artifacts = plan
        .manifest
        .iter()
        .map(|entry| ReportArtifact {
            role: entry.role,
            declared_path: entry.declared_path.clone(),
            canonical_path: entry.canonical_path.clone(),
            precondition: match entry.precondition {
                PlanPrecondition::Hash(_) => "hash",
                PlanPrecondition::Absent => "absent",
            },
            hash: match &entry.precondition {
                PlanPrecondition::Hash(hash) => Some(hash.hex.clone()),
                PlanPrecondition::Absent => None,
            },
            will_write: matches!(
                entry.role,
                ManifestRole::Destination | ManifestRole::InPlaceTarget
            ),
        })
        .collect();

    let mut entries: Vec<ChangeEntry> = Vec::new();
    let mut allocated_ids: Vec<AllocatedIds> = Vec::new();
    match &plan.patches {
        PlanPatches::Dump {
            source,
            bundles,
            expected_state,
            edit_format_version: _,
        } => {
            dump_entries(plan, source, bundles, expected_state, &mut entries)?;
        }
        PlanPatches::Compile {
            library_ops,
            components,
        } => {
            compile_entries(
                plan,
                library_ops,
                components,
                &mut entries,
                &mut allocated_ids,
            )?;
        }
    }

    let mut summary_counts: BTreeMap<(Option<String>, ReportCollection, ReportOp), usize> =
        BTreeMap::new();
    for entry in &entries {
        *summary_counts
            .entry((
                entry.address.artifact.clone(),
                entry.address.collection,
                entry.op,
            ))
            .or_insert(0) += 1;
    }
    let summary = summary_counts
        .into_iter()
        .map(|((artifact, collection, op), count)| PlanReportSummaryRow {
            artifact,
            collection,
            op,
            count,
        })
        .collect();

    Ok(PlanReport {
        domain: plan.domain,
        direction: plan.direction,
        schema_version: plan.schema_version,
        state_format_version: plan.state_format_version,
        artifacts,
        entries,
        summary,
        allocated_ids,
        diagnostics: plan
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect(),
    })
}

/// The compile-direction entry expansion (increment B, unchanged semantics;
/// every entry is stamped with the single write artifact).
fn compile_entries(
    plan: &SavedPlan,
    library_ops: &[PlanLibraryOp],
    components: &[PlanComponentPatch],
    entries: &mut Vec<ChangeEntry>,
    allocated_ids: &mut Vec<AllocatedIds>,
) -> Result<(), PlanError> {
    let entry_floor = entries.len();
    let created_by_name: BTreeMap<&str, &ComponentStatePayload> = components
        .iter()
        .filter_map(|patch| match patch {
            PlanComponentPatch::Create {
                lib_reference,
                after,
            } => Some((lib_reference.as_str(), after)),
            _ => None,
        })
        .collect();
    let mut deleted_inputs: Vec<usize> = components
        .iter()
        .filter_map(|patch| match patch {
            PlanComponentPatch::Delete { address, .. } => Some(address.index),
            _ => None,
        })
        .collect();
    deleted_inputs.sort_unstable();
    // The input slot a kept component's final index corresponds to
    // (creations excluded): final index f skips the deleted input slots.
    let input_slot_of_final = |final_index: usize| -> usize {
        let mut slot = final_index;
        for &deleted in &deleted_inputs {
            if deleted <= slot {
                slot += 1;
            }
        }
        slot
    };
    // A created component's final index, recoverable exactly when the plan
    // pairs the creation with a content replacement.
    let replace_index_of = |name: &str| -> Option<usize> {
        components.iter().find_map(|patch| match patch {
            PlanComponentPatch::Replace { address, .. } if address.lib_reference == name => {
                Some(address.index)
            }
            _ => None,
        })
    };

    // Library-namespace ops.
    for op in library_ops {
        match op {
            PlanLibraryOp::FontAppend(append) => {
                let context = "reporting a font append";
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: None,
                        owner_path: None,
                        collection: ReportCollection::Fonts,
                        old_index: None,
                        new_index: None,
                    },
                    op: ReportOp::Create,
                    fields: one_sided_fields(
                        &serde_json::to_value(crate::plan::PlanFontAppend::clone(append))
                            .map_err(json_error(context))?,
                        false,
                    ),
                });
            }
            PlanLibraryOp::ImageAssetInsert {
                index,
                name,
                payload,
            }
            | PlanLibraryOp::ImageAssetReplace {
                index,
                name,
                payload,
            } => {
                let op_kind = if matches!(op, PlanLibraryOp::ImageAssetInsert { .. }) {
                    ReportOp::Create
                } else {
                    ReportOp::Replace
                };
                // The before side is always `None`, for a Replace too: the
                // plan does not embed the replaced asset's prior body (the
                // store is whole-file-hash protected); only the incoming
                // value is reportable.
                let mut fields = Vec::new();
                for (field, value) in [
                    ("name", Value::String(name.clone())),
                    ("byte_len", Value::from(payload.content_hash.byte_len)),
                    (
                        "content_hash",
                        Value::String(payload.content_hash.hex.clone()),
                    ),
                    ("source_path", Value::String(payload.source_path.clone())),
                ] {
                    fields.push(FieldChange {
                        name: field.to_owned(),
                        before: None,
                        after: Some(value),
                    });
                }
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: None,
                        owner_path: None,
                        collection: ReportCollection::ImageAssets,
                        old_index: (op_kind == ReportOp::Replace).then_some(*index),
                        new_index: Some(*index),
                    },
                    op: op_kind,
                    fields,
                });
            }
            PlanLibraryOp::ImageAssetRemove { index } => {
                // The plan does not embed the removed asset's body; the
                // entry names the exact store slot destroyed.
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: None,
                        owner_path: None,
                        collection: ReportCollection::ImageAssets,
                        old_index: Some(*index),
                        new_index: None,
                    },
                    op: ReportOp::Delete,
                    fields: Vec::new(),
                });
            }
            PlanLibraryOp::AliasAdd { component, alias } => {
                // Alias adds subsumed by a created component's state are
                // reported through that component's Create expansion —
                // VERIFIED, never assumed (mirroring apply's cross-check):
                // a plan whose created after-state does not carry the alias
                // is invalid, not silently under-reported.
                if let Some(payload) = created_by_name.get(component.as_str()) {
                    if !payload.aliases().any(|candidate| candidate == alias) {
                        return Err(PlanError::InvalidPlan {
                            detail: format!(
                                "alias '{alias}' is added to created component \
                                 '{component}', but the embedded after-state does not \
                                 carry it"
                            ),
                        });
                    }
                    continue;
                }
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(component.clone()),
                        owner_path: None,
                        collection: ReportCollection::Aliases,
                        old_index: None,
                        new_index: None,
                    },
                    op: ReportOp::Create,
                    fields: vec![FieldChange {
                        name: "alias".to_owned(),
                        before: None,
                        after: Some(Value::String(alias.clone())),
                    }],
                });
            }
            PlanLibraryOp::AliasRemove { component, alias } => {
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(component.clone()),
                        owner_path: None,
                        collection: ReportCollection::Aliases,
                        old_index: None,
                        new_index: None,
                    },
                    op: ReportOp::Delete,
                    fields: vec![FieldChange {
                        name: "alias".to_owned(),
                        before: Some(Value::String(alias.clone())),
                        after: None,
                    }],
                });
            }
        }
    }

    // Component patches.
    for patch in components {
        match patch {
            PlanComponentPatch::Create {
                lib_reference,
                after,
            } => {
                let view = after.component_view();
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(lib_reference.clone()),
                        owner_path: None,
                        collection: ReportCollection::Components,
                        old_index: None,
                        new_index: replace_index_of(lib_reference),
                    },
                    op: ReportOp::Create,
                    fields: scalar_fields(&view)?
                        .into_iter()
                        .map(|(name, value)| FieldChange {
                            name: name.to_owned(),
                            before: None,
                            after: Some(value),
                        })
                        .collect(),
                });
                let projections = child_projections(&view)?;
                one_sided_component_children(
                    entries,
                    lib_reference,
                    &projections,
                    ReportOp::Create,
                );
                allocated_ids.push(AllocatedIds {
                    lib_reference: lib_reference.clone(),
                    unique_ids: collect_unique_ids(after)?,
                });
            }
            PlanComponentPatch::Replace {
                address,
                before,
                after,
                parts_shrink,
            } => {
                // A created component's content replacement is fully covered
                // by its Create expansion — but only when the two embedded
                // after-states are IDENTICAL, which is verified here rather
                // than assumed: a plan pairing a creation with a divergent
                // replacement would otherwise silently drop an executable
                // change from the report.
                if let Some(created_after) = created_by_name.get(address.lib_reference.as_str()) {
                    if *created_after != after {
                        return Err(PlanError::InvalidPlan {
                            detail: format!(
                                "created component '{}' has a paired content replacement \
                                 whose after-state differs from the creation's embedded \
                                 after-state",
                                address.lib_reference
                            ),
                        });
                    }
                    continue;
                }
                let before_view = before.component_view();
                let after_view = after.component_view();
                let before_scalars = scalar_fields(&before_view)?;
                let after_scalars = scalar_fields(&after_view)?;
                let mut fields: Vec<FieldChange> = before_scalars
                    .into_iter()
                    .zip(after_scalars)
                    .filter(|((_, before_value), (_, after_value))| before_value != after_value)
                    .map(|((name, before_value), (_, after_value))| FieldChange {
                        name: name.to_owned(),
                        before: Some(before_value),
                        after: Some(after_value),
                    })
                    .collect();
                if let Some(shrink) = parts_shrink
                    && let Some(adjustment) = &shrink.current_part_id_adjustment
                {
                    fields.push(FieldChange {
                        name: "current_part_id".to_owned(),
                        before: Some(Value::from(adjustment.before)),
                        after: Some(Value::from(adjustment.after)),
                    });
                }
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(address.lib_reference.clone()),
                        owner_path: None,
                        collection: ReportCollection::Components,
                        old_index: Some(input_slot_of_final(address.index)),
                        new_index: Some(address.index),
                    },
                    op: ReportOp::Replace,
                    fields,
                });
                let before_children = child_projections(&before_view)?;
                let after_children = child_projections(&after_view)?;
                component_child_entries(
                    entries,
                    &address.lib_reference,
                    &before_children,
                    &after_children,
                );
                if let Some(shrink) = parts_shrink {
                    parts_shrink_entries(entries, &address.lib_reference, shrink)?;
                }
            }
            PlanComponentPatch::Delete { address, before } => {
                let view = before.component_view();
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: None,
                        component: Some(address.lib_reference.clone()),
                        owner_path: None,
                        collection: ReportCollection::Components,
                        old_index: Some(address.index),
                        new_index: None,
                    },
                    op: ReportOp::Delete,
                    fields: scalar_fields(&view)?
                        .into_iter()
                        .map(|(name, value)| FieldChange {
                            name: name.to_owned(),
                            before: Some(value),
                            after: None,
                        })
                        .collect(),
                });
                let projections = child_projections(&view)?;
                one_sided_component_children(
                    entries,
                    &address.lib_reference,
                    &projections,
                    ReportOp::Delete,
                );
            }
        }
    }

    // Every compile entry belongs to the plan's single write artifact
    // (declared InPlaceTarget or Destination), now named explicitly on each
    // entry (review finding B-3).
    let write = plan.manifest.iter().find(|entry| {
        matches!(
            entry.role,
            ManifestRole::Destination | ManifestRole::InPlaceTarget
        )
    });
    if let Some(write) = write {
        for entry in entries.iter_mut().skip(entry_floor) {
            entry.address.artifact = Some(write.declared_path.clone());
        }
    }
    Ok(())
}

/// Dump-direction entry expansion (memo §3): per source file, a created
/// file's full-text summary with expected-state counts, or each structured
/// edit with its locator path and fragment summary; staged bundles by
/// hash/length (bodies never render).
fn dump_entries(
    plan: &SavedPlan,
    source: &crate::plan::PlanSourcePatch,
    bundles: &[crate::plan::PlanBundlePatch],
    expected_state: &crate::plan::ExpectedSourceState,
    entries: &mut Vec<ChangeEntry>,
) -> Result<(), PlanError> {
    use crate::plan::PlanSourcePatch;
    let declared_of = |canonical: &str| {
        plan.manifest
            .iter()
            .find(|entry| entry.canonical_path == canonical)
            .map(|entry| entry.declared_path.clone())
            .unwrap_or_else(|| canonical.to_owned())
    };
    let expected_counts = || -> Vec<FieldChange> {
        let components = expected_state.components.len();
        let parameters: usize = expected_state
            .components
            .iter()
            .map(|component| component.parameter_names.len())
            .sum();
        let aliases: usize = expected_state
            .components
            .iter()
            .map(|component| component.alias_values.len())
            .sum();
        let footprints: usize = expected_state
            .components
            .iter()
            .map(|component| component.footprint_models.len())
            .sum();
        let pins: usize = expected_state
            .components
            .iter()
            .map(|component| {
                component.component_scope_pins
                    + component.parts.iter().map(|part| part.pins).sum::<usize>()
            })
            .sum();
        let graphics: usize = expected_state
            .components
            .iter()
            .map(|component| {
                component.component_scope_graphics
                    + component
                        .parts
                        .iter()
                        .map(|part| part.graphics)
                        .sum::<usize>()
            })
            .sum();
        [
            ("expected_components", components),
            ("expected_parameters", parameters),
            ("expected_aliases", aliases),
            ("expected_footprints", footprints),
            ("expected_pins", pins),
            ("expected_graphics", graphics),
        ]
        .into_iter()
        .map(|(name, count)| FieldChange {
            name: name.to_owned(),
            before: None,
            after: Some(Value::from(count)),
        })
        .collect()
    };
    match source {
        PlanSourcePatch::Create {
            destination,
            text,
            result_hash,
        } => {
            let mut fields = vec![
                FieldChange {
                    name: "text_bytes".to_owned(),
                    before: None,
                    after: Some(Value::from(text.len())),
                },
                FieldChange {
                    name: "text_lines".to_owned(),
                    before: None,
                    after: Some(Value::from(text.lines().count())),
                },
                FieldChange {
                    name: "result_hash".to_owned(),
                    before: None,
                    after: Some(Value::String(result_hash.hex.clone())),
                },
            ];
            fields.extend(expected_counts());
            entries.push(ChangeEntry {
                address: ChangeAddress {
                    artifact: Some(declared_of(destination)),
                    component: None,
                    owner_path: None,
                    collection: ReportCollection::Source,
                    old_index: None,
                    new_index: None,
                },
                op: ReportOp::Create,
                fields,
            });
        }
        PlanSourcePatch::Edit {
            source: source_path,
            edits,
            result_hash,
        } => {
            use altium_spec_lang::PlannedSpecEdit;
            let artifact = declared_of(source_path);
            for (index, edit) in edits.iter().enumerate() {
                let summarize = |text: &str| {
                    let first = text.lines().next().unwrap_or_default();
                    if first.len() > SUMMARY_BYTE_BUDGET {
                        // Slicing on a raw byte offset would panic whenever a
                        // multi-byte character straddles the budget; library
                        // text legitimately carries Ω, µ, ° and CJK.
                        let mut cut = SUMMARY_BYTE_BUDGET;
                        while !first.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        format!("{}…", &first[..cut])
                    } else if text.lines().count() > 1 {
                        format!("{first}…")
                    } else {
                        first.to_owned()
                    }
                };
                let (op, locator, fragment) = match edit {
                    PlannedSpecEdit::InsertBlock { parent, block } => (
                        if block.replacement {
                            ReportOp::Replace
                        } else {
                            ReportOp::Create
                        },
                        parent.path_display(),
                        format!("block: {}", summarize(&block.text)),
                    ),
                    PlannedSpecEdit::DeleteBlock { target } => (
                        ReportOp::Delete,
                        target.path_display(),
                        format!("block: {}", summarize(&target.expected_text)),
                    ),
                    PlannedSpecEdit::SetProperty { target, key, value } => (
                        ReportOp::Replace,
                        target.path_display(),
                        format!("{key}: {}", summarize(value)),
                    ),
                    PlannedSpecEdit::RemoveProperty { target, key } => (
                        ReportOp::Delete,
                        target.path_display(),
                        format!("property '{key}'"),
                    ),
                    PlannedSpecEdit::SetAnnotation { target, binding } => (
                        ReportOp::Replace,
                        target.path_display(),
                        format!(
                            "annotation(id: {:?}, stable: {:?}, group: {:?}, source_id: {:?})",
                            binding.id, binding.stable, binding.group, binding.source_id
                        ),
                    ),
                };
                entries.push(ChangeEntry {
                    address: ChangeAddress {
                        artifact: Some(artifact.clone()),
                        component: None,
                        owner_path: Some(format!("node {locator}")),
                        collection: ReportCollection::Source,
                        old_index: Some(index),
                        new_index: Some(index),
                    },
                    op,
                    fields: vec![
                        FieldChange {
                            name: "edit".to_owned(),
                            before: None,
                            after: Some(Value::String(fragment)),
                        },
                        // Keeps the exact post-edit hash visible on every edit
                        // entry without re-rendering the whole file, matching
                        // the created-source entry's result_hash field.
                        FieldChange {
                            name: "result_hash".to_owned(),
                            before: None,
                            after: Some(Value::String(result_hash.hex.clone())),
                        },
                    ],
                });
            }
        }
    }
    for bundle in bundles {
        let declared = declared_of(&bundle.destination);
        let precondition = plan
            .manifest
            .iter()
            .find(|entry| entry.canonical_path == bundle.destination)
            .map(|entry| entry.precondition.clone());
        let op = match precondition {
            Some(PlanPrecondition::Absent) | None => ReportOp::Create,
            Some(PlanPrecondition::Hash(_)) => ReportOp::Replace,
        };
        entries.push(ChangeEntry {
            address: ChangeAddress {
                artifact: Some(declared),
                component: None,
                owner_path: None,
                collection: ReportCollection::ImageAssets,
                old_index: None,
                new_index: None,
            },
            op,
            fields: vec![
                FieldChange {
                    name: "resource_name".to_owned(),
                    before: None,
                    after: Some(Value::String(bundle.resource_name.clone())),
                },
                FieldChange {
                    name: "byte_len".to_owned(),
                    before: None,
                    after: Some(Value::from(bundle.content_hash.byte_len)),
                },
                FieldChange {
                    name: "content_hash".to_owned(),
                    before: None,
                    after: Some(Value::String(bundle.content_hash.hex.clone())),
                },
            ],
        });
    }
    Ok(())
}

/// Plain-text rendering for tests and example drivers (no CLI in step 3).
pub fn render_plan_report(report: &PlanReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "plan report: domain {:?}, direction {:?}, schema v{}, state v{}",
        report.domain, report.direction, report.schema_version, report.state_format_version
    );
    let _ = writeln!(out, "artifacts:");
    for artifact in &report.artifacts {
        let _ = writeln!(
            out,
            "  {:?} {} [{}]{}",
            artifact.role,
            artifact.declared_path,
            artifact.precondition,
            if artifact.will_write { " (write)" } else { "" }
        );
    }
    let _ = writeln!(out, "changes ({}):", report.entries.len());
    for entry in &report.entries {
        let indices = match (entry.address.old_index, entry.address.new_index) {
            (Some(old), Some(new)) if old == new => format!("[{old}]"),
            (Some(old), Some(new)) => format!("[{old} -> {new}]"),
            (Some(old), None) => format!("[{old} -> gone]"),
            (None, Some(new)) => format!("[new {new}]"),
            (None, None) => String::new(),
        };
        let subject = match (&entry.address.component, &entry.address.owner_path) {
            (Some(component), Some(owner)) => format!("{component}/{owner}"),
            (Some(component), None) => component.clone(),
            (None, _) => "library".to_owned(),
        };
        let _ = writeln!(
            out,
            "  {} {} {}{}",
            entry.op.name(),
            entry.address.collection.name(),
            subject,
            indices
        );
        for field in &entry.fields {
            let render = |value: &Option<Value>| match value {
                Some(value) => value.to_string(),
                None => "-".to_owned(),
            };
            let _ = writeln!(
                out,
                "    {}: {} => {}",
                field.name,
                render(&field.before),
                render(&field.after)
            );
        }
    }
    let _ = writeln!(out, "summary:");
    for row in &report.summary {
        let _ = writeln!(
            out,
            "  {} {}: {}",
            row.op.name(),
            row.collection.name(),
            row.count
        );
    }
    if !report.allocated_ids.is_empty() {
        let _ = writeln!(out, "allocated ids:");
        for allocated in &report.allocated_ids {
            let _ = writeln!(
                out,
                "  {}: {}",
                allocated.lib_reference,
                allocated.unique_ids.join(", ")
            );
        }
    }
    for diagnostic in &report.diagnostics {
        let _ = writeln!(out, "note: {diagnostic}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{
        PlanArtifact, PlanArtifactContents, PlanScope, SavedPlan, load_schlib_hashed,
    };
    use crate::schlib::SchLibPatch;
    use altium_format::SchLib;
    use altium_format::api::{SchLibComponentEdit, SchLibEdit, SchNewComponent};
    use altium_spec_lang::ast::{CollectionDelete, CollectionMode};
    use altium_spec_lang::diagnostic::{Span, Spanned};
    use altium_spec_lang::intent::{
        Collection, CollectionScope, ComponentIntent, FieldIntent, INTENT_SCHEMA_VERSION,
        ParameterIntent, PartIntent, PartNumber, SchLibIntent,
    };
    use std::path::{Path, PathBuf};

    fn spanned<T>(value: T) -> Spanned<T> {
        Spanned::new(value, Span::new(0, 0))
    }
    fn authored<T>(value: T) -> FieldIntent<T> {
        FieldIntent::Set(spanned(value))
    }
    fn complete_scope() -> CollectionScope {
        CollectionScope {
            mode: CollectionMode::Complete,
            delete: CollectionDelete::Allow,
            span: Some(Span::new(0, 0)),
        }
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

    /// The apply-test fixture shape: Component_1 grown to two parts plus a
    /// VICTIM clone, on disk, with a create+replace+delete plan against it.
    fn full_plan(temp: &Path) -> SavedPlan {
        let library = SchLib::new_blank_ad26().unwrap();
        let input = library.snapshot().unwrap();
        let cloned = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::New(SchNewComponent::CloneOf {
                        source: 0,
                        lib_reference: "VICTIM".to_owned(),
                    }),
                ]),
                ..Default::default()
            })
            .unwrap();
        let grown = cloned.component(0).unwrap().with_parts_appended(1).unwrap();
        let library = cloned
            .replace_component(0, grown)
            .unwrap()
            .materialize()
            .unwrap();
        let library_path: PathBuf = temp.join("report-base.SchLib");
        library.save(&library_path).unwrap();

        let mut existing = blank_component_intent("Component_1");
        existing.description = authored(Some("reported description".to_owned()));
        existing.parts = Collection {
            scope: complete_scope(),
            items: vec![PartIntent {
                number: spanned(PartNumber::new(1).unwrap()),
                part_swap_group: FieldIntent::Absent,
                pins: Collection::default(),
                graphics: Collection::default(),
            }],
        };
        let mut probe = blank_component_intent("PLAN_PROBE");
        probe.parameters = Collection {
            scope: CollectionScope::default(),
            items: vec![ParameterIntent {
                name: spanned("PLAN_PARAM".to_owned()),
                text: authored("created by the plan".to_owned()),
                is_hidden: FieldIntent::Absent,
            }],
        };
        probe.aliases = Collection {
            scope: CollectionScope::default(),
            items: vec![spanned("PLAN_ALIAS".to_owned())],
        };
        let intent = SchLibIntent {
            schema_version: INTENT_SCHEMA_VERSION,
            components: Collection {
                scope: complete_scope(),
                items: vec![existing, probe],
            },
        };
        let loaded = load_schlib_hashed(&library_path).unwrap();
        let patch = SchLibPatch::from_intent(&loaded.library.snapshot().unwrap(), &intent).unwrap();
        SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            vec![
                PlanArtifact {
                    role: ManifestRole::TargetDocument,
                    declared_path: library_path,
                    contents: PlanArtifactContents::Bytes(loaded.bytes),
                },
                PlanArtifact {
                    role: ManifestRole::Destination,
                    declared_path: temp.join("report-out.SchLib"),
                    contents: PlanArtifactContents::ExpectedAbsent,
                },
            ],
            None,
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn report_expands_deletes_field_diffs_and_allocated_ids() {
        let temp = tempfile::tempdir().unwrap();
        let plan = full_plan(temp.path());
        // The report is derived from the DECODED plan, exactly as review
        // tooling would hold it.
        let plan = SavedPlan::from_json_bytes(&plan.to_json_bytes().unwrap()).unwrap();
        let derived = report(&plan).unwrap();

        // Artifacts mirror the manifest, with write flags.
        assert_eq!(derived.artifacts.len(), 2);
        assert_eq!(derived.artifacts[0].precondition, "hash");
        assert!(!derived.artifacts[0].will_write);
        assert_eq!(derived.artifacts[1].precondition, "absent");
        assert!(derived.artifacts[1].will_write);

        // The deleted component is EXPANDED: a component-level delete entry
        // with old index and before values, plus per-child delete entries
        // (never counts-only).
        let victim_delete = derived
            .entries
            .iter()
            .find(|entry| {
                entry.op == ReportOp::Delete
                    && entry.address.collection == ReportCollection::Components
                    && entry.address.component.as_deref() == Some("VICTIM")
            })
            .expect("expanded VICTIM delete entry");
        assert_eq!(victim_delete.address.old_index, Some(1));
        assert_eq!(victim_delete.address.new_index, None);
        let description = victim_delete
            .fields
            .iter()
            .find(|field| field.name == "description")
            .expect("before description");
        assert!(description.before.is_some() && description.after.is_none());
        assert!(
            derived.entries.iter().any(|entry| {
                entry.op == ReportOp::Delete
                    && entry.address.collection == ReportCollection::Parts
                    && entry.address.component.as_deref() == Some("VICTIM")
            }),
            "VICTIM's part slots expand as delete entries"
        );

        // The replaced component carries field-level before/after pairs.
        let replace = derived
            .entries
            .iter()
            .find(|entry| {
                entry.op == ReportOp::Replace
                    && entry.address.collection == ReportCollection::Components
                    && entry.address.component.as_deref() == Some("Component_1")
            })
            .expect("Component_1 replace entry");
        assert_eq!(replace.address.old_index, Some(0));
        assert_eq!(replace.address.new_index, Some(0));
        let description = replace
            .fields
            .iter()
            .find(|field| field.name == "description")
            .expect("description diff");
        assert_eq!(
            description.after,
            Some(serde_json::json!("reported description"))
        );
        assert!(
            replace
                .fields
                .iter()
                .any(|field| field.name == "effective_part_count"),
            "the 2 -> 1 shrink surfaces as a scalar diff"
        );
        // The removed trailing part appears as a keyed parts delete.
        assert!(
            derived.entries.iter().any(|entry| {
                entry.op == ReportOp::Delete
                    && entry.address.collection == ReportCollection::Parts
                    && entry.address.component.as_deref() == Some("Component_1")
            }),
            "removed part slot expands as a delete entry"
        );

        // The created component expands with after values and its children.
        let create = derived
            .entries
            .iter()
            .find(|entry| {
                entry.op == ReportOp::Create
                    && entry.address.collection == ReportCollection::Components
                    && entry.address.component.as_deref() == Some("PLAN_PROBE")
            })
            .expect("PLAN_PROBE create entry");
        assert_eq!(create.address.old_index, None);
        assert_eq!(
            create.address.new_index,
            Some(1),
            "recovered from the paired content replacement"
        );
        assert!(
            derived.entries.iter().any(|entry| {
                entry.op == ReportOp::Create
                    && entry.address.collection == ReportCollection::Parameters
                    && entry.address.component.as_deref() == Some("PLAN_PROBE")
                    && entry.fields.iter().any(|field| {
                        field.name == "name" && field.after == Some(serde_json::json!("PLAN_PARAM"))
                    })
            }),
            "created parameter expands with after values"
        );
        assert!(
            derived.entries.iter().any(|entry| {
                entry.op == ReportOp::Create
                    && entry.address.collection == ReportCollection::Aliases
                    && entry.address.component.as_deref() == Some("PLAN_PROBE")
            }),
            "created alias expands"
        );

        // Allocated ids come from the created after-state and are exactly
        // the ids the determinism normalizer masks.
        assert_eq!(derived.allocated_ids.len(), 1);
        assert_eq!(derived.allocated_ids[0].lib_reference, "PLAN_PROBE");
        assert!(!derived.allocated_ids[0].unique_ids.is_empty());
        for id in &derived.allocated_ids[0].unique_ids {
            assert!(!id.is_empty());
        }

        // Summary counts agree with the entries.
        let total: usize = derived.summary.iter().map(|row| row.count).sum();
        assert_eq!(total, derived.entries.len());

        // The B-3 revisit (increment C): every compile entry and summary row
        // names the plan's single write artifact, and hash preconditions
        // surface their hex digests in the artifact table.
        let write_path = &derived.artifacts[1].declared_path;
        assert!(
            derived
                .entries
                .iter()
                .all(|entry| entry.address.artifact.as_deref() == Some(write_path.as_str()))
        );
        assert!(
            derived
                .summary
                .iter()
                .all(|row| row.artifact.as_deref() == Some(write_path.as_str()))
        );
        assert!(
            derived.artifacts[0]
                .hash
                .as_ref()
                .is_some_and(|hash| hash.len() == 64)
        );
        assert_eq!(derived.artifacts[1].hash, None);

        // The renderer mentions every key element.
        let text = render_plan_report(&derived);
        for needle in [
            "delete components VICTIM",
            "replace components Component_1",
            "create components PLAN_PROBE",
            "reported description",
            "allocated ids:",
            "PLAN_PROBE:",
        ] {
            assert!(text.contains(needle), "missing '{needle}' in:\n{text}");
        }

        // Pure function: deriving twice yields the identical report.
        assert_eq!(report(&plan).unwrap(), derived);
    }

    /// A case-only parameter respelling pairs under the planner's native
    /// case-insensitive equivalence: ONE Replace entry whose `name` field
    /// diff shows the respelling — never a delete + create pair.
    #[test]
    fn case_respelled_parameter_reports_as_replace_not_delete_create() {
        let temp = tempfile::tempdir().unwrap();
        // A library whose Component_1 carries the parameter "Value".
        let library = SchLib::new_blank_ad26().unwrap();
        let parameter_intent = |name: &str| {
            let mut component = blank_component_intent("Component_1");
            component.parameters = Collection {
                scope: CollectionScope::default(),
                items: vec![ParameterIntent {
                    name: spanned(name.to_owned()),
                    text: authored("10k".to_owned()),
                    is_hidden: FieldIntent::Absent,
                }],
            };
            SchLibIntent {
                schema_version: INTENT_SCHEMA_VERSION,
                components: Collection {
                    scope: CollectionScope::default(),
                    items: vec![component],
                },
            }
        };
        let seed =
            SchLibPatch::from_intent(&library.snapshot().unwrap(), &parameter_intent("Value"))
                .unwrap();
        let library_path = temp.path().join("case-base.SchLib");
        seed.result_snapshot()
            .materialize()
            .unwrap()
            .save(&library_path)
            .unwrap();

        // Author the SAME parameter respelled to "VALUE" (same text).
        let loaded = load_schlib_hashed(&library_path).unwrap();
        let patch = SchLibPatch::from_intent(
            &loaded.library.snapshot().unwrap(),
            &parameter_intent("VALUE"),
        )
        .unwrap();
        let plan = SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            vec![
                PlanArtifact {
                    role: ManifestRole::TargetDocument,
                    declared_path: library_path,
                    contents: PlanArtifactContents::Bytes(loaded.bytes),
                },
                PlanArtifact {
                    role: ManifestRole::Destination,
                    declared_path: temp.path().join("case-out.SchLib"),
                    contents: PlanArtifactContents::ExpectedAbsent,
                },
            ],
            None,
            Vec::new(),
        )
        .unwrap();
        let plan = SavedPlan::from_json_bytes(&plan.to_json_bytes().unwrap()).unwrap();
        let derived = report(&plan).unwrap();

        let parameter_entries: Vec<_> = derived
            .entries
            .iter()
            .filter(|entry| entry.address.collection == ReportCollection::Parameters)
            .collect();
        assert_eq!(
            parameter_entries.len(),
            1,
            "one paired parameter entry, not delete + create:\n{}",
            render_plan_report(&derived)
        );
        let entry = parameter_entries[0];
        assert_eq!(entry.op, ReportOp::Replace);
        // Paired in place: same slot on both sides (the absolute index
        // depends on the blank component's own parameter records).
        assert!(entry.address.old_index.is_some());
        assert_eq!(entry.address.old_index, entry.address.new_index);
        let name_diff = entry
            .fields
            .iter()
            .find(|field| field.name == "name")
            .expect("the respelling surfaces as a name field diff");
        assert_eq!(name_diff.before, Some(serde_json::json!("Value")));
        assert_eq!(name_diff.after, Some(serde_json::json!("VALUE")));
    }
}
