//! Concrete SchLib snapshot-bound patches.
//!
//! A [`SchLibPatch`] binds one immutable typed input snapshot to a reviewed
//! result snapshot. Three constructors exist:
//!
//! - [`SchLibPatch::component_properties`] — the scalar elaboration path over
//!   explicitly addressed component slots (increment-A era surface, kept).
//! - [`SchLibPatch::from_library_ops`] — typed library-level namespace ops
//!   (step-2 increment E1, resolving the D1 deferral): alias add/remove,
//!   image-asset insert/replace/remove (payload via
//!   [`crate::image_asset::CapturedImageAsset`]), font appends, and component
//!   create ([`SchNewComponent::Blank`]/[`SchNewComponent::CloneOf`]) /
//!   remove. Ops lower to exactly ONE [`SchLibEdit`] per patch application
//!   through `SchLibSnapshot::with_library_edit`.
//! - [`SchLibPatch::from_intent`] / [`SchLibPatch::from_intent_with_assets`]
//!   — definition-scope elaboration of a
//!   [`altium_spec_lang::intent::SchLibIntent`]: keyed collections
//!   (components, parameters, aliases, footprints incl. their positional
//!   pin-pad maps), positional per-owner-group pins/graphics with layout and
//!   auto-size evaluation, parts structure (growth, complete-scope trailing
//!   shrink with the typed [`SchLibPartsShrink`] report), part swap-group
//!   fan-out, and image-source graphics resolved against caller-captured
//!   assets ([`SchLibIntentAssets`]; elaboration performs no filesystem
//!   reads), plus the library ops above; see the `elaborate` and
//!   `positional` module docs for the exact contracts.
//!
//! Application order (the D1 two-step composition): the library-level
//! namespace transition applies FIRST as one `SchLibEdit`; component content
//! replacements are captured against the RESULT of that edit (the
//! "intermediate" snapshot with its recomputed reference context) and applied
//! second through `replace_components`. [`ComponentReplacement::index`]
//! therefore addresses the post-library-edit component sequence; when the
//! patch changes no component membership/order this equals the input slot.
//!
//! The reviewed result snapshot is computed ONCE at construction and cached:
//! created components receive fresh `UniqueId`s, so recomputing at apply time
//! would silently diverge from the reviewed state. `apply_in_memory` executes
//! exactly the cached result after verifying the whole-input `StaleInput`
//! precondition and the captured image-asset inputs.
pub(crate) mod dump_source;
mod elaborate;
pub(crate) use elaborate::parameter_key_fold;
pub(crate) mod positional;

use std::path::{Path, PathBuf};

use crate::collection::{
    CollectionContext, CollectionRequest, IndexedEdit, PlannedItem, plan_collection,
};
use crate::image_asset::{CapturedImageAsset, ImageAssetError};
use altium_format::{
    SchLib,
    api::{
        SchAliasOp, SchCurrentPartIdAdjustment, SchFontAppend, SchImageAssetEdit,
        SchLibComponentEdit, SchLibComponentSnapshot, SchLibEdit, SchLibSnapshot, SchNewComponent,
        SchRemovedPartChild,
    },
};
use altium_format_types::ComponentKind;
use altium_spec_lang::intent::{
    COMPONENT_DESIGNATOR, COMPONENT_KIND, FieldIntent, FieldPolicy, OPTIONAL_DESCRIPTION,
};

/// Explicit scope for the supported library-definition scalar properties.
/// Child collections and part-count edits are intentionally separate inputs to
/// the full planner; this type never implies permission to change them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentPropertiesIntent {
    pub description: FieldIntent<Option<String>>,
    pub designator: FieldIntent<Option<String>>,
    pub component_kind: FieldIntent<ComponentKind>,
    pub show_hidden_pins: FieldIntent<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum SchLibPatchError {
    #[error("{context}: {source}")]
    Format {
        context: String,
        #[source]
        source: altium_format::AltiumFormatError,
    },
    #[error("component #{index} {field}: {source}")]
    Intent {
        index: usize,
        field: &'static str,
        #[source]
        source: altium_spec_lang::SpecError,
    },
    #[error("{context}: {source}")]
    Spec {
        context: String,
        #[source]
        source: altium_spec_lang::SpecError,
    },
    #[error("component #{index} is addressed more than once in the same patch")]
    Overlap { index: usize },
    #[error("SchLib typed input state changed since planning; replan")]
    StaleInput,
    #[error("{detail}")]
    Collection { detail: String },
    #[error("{context}: {detail}")]
    Elaboration { context: String, detail: String },
    #[error("{context}: {source}")]
    ImageAsset {
        context: String,
        #[source]
        source: ImageAssetError,
    },
}

/// One complete component after-state addressed in the patch's final
/// (post-library-edit) component sequence, together with the exact before
/// state it replaces. Native snapshot fields remain private to altium-format.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentReplacement {
    index: usize,
    before: SchLibComponentSnapshot,
    after: SchLibComponentSnapshot,
}

/// One scalar field difference between a replacement's before and after
/// states, in Debug-rendered form. This is inspection data for exact-indexed
/// diff reporting, not an execution input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchComponentFieldDelta {
    pub field: &'static str,
    pub before: String,
    pub after: String,
}

impl ComponentReplacement {
    /// Position in the patch's final (post-library-edit) component sequence;
    /// equals the input slot when the patch changes no component
    /// membership/order.
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn before(&self) -> &SchLibComponentSnapshot {
        &self.before
    }
    pub fn after(&self) -> &SchLibComponentSnapshot {
        &self.after
    }

    /// The cheap scalar before/after pairs that differ (description,
    /// designator, component kind, show-hidden-pins). Child-collection
    /// differences are visible through the public projection views on
    /// [`Self::before`]/[`Self::after`]; no rendering is attempted here.
    pub fn scalar_changes(&self) -> Result<Vec<SchComponentFieldDelta>, SchLibPatchError> {
        let mut changes = Vec::new();
        let mut push = |field: &'static str, before: String, after: String| {
            if before != after {
                changes.push(SchComponentFieldDelta {
                    field,
                    before,
                    after,
                });
            }
        };
        push(
            "description",
            format!("{:?}", self.before.description()),
            format!("{:?}", self.after.description()),
        );
        let designator = |component: &SchLibComponentSnapshot| -> Result<String, SchLibPatchError> {
            Ok(format!(
                "{:?}",
                component
                    .designator()
                    .map_err(|source| SchLibPatchError::Format {
                        context: format!(
                            "projecting designator of component '{}' for the scalar diff",
                            component.lib_reference()
                        ),
                        source,
                    })?
            ))
        };
        push(
            "designator",
            designator(&self.before)?,
            designator(&self.after)?,
        );
        push(
            "component_kind",
            format!("{:?}", self.before.component_kind()),
            format!("{:?}", self.after.component_kind()),
        );
        push(
            "show_hidden_pins",
            format!("{:?}", self.before.show_hidden_pins()),
            format!("{:?}", self.after.show_hidden_pins()),
        );
        Ok(changes)
    }
}

/// One alias set change carried by a patch, addressed by component
/// `lib_reference` exactly as the D1 [`SchAliasOp`] surface addresses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchLibAliasChange {
    pub component: String,
    pub alias: String,
}

/// One image-asset store op carried by a patch. The payload is a
/// [`CapturedImageAsset`]: exact reviewed bytes bound to a canonical path and
/// content hash, re-verified (`verify_unchanged`) before every apply.
#[derive(Debug, Clone, PartialEq)]
pub enum SchLibImageAssetOp {
    Insert {
        index: usize,
        name: String,
        asset: CapturedImageAsset,
    },
    Replace {
        index: usize,
        name: String,
        asset: CapturedImageAsset,
    },
    Remove {
        index: usize,
    },
}

impl SchLibImageAssetOp {
    fn verify_asset(&self) -> Result<(), ImageAssetError> {
        match self {
            Self::Insert { asset, .. } | Self::Replace { asset, .. } => asset.verify_unchanged(),
            Self::Remove { .. } => Ok(()),
        }
    }
    fn to_edit(&self) -> SchImageAssetEdit {
        match self {
            Self::Insert { index, name, asset } => SchImageAssetEdit::Insert {
                index: *index,
                name: name.clone(),
                payload: asset.bytes().to_vec(),
            },
            Self::Replace { index, name, asset } => SchImageAssetEdit::Replace {
                index: *index,
                name: name.clone(),
                payload: asset.bytes().to_vec(),
            },
            Self::Remove { index } => SchImageAssetEdit::Remove { index: *index },
        }
    }
}

/// One component removed by the patch, recorded for inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchLibComponentRemoval {
    pub input_index: usize,
    pub lib_reference: String,
}

/// One component's trailing part-slot shrink performed by an intent's
/// complete parts scope with `delete: allow`, surfaced for inspection: the
/// removed slots, every deleted descendant record, and the memo-§3-rule-4
/// `current_part_id` decrement when one occurred (never a silent clamp).
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibPartsShrink {
    /// The component's `lib_reference`.
    pub component: String,
    /// The removed trailing effective slot numbers, ascending.
    pub removed_parts: Vec<i32>,
    /// Every deleted child record, in the pre-shrink child encounter order.
    pub deleted_children: Vec<SchRemovedPartChild>,
    /// The reported `current_part_id` adjustment, when the stored value was
    /// above 1 (the native `DeletePart` decrement).
    pub current_part_id_adjustment: Option<SchCurrentPartIdAdjustment>,
}

/// Caller-captured image-asset bundles for intent elaboration, keyed by the
/// REQUESTED bundle path (declaring source's directory joined with the
/// authored relative path — the same pure path arithmetic
/// [`crate::image_asset::requested_bundle_path`] and capture use).
/// Elaboration never sources bundle bytes from the filesystem — the captured
/// bytes are authoritative, and an authored image source whose requested
/// path has no entry here blocks with context — but it DOES re-verify each
/// required capture's path resolution and content hash against disk
/// ([`CapturedImageAsset::verify_unchanged`]), so bundles must still exist
/// unchanged at plan time.
#[derive(Debug, Clone, Default)]
pub struct SchLibIntentAssets {
    by_requested_path: std::collections::BTreeMap<PathBuf, CapturedImageAsset>,
}

impl SchLibIntentAssets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a captured bundle under its requested path, returning any
    /// previously registered capture for the same path.
    pub fn insert(&mut self, asset: CapturedImageAsset) -> Option<CapturedImageAsset> {
        self.by_requested_path
            .insert(asset.requested_path().to_owned(), asset)
    }

    pub fn get(&self, requested_path: &Path) -> Option<&CapturedImageAsset> {
        self.by_requested_path.get(requested_path)
    }

    pub fn is_empty(&self) -> bool {
        self.by_requested_path.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_requested_path.len()
    }
}

/// One typed library-level op for [`SchLibPatch::from_library_ops`] (the D1
/// deferral surface). Ops apply in the D1 `SchLibEdit` order — font appends,
/// image-asset ops (in the given relative order), component slots, alias
/// removes then adds — regardless of their order in the input vector, except
/// that image-asset ops keep their relative order (each op's index addresses
/// the store as left by the preceding one) and component creations append in
/// the given order.
///
/// `ComponentRemove` is itself the removal authorization: it names the exact
/// component whose complete aggregate is deleted (this imperative op surface
/// has no declarative `delete:` scope). Unlike intent elaboration it does NOT
/// auto-release the removed component's aliases — supply explicit
/// `AliasRemove` ops, exactly as the D1 format contract requires.
#[derive(Debug, Clone, PartialEq)]
pub enum SchLibLibraryOp {
    AliasAdd(SchLibAliasChange),
    AliasRemove(SchLibAliasChange),
    ImageAsset(SchLibImageAssetOp),
    FontAppend(SchFontAppend),
    ComponentCreate(SchNewComponent),
    ComponentRemove { lib_reference: String },
}

/// One slot of the patch's final component sequence.
#[derive(Debug, Clone, PartialEq)]
enum SchLibPatchSlot {
    /// The input component at this index, preserved (content edits, if any,
    /// are separate [`ComponentReplacement`]s).
    Keep(usize),
    /// A newly created component.
    New(SchNewComponent),
}

/// The library-level half of a patch under construction: everything that
/// lowers into the single `SchLibEdit`.
#[derive(Debug, Clone, PartialEq, Default)]
struct PendingLibraryEdit {
    component_slots: Option<Vec<SchLibPatchSlot>>,
    allow_component_removal: bool,
    removed_components: Vec<SchLibComponentRemoval>,
    alias_additions: Vec<SchLibAliasChange>,
    alias_removals: Vec<SchLibAliasChange>,
    image_asset_ops: Vec<SchLibImageAssetOp>,
    font_appends: Vec<SchFontAppend>,
}

impl PendingLibraryEdit {
    fn is_empty(&self) -> bool {
        self.component_slots.is_none()
            && self.alias_additions.is_empty()
            && self.alias_removals.is_empty()
            && self.image_asset_ops.is_empty()
            && self.font_appends.is_empty()
    }

    /// Drops an identity component sequence (all input slots kept in order,
    /// nothing removed or created) so a content-only patch carries
    /// `components: None` and a fully no-op patch stays empty.
    fn normalize_component_slots(&mut self, input_len: usize) {
        if let Some(slots) = &self.component_slots {
            let identity = self.removed_components.is_empty()
                && slots.len() == input_len
                && slots.iter().enumerate().all(
                    |(position, slot)| matches!(slot, SchLibPatchSlot::Keep(i) if *i == position),
                );
            if identity {
                self.component_slots = None;
                self.allow_component_removal = false;
            }
        }
    }

    fn to_edit(&self) -> SchLibEdit {
        SchLibEdit {
            components: self.component_slots.as_ref().map(|slots| {
                slots
                    .iter()
                    .map(|slot| match slot {
                        SchLibPatchSlot::Keep(index) => SchLibComponentEdit::Keep(*index),
                        SchLibPatchSlot::New(new) => SchLibComponentEdit::New(new.clone()),
                    })
                    .collect()
            }),
            alias_ops: self
                .alias_removals
                .iter()
                .map(|change| SchAliasOp::Remove {
                    component: change.component.clone(),
                    alias: change.alias.clone(),
                })
                .chain(self.alias_additions.iter().map(|change| SchAliasOp::Add {
                    component: change.component.clone(),
                    alias: change.alias.clone(),
                }))
                .collect(),
            image_assets: self
                .image_asset_ops
                .iter()
                .map(SchLibImageAssetOp::to_edit)
                .collect(),
            font_appends: self.font_appends.clone(),
            allow_component_removal: self.allow_component_removal,
        }
    }

    /// Applies the single library edit, or returns `None` when the patch has
    /// no library-level content (the component sequence is then the input's).
    fn intermediate(
        &self,
        input: &SchLibSnapshot,
    ) -> Result<Option<SchLibSnapshot>, SchLibPatchError> {
        if self.is_empty() {
            return Ok(None);
        }
        input
            .with_library_edit(self.to_edit())
            .map(Some)
            .map_err(|source| SchLibPatchError::Format {
                context: "applying the patch's library-level namespace transition".into(),
                source,
            })
    }
}

/// Elaborates the four scalar component properties: only authored fields are
/// applied, through their declared [`FieldPolicy`]s; `Absent` never writes,
/// on existing and newly created components alike.
fn elaborate_component_properties(
    index: usize,
    before: &SchLibComponentSnapshot,
    intent: &ComponentPropertiesIntent,
) -> Result<SchLibComponentSnapshot, SchLibPatchError> {
    let mut after = before.clone();
    macro_rules! property {
        ($field:ident, $policy:expr, $setter:ident) => {
            if intent.$field.is_authored() {
                let value = $policy.definition(&intent.$field).map_err(|source| {
                    SchLibPatchError::Intent {
                        index,
                        field: stringify!($field),
                        source,
                    }
                })?;
                after = after
                    .$setter(value)
                    .map_err(|source| SchLibPatchError::Format {
                        context: format!(
                            "elaborating {} of component #{index} '{}'",
                            stringify!($field),
                            before.lib_reference()
                        ),
                        source,
                    })?;
            }
        };
    }
    property!(description, OPTIONAL_DESCRIPTION, with_description);
    property!(designator, COMPONENT_DESIGNATOR, with_designator);
    property!(component_kind, COMPONENT_KIND, with_component_kind);
    property!(
        show_hidden_pins,
        FieldPolicy {
            name: "component.show_hidden_pins",
            default: Some(|| false),
            clear: None,
        },
        with_show_hidden_pins
    );
    Ok(after)
}

/// A checked in-memory patch bound to the entire typed input, including fields
/// outside the edited API projection. File hashes and saved-plan serialization
/// must additionally bind physical input artifacts in the transaction layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibPatch {
    input: SchLibSnapshot,
    /// The reviewed result, computed once at construction (created components
    /// carry fresh `UniqueId`s, so recomputation would diverge from review).
    result: SchLibSnapshot,
    pending: PendingLibraryEdit,
    replacements: Vec<ComponentReplacement>,
    parts_shrinks: Vec<SchLibPartsShrink>,
}

impl SchLibPatch {
    /// Elaborate explicitly addressed description fields on library definitions.
    /// Omission preserves the existing target; explicit inherit/reset uses the
    /// declared definition default. This function does not interpret instance
    /// inheritance or grant scope to other fields or child collections.
    pub fn descriptions(
        input: &SchLibSnapshot,
        fields: impl IntoIterator<Item = (usize, FieldIntent<Option<String>>)>,
    ) -> Result<Self, SchLibPatchError> {
        Self::component_properties(
            input,
            fields.into_iter().map(|(index, description)| {
                (
                    index,
                    ComponentPropertiesIntent {
                        description,
                        ..Default::default()
                    },
                )
            }),
        )
    }

    /// Resolve only explicitly supplied component properties. Absent values
    /// preserve the existing complete aggregate; definition inherit/reset use
    /// declared defaults. This is not a full SchLibIntent or instance planner.
    pub fn component_properties(
        input: &SchLibSnapshot,
        fields: impl IntoIterator<Item = (usize, ComponentPropertiesIntent)>,
    ) -> Result<Self, SchLibPatchError> {
        let mut seen = std::collections::BTreeSet::new();
        let mut edited = Vec::new();
        for (index, intent) in fields {
            if !seen.insert(index) {
                return Err(SchLibPatchError::Overlap { index });
            }
            let before = input
                .component(index)
                .map_err(|source| SchLibPatchError::Format {
                    context: format!("resolving input component #{index}"),
                    source,
                })?;
            let after = elaborate_component_properties(index, &before, &intent)?;
            if after != before {
                edited.push(ComponentReplacement {
                    index,
                    before,
                    after,
                });
            }
        }
        // Reuse the collection planner's original-index assembly. This scalar
        // entry point grants partial replacement only, never collection deletion.
        let collection = plan_collection(
            input.component_count(),
            &altium_spec_lang::intent::CollectionScope::default(),
            CollectionRequest::Partial(
                edited
                    .into_iter()
                    .map(|replacement| IndexedEdit::Replace {
                        index: replacement.index,
                        after: replacement,
                    })
                    .collect(),
            ),
            CollectionContext {
                owner: "SchLib".into(),
                kind: altium_spec_lang::ast::CollectionKind::Components,
            },
        )
        .map_err(|error| SchLibPatchError::Collection {
            detail: error.to_string(),
        })?;
        let mut replacements = Vec::new();
        for item in collection.into_items() {
            match item {
                PlannedItem::Keep { .. } => {} // Preserved by the complete input snapshot.
                PlannedItem::Replace { after, .. } => replacements.push(after),
                PlannedItem::New { .. } => {
                    return Err(SchLibPatchError::Collection {
                        detail: "component-property patch cannot create a component".into(),
                    });
                }
            }
        }
        Self::finish(
            input,
            PendingLibraryEdit::default(),
            None,
            replacements,
            Vec::new(),
        )
    }

    /// Builds a patch from typed library-level namespace ops (see
    /// [`SchLibLibraryOp`]). The whole batch lowers into exactly one
    /// `SchLibEdit`; the result is computed and validated eagerly.
    pub fn from_library_ops(
        input: &SchLibSnapshot,
        ops: impl IntoIterator<Item = SchLibLibraryOp>,
    ) -> Result<Self, SchLibPatchError> {
        let mut pending = PendingLibraryEdit::default();
        let mut creations: Vec<SchNewComponent> = Vec::new();
        let mut removal_names: Vec<String> = Vec::new();
        for (position, op) in ops.into_iter().enumerate() {
            match op {
                SchLibLibraryOp::AliasAdd(change) => pending.alias_additions.push(change),
                SchLibLibraryOp::AliasRemove(change) => pending.alias_removals.push(change),
                SchLibLibraryOp::ImageAsset(op) => {
                    op.verify_asset()
                        .map_err(|source| SchLibPatchError::ImageAsset {
                            context: format!(
                                "library op #{position}: verifying the captured image asset input"
                            ),
                            source,
                        })?;
                    pending.image_asset_ops.push(op);
                }
                SchLibLibraryOp::FontAppend(append) => pending.font_appends.push(append),
                SchLibLibraryOp::ComponentCreate(new) => creations.push(new),
                SchLibLibraryOp::ComponentRemove { lib_reference } => {
                    if removal_names.contains(&lib_reference) {
                        return Err(SchLibPatchError::Elaboration {
                            context: format!("library op #{position}"),
                            detail: format!(
                                "component '{lib_reference}' is removed more than once"
                            ),
                        });
                    }
                    removal_names.push(lib_reference);
                }
            }
        }
        if !creations.is_empty() || !removal_names.is_empty() {
            let names: Vec<&str> = input.component_lib_references().collect();
            let mut removed = Vec::new();
            for name in &removal_names {
                let Some(index) = names.iter().position(|candidate| candidate == name) else {
                    return Err(SchLibPatchError::Elaboration {
                        context: "library component removal".into(),
                        detail: format!("no component '{name}' exists in the input snapshot"),
                    });
                };
                removed.push(SchLibComponentRemoval {
                    input_index: index,
                    lib_reference: name.clone(),
                });
            }
            removed.sort_by_key(|removal| removal.input_index);
            let removed_indices: std::collections::BTreeSet<usize> =
                removed.iter().map(|removal| removal.input_index).collect();
            let mut slots: Vec<SchLibPatchSlot> = (0..names.len())
                .filter(|index| !removed_indices.contains(index))
                .map(SchLibPatchSlot::Keep)
                .collect();
            slots.extend(creations.into_iter().map(SchLibPatchSlot::New));
            // The explicit ComponentRemove op IS the removal authorization on
            // this imperative surface (type docs).
            pending.allow_component_removal = !removed.is_empty();
            pending.removed_components = removed;
            pending.component_slots = Some(slots);
            pending.normalize_component_slots(names.len());
        }
        let intermediate = pending.intermediate(input)?;
        Self::finish(input, pending, intermediate, Vec::new(), Vec::new())
    }

    /// Definition-scope elaboration of an authored `SchLibIntent` with no
    /// captured image assets: an authored image `source:` blocks. See
    /// [`Self::from_intent_with_assets`].
    pub fn from_intent(
        input: &SchLibSnapshot,
        intent: &altium_spec_lang::intent::SchLibIntent,
    ) -> Result<Self, SchLibPatchError> {
        elaborate::from_intent(input, intent, &SchLibIntentAssets::default())
    }

    /// Definition-scope elaboration of an authored `SchLibIntent` (keyed
    /// collections, positional per-owner-group pins/graphics, parts, layout,
    /// swap fan-out, library-level ops) resolving authored image sources
    /// against caller-captured bundles. See the `elaborate` and `positional`
    /// module docs for the complete contract.
    pub fn from_intent_with_assets(
        input: &SchLibSnapshot,
        intent: &altium_spec_lang::intent::SchLibIntent,
        assets: &SchLibIntentAssets,
    ) -> Result<Self, SchLibPatchError> {
        elaborate::from_intent(input, intent, assets)
    }

    /// Validates and seals a patch: computes the reviewed result exactly once.
    fn finish(
        input: &SchLibSnapshot,
        pending: PendingLibraryEdit,
        intermediate: Option<SchLibSnapshot>,
        replacements: Vec<ComponentReplacement>,
        parts_shrinks: Vec<SchLibPartsShrink>,
    ) -> Result<Self, SchLibPatchError> {
        let base = intermediate.as_ref().unwrap_or(input);
        let result = base
            .replace_components(
                replacements
                    .iter()
                    .map(|replacement| (replacement.index, replacement.after.clone())),
            )
            .map_err(|source| SchLibPatchError::Format {
                context: "replacing component slots in the reviewed candidate".into(),
                source,
            })?;
        Ok(Self {
            input: input.clone(),
            result,
            pending,
            replacements,
            parts_shrinks,
        })
    }

    // ── Inspection (the exact-indexed-diff foundation; no rendering) ──────

    /// Component content replacements against the final component sequence.
    pub fn replacements(&self) -> &[ComponentReplacement] {
        &self.replacements
    }
    /// Components the patch creates, in final-sequence order.
    pub fn created_components(&self) -> impl Iterator<Item = &SchNewComponent> {
        self.pending
            .component_slots
            .iter()
            .flatten()
            .filter_map(|slot| match slot {
                SchLibPatchSlot::New(new) => Some(new),
                SchLibPatchSlot::Keep(_) => None,
            })
    }
    /// Components the patch removes (complete aggregates), by input slot.
    pub fn removed_components(&self) -> &[SchLibComponentRemoval] {
        &self.pending.removed_components
    }
    pub fn alias_additions(&self) -> &[SchLibAliasChange] {
        &self.pending.alias_additions
    }
    pub fn alias_removals(&self) -> &[SchLibAliasChange] {
        &self.pending.alias_removals
    }
    pub fn image_asset_ops(&self) -> &[SchLibImageAssetOp] {
        &self.pending.image_asset_ops
    }
    pub fn font_appends(&self) -> &[SchFontAppend] {
        &self.pending.font_appends
    }
    /// Trailing part-slot shrinks performed by intent parts scopes, with the
    /// deleted descendants and any `current_part_id` adjustment expanded.
    pub fn parts_shrinks(&self) -> &[SchLibPartsShrink] {
        &self.parts_shrinks
    }
    /// Whether the patch changes nothing: no replacements, no component
    /// membership/order change, no alias/image/font ops.
    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty() && self.pending.is_empty()
    }

    /// The reviewed result snapshot (cached; identical across calls even for
    /// patches that create components with fresh `UniqueId`s).
    pub fn resulting_snapshot(&self) -> Result<SchLibSnapshot, SchLibPatchError> {
        Ok(self.result.clone())
    }

    // ── Saved-plan lowering surface (crate-internal) ─────────────────────

    /// The complete typed input snapshot the patch was planned against.
    pub(crate) fn input_snapshot(&self) -> &SchLibSnapshot {
        &self.input
    }
    /// The cached reviewed result (borrowed; see [`Self::resulting_snapshot`]).
    pub(crate) fn result_snapshot(&self) -> &SchLibSnapshot {
        &self.result
    }
    /// Whether the final component sequence has the canonical saved-plan
    /// shape: every kept input slot in ascending input order, followed by
    /// all creations. The saved-plan schema records deletions (by input
    /// slot) and creations (in final order) but no keep permutation, so a
    /// plan can faithfully represent exactly the patches with this shape;
    /// lowering hard-errors on any other sequence (`plan.rs`).
    pub(crate) fn component_sequence_is_canonical(&self) -> bool {
        match &self.pending.component_slots {
            None => true,
            Some(slots) => {
                let mut last_keep: Option<usize> = None;
                let mut seen_new = false;
                for slot in slots {
                    match slot {
                        SchLibPatchSlot::Keep(index) => {
                            if seen_new || last_keep.is_some_and(|previous| *index <= previous) {
                                return false;
                            }
                            last_keep = Some(*index);
                        }
                        SchLibPatchSlot::New(_) => seen_new = true,
                    }
                }
                true
            }
        }
    }
    /// Final-sequence positions of the components this patch creates, in
    /// final order (the after-state of each lives in the cached result).
    pub(crate) fn created_component_positions(&self) -> Vec<usize> {
        self.pending
            .component_slots
            .iter()
            .flatten()
            .enumerate()
            .filter_map(|(position, slot)| {
                matches!(slot, SchLibPatchSlot::New(_)).then_some(position)
            })
            .collect()
    }

    /// Execute the reviewed result only after verifying complete typed
    /// before-state and every captured image-asset input. The result was
    /// validated at construction; it is materialized before the swap.
    /// This does not write files or implement a durable filesystem transaction.
    pub fn apply_in_memory(&self, target: &mut SchLib) -> Result<(), SchLibPatchError> {
        let actual = target
            .snapshot()
            .map_err(|source| SchLibPatchError::Format {
                context: "capturing target before applying component patch".into(),
                source,
            })?;
        if actual != self.input {
            return Err(SchLibPatchError::StaleInput);
        }
        for (position, op) in self.pending.image_asset_ops.iter().enumerate() {
            op.verify_asset()
                .map_err(|source| SchLibPatchError::ImageAsset {
                    context: format!("verifying image asset op #{position} input before apply"),
                    source,
                })?;
        }
        let candidate = self
            .result
            .materialize()
            .map_err(|source| SchLibPatchError::Format {
                context: "materializing complete SchLib patch result".into(),
                source,
            })?;
        *target = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_spec_lang::diagnostic::{Span, Spanned};

    fn set(value: &str) -> FieldIntent<Option<String>> {
        FieldIntent::Set(Spanned::new(Some(value.into()), Span::new(0, 1)))
    }

    #[test]
    fn component_properties_survive_native_save_and_replan_without_changes() {
        let mut doc = SchLib::new_blank_ad26().unwrap();
        let input = doc.snapshot().unwrap();
        let properties = ComponentPropertiesIntent {
            description: set("complete property edit"),
            designator: set("R?"),
            component_kind: FieldIntent::Set(Spanned::new(ComponentKind::Jumper, Span::new(0, 1))),
            show_hidden_pins: FieldIntent::Set(Spanned::new(true, Span::new(0, 1))),
        };
        let patch = SchLibPatch::component_properties(&input, [(0, properties.clone())]).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        let component = expected.component(0).unwrap();
        assert_eq!(component.component_kind(), ComponentKind::Jumper);
        assert_eq!(component.designator().unwrap(), Some("R?"));
        assert!(component.show_hidden_pins());
        let changes = patch.replacements()[0].scalar_changes().unwrap();
        let fields: Vec<&str> = changes.iter().map(|change| change.field).collect();
        assert_eq!(
            fields,
            [
                "description",
                "designator",
                "component_kind",
                "show_hidden_pins"
            ]
        );
        patch.apply_in_memory(&mut doc).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("properties.SchLib");
        doc.save(&path).unwrap();
        let reopened = SchLib::open(&path).unwrap().snapshot().unwrap();
        assert_eq!(reopened, expected);
        assert!(
            SchLibPatch::component_properties(&reopened, [(0, properties)])
                .unwrap()
                .is_empty()
        );
        assert!(
            SchLibPatch::component_properties(
                &reopened,
                [(0, ComponentPropertiesIntent::default())]
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn clear_and_reset_only_change_explicit_component_properties() {
        let doc = SchLib::new_blank_ad26().unwrap();
        let original = doc.snapshot().unwrap();
        let authored = SchLibPatch::component_properties(
            &original,
            [(
                0,
                ComponentPropertiesIntent {
                    description: set("retained"),
                    designator: set("X?"),
                    show_hidden_pins: FieldIntent::Set(Spanned::new(true, Span::new(0, 1))),
                    ..Default::default()
                },
            )],
        )
        .unwrap()
        .resulting_snapshot()
        .unwrap();
        let result = SchLibPatch::component_properties(
            &authored,
            [(
                0,
                ComponentPropertiesIntent {
                    designator: FieldIntent::Clear(Span::new(0, 1)),
                    show_hidden_pins: FieldIntent::Reset(Span::new(0, 1)),
                    ..Default::default()
                },
            )],
        )
        .unwrap()
        .resulting_snapshot()
        .unwrap();
        let component = result.component(0).unwrap();
        assert_eq!(component.designator().unwrap(), None);
        assert_eq!(component.description(), Some("retained"));
        assert!(!component.show_hidden_pins());
        let error = SchLibPatch::component_properties(
            &result,
            [(
                0,
                ComponentPropertiesIntent {
                    show_hidden_pins: FieldIntent::Clear(Span::new(0, 1)),
                    ..Default::default()
                },
            )],
        )
        .unwrap_err();
        assert!(error.to_string().contains("show_hidden_pins"), "{error}");
    }

    #[test]
    fn description_patch_materializes_reviewed_state_and_replans_empty() {
        let mut doc = SchLib::new_blank_ad26().unwrap();
        doc.insert_image_resource(0, "shared".into(), vec![1, 0, 255])
            .unwrap();
        let input = doc.snapshot().unwrap();
        let patch = SchLibPatch::descriptions(&input, [(0, set("reviewed description"))]).unwrap();
        assert_eq!(patch.replacements().len(), 1);
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.snapshot().unwrap(), expected);
        assert_eq!(doc.image_resources().next().unwrap().payload, &[1, 0, 255]);
        assert_eq!(
            expected.component(0).unwrap().description(),
            Some("reviewed description")
        );
        assert!(
            SchLibPatch::descriptions(&expected, [(0, set("reviewed description"))])
                .unwrap()
                .is_empty()
        );
        assert!(
            SchLibPatch::descriptions(&expected, [(0, FieldIntent::Absent)])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn edits_outside_description_still_invalidate_patch_atomically() {
        let mut doc = SchLib::new_blank_ad26().unwrap();
        let patch = SchLibPatch::descriptions(&doc.snapshot().unwrap(), [(0, set("new"))]).unwrap();
        doc.insert_image_resource(0, "later".into(), vec![4, 5])
            .unwrap();
        let changed = doc.snapshot().unwrap();
        assert!(matches!(
            patch.apply_in_memory(&mut doc),
            Err(SchLibPatchError::StaleInput)
        ));
        assert_eq!(
            doc.snapshot().unwrap(),
            changed,
            "rejected execution must not mutate target"
        );
    }

    #[test]
    fn conflicting_or_missing_input_slots_error_and_clear_is_explicit() {
        let doc = SchLib::new_blank_ad26().unwrap();
        let input = doc.snapshot().unwrap();
        assert!(matches!(
            SchLibPatch::descriptions(&input, [(0, set("a")), (0, set("b"))]),
            Err(SchLibPatchError::Overlap { index: 0 })
        ));
        assert!(SchLibPatch::descriptions(&input, [(usize::MAX, set("a"))]).is_err());
        let written = SchLibPatch::descriptions(&input, [(0, set("a"))])
            .unwrap()
            .resulting_snapshot()
            .unwrap();
        let cleared =
            SchLibPatch::descriptions(&written, [(0, FieldIntent::Clear(Span::new(0, 1)))])
                .unwrap()
                .resulting_snapshot()
                .unwrap();
        assert_eq!(cleared.component(0).unwrap().description(), None);
    }
}
