//! Definition-scope elaboration of `SchLibIntent` into a [`SchLibPatch`]
//! (step-2 increment E1: keyed collections + library-level ops).
//!
//! # Keyed alignment contract (snapshot-intent proposal, keyed collections)
//!
//! - **Components** are keyed by EXACT `lib_reference`. A matched intent
//!   component updates the destination slot; an unmatched one is a creation
//!   ([`SchNewComponent::Blank`] plus elaborated keyed children); renames are
//!   never inferred — a new name is a create, an omitted name is a remove
//!   (complete scope + `delete: allow` only, lowered to the D1 removal with
//!   the authorization flag; the removal covers the aggregate, so the removed
//!   component's aliases are released in the same library edit).
//! - **Parameters** are keyed by name, compared CASE-INSENSITIVELY: AD26
//!   resolves component parameters with `StringComparison.OrdinalIgnoreCase`
//!   (`SchDataContainer.GetParameterByName`,
//!   `Altium.Sch.DataModel.Objects/SchDataContainer.cs:633-647`), so two
//!   names differing only in case are one native parameter. Our fold is
//!   per-character simple uppercase (ordinal, no locale). The AUTHORED
//!   spelling is written on update, so a case-variant intent name is a real
//!   (byte-level) edit that re-elaborates to a no-op afterwards. This holds
//!   for a BARE mention too (an item authoring no fields at all): the item
//!   still writes its authored spelling, so a case-variant bare mention is
//!   a rename — idempotent, and reviewable via `replacements()`, but never
//!   silently ignored; mirror-intent generators must emit destination
//!   spellings to avoid spurious renames.
//! - **Aliases** are keyed by exact alias string. An alias has no fields, so
//!   partial scope only appends; complete scope defines membership. Order of
//!   surviving aliases keeps destination order and additions append —
//!   AD26 itself stores aliases as a sorted SET (D1's recorded deviation), so
//!   alias order carries no native meaning and authored order is not honored
//!   beyond append order.
//! - **Footprints** are keyed by exact `model_name`.
//!   `FootprintIntent::source` is provenance only and NEVER a matching key.
//!   `explicit_maps == false` leaves the definers unaddressed
//!   (`pin_pad_maps: None` → preserved verbatim); an explicit maps body
//!   elaborates positionally through the planner (partial = update indices
//!   `0..N-1` + append; complete = final list, deletion under `delete: allow`).
//!   `is_current` is not intent-expressible: matched footprints preserve the
//!   stored value; a created footprint is current only when the destination
//!   PCBLIB collection was empty and it is the first authored one (the native
//!   sole-footprint-must-be-current rule). Consequently a complete
//!   replacement that removes a coherent component's current footprint while
//!   keeping or creating others hard-errors in the format's coherence guard
//!   — resolve by keeping the current footprint, or by emptying the
//!   collection first (complete + `delete: allow`, zero entries) and
//!   authoring the new set in a follow-up patch against the then-empty
//!   collection (E1 staging restriction; single-patch whole-set replacement
//!   is not expressible).
//!   Native MapDefiner records may carry SEVERAL pads while a pin-pad
//!   intent pair expresses exactly one, so an authored maps entry that
//!   positionally addresses a multi-pad destination definer is an E1 hard
//!   error — never a silent narrowing. Unaddressed tail definers are
//!   preserved with all their pads, and complete + `delete: allow` with a
//!   shorter authored list removes a multi-pad definer explicitly.
//!
//! Duplicate keys on EITHER side (under the collection's matching
//! equivalence) block the managed collection with both locations reported.
//! Unmanaged collections (no authored items and no authored scope
//! declaration) are untouched, so wild duplicate keys in preserved content
//! never block.
//!
//! # No-op detection
//!
//! Every elaborated child equal to its destination state produces no edit; a
//! collection whose plan keeps every original slot in place produces no
//! batch; a component whose scalars and collections all no-op produces no
//! replacement; a patch with no replacements, membership changes, or library
//! ops is empty. Applying a patch and re-elaborating the same intent
//! therefore yields an empty patch.
//!
//! # Programmatic scope validation
//!
//! `from_intent` is a public API accepting hand-built intents, so every
//! collection scope in the authored tree must be self-consistent before
//! elaboration: a non-default mode/delete without a declaration span (the
//! compiler stamps one on every authored declaration, and [`managed`] keys
//! off it) and the contract-illegal `partial` + `delete: allow` combination
//! are hard errors — a non-default scope value is authored intent and is
//! never silently dropped.
//!
//! # Positional children, parts, layout, swap fan-out, image sources (E2)
//!
//! Pins and graphics elaborate POSITIONALLY per owner group (contracts memo
//! §1/§2), parts grow/shrink structurally with `part_count` through its
//! contextual policy, pin placements and auto-sized graphics evaluate through
//! `altium_spec_lang::intent::evaluate_layout` with effective geometry
//! resolved authored-else-destination-else-default, `PartIntent::
//! part_swap_group` fans out per memo §5, and authored image sources lower to
//! whole-resource asset ops applied in the SAME library edit that precedes
//! component content (so the created/replaced asset is in the reference
//! context the child batch validates against). See the `positional` module
//! docs for the complete owner-group contract. Created components elaborate
//! all of this against the Blank factory shape — the E1 staging restrictions
//! are gone.
//!
//! Image-source bundles are never SOURCED from the filesystem here: the
//! caller captures them (`CapturedImageAsset::capture`) and passes them
//! through [`super::SchLibIntentAssets`], keyed by the requested bundle path;
//! an authored source without a provided capture blocks, and the captured
//! bytes are authoritative. Elaboration DOES re-verify each required
//! capture's path resolution and content hash against disk
//! (`CapturedImageAsset::verify_unchanged`), so the bundle must still exist
//! unchanged at plan time — a deleted or retargeted bundle blocks with
//! replan context rather than planning against stale bytes. Source
//! `clear`/`reset` never deletes an asset — asset removal stays an explicit
//! library op.
use std::collections::BTreeMap;

use altium_format::api::{
    SchChildBatch, SchFootprintEdit, SchFootprintInit, SchFootprintMapView,
    SchLibComponentSnapshot, SchLibSnapshot, SchNewComponent, SchParameterEdit, SchParameterInit,
    SchPinPadMapInit,
};
use altium_spec_lang::ast::{CollectionDelete, CollectionKind, CollectionMode};
use altium_spec_lang::diagnostic::Spanned;
use altium_spec_lang::intent::{
    Collection, ComponentIntent, FootprintIntent, HIDDEN, INTENT_SCHEMA_VERSION,
    OPTIONAL_DESCRIPTION, PARAMETER_TEXT, ParameterIntent, PinPadIntent, SchLibIntent,
};

use crate::collection::{
    CollectionContext, CollectionError, CollectionRequest, FinalItem, IndexedEdit, PlannedItem,
    plan_collection,
};
use crate::image_asset::requested_bundle_path;

use super::positional;
use super::{
    ComponentPropertiesIntent, ComponentReplacement, PendingLibraryEdit, SchLibAliasChange,
    SchLibComponentRemoval, SchLibImageAssetOp, SchLibIntentAssets, SchLibPartsShrink, SchLibPatch,
    SchLibPatchError, SchLibPatchSlot, elaborate_component_properties,
};

fn collection_error(error: CollectionError) -> SchLibPatchError {
    SchLibPatchError::Collection {
        detail: error.to_string(),
    }
}

fn format_error(
    context: String,
) -> impl FnOnce(altium_format::AltiumFormatError) -> SchLibPatchError {
    move |source| SchLibPatchError::Format { context, source }
}

/// Whether an authored collection manages its destination: authored items or
/// an authored scope declaration. Unmanaged collections are untouched.
/// [`validate_scope`] runs first on every collection, so a non-default scope
/// without a span can never reach this test and be silently dropped.
fn managed<T>(collection: &Collection<T>) -> bool {
    !collection.items.is_empty() || collection.scope.span.is_some()
}

/// Rejects an inconsistent hand-built scope (`from_intent` is a public API):
/// the compiler stamps a span on every authored scope declaration — which
/// [`managed`] keys off — and rejects `partial` + `delete: allow`
/// (snapshot-intent: the only legal combination with `delete: allow` is
/// `mode: complete`). A non-default scope value is authored intent, so a
/// span-less non-default scope hard-errors instead of being silently ignored.
fn validate_scope(
    owner: &str,
    collection: CollectionKind,
    scope: &altium_spec_lang::intent::CollectionScope,
) -> Result<(), SchLibPatchError> {
    let default_values =
        scope.mode == CollectionMode::Partial && scope.delete == CollectionDelete::Deny;
    if scope.span.is_none() && !default_values {
        return Err(SchLibPatchError::Elaboration {
            context: format!("{owner} {collection:?} scope"),
            detail: format!(
                "inconsistent programmatic scope: non-default mode/delete ({:?}, {:?}) \
                 without a declaration span; stamp a span on every authored scope \
                 declaration (the compiler always does) so the collection is managed",
                scope.mode, scope.delete
            ),
        });
    }
    if scope.mode == CollectionMode::Partial && scope.delete == CollectionDelete::Allow {
        return Err(SchLibPatchError::Elaboration {
            context: format!("{owner} {collection:?} scope"),
            detail: "the only legal combination with delete: allow is mode: complete \
                     (partial scope cannot express removal)"
                .into(),
        });
    }
    Ok(())
}

/// The native parameter-name equivalence: ordinal case-insensitive
/// (`StringComparison.OrdinalIgnoreCase` in `GetParameterByName`,
/// SchDataContainer.cs:633-647), approximated by per-character simple
/// uppercase (characters whose uppercasing expands to multiple characters
/// stay themselves, matching the ordinal per-code-unit fold).
fn parameter_key_fold(name: &str) -> String {
    name.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => single,
                _ => c,
            }
        })
        .collect()
}

/// Blocks duplicate keys on either side of a managed keyed collection,
/// reporting both locations. `fold` is the collection's matching equivalence.
fn keyed_index<'a>(
    owner: &str,
    collection: CollectionKind,
    side: &str,
    keys: impl Iterator<Item = &'a str>,
    fold: fn(&str) -> String,
) -> Result<BTreeMap<String, usize>, SchLibPatchError> {
    let mut by_key: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for (position, key) in keys.enumerate() {
        if let Some((previous, previous_key)) = by_key.insert(fold(key), (position, key.to_owned()))
        {
            return Err(SchLibPatchError::Elaboration {
                context: format!("{owner} {collection:?}"),
                detail: format!(
                    "{side} entries #{previous} '{previous_key}' and #{position} '{key}' \
                     collide under the collection's matching equivalence; keyed alignment \
                     is blocked"
                ),
            });
        }
    }
    Ok(by_key
        .into_iter()
        .map(|(key, (position, _))| (key, position))
        .collect())
}

fn exact(name: &str) -> String {
    name.to_owned()
}

pub(super) fn from_intent(
    input: &SchLibSnapshot,
    intent: &SchLibIntent,
    assets: &SchLibIntentAssets,
) -> Result<SchLibPatch, SchLibPatchError> {
    if intent.schema_version != INTENT_SCHEMA_VERSION {
        return Err(SchLibPatchError::Elaboration {
            context: "SchLib intent".into(),
            detail: format!(
                "schema version {} is not the supported version {INTENT_SCHEMA_VERSION}",
                intent.schema_version
            ),
        });
    }
    // Scope self-consistency over the whole authored tree, before any
    // elaboration: see `validate_scope`.
    validate_scope(
        "SchLib",
        CollectionKind::Components,
        &intent.components.scope,
    )?;
    for component in &intent.components.items {
        let owner = format!("component '{}'", component.name.node);
        validate_scope(&owner, CollectionKind::Pins, &component.pins.scope)?;
        validate_scope(&owner, CollectionKind::Graphics, &component.graphics.scope)?;
        validate_scope(&owner, CollectionKind::Parts, &component.parts.scope)?;
        validate_scope(
            &owner,
            CollectionKind::Parameters,
            &component.parameters.scope,
        )?;
        validate_scope(&owner, CollectionKind::Aliases, &component.aliases.scope)?;
        validate_scope(
            &owner,
            CollectionKind::Footprints,
            &component.footprints.scope,
        )?;
        for item in &component.footprints.items {
            validate_scope(
                &format!("{owner} footprint '{}'", item.model_name.node),
                CollectionKind::Maps,
                &item.maps.scope,
            )?;
        }
        for part in &component.parts.items {
            let part_owner = format!("{owner} part {}", part.number.node.get());
            validate_scope(&part_owner, CollectionKind::Pins, &part.pins.scope)?;
            validate_scope(&part_owner, CollectionKind::Graphics, &part.graphics.scope)?;
        }
    }
    let destination_names: Vec<String> = input
        .component_lib_references()
        .map(str::to_owned)
        .collect();
    // Components are keyed by exact lib_reference. Destination duplicates
    // cannot survive snapshot capture (library invariants enforce unique lib
    // references), but the guard stays for defense in depth.
    let by_name = keyed_index(
        "SchLib",
        CollectionKind::Components,
        "destination",
        destination_names.iter().map(String::as_str),
        exact,
    )?;
    keyed_index(
        "SchLib",
        CollectionKind::Components,
        "authored",
        intent
            .components
            .items
            .iter()
            .map(|ci| ci.name.node.as_str()),
        exact,
    )?;

    // Class-aware planning over the components collection.
    let scope = &intent.components.scope;
    let request = match scope.mode {
        CollectionMode::Partial => CollectionRequest::Partial(
            intent
                .components
                .items
                .iter()
                .map(|ci| match by_name.get(ci.name.node.as_str()) {
                    Some(&index) => IndexedEdit::Replace { index, after: ci },
                    None => IndexedEdit::Insert {
                        before: destination_names.len(),
                        after: ci,
                    },
                })
                .collect(),
        ),
        CollectionMode::Complete => CollectionRequest::Complete(
            intent
                .components
                .items
                .iter()
                .map(|ci| match by_name.get(ci.name.node.as_str()) {
                    Some(&index) => FinalItem::Replace { index, after: ci },
                    None => FinalItem::New { after: ci },
                })
                .collect(),
        ),
    };
    let plan = plan_collection(
        destination_names.len(),
        scope,
        request,
        CollectionContext {
            owner: "SchLib".into(),
            kind: CollectionKind::Components,
        },
    )
    .map_err(collection_error)?;

    // Removals (complete + delete: allow only; the planner enforced that).
    // The removal authorization covers the complete aggregate, aliases
    // included: release each removed component's aliases in the same library
    // edit so the D1 dangling-redirection check cannot fire.
    let mut pending = PendingLibraryEdit::default();
    for index in plan.removed() {
        let input_index = index.get();
        let lib_reference = destination_names[input_index].clone();
        let component = input.component(input_index).map_err(format_error(format!(
            "resolving removed input component #{input_index} '{lib_reference}'"
        )))?;
        for alias in component.aliases() {
            pending.alias_removals.push(SchLibAliasChange {
                component: lib_reference.clone(),
                alias: alias.to_owned(),
            });
        }
        pending.removed_components.push(SchLibComponentRemoval {
            input_index,
            lib_reference,
        });
    }
    pending.allow_component_removal = !pending.removed_components.is_empty();

    // Final component sequence + per-final-position elaboration work.
    let mut slots = Vec::with_capacity(plan.items().len());
    let mut work: Vec<(usize, Option<usize>, &ComponentIntent)> = Vec::new();
    for (position, item) in plan.items().iter().enumerate() {
        match item {
            PlannedItem::Keep { index } => slots.push(SchLibPatchSlot::Keep(index.get())),
            PlannedItem::Replace { index, after } => {
                slots.push(SchLibPatchSlot::Keep(index.get()));
                work.push((position, Some(index.get()), after));
            }
            PlannedItem::New { after } => {
                slots.push(SchLibPatchSlot::New(SchNewComponent::Blank {
                    lib_reference: after.name.node.clone(),
                }));
                work.push((position, None, after));
            }
        }
    }
    pending.component_slots = Some(slots);
    pending.normalize_component_slots(destination_names.len());

    // Alias ops resolve against INPUT alias state (created components start
    // with none); they land in the same single library edit.
    for (_, matched, ci) in &work {
        let destination: Vec<String> = match matched {
            Some(index) => input
                .component(*index)
                .map_err(format_error(format!(
                    "resolving input component #{index} '{}' for alias elaboration",
                    ci.name.node
                )))?
                .aliases()
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        };
        elaborate_aliases(&ci.name.node, &destination, &ci.aliases, &mut pending)?;
    }

    // Authored image sources lower to whole-resource asset ops carried by
    // the SAME library edit that precedes component content, so the child
    // batch validates its embedded references against a context that already
    // holds the asset (memo §8c rule 4; the D1 two-step composition folded
    // into one patch).
    collect_image_asset_ops(input, &work, assets, &mut pending)?;

    // D1 two-step order: the ONE library edit first, then component content
    // against the result (its recomputed reference context).
    let intermediate = pending.intermediate(input)?;
    let base = intermediate.as_ref().unwrap_or(input);
    let mut replacements = Vec::new();
    let mut parts_shrinks = Vec::new();
    for (position, matched, ci) in &work {
        let before = base.component(*position).map_err(format_error(format!(
            "resolving component slot #{position} '{}' after the library edit",
            ci.name.node
        )))?;
        let (after, shrink) =
            elaborate_component_content(&before, ci, *position, matched.is_none())?;
        if let Some(shrink) = shrink {
            parts_shrinks.push(shrink);
        }
        if after != before {
            replacements.push(ComponentReplacement {
                index: *position,
                before,
                after,
            });
        }
    }
    SchLibPatch::finish(input, pending, intermediate, replacements, parts_shrinks)
}

/// The image-asset pre-pass: resolves every authored image source to its
/// captured bytes and storage resource name, resolves each name to its
/// concrete pairing target (the stored asset it addresses, or a new insert
/// slot), dedupes PER TARGET (conflicting bodies for one target block — the
/// pairing target, not the name spelling, is the resource identity), skips
/// ops whose stored payload already equals the captured bundle
/// (replan-empty), and queues the rest as ordered insert/replace ops on the
/// pending library edit. Never removes an asset.
fn collect_image_asset_ops(
    input: &SchLibSnapshot,
    work: &[(usize, Option<usize>, &ComponentIntent)],
    assets: &SchLibIntentAssets,
    pending: &mut PendingLibraryEdit,
) -> Result<(), SchLibPatchError> {
    struct AuthoredAsset {
        context: String,
        /// The authored resource-name spelling (a new insert's stored name).
        name: String,
        asset: crate::image_asset::CapturedImageAsset,
    }
    /// One storage slot the patch writes: an existing stored asset (by input
    /// index) or a not-yet-stored insert (by ASCII-case-folded name — the
    /// native pairing is case-insensitive, so case-variant spellings of one
    /// new resource address one slot, never two).
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Target {
        Stored(usize),
        New(String),
    }
    let mut authored_assets: Vec<AuthoredAsset> = Vec::new();
    for (_, matched, ci) in work {
        let destination = matched
            .map(|index| {
                input.component(index).map_err(format_error(format!(
                    "resolving input component #{index} '{}' for image-source discovery",
                    ci.name.node
                )))
            })
            .transpose()?;
        let authored =
            positional::collect_authored_image_sources(&ci.name.node, destination.as_ref(), ci)?;
        for source in authored {
            let requested = requested_bundle_path(source.source).map_err(|error| {
                SchLibPatchError::ImageAsset {
                    context: source.context.clone(),
                    source: error,
                }
            })?;
            let asset = assets
                .get(&requested)
                .ok_or_else(|| SchLibPatchError::Elaboration {
                    context: source.context.clone(),
                    detail: format!(
                        "no captured image asset was provided for source '{}' (requested \
                         bundle path '{}'); capture it with CapturedImageAsset::capture and \
                         pass it via SchLibIntentAssets — elaboration never sources bundle \
                         bytes from the filesystem",
                        source.source.path,
                        requested.display()
                    ),
                })?;
            authored_assets.push(AuthoredAsset {
                context: source.context,
                name: source.resource_name,
                asset: asset.clone(),
            });
        }
    }
    if authored_assets.is_empty() {
        return Ok(());
    }
    let stored: Vec<(String, Vec<u8>)> = input
        .image_assets()
        .map(|resource| (resource.name.to_owned(), resource.payload.to_vec()))
        .collect();
    let mut requirements: BTreeMap<Target, AuthoredAsset> = BTreeMap::new();
    for entry in authored_assets {
        // The native pairing order: exact name first, then the first
        // case-insensitive match; an unmatched name inserts under its folded
        // identity so case-variant spellings collide HERE instead of landing
        // as silently self-overwriting duplicate ops on one slot.
        let target = stored
            .iter()
            .position(|(stored_name, _)| *stored_name == entry.name)
            .or_else(|| {
                stored
                    .iter()
                    .position(|(stored_name, _)| stored_name.eq_ignore_ascii_case(&entry.name))
            })
            .map(Target::Stored)
            .unwrap_or_else(|| Target::New(entry.name.to_ascii_lowercase()));
        match requirements.get(&target) {
            Some(existing)
                if existing.asset.canonical_path() != entry.asset.canonical_path()
                    || existing.asset.content_hash() != entry.asset.content_hash() =>
            {
                let slot = match &target {
                    Target::Stored(index) => {
                        format!("pairing stored image asset #{index} '{}'", stored[*index].0)
                    }
                    Target::New(_) => format!("inserting new image asset '{}'", existing.name),
                };
                return Err(SchLibPatchError::Elaboration {
                    context: entry.context,
                    detail: format!(
                        "storage resource '{}' ({slot}) receives conflicting bodies: {} \
                         (resource '{}') supplies '{}' while this entry supplies '{}'; one \
                         storage slot takes exactly one asset per patch",
                        entry.name,
                        existing.context,
                        existing.name,
                        existing.asset.canonical_path().display(),
                        entry.asset.canonical_path().display(),
                    ),
                });
            }
            // The same body authored again (any spelling): one slot, one op.
            Some(_) => {}
            None => {
                requirements.insert(target, entry);
            }
        }
    }
    let mut inserts = 0usize;
    for (target, requirement) in requirements {
        requirement
            .asset
            .verify_unchanged()
            .map_err(|source| SchLibPatchError::ImageAsset {
                context: format!(
                    "{}: verifying the captured image asset input",
                    requirement.context
                ),
                source,
            })?;
        match target {
            Target::Stored(index) => {
                if stored[index].1 == requirement.asset.bytes() {
                    // Identical payload already stored: a no-op source.
                    continue;
                }
                pending.image_asset_ops.push(SchLibImageAssetOp::Replace {
                    index,
                    // Whole-resource payload replacement under the STORED
                    // name; renames are not inferred from record spellings.
                    name: stored[index].0.clone(),
                    asset: requirement.asset,
                });
            }
            Target::New(_) => {
                pending.image_asset_ops.push(SchLibImageAssetOp::Insert {
                    index: stored.len() + inserts,
                    // The first-authored spelling names the new resource.
                    name: requirement.name,
                    asset: requirement.asset,
                });
                inserts += 1;
            }
        }
    }
    Ok(())
}

/// Elaborates one component's scalar properties, parts structure, and child
/// collections (keyed and positional) against its (post-library-edit) before
/// state. Order: scalars → part growth/shrink (so child owners validate
/// against the resulting count, and shrink-doomed children leave every
/// surviving group's filtered projection untouched) → ONE child batch over
/// all four collections. A trailing shrink's typed report is returned for
/// `SchLibPatch::parts_shrinks`.
fn elaborate_component_content(
    before: &SchLibComponentSnapshot,
    intent: &ComponentIntent,
    slot: usize,
    created: bool,
) -> Result<(SchLibComponentSnapshot, Option<SchLibPartsShrink>), SchLibPatchError> {
    let name = before.lib_reference().to_owned();
    let properties = ComponentPropertiesIntent {
        description: intent.description.clone(),
        designator: intent.designator.clone(),
        component_kind: intent.component_kind.clone(),
        show_hidden_pins: intent.show_hidden_pins.clone(),
    };
    let mut after = elaborate_component_properties(slot, before, &properties)?;
    let destination_effective = after.effective_part_count().map_err(format_error(format!(
        "resolving the effective part count of component '{name}' (slot #{slot})"
    )))?;
    let resolution = positional::resolve_parts_structure(&name, destination_effective, intent)?;
    if resolution.growth > 0 {
        after = after
            .with_parts_appended(resolution.growth)
            .map_err(format_error(format!(
                "growing component '{name}' (slot #{slot}) by {} part slot(s)",
                resolution.growth
            )))?;
    }
    let mut shrink = None;
    if !resolution.removed_slots.is_empty() {
        // The intent's complete parts scope with delete: allow IS the
        // descendant-deletion authorization (resolve_parts_structure only
        // emits removals under it).
        let report = after
            .with_parts_removed(&resolution.removed_slots, true)
            .map_err(format_error(format!(
                "removing part slots {:?} of component '{name}' (slot #{slot})",
                resolution.removed_slots
            )))?;
        after = report.component;
        shrink = Some(SchLibPartsShrink {
            component: name.clone(),
            removed_parts: report.removed_parts,
            deleted_children: report.deleted_children,
            current_part_id_adjustment: report.current_part_id_adjustment,
        });
    }
    let positional_edits = positional::elaborate_positional_children(
        &name,
        &after,
        intent,
        destination_effective,
        resolution.resulting_effective,
        created,
    )?;
    let parameters = elaborate_parameters(&name, &after, &intent.parameters)?;
    let footprints = elaborate_footprints(&name, &after, &intent.footprints)?;
    if parameters.is_some()
        || footprints.is_some()
        || positional_edits.pins.is_some()
        || positional_edits.graphics.is_some()
    {
        let allow_deletion = parameters.as_ref().is_some_and(|edits| edits.removals)
            || footprints.as_ref().is_some_and(|edits| edits.removals)
            || positional_edits.removals;
        after = after
            .with_children(SchChildBatch {
                parameters: parameters.map(|edits| edits.edits),
                graphics: positional_edits.graphics,
                pins: positional_edits.pins,
                footprints: footprints.map(|edits| edits.edits),
                allow_deletion,
            })
            .map_err(format_error(format!(
                "applying child edits of component '{name}' (slot #{slot})"
            )))?;
    }
    Ok((after, shrink))
}

/// A lowered child collection: the final edit sequence plus whether the plan
/// removes original members (each removal was authorized by its own scope).
struct ElaboratedChildEdits<E> {
    edits: Vec<E>,
    removals: bool,
}

/// Converts a validated plan into batch edits, or `None` when the plan keeps
/// every original slot in place (a no-op over this collection).
fn lower_plan<T, E>(
    plan: crate::collection::CollectionPlan<T>,
    input_len: usize,
    keep: fn(usize) -> E,
    replace: fn(usize, T) -> E,
    new: fn(T) -> E,
) -> Option<ElaboratedChildEdits<E>> {
    let removals = !plan.removed().is_empty();
    let identity = !removals
        && plan.items().len() == input_len
        && plan
            .items()
            .iter()
            .enumerate()
            .all(|(position, item)| matches!(item, PlannedItem::Keep { index } if index.get() == position));
    if identity {
        return None;
    }
    let edits = plan
        .into_items()
        .map(|item| match item {
            PlannedItem::Keep { index } => keep(index.get()),
            PlannedItem::Replace { index, after } => replace(index.get(), after),
            PlannedItem::New { after } => new(after),
        })
        .collect();
    Some(ElaboratedChildEdits { edits, removals })
}

fn elaborate_parameters(
    owner: &str,
    component: &SchLibComponentSnapshot,
    collection: &Collection<ParameterIntent>,
) -> Result<Option<ElaboratedChildEdits<SchParameterEdit>>, SchLibPatchError> {
    if !managed(collection) {
        return Ok(None);
    }
    let owner_context = format!("component '{owner}'");
    let views = component.parameters().map_err(format_error(format!(
        "projecting parameters of component '{owner}'"
    )))?;
    let by_key = keyed_index(
        &owner_context,
        CollectionKind::Parameters,
        "destination",
        views.iter().map(|view| view.name),
        parameter_key_fold,
    )?;
    keyed_index(
        &owner_context,
        CollectionKind::Parameters,
        "authored",
        collection.items.iter().map(|item| item.name.node.as_str()),
        parameter_key_fold,
    )?;
    let elaborate_item = |item: &ParameterIntent,
                          existing: Option<&SchParameterInit>|
     -> Result<SchParameterInit, SchLibPatchError> {
        let spec_error = |field: &str| {
            let context = format!("{owner_context} parameter '{}' {field}", item.name.node);
            move |source| SchLibPatchError::Spec { context, source }
        };
        let text = if item.text.is_authored() || existing.is_none() {
            PARAMETER_TEXT
                .definition(&item.text)
                .map_err(spec_error("text"))?
        } else {
            existing
                .map(|current| current.text.clone())
                .unwrap_or_default()
        };
        let is_hidden = if item.is_hidden.is_authored() || existing.is_none() {
            HIDDEN
                .definition(&item.is_hidden)
                .map_err(spec_error("is_hidden"))?
        } else {
            existing.is_some_and(|current| current.is_hidden)
        };
        Ok(SchParameterInit {
            name: item.name.node.clone(),
            text,
            is_hidden,
        })
    };
    let request = match collection.scope.mode {
        CollectionMode::Partial => {
            let mut edits = Vec::new();
            for item in &collection.items {
                match by_key.get(&parameter_key_fold(&item.name.node)) {
                    Some(&index) => {
                        let current = SchParameterInit::from_view(&views[index]);
                        let init = elaborate_item(item, Some(&current))?;
                        if init != current {
                            edits.push(IndexedEdit::Replace { index, after: init });
                        }
                    }
                    None => edits.push(IndexedEdit::Insert {
                        before: views.len(),
                        after: elaborate_item(item, None)?,
                    }),
                }
            }
            CollectionRequest::Partial(edits)
        }
        CollectionMode::Complete => {
            let mut entries = Vec::new();
            for item in &collection.items {
                entries.push(match by_key.get(&parameter_key_fold(&item.name.node)) {
                    Some(&index) => {
                        let current = SchParameterInit::from_view(&views[index]);
                        let init = elaborate_item(item, Some(&current))?;
                        if init == current {
                            FinalItem::Keep { index }
                        } else {
                            FinalItem::Replace { index, after: init }
                        }
                    }
                    None => FinalItem::New {
                        after: elaborate_item(item, None)?,
                    },
                });
            }
            CollectionRequest::Complete(entries)
        }
    };
    let plan = plan_collection(
        views.len(),
        &collection.scope,
        request,
        CollectionContext {
            owner: owner_context,
            kind: CollectionKind::Parameters,
        },
    )
    .map_err(collection_error)?;
    Ok(lower_plan(
        plan,
        views.len(),
        SchParameterEdit::Keep,
        SchParameterEdit::Replace,
        SchParameterEdit::New,
    ))
}

fn elaborate_aliases(
    component: &str,
    destination: &[String],
    collection: &Collection<Spanned<String>>,
    pending: &mut PendingLibraryEdit,
) -> Result<(), SchLibPatchError> {
    if !managed(collection) {
        return Ok(());
    }
    let owner_context = format!("component '{component}'");
    let by_name = keyed_index(
        &owner_context,
        CollectionKind::Aliases,
        "destination",
        destination.iter().map(String::as_str),
        exact,
    )?;
    keyed_index(
        &owner_context,
        CollectionKind::Aliases,
        "authored",
        collection.items.iter().map(|alias| alias.node.as_str()),
        exact,
    )?;
    let request = match collection.scope.mode {
        CollectionMode::Partial => CollectionRequest::Partial(
            collection
                .items
                .iter()
                .filter(|alias| !by_name.contains_key(alias.node.as_str()))
                .map(|alias| IndexedEdit::Insert {
                    before: destination.len(),
                    after: alias.node.clone(),
                })
                .collect(),
        ),
        CollectionMode::Complete => CollectionRequest::Complete(
            collection
                .items
                .iter()
                .map(|alias| match by_name.get(alias.node.as_str()) {
                    Some(&index) => FinalItem::Keep { index },
                    None => FinalItem::New {
                        after: alias.node.clone(),
                    },
                })
                .collect(),
        ),
    };
    let plan = plan_collection(
        destination.len(),
        &collection.scope,
        request,
        CollectionContext {
            owner: owner_context,
            kind: CollectionKind::Aliases,
        },
    )
    .map_err(collection_error)?;
    for index in plan.removed() {
        pending.alias_removals.push(SchLibAliasChange {
            component: component.to_owned(),
            alias: destination[index.get()].clone(),
        });
    }
    for item in plan.into_items() {
        if let PlannedItem::New { after } = item {
            pending.alias_additions.push(SchLibAliasChange {
                component: component.to_owned(),
                alias: after,
            });
        }
    }
    Ok(())
}

fn map_init_from_view(view: &altium_format::api::SchPinPadMapView<'_>) -> SchPinPadMapInit {
    SchPinPadMapInit {
        pin: view.pin.to_owned(),
        pads: view.pads.to_vec(),
    }
}

fn authored_map_init(item: &PinPadIntent) -> SchPinPadMapInit {
    SchPinPadMapInit {
        pin: item.pin.node.clone(),
        pads: vec![item.pad.node.clone()],
    }
}

/// Elaborates one explicit maps body positionally against the destination
/// definer list, returning the FINAL definer list (planner-materialized).
fn elaborate_maps(
    owner: &str,
    footprint: &str,
    current: &[SchPinPadMapInit],
    maps: &Collection<PinPadIntent>,
) -> Result<Vec<SchPinPadMapInit>, SchLibPatchError> {
    // An authored pair expresses exactly ONE pad, but native MapDefiner
    // records may carry several (`SchPinPadMapView::pads`). Both modes pair
    // authored entry N with destination definer N, so an authored entry that
    // addresses a multi-pad definer could only rewrite it down to one pad —
    // a silent drop of pad data. Hard error instead (module docs): leave the
    // definer unaddressed (fewer authored entries, or no explicit maps
    // body), or remove it explicitly (complete + `delete: allow` with a
    // shorter list).
    for (position, _) in maps.items.iter().enumerate() {
        if let Some(existing) = current.get(position) {
            if existing.pads.len() > 1 {
                return Err(SchLibPatchError::Elaboration {
                    context: format!(
                        "component '{owner}' footprint '{footprint}' maps entry #{position}"
                    ),
                    detail: format!(
                        "the destination definer at this position (pin '{}') carries {} pads, \
                         but a pin-pad intent pair expresses exactly one — rewriting it would \
                         silently drop pad data. Multi-pad definers are intent-unrepresentable \
                         content in increment E1: leave this position unaddressed (author \
                         fewer entries, or no explicit maps body), or remove the definer \
                         explicitly (complete maps with delete: allow listing at most \
                         {position} entries)",
                        existing.pin,
                        existing.pads.len(),
                    ),
                });
            }
        }
    }
    let request = match maps.scope.mode {
        CollectionMode::Partial => CollectionRequest::Partial(
            maps.items
                .iter()
                .enumerate()
                .filter_map(|(position, item)| {
                    let init = authored_map_init(item);
                    if position < current.len() {
                        (init != current[position]).then_some(IndexedEdit::Replace {
                            index: position,
                            after: init,
                        })
                    } else {
                        Some(IndexedEdit::Insert {
                            before: current.len(),
                            after: init,
                        })
                    }
                })
                .collect(),
        ),
        CollectionMode::Complete => CollectionRequest::Complete(
            maps.items
                .iter()
                .enumerate()
                .map(|(position, item)| {
                    let init = authored_map_init(item);
                    if position < current.len() {
                        if init == current[position] {
                            FinalItem::Keep { index: position }
                        } else {
                            FinalItem::Replace {
                                index: position,
                                after: init,
                            }
                        }
                    } else {
                        FinalItem::New { after: init }
                    }
                })
                .collect(),
        ),
    };
    let plan = plan_collection(
        current.len(),
        &maps.scope,
        request,
        CollectionContext {
            owner: format!("component '{owner}' footprint '{footprint}'"),
            kind: CollectionKind::Maps,
        },
    )
    .map_err(collection_error)?;
    plan.materialize(current).map_err(collection_error)
}

fn elaborate_footprints(
    owner: &str,
    component: &SchLibComponentSnapshot,
    collection: &Collection<FootprintIntent>,
) -> Result<Option<ElaboratedChildEdits<SchFootprintEdit>>, SchLibPatchError> {
    if !managed(collection) {
        return Ok(None);
    }
    let owner_context = format!("component '{owner}'");
    let maps_view = component.footprint_maps().map_err(format_error(format!(
        "projecting footprints of component '{owner}'"
    )))?;
    let views = &maps_view.footprints;
    let by_name = keyed_index(
        &owner_context,
        CollectionKind::Footprints,
        "destination",
        views.iter().map(|view| view.model_name),
        exact,
    )?;
    keyed_index(
        &owner_context,
        CollectionKind::Footprints,
        "authored",
        collection
            .items
            .iter()
            .map(|item| item.model_name.node.as_str()),
        exact,
    )?;
    // A created footprint is current only when the destination collection was
    // empty and it is the first authored one (module docs).
    let mut first_new_is_current = views.is_empty();
    let mut elaborate_item = |item: &FootprintIntent,
                              existing: Option<&SchFootprintMapView>|
     -> Result<SchFootprintInit, SchLibPatchError> {
        let model = &item.model_name.node;
        if !item.explicit_maps && (!item.maps.items.is_empty() || item.maps.scope.span.is_some()) {
            return Err(SchLibPatchError::Elaboration {
                context: format!("{owner_context} footprint '{model}'"),
                detail: "authored maps content without an explicit maps body; \
                         inconsistent FootprintIntent"
                    .into(),
            });
        }
        let description = if item.description.is_authored() || existing.is_none() {
            OPTIONAL_DESCRIPTION
                .definition(&item.description)
                .map_err(|source| SchLibPatchError::Spec {
                    context: format!("{owner_context} footprint '{model}' description"),
                    source,
                })?
                .unwrap_or_default()
        } else {
            existing
                .map(|view| view.description.to_owned())
                .unwrap_or_default()
        };
        let is_current = match existing {
            Some(view) => view.is_current,
            None => std::mem::take(&mut first_new_is_current),
        };
        let pin_pad_maps = if item.explicit_maps {
            match existing {
                Some(view) => {
                    let current: Vec<SchPinPadMapInit> =
                        view.pin_pad_maps.iter().map(map_init_from_view).collect();
                    let final_list = elaborate_maps(owner, model, &current, &item.maps)?;
                    (final_list != current).then_some(final_list)
                }
                None => Some(item.maps.items.iter().map(authored_map_init).collect()),
            }
        } else {
            None
        };
        Ok(SchFootprintInit {
            model_name: model.clone(),
            description,
            is_current,
            pin_pad_maps,
        })
    };
    let request = match collection.scope.mode {
        CollectionMode::Partial => {
            let mut edits = Vec::new();
            for item in &collection.items {
                match by_name.get(item.model_name.node.as_str()) {
                    Some(&index) => {
                        let view = &views[index];
                        let init = elaborate_item(item, Some(view))?;
                        let unchanged = init.model_name == view.model_name
                            && init.description == view.description
                            && init.is_current == view.is_current
                            && init.pin_pad_maps.is_none();
                        if !unchanged {
                            edits.push(IndexedEdit::Replace { index, after: init });
                        }
                    }
                    None => edits.push(IndexedEdit::Insert {
                        before: views.len(),
                        after: elaborate_item(item, None)?,
                    }),
                }
            }
            CollectionRequest::Partial(edits)
        }
        CollectionMode::Complete => {
            let mut entries = Vec::new();
            for item in &collection.items {
                entries.push(match by_name.get(item.model_name.node.as_str()) {
                    Some(&index) => {
                        let view = &views[index];
                        let init = elaborate_item(item, Some(view))?;
                        let unchanged = init.model_name == view.model_name
                            && init.description == view.description
                            && init.is_current == view.is_current
                            && init.pin_pad_maps.is_none();
                        if unchanged {
                            FinalItem::Keep { index }
                        } else {
                            FinalItem::Replace { index, after: init }
                        }
                    }
                    None => FinalItem::New {
                        after: elaborate_item(item, None)?,
                    },
                });
            }
            CollectionRequest::Complete(entries)
        }
    };
    let plan = plan_collection(
        views.len(),
        &collection.scope,
        request,
        CollectionContext {
            owner: owner_context,
            kind: CollectionKind::Footprints,
        },
    )
    .map_err(collection_error)?;
    Ok(lower_plan(
        plan,
        views.len(),
        SchFootprintEdit::Keep,
        SchFootprintEdit::Replace,
        SchFootprintEdit::New,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_asset::CapturedImageAsset;
    use crate::schlib::{
        SchLibImageAssetOp, SchLibIntentAssets, SchLibLibraryOp, SchLibPatch, SchLibPatchError,
    };
    use altium_format::SchLib;
    use altium_format::api::{
        SchAliasOp, SchChildBatch, SchFontAppend, SchFootprintEdit, SchFootprintInit,
        SchGraphicDetail, SchGraphicEdit, SchGraphicInit, SchGraphicShape, SchLibEdit,
        SchNewComponent, SchOwnerGroup, SchParameterEdit, SchParameterInit, SchPartSwapGroup,
        SchPinEdit, SchPinInit, SchPinPadMapInit,
    };
    use altium_format_types::{
        Color, Coord, CoordPoint, LineStyle, PenWidth, PinElectricalType, RotationBy90,
    };
    use altium_spec_lang::ast::{CollectionDelete, CollectionMode};
    use altium_spec_lang::diagnostic::{Span, Spanned};
    use altium_spec_lang::intent::{
        AnchorEdge, AnchorPosition, AutoSizeIntent, CollectionScope, FieldIntent, GraphicFields,
        GraphicIntent, GraphicKind, GraphicSourceRef, ImageAssetSource, LayoutOwner, PartCount,
        PartIntent, PartNumber, PinIntent, PinPlacementIntent, SchLibIntent,
    };

    fn spanned<T>(value: T) -> Spanned<T> {
        Spanned::new(value, Span::new(0, 1))
    }
    fn set<T>(value: T) -> FieldIntent<T> {
        FieldIntent::Set(spanned(value))
    }
    fn authored_scope(mode: CollectionMode, delete: CollectionDelete) -> CollectionScope {
        CollectionScope {
            mode,
            delete,
            span: Some(Span::new(0, 1)),
        }
    }
    fn items<T>(items: Vec<T>) -> Collection<T> {
        Collection {
            scope: CollectionScope::default(),
            items,
        }
    }
    fn scoped<T>(scope: CollectionScope, values: Vec<T>) -> Collection<T> {
        Collection {
            scope,
            items: values,
        }
    }
    fn component_intent(name: &str) -> ComponentIntent {
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
            components: scoped(scope, components),
        }
    }
    fn partial_intent(components: Vec<ComponentIntent>) -> SchLibIntent {
        intent_of(CollectionScope::default(), components)
    }
    fn parameter(name: &str) -> ParameterIntent {
        ParameterIntent {
            name: spanned(name.to_owned()),
            text: FieldIntent::Absent,
            is_hidden: FieldIntent::Absent,
        }
    }
    fn footprint(model: &str) -> FootprintIntent {
        FootprintIntent {
            source: None,
            model_name: spanned(model.to_owned()),
            description: FieldIntent::Absent,
            maps: Collection::default(),
            explicit_maps: false,
        }
    }
    fn map_pair(pin: &str, pad: &str) -> PinPadIntent {
        PinPadIntent {
            pin: spanned(pin.to_owned()),
            pad: spanned(pad.to_owned()),
        }
    }

    /// Blank AD26 library ("Component_1" with a Comment parameter) extended
    /// with two root parameters, one footprint carrying two definers, and one
    /// alias.
    fn seeded_library() -> SchLib {
        let doc = SchLib::new_blank_ad26().unwrap();
        let snapshot = doc.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Value".to_owned(),
                        text: "10k".to_owned(),
                        is_hidden: false,
                    }),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Tolerance".to_owned(),
                        text: "1%".to_owned(),
                        is_hidden: true,
                    }),
                ]),
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::New(SchFootprintInit {
                    model_name: "FP1".to_owned(),
                    description: "original footprint".to_owned(),
                    is_current: true,
                    pin_pad_maps: Some(vec![
                        SchPinPadMapInit {
                            pin: "1".to_owned(),
                            pads: vec!["A1".to_owned()],
                        },
                        SchPinPadMapInit {
                            pin: "2".to_owned(),
                            pads: vec!["B2".to_owned()],
                        },
                    ]),
                })]),
                allow_deletion: false,
            })
            .unwrap();
        let with_children = snapshot.replace_component(0, edited).unwrap();
        let with_alias = with_children
            .with_library_edit(SchLibEdit {
                alias_ops: vec![SchAliasOp::Add {
                    component: "Component_1".to_owned(),
                    alias: "ALIAS_A".to_owned(),
                }],
                ..Default::default()
            })
            .unwrap();
        with_alias.materialize().unwrap()
    }

    fn parameter_tuples(snapshot: &SchLibSnapshot, index: usize) -> Vec<(String, String, bool)> {
        snapshot
            .component(index)
            .unwrap()
            .parameters()
            .unwrap()
            .iter()
            .map(|view| (view.name.to_owned(), view.text.to_owned(), view.is_hidden))
            .collect()
    }

    fn save_reopen_equals(doc: &SchLib, expected: &SchLibSnapshot) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("elaborated.SchLib");
        doc.save(&path).unwrap();
        assert_eq!(&SchLib::open(&path).unwrap().snapshot().unwrap(), expected);
    }

    #[test]
    fn keyed_partial_parameters_update_and_append_then_replan_empty() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let mut component = component_intent("Component_1");
        component.parameters = items(vec![
            ParameterIntent {
                // Case-variant key: matches "Value" per the native
                // OrdinalIgnoreCase comparison; the authored spelling wins.
                name: spanned("VALUE".to_owned()),
                text: set("22k".to_owned()),
                is_hidden: FieldIntent::Absent,
            },
            ParameterIntent {
                name: spanned("MaxPower".to_owned()),
                text: set("0.5W".to_owned()),
                is_hidden: set(true),
            },
        ]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert_eq!(patch.replacements().len(), 1);
        assert_eq!(patch.created_components().count(), 0);
        assert!(patch.removed_components().is_empty());
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.snapshot().unwrap(), expected);
        assert_eq!(
            parameter_tuples(&expected, 0),
            [
                ("Comment".to_owned(), "*".to_owned(), true),
                // Updated in its original slot; unauthored is_hidden preserved.
                ("VALUE".to_owned(), "22k".to_owned(), false),
                ("Tolerance".to_owned(), "1%".to_owned(), true),
                // Appended in source order.
                ("MaxPower".to_owned(), "0.5W".to_owned(), true),
            ]
        );
        save_reopen_equals(&doc, &expected);
        let replanned = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent).unwrap();
        assert!(replanned.is_empty(), "apply → re-elaborate must be empty");
    }

    #[test]
    fn keyed_complete_parameters_reorder_and_removal_permission_matrix() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let complete = |delete, names: &[&str]| {
            let mut component = component_intent("Component_1");
            component.parameters = scoped(
                authored_scope(CollectionMode::Complete, delete),
                names.iter().map(|name| parameter(name)).collect(),
            );
            partial_intent(vec![component])
        };

        // Identity listing (destination order, no field writes) is a no-op.
        let identity = complete(CollectionDelete::Deny, &["Comment", "Value", "Tolerance"]);
        assert!(
            SchLibPatch::from_intent(&input, &identity)
                .unwrap()
                .is_empty()
        );

        // Reorder without membership change needs no deletion permission.
        let reorder = complete(CollectionDelete::Deny, &["Tolerance", "Value", "Comment"]);
        let patch = SchLibPatch::from_intent(&input, &reorder).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            parameter_tuples(&expected, 0)
                .iter()
                .map(|(name, _, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Tolerance", "Value", "Comment"]
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &reorder)
                .unwrap()
                .is_empty()
        );

        // Omission under deny blocks; under allow it removes exactly the
        // omitted parameter.
        let omitting_deny = complete(CollectionDelete::Deny, &["Tolerance", "Comment"]);
        let error = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &omitting_deny).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Collection { detail }
                if detail.contains("delete: allow") && detail.contains("Parameters")),
            "{error}"
        );
        let omitting_allow = complete(CollectionDelete::Allow, &["Tolerance", "Comment"]);
        let removal = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &omitting_allow).unwrap();
        let removed = removal.resulting_snapshot().unwrap();
        removal.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            parameter_tuples(&removed, 0)
                .iter()
                .map(|(name, _, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Tolerance", "Comment"]
        );
        save_reopen_equals(&doc, &removed);
    }

    #[test]
    fn complete_component_reorder_binds_content_edits_to_final_positions() {
        // Two components (Component_1 + a clone), reordered by a complete
        // components intent that also edits both — the replacement indices
        // must address the POST-library-edit (reordered) sequence.
        let mut doc = seeded_library();
        let clone = SchLibPatch::from_library_ops(
            &doc.snapshot().unwrap(),
            [SchLibLibraryOp::ComponentCreate(SchNewComponent::CloneOf {
                source: 0,
                lib_reference: "SECOND".to_owned(),
            })],
        )
        .unwrap();
        clone.apply_in_memory(&mut doc).unwrap();
        let input = doc.snapshot().unwrap();

        let mut second = component_intent("SECOND");
        second.description = set(Some("second first".to_owned()));
        let mut first = component_intent("Component_1");
        first.description = set(Some("first second".to_owned()));
        let intent = intent_of(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![second, first],
        );
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert_eq!(patch.replacements().len(), 2);
        assert_eq!(
            patch
                .replacements()
                .iter()
                .map(|replacement| (replacement.index(), replacement.after().lib_reference()))
                .collect::<Vec<_>>(),
            [(0, "SECOND"), (1, "Component_1")],
            "indices bind the reordered final sequence"
        );
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(expected.component(0).unwrap().lib_reference(), "SECOND");
        assert_eq!(
            expected.component(0).unwrap().description(),
            Some("second first")
        );
        assert_eq!(
            expected.component(1).unwrap().lib_reference(),
            "Component_1"
        );
        assert_eq!(
            expected.component(1).unwrap().description(),
            Some("first second")
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn duplicate_keys_block_managed_collections_on_either_side() {
        // Destination side: two parameters whose names collide under the
        // native case-insensitive comparison.
        let doc = SchLib::new_blank_ad26().unwrap();
        let snapshot = doc.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Value".to_owned(),
                        text: "10k".to_owned(),
                        is_hidden: false,
                    }),
                    SchParameterEdit::New(SchParameterInit {
                        name: "VALUE".to_owned(),
                        text: "wild duplicate".to_owned(),
                        is_hidden: false,
                    }),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let wild = snapshot.replace_component(0, edited).unwrap();
        let mut managed_params = component_intent("Component_1");
        managed_params.parameters = items(vec![parameter("Comment")]);
        let error =
            SchLibPatch::from_intent(&wild, &partial_intent(vec![managed_params])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Parameters")
                    && detail.contains("'Value'")
                    && detail.contains("'VALUE'")
                    && detail.contains("destination")),
            "{error}"
        );
        // The same wild state is untouchable but never blocks an intent that
        // does not manage the parameters collection.
        let untouched = component_intent("Component_1");
        assert!(
            SchLibPatch::from_intent(&wild, &partial_intent(vec![untouched]))
                .unwrap()
                .is_empty()
        );

        // Authored side: duplicate parameter keys in one component intent.
        let mut duplicated = component_intent("Component_1");
        duplicated.parameters = items(vec![parameter("value"), parameter("VALUE")]);
        let error =
            SchLibPatch::from_intent(&snapshot, &partial_intent(vec![duplicated])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("authored") && detail.contains("'value'")),
            "{error}"
        );

        // Authored side: duplicate alias keys. (A destination-side duplicate
        // alias is wild loaded state that no public mutation path can build —
        // D1 enforces global alias uniqueness — so the destination guard is
        // exercised through the constructible parameter case above.)
        let mut duplicate_aliases = component_intent("Component_1");
        duplicate_aliases.aliases =
            items(vec![spanned("DUP".to_owned()), spanned("DUP".to_owned())]);
        let error = SchLibPatch::from_intent(&snapshot, &partial_intent(vec![duplicate_aliases]))
            .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Aliases") && detail.contains("'DUP'")),
            "{error}"
        );

        // Authored side: duplicate component keys.
        let error = SchLibPatch::from_intent(
            &snapshot,
            &partial_intent(vec![
                component_intent("Component_1"),
                component_intent("Component_1"),
            ]),
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, .. }
                if context.contains("Components")),
            "{error}"
        );
    }

    #[test]
    fn alias_add_and_remove_via_intent() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();

        // Partial: matched alias is a no-op, unmatched appends.
        let mut component = component_intent("Component_1");
        component.aliases = items(vec![
            spanned("ALIAS_A".to_owned()),
            spanned("ALIAS_B".to_owned()),
        ]);
        let add_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &add_intent).unwrap();
        assert_eq!(
            patch.alias_additions(),
            [SchLibAliasChange {
                component: "Component_1".to_owned(),
                alias: "ALIAS_B".to_owned(),
            }]
        );
        assert!(patch.alias_removals().is_empty());
        assert!(patch.replacements().is_empty());
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        let aliased = expected.component(0).unwrap();
        let aliases: Vec<&str> = aliased.aliases().collect();
        assert_eq!(aliases, ["ALIAS_A", "ALIAS_B"]);
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &add_intent)
                .unwrap()
                .is_empty()
        );

        // Complete + deny: omitting ALIAS_B blocks with alias context.
        let mut deny = component_intent("Component_1");
        deny.aliases = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![spanned("ALIAS_A".to_owned())],
        );
        let error = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &partial_intent(vec![deny]))
            .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Collection { detail }
                if detail.contains("Aliases") && detail.contains("delete: allow")),
            "{error}"
        );

        // Complete + allow removes exactly the omitted alias, restoring the
        // original state.
        let mut allow = component_intent("Component_1");
        allow.aliases = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Allow),
            vec![spanned("ALIAS_A".to_owned())],
        );
        let removal =
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &partial_intent(vec![allow]))
                .unwrap();
        assert_eq!(removal.alias_removals().len(), 1);
        let removed = removal.resulting_snapshot().unwrap();
        removal.apply_in_memory(&mut doc).unwrap();
        assert_eq!(removed, input);
        save_reopen_equals(&doc, &removed);
    }

    #[test]
    fn footprint_replace_maps_rebuild_and_unaddressed_preservation() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let original_maps: Vec<(String, Vec<String>)> = input
            .component(0)
            .unwrap()
            .footprint_maps()
            .unwrap()
            .footprints[0]
            .pin_pad_maps
            .iter()
            .map(|map| (map.pin.to_owned(), map.pads.to_vec()))
            .collect();

        // Unaddressed maps (explicit_maps=false): description replaced,
        // definers and is_current preserved verbatim.
        let mut component = component_intent("Component_1");
        let mut replace = footprint("FP1");
        replace.description = set(Some("replaced description".to_owned()));
        component.footprints = items(vec![replace]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let maps = component.footprint_maps().unwrap();
            assert_eq!(maps.footprints[0].description, "replaced description");
            assert!(maps.footprints[0].is_current);
            let preserved: Vec<(String, Vec<String>)> = maps.footprints[0]
                .pin_pad_maps
                .iter()
                .map(|map| (map.pin.to_owned(), map.pads.to_vec()))
                .collect();
            assert_eq!(preserved, original_maps);
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty()
        );

        // Explicit partial maps: index 0 rebuilt, tail preserved.
        let mut component = component_intent("Component_1");
        let mut rebuild = footprint("FP1");
        rebuild.explicit_maps = true;
        rebuild.maps = items(vec![map_pair("1", "A9")]);
        component.footprints = items(vec![rebuild]);
        let rebuild_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &rebuild_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let maps = component.footprint_maps().unwrap();
            let rebuilt: Vec<(String, Vec<String>)> = maps.footprints[0]
                .pin_pad_maps
                .iter()
                .map(|map| (map.pin.to_owned(), map.pads.to_vec()))
                .collect();
            assert_eq!(
                rebuilt,
                [
                    ("1".to_owned(), vec!["A9".to_owned()]),
                    ("2".to_owned(), vec!["B2".to_owned()]),
                ]
            );
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &rebuild_intent)
                .unwrap()
                .is_empty()
        );

        // Complete maps under deny cannot shrink; under allow they define the
        // final definer list.
        let complete_maps = |delete| {
            let mut component = component_intent("Component_1");
            let mut shrink = footprint("FP1");
            shrink.explicit_maps = true;
            shrink.maps = scoped(
                authored_scope(CollectionMode::Complete, delete),
                vec![map_pair("1", "A9")],
            );
            component.footprints = items(vec![shrink]);
            partial_intent(vec![component])
        };
        let error = SchLibPatch::from_intent(
            &doc.snapshot().unwrap(),
            &complete_maps(CollectionDelete::Deny),
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Collection { detail }
                if detail.contains("Maps") && detail.contains("delete: allow")),
            "{error}"
        );
        let patch = SchLibPatch::from_intent(
            &doc.snapshot().unwrap(),
            &complete_maps(CollectionDelete::Allow),
        )
        .unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            expected
                .component(0)
                .unwrap()
                .footprint_maps()
                .unwrap()
                .footprints[0]
                .pin_pad_maps
                .len(),
            1
        );
        save_reopen_equals(&doc, &expected);

        // Unmatched model name creates a footprint; alongside the existing
        // current footprint the new one stays non-current.
        let mut component = component_intent("Component_1");
        let mut new_footprint = footprint("FP2");
        new_footprint.description = set(Some("second model".to_owned()));
        new_footprint.explicit_maps = true;
        new_footprint.maps = items(vec![map_pair("3", "C3")]);
        component.footprints = items(vec![new_footprint]);
        let create_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &create_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let maps = component.footprint_maps().unwrap();
            assert_eq!(maps.footprints.len(), 2);
            assert_eq!(maps.footprints[1].model_name, "FP2");
            assert!(!maps.footprints[1].is_current);
            assert!(maps.footprints[0].is_current);
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &create_intent)
                .unwrap()
                .is_empty()
        );
    }

    /// Blank library plus one footprint whose SECOND definer carries two
    /// pads (the native multi-pad MapDefiner shape).
    fn multi_pad_library() -> SchLib {
        let doc = SchLib::new_blank_ad26().unwrap();
        let snapshot = doc.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::New(SchFootprintInit {
                    model_name: "FP1".to_owned(),
                    description: "multi-pad footprint".to_owned(),
                    is_current: true,
                    pin_pad_maps: Some(vec![
                        SchPinPadMapInit {
                            pin: "1".to_owned(),
                            pads: vec!["A1".to_owned()],
                        },
                        SchPinPadMapInit {
                            pin: "2".to_owned(),
                            pads: vec!["B1".to_owned(), "B2".to_owned()],
                        },
                    ]),
                })]),
                allow_deletion: false,
            })
            .unwrap();
        snapshot
            .replace_component(0, edited)
            .unwrap()
            .materialize()
            .unwrap()
    }

    #[test]
    fn multi_pad_definers_block_addressing_but_survive_unaddressed_tails() {
        let mut doc = multi_pad_library();
        let input = doc.snapshot().unwrap();
        let explicit = |scope: CollectionScope, pairs: Vec<PinPadIntent>| {
            let mut component = component_intent("Component_1");
            let mut fp = footprint("FP1");
            fp.explicit_maps = true;
            fp.maps = scoped(scope, pairs);
            component.footprints = items(vec![fp]);
            partial_intent(vec![component])
        };
        let maps_of = |snapshot: &SchLibSnapshot| -> Vec<(String, Vec<String>)> {
            snapshot
                .component(0)
                .unwrap()
                .footprint_maps()
                .unwrap()
                .footprints[0]
                .pin_pad_maps
                .iter()
                .map(|map| (map.pin.to_owned(), map.pads.to_vec()))
                .collect()
        };

        // Partial authored prefix reaching position 1 (the multi-pad
        // definer) hard-errors with position, pin, and pad-count context —
        // never a silent narrowing to one pad.
        let error = SchLibPatch::from_intent(
            &input,
            &explicit(
                CollectionScope::default(),
                vec![map_pair("1", "A1"), map_pair("2", "B1")],
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("footprint 'FP1'")
                    && context.contains("#1")
                    && detail.contains("pin '2'")
                    && detail.contains("2 pads")
                    && detail.contains("silently drop")),
            "{error}"
        );

        // A complete listing that reaches the multi-pad position errors too,
        // even under delete: allow (replacement is narrowing, not removal).
        let error = SchLibPatch::from_intent(
            &input,
            &explicit(
                authored_scope(CollectionMode::Complete, CollectionDelete::Allow),
                vec![map_pair("1", "A1"), map_pair("2", "B1")],
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("intent-unrepresentable")),
            "{error}"
        );

        // A partial prefix BELOW the multi-pad position rebuilds index 0 and
        // preserves the multi-pad tail definer with all its pads.
        let rebuild = explicit(CollectionScope::default(), vec![map_pair("1", "A9")]);
        let patch = SchLibPatch::from_intent(&input, &rebuild).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            maps_of(&expected),
            [
                ("1".to_owned(), vec!["A9".to_owned()]),
                ("2".to_owned(), vec!["B1".to_owned(), "B2".to_owned()]),
            ]
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &rebuild)
                .unwrap()
                .is_empty()
        );

        // Complete + allow with only the single-pad prefix removes the
        // multi-pad definer explicitly: authorized deletion, not a drop.
        let shrink = explicit(
            authored_scope(CollectionMode::Complete, CollectionDelete::Allow),
            vec![map_pair("1", "A9")],
        );
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &shrink).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            maps_of(&expected),
            [("1".to_owned(), vec!["A9".to_owned()])]
        );
        save_reopen_equals(&doc, &expected);
    }

    #[test]
    fn inconsistent_programmatic_scopes_are_rejected_not_dropped() {
        let input = seeded_library().snapshot().unwrap();
        let spanless = |mode, delete| CollectionScope {
            mode,
            delete,
            span: None,
        };

        // Library components collection.
        let intent = intent_of(
            spanless(CollectionMode::Complete, CollectionDelete::Deny),
            vec![],
        );
        let error = SchLibPatch::from_intent(&input, &intent).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Components") && detail.contains("declaration span")),
            "{error}"
        );

        // A span-less complete pins scope with no items previously bypassed
        // BOTH managed() and the E2 rejection — now a hard error.
        let mut component = component_intent("Component_1");
        component.pins = scoped(
            spanless(CollectionMode::Complete, CollectionDelete::Allow),
            vec![],
        );
        let error = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, .. }
                if context.contains("Pins")),
            "{error}"
        );

        // Keyed child collections get the same rule.
        let mut component = component_intent("Component_1");
        component.parameters = scoped(
            spanless(CollectionMode::Complete, CollectionDelete::Deny),
            vec![],
        );
        assert!(SchLibPatch::from_intent(&input, &partial_intent(vec![component])).is_err());

        // Nested maps scopes too — with explicit_maps=false this scope was
        // previously dropped silently as "no maps body".
        let mut component = component_intent("Component_1");
        let mut fp = footprint("FP1");
        fp.maps = scoped(
            spanless(CollectionMode::Complete, CollectionDelete::Allow),
            vec![],
        );
        component.footprints = items(vec![fp]);
        let error = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, .. }
                if context.contains("Maps")),
            "{error}"
        );

        // Part-item pin/graphic scopes are walked too (increment E2).
        let mut component = component_intent("Component_1");
        let mut part = part_intent(1);
        part.pins = scoped(
            spanless(CollectionMode::Complete, CollectionDelete::Allow),
            vec![],
        );
        component.parts = items(vec![part]);
        let error = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, .. }
                if context.contains("part 1") && context.contains("Pins")),
            "{error}"
        );

        // partial + delete: allow is contract-illegal even WITH a span.
        let mut component = component_intent("Component_1");
        component.aliases = scoped(
            authored_scope(CollectionMode::Partial, CollectionDelete::Allow),
            vec![spanned("X".to_owned())],
        );
        let error = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Aliases") && detail.contains("delete: allow")),
            "{error}"
        );

        // The true default (span-less partial + deny) stays unmanaged.
        assert!(
            SchLibPatch::from_intent(
                &input,
                &partial_intent(vec![component_intent("Component_1")])
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn component_create_with_keyed_children_then_remove_under_permission() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();

        // Partial append: unmatched name creates a Blank with elaborated
        // keyed children.
        let mut probe = component_intent("STEP2E_PROBE");
        probe.description = set(Some("created by intent".to_owned()));
        probe.parameters = items(vec![ParameterIntent {
            name: spanned("P1".to_owned()),
            text: set("probe parameter".to_owned()),
            is_hidden: FieldIntent::Absent,
        }]);
        probe.aliases = items(vec![spanned("PROBE_ALIAS".to_owned())]);
        let mut probe_footprint = footprint("FP_PROBE");
        probe_footprint.explicit_maps = true;
        probe_footprint.maps = items(vec![map_pair("1", "P1")]);
        probe.footprints = items(vec![probe_footprint]);
        let create_intent = partial_intent(vec![probe]);
        let patch = SchLibPatch::from_intent(&input, &create_intent).unwrap();
        assert_eq!(patch.created_components().count(), 1);
        assert!(matches!(
            patch.created_components().next().unwrap(),
            SchNewComponent::Blank { lib_reference } if lib_reference == "STEP2E_PROBE"
        ));
        assert_eq!(patch.alias_additions().len(), 1);
        assert_eq!(
            patch.replacements().len(),
            1,
            "created content lands as a replacement"
        );
        let expected = patch.resulting_snapshot().unwrap();
        // The cached result is stable across calls even though creation hands
        // out fresh unique ids.
        assert_eq!(patch.resulting_snapshot().unwrap(), expected);
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(expected.component_count(), 2);
        {
            let created = expected.component(1).unwrap();
            assert_eq!(created.lib_reference(), "STEP2E_PROBE");
            assert_eq!(created.description(), Some("created by intent"));
            let aliases: Vec<&str> = created.aliases().collect();
            assert_eq!(aliases, ["PROBE_ALIAS"]);
            assert_eq!(
                parameter_tuples(&expected, 1)
                    .iter()
                    .map(|(name, _, _)| name.as_str())
                    .collect::<Vec<_>>(),
                ["Comment", "P1"],
                "children elaborate against the blank factory shape"
            );
            let maps = created.footprint_maps().unwrap();
            assert_eq!(maps.footprints.len(), 1);
            assert!(
                maps.footprints[0].is_current,
                "sole footprint of a created component must be current"
            );
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &create_intent)
                .unwrap()
                .is_empty(),
            "apply → re-elaborate the creating intent must be empty"
        );

        // Complete + deny: omitting the probe blocks.
        let complete = |delete| {
            intent_of(
                authored_scope(CollectionMode::Complete, delete),
                vec![component_intent("Component_1")],
            )
        };
        let error =
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &complete(CollectionDelete::Deny))
                .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Collection { detail }
                if detail.contains("Components") && detail.contains("delete: allow")),
            "{error}"
        );
        // Partial omission preserves (removal is never inferred).
        assert!(
            SchLibPatch::from_intent(
                &doc.snapshot().unwrap(),
                &partial_intent(vec![component_intent("Component_1")])
            )
            .unwrap()
            .is_empty()
        );

        // Complete + allow removes exactly the probe, releasing its alias,
        // and restores the original input state.
        let removal =
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &complete(CollectionDelete::Allow))
                .unwrap();
        assert_eq!(removal.removed_components().len(), 1);
        assert_eq!(
            removal.removed_components()[0].lib_reference,
            "STEP2E_PROBE"
        );
        assert_eq!(
            removal.alias_removals(),
            [SchLibAliasChange {
                component: "STEP2E_PROBE".to_owned(),
                alias: "PROBE_ALIAS".to_owned(),
            }],
            "component removal releases the aggregate's aliases"
        );
        let removed = removal.resulting_snapshot().unwrap();
        removal.apply_in_memory(&mut doc).unwrap();
        assert_eq!(removed, input, "removal restores the pre-creation state");
        save_reopen_equals(&doc, &removed);
    }

    // ── E2 positional helpers ──────────────────────────────────────────────

    fn pin_intent(designator: &str) -> PinIntent {
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
    fn graphic_intent_of(kind: GraphicKind) -> GraphicIntent {
        GraphicIntent {
            auto_size: None,
            kind: spanned(kind),
            fields: GraphicFields::default(),
        }
    }
    fn part_intent(number: i32) -> PartIntent {
        PartIntent {
            number: spanned(PartNumber::new(number).unwrap()),
            part_swap_group: FieldIntent::Absent,
            pins: Collection::default(),
            graphics: Collection::default(),
        }
    }
    fn mil(value: i32) -> Coord {
        Coord::from_mils(value).expect("test coordinate fits Coord")
    }
    fn pin_init(owner: i32, designator: &str, name: &str, part_swap: Option<&str>) -> SchPinInit {
        SchPinInit {
            owner_part_id: owner,
            owner_part_display_mode: None,
            designator: designator.to_owned(),
            name: name.to_owned(),
            electrical: PinElectricalType::Passive,
            length: mil(300),
            location: CoordPoint::zero(),
            orientation: RotationBy90::Rotate0,
            is_hidden: false,
            hidden_net_name: String::new(),
            swap_group: String::new(),
            part_swap_group: part_swap.map(str::to_owned),
            part_pin_swap_suffix: None,
            pair_swap_group: String::new(),
        }
    }
    fn rect_init(owner: i32) -> SchGraphicInit {
        SchGraphicInit {
            owner_part_id: owner,
            owner_part_display_mode: None,
            shape: SchGraphicShape::Rectangle {
                from: CoordPoint::zero(),
                to: CoordPoint::new(mil(100), mil(100)),
                line_width: PenWidth::Small,
                line_style: LineStyle::Solid,
                color: Color::BLACK,
                area_color: Color::BLACK,
                is_solid: false,
                transparent: false,
            },
        }
    }
    fn line_init(owner: i32) -> SchGraphicInit {
        SchGraphicInit {
            owner_part_id: owner,
            owner_part_display_mode: None,
            shape: SchGraphicShape::Line {
                from: CoordPoint::zero(),
                to: CoordPoint::new(mil(50), mil(0)),
                line_width: PenWidth::Small,
                line_style: LineStyle::Solid,
                color: Color::BLACK,
            },
        }
    }

    /// Blank library grown to two effective parts, with two pins and one
    /// graphic per part plus one shared (owner-0) pin. Full `pins()` order:
    /// ["1","2"] (part 1), ["3","4"] (part 2), ["9"] (shared); `graphics()`:
    /// [Rectangle (part 1), Line (part 2)]. Part-2 pins carry the given swap
    /// prefixes and the shared pin prefix "X".
    fn multi_part_library_with_swaps(prefix_a: &str, prefix_b: &str) -> SchLib {
        let doc = SchLib::new_blank_ad26().unwrap();
        let snapshot = doc.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let grown = component.with_parts_appended(1).unwrap();
        let edited = grown
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::New(rect_init(1)),
                    SchGraphicEdit::New(line_init(2)),
                ]),
                pins: Some(vec![
                    SchPinEdit::New(pin_init(1, "1", "P1A", None)),
                    SchPinEdit::New(pin_init(1, "2", "P1B", None)),
                    SchPinEdit::New(pin_init(2, "3", "P2A", Some(prefix_a))),
                    SchPinEdit::New(pin_init(2, "4", "P2B", Some(prefix_b))),
                    SchPinEdit::New(pin_init(0, "9", "SHARED", Some("X"))),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        snapshot
            .replace_component(0, edited)
            .unwrap()
            .materialize()
            .unwrap()
    }
    fn multi_part_library() -> SchLib {
        multi_part_library_with_swaps("", "")
    }
    fn pin_fingerprints(component: &altium_format::api::SchLibComponentSnapshot) -> Vec<String> {
        component
            .pins()
            .iter()
            .map(|view| format!("{view:?}"))
            .collect()
    }
    fn graphic_kinds(component: &altium_format::api::SchLibComponentSnapshot) -> Vec<&'static str> {
        component
            .graphics()
            .iter()
            .map(|view| view.detail.kind_name())
            .collect()
    }

    // ── E2 positional tests ────────────────────────────────────────────────

    #[test]
    fn multi_part_positional_edits_touch_only_their_group() {
        // The recorded B/C obligation: per-owner-part positional addresses
        // resolve through the owner_group-filtered → full-projection mapping,
        // and edits to part 2 disturb neither part 1 nor any other group.
        let mut doc = multi_part_library();
        let input = doc.snapshot().unwrap();
        let before = pin_fingerprints(&input.component(0).unwrap());

        // Part 2, filtered index 0 = full pins()[2] ("3").
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        let mut rename = pin_intent("3");
        rename.name = set("RENAMED".to_owned());
        part2.pins = items(vec![rename]);
        component.parts = items(vec![part2]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let pins = component.pins();
            assert_eq!(pins[2].name, "RENAMED");
            assert_eq!(pins[2].owner_group, SchOwnerGroup::Part(2));
            let after = pin_fingerprints(&component);
            for index in [0usize, 1, 3, 4] {
                assert_eq!(after[index], before[index], "pins()[{index}] must not move");
            }
            assert_eq!(graphic_kinds(&component), ["Rectangle", "Line"]);
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty(),
            "apply → re-elaborate must be empty"
        );

        // Component scope on a multi-part component addresses the SHARED
        // (owner 0) group: filtered index 0 = full pins()[4] ("9").
        let state = doc.snapshot().unwrap();
        let shared_before = pin_fingerprints(&state.component(0).unwrap());
        let mut component = component_intent("Component_1");
        let mut rename = pin_intent("9");
        rename.name = set("SHARED_RENAMED".to_owned());
        component.pins = items(vec![rename]);
        let shared_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&state, &shared_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let pins = component.pins();
            assert_eq!(pins[4].name, "SHARED_RENAMED");
            assert_eq!(pins[4].owner_group, SchOwnerGroup::SharedPins);
            let after = pin_fingerprints(&component);
            for index in 0..4 {
                assert_eq!(after[index], shared_before[index]);
            }
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &shared_intent)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn overflow_appends_force_the_scope_group_owner() {
        let mut doc = multi_part_library();
        let input = doc.snapshot().unwrap();
        // Part 2 has two pins; items #0/#1 are bare mirrors (Keep), item #2
        // overflows and appends with owner forced to 2. The shared scope
        // appends with owner forced to 0.
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        let mut appended = pin_intent("5");
        appended.name = set("P2C".to_owned());
        part2.pins = items(vec![pin_intent("3"), pin_intent("4"), appended]);
        component.parts = items(vec![part2]);
        let mut shared_new = pin_intent("10");
        shared_new.name = set("SHARED2".to_owned());
        component.pins = items(vec![pin_intent("9"), shared_new]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let pins = component.pins();
            assert_eq!(pins.len(), 7);
            let appended: Vec<(&str, SchOwnerGroup)> = pins[5..]
                .iter()
                .map(|view| (view.designator, view.owner_group))
                .collect();
            assert_eq!(
                appended,
                [
                    // Appends land after every surviving slot, in owner-unit
                    // order: component scope first, then parts ascending.
                    ("10", SchOwnerGroup::SharedPins),
                    ("5", SchOwnerGroup::Part(2)),
                ],
                "overflow appends force the scope's group owner"
            );
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn complete_scope_defines_only_its_group() {
        let mut doc = multi_part_library();
        let input = doc.snapshot().unwrap();
        // Complete part-1 pins keeping only "1": under deny it blocks; under
        // allow it removes exactly pins()[1] ("2") — part 2 and the shared
        // group keep every member.
        let complete = |delete| {
            let mut component = component_intent("Component_1");
            let mut part1 = part_intent(1);
            part1.pins = scoped(
                authored_scope(CollectionMode::Complete, delete),
                vec![pin_intent("1")],
            );
            component.parts = items(vec![part1]);
            partial_intent(vec![component])
        };
        let error =
            SchLibPatch::from_intent(&input, &complete(CollectionDelete::Deny)).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Collection { detail }
                if detail.contains("Pins") && detail.contains("delete: allow")),
            "{error}"
        );
        let patch = SchLibPatch::from_intent(&input, &complete(CollectionDelete::Allow)).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let designators: Vec<&str> = component
                .pins()
                .iter()
                .map(|view| view.designator)
                .collect();
            assert_eq!(designators, ["1", "3", "4", "9"]);
            assert_eq!(graphic_kinds(&component), ["Rectangle", "Line"]);
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &complete(CollectionDelete::Allow))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cross_kind_positional_replacement_blocks_with_both_kinds_named() {
        let input = multi_part_library().snapshot().unwrap();
        // Part 1's graphic is a Rectangle; authoring a Line at its position
        // has no preserved-field mapping.
        let mut component = component_intent("Component_1");
        let mut part1 = part_intent(1);
        part1.graphics = items(vec![graphic_intent_of(GraphicKind::Line)]);
        component.parts = items(vec![part1]);
        let error = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("part 1")
                    && detail.contains("Line")
                    && detail.contains("Rectangle")
                    && detail.contains("cross-kind")),
            "{error}"
        );
        // The matching kind at the same position elaborates (a recolor).
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        let mut recolor = graphic_intent_of(GraphicKind::Line);
        recolor.fields.color = set(Color::new(0x0000_00FF));
        part2.graphics = items(vec![recolor]);
        component.parts = items(vec![part2]);
        let patch = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        let component = expected.component(0).unwrap();
        let graphics = component.graphics();
        assert!(matches!(
            &graphics[1].detail,
            SchGraphicDetail::Line { color, .. } if *color == Color::new(0x0000_00FF)
        ));
        assert!(matches!(
            &graphics[0].detail,
            SchGraphicDetail::Rectangle { .. }
        ));
    }

    #[test]
    fn owner_group_mapping_errors_are_precise() {
        // Component-level graphics on a multi-part component cite part blocks.
        let multi = multi_part_library().snapshot().unwrap();
        let mut component = component_intent("Component_1");
        component.graphics = items(vec![graphic_intent_of(GraphicKind::Rectangle)]);
        let error = SchLibPatch::from_intent(&multi, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Graphics")
                    && detail.contains("part block")
                    && detail.contains("no shared graphics owner")),
            "{error}"
        );

        // Component-scope pins + a growth crossing the single/multi boundary
        // would silently retarget the authored items — hard error.
        let single = seeded_library().snapshot().unwrap();
        let mut component = component_intent("Component_1");
        component.pins = items(vec![pin_intent("1")]);
        component.parts = items(vec![part_intent(2)]);
        let error =
            SchLibPatch::from_intent(&single, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Pins") && detail.contains("boundary")),
            "{error}"
        );

        // The opposite direction: component-scope pins + a complete parts
        // shrink crossing multi → single is the same silent-retarget hazard.
        let mut component = component_intent("Component_1");
        component.pins = items(vec![pin_intent("9")]);
        component.parts = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Allow),
            vec![part_intent(1)],
        );
        let error = SchLibPatch::from_intent(&multi, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Pins")
                    && detail.contains("boundary")
                    && detail.contains("from 2 to 1")),
            "{error}"
        );

        // Two scopes over one group (component scope and part 1 on a
        // single-part component) conflict.
        let mut component = component_intent("Component_1");
        component.pins = items(vec![pin_intent("1")]);
        let mut part1 = part_intent(1);
        part1.pins = items(vec![pin_intent("1")]);
        component.parts = items(vec![part1]);
        let error =
            SchLibPatch::from_intent(&single, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Pins") && detail.contains("exactly one scope")),
            "{error}"
        );

        // `inherit` on a native pin field is out of definition scope.
        let mut component = component_intent("Component_1");
        let mut pin = pin_intent("1");
        pin.name = FieldIntent::Inherit(Span::new(0, 1));
        component.pins = items(vec![pin]);
        let error =
            SchLibPatch::from_intent(&single, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("step 7") && detail.contains("SchDoc")),
            "{error}"
        );
    }

    #[test]
    fn pin_axes_preserve_independently_and_hidden_net_is_guarded() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();

        // Create a pin at an explicit location (single-part component scope
        // maps to Part(1)).
        let mut component = component_intent("Component_1");
        let mut created = pin_intent("1");
        created.name = set("D0".to_owned());
        created.location = set(CoordPoint::new(mil(100), mil(200)));
        component.pins = items(vec![created]);
        let create_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &create_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            expected.component(0).unwrap().pins()[0].location,
            CoordPoint::new(mil(100), mil(200))
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &create_intent)
                .unwrap()
                .is_empty()
        );

        // Authoring ONLY x preserves the destination y (independent axes).
        let mut component = component_intent("Component_1");
        let mut move_x = pin_intent("1");
        move_x.location_x = set(mil(300));
        component.pins = items(vec![move_x]);
        let move_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &move_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            expected.component(0).unwrap().pins()[0].location,
            CoordPoint::new(mil(300), mil(200)),
            "the unauthored y axis must be preserved"
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &move_intent)
                .unwrap()
                .is_empty()
        );

        // An unsatisfiable hidden_net_name Set hard-errors with the
        // increment-C rationale (stored-or-empty only).
        let mut component = component_intent("Component_1");
        let mut hidden = pin_intent("1");
        hidden.hidden_net_name = set("VCC".to_owned());
        component.pins = items(vec![hidden]);
        let error =
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &partial_intent(vec![component]))
                .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("hidden_net_name")
                    && detail.contains("preserved-only")
                    && detail.contains("stored value")),
            "{error}"
        );
    }

    #[test]
    fn part_growth_via_intent_creates_children_in_the_new_part() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        let mut pin = pin_intent("7");
        pin.name = set("NEW_PART_PIN".to_owned());
        part2.pins = items(vec![pin]);
        let mut rect = graphic_intent_of(GraphicKind::Rectangle);
        rect.fields.from = set(CoordPoint::zero());
        rect.fields.to = set(CoordPoint::new(mil(80), mil(40)));
        part2.graphics = items(vec![rect]);
        component.parts = items(vec![part2]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert!(patch.parts_shrinks().is_empty());
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            assert_eq!(component.effective_part_count().unwrap(), 2);
            let pins = component.pins();
            assert_eq!(pins.len(), 1);
            assert_eq!(pins[0].owner_group, SchOwnerGroup::Part(2));
            let graphics = component.graphics();
            assert_eq!(graphics.len(), 1);
            assert_eq!(graphics[0].owner_group, SchOwnerGroup::Part(2));
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty()
        );

        // part_count alone grows too; lowering it under partial scope blocks.
        let state = doc.snapshot().unwrap();
        let mut grow = component_intent("Component_1");
        grow.part_count = set(PartCount::new(4).unwrap());
        let grow_intent = partial_intent(vec![grow]);
        let patch = SchLibPatch::from_intent(&state, &grow_intent).unwrap();
        let grown = patch.resulting_snapshot().unwrap();
        assert_eq!(
            grown.component(0).unwrap().effective_part_count().unwrap(),
            4
        );
        let mut lower = component_intent("Component_1");
        lower.part_count = set(PartCount::new(1).unwrap());
        let error = SchLibPatch::from_intent(&state, &partial_intent(vec![lower])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Parts") && detail.contains("complete parts scope")),
            "{error}"
        );
    }

    #[test]
    fn complete_parts_shrink_reports_deletions_through_patch_inspection() {
        let mut doc = seeded_library();
        let pristine = doc.snapshot().unwrap();

        // Grow to two parts with children in part 2 (via intent).
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        let mut pin = pin_intent("7");
        pin.name = set("DOOMED".to_owned());
        part2.pins = items(vec![pin]);
        let mut rect = graphic_intent_of(GraphicKind::Rectangle);
        rect.fields.to = set(CoordPoint::new(mil(80), mil(40)));
        part2.graphics = items(vec![rect]);
        component.parts = items(vec![part2]);
        let grow_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&pristine, &grow_intent).unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        let grown = doc.snapshot().unwrap();

        // Complete parts scope retaining only part 1: deny blocks naming the
        // permission; allow shrinks and reports every deleted descendant.
        let shrink = |delete| {
            let mut component = component_intent("Component_1");
            component.parts = scoped(
                authored_scope(CollectionMode::Complete, delete),
                vec![part_intent(1)],
            );
            partial_intent(vec![component])
        };
        let error = SchLibPatch::from_intent(&grown, &shrink(CollectionDelete::Deny)).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("Parts") && detail.contains("delete: allow")),
            "{error}"
        );
        let patch = SchLibPatch::from_intent(&grown, &shrink(CollectionDelete::Allow)).unwrap();
        assert_eq!(patch.parts_shrinks().len(), 1);
        let report = &patch.parts_shrinks()[0];
        assert_eq!(report.component, "Component_1");
        assert_eq!(report.removed_parts, [2]);
        assert_eq!(
            report.deleted_children.len(),
            2,
            "the doomed pin and rectangle are listed: {:?}",
            report.deleted_children
        );
        assert!(
            report
                .deleted_children
                .iter()
                .all(|child| child.owner_part_id == 2)
        );
        assert_eq!(
            report.current_part_id_adjustment, None,
            "current part 1 needs no decrement (the >1 case is format-tested; \
             no public path raises current_part_id)"
        );
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            expected, pristine,
            "growth followed by complete shrink restores the pristine state"
        );
        save_reopen_equals(&doc, &expected);
        // An empty complete parts scope can never drop the last slot.
        let mut component = component_intent("Component_1");
        component.parts = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Allow),
            vec![],
        );
        let error =
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &partial_intent(vec![component]))
                .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("at least one")),
            "{error}"
        );
    }

    #[test]
    fn part_swap_fanout_rewrites_prefixes_excluding_owner_zero() {
        // Unanimous prefix "A" on part 2; shared pin keeps its own "X".
        let mut doc = multi_part_library_with_swaps("A", "A");
        let input = doc.snapshot().unwrap();
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        part2.part_swap_group = set("B".to_owned());
        component.parts = items(vec![part2]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let pins = component.pins();
            assert_eq!(pins[2].part_swap_group, "B");
            assert_eq!(pins[3].part_swap_group, "B");
            assert_eq!(
                pins[4].part_swap_group, "X",
                "owner-0 pins are excluded from fan-out"
            );
            assert_eq!(pins[0].part_swap_group, "");
            let parts = component.parts().unwrap();
            assert_eq!(parts[1].swap_group, SchPartSwapGroup::Group("B".to_owned()));
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty(),
            "fan-out to the already-unanimous value must replan empty"
        );

        // Clear fans out the empty prefix.
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        part2.part_swap_group = FieldIntent::Clear(Span::new(0, 1));
        component.parts = items(vec![part2]);
        let clear_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &clear_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        let component = expected.component(0).unwrap();
        assert_eq!(component.pins()[2].part_swap_group, "");
        assert_eq!(
            component.parts().unwrap()[1].swap_group,
            SchPartSwapGroup::None
        );
        save_reopen_equals(&doc, &expected);

        // A Mixed destination projection blocks any fan-out.
        let mixed = multi_part_library_with_swaps("A", "B").snapshot().unwrap();
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        part2.part_swap_group = set("C".to_owned());
        component.parts = items(vec![part2]);
        let error = SchLibPatch::from_intent(&mixed, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { context, detail }
                if context.contains("part 2") && detail.contains("MIXED")),
            "{error}"
        );

        // A pin-level authored prefix contradicting the fan-out target blocks.
        let unanimous = multi_part_library_with_swaps("A", "A").snapshot().unwrap();
        let mut component = component_intent("Component_1");
        let mut part2 = part_intent(2);
        part2.part_swap_group = set("B".to_owned());
        let mut pin = pin_intent("3");
        pin.part_swap_group = set("C".to_owned());
        part2.pins = items(vec![pin]);
        component.parts = items(vec![part2]);
        let error =
            SchLibPatch::from_intent(&unanimous, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("contradict")),
            "{error}"
        );
    }

    #[test]
    fn layout_anchor_and_auto_size_evaluate_to_a_fixed_point() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let span = Span::new(0, 1);
        let mut component = component_intent("Component_1");
        let mut body = graphic_intent_of(GraphicKind::Rectangle);
        body.auto_size = Some(AutoSizeIntent {
            padding: FieldIntent::Absent,
            min_width: set(mil(200)),
            min_height: set(mil(100)),
            infer_from: true,
            infer_to: true,
            span,
        });
        let mut pin = pin_intent("1");
        pin.name = set("ANCHORED".to_owned());
        pin.placement = Some(PinPlacementIntent {
            graphic: Spanned::new(
                GraphicSourceRef {
                    owner: LayoutOwner::Component,
                    index: 0,
                },
                span,
            ),
            edge: Spanned::new(AnchorEdge::Left, span),
            position: Spanned::new(AnchorPosition::Center, span),
            gap: FieldIntent::Absent,
            side: FieldIntent::Absent,
            offset: FieldIntent::Absent,
            span,
        });
        component.graphics = items(vec![body]);
        component.pins = items(vec![pin]);
        let intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let graphics = component.graphics();
            let SchGraphicDetail::Rectangle { from, to, .. } = &graphics[0].detail else {
                panic!("auto-sized rectangle expected");
            };
            assert_eq!(to.x.raw() - from.x.raw(), mil(200).raw());
            assert_eq!(to.y.raw() - from.y.raw(), mil(100).raw());
            let pins = component.pins();
            assert_eq!(pins[0].orientation, RotationBy90::Rotate0);
            assert_eq!(
                pins[0].location.x.raw(),
                from.x.raw() - pins[0].length.raw(),
                "left-edge outside placement puts the pin base one length out"
            );
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &intent)
                .unwrap()
                .is_empty(),
            "layout evaluation must be a fixed point on re-elaboration"
        );

        // Anchored placement combined with absolute coordinates errors with
        // the span-carrying spec diagnostic.
        let mut component = component_intent("Component_1");
        let mut conflicted = pin_intent("1");
        conflicted.placement = Some(PinPlacementIntent {
            graphic: Spanned::new(
                GraphicSourceRef {
                    owner: LayoutOwner::Component,
                    index: 0,
                },
                span,
            ),
            edge: Spanned::new(AnchorEdge::Left, span),
            position: Spanned::new(AnchorPosition::Center, span),
            gap: FieldIntent::Absent,
            side: FieldIntent::Absent,
            offset: FieldIntent::Absent,
            span,
        });
        conflicted.location_x = set(mil(10));
        component.pins = items(vec![conflicted]);
        let error =
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &partial_intent(vec![component]))
                .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Spec { source, .. }
                if source.to_string().contains("anchored placement")),
            "{error}"
        );

        // Auto-size with NO managed pins scope at all (regression: the
        // effective-pin flag vector must not assume a pins scope exists).
        // The partial item addresses the stage-1 rectangle at filtered
        // index 0 and re-solves its box (infer-both is origin-centered).
        let mut lone = graphic_intent_of(GraphicKind::Rectangle);
        lone.auto_size = Some(AutoSizeIntent {
            padding: FieldIntent::Absent,
            min_width: set(mil(80)),
            min_height: set(mil(40)),
            infer_from: true,
            infer_to: true,
            span,
        });
        let lone_intent = partial_intent(vec![{
            let mut c = component_intent("Component_1");
            c.graphics = items(vec![lone]);
            c
        }]);
        let base = doc.snapshot().unwrap();
        let patch = SchLibPatch::from_intent(&base, &lone_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let graphics = component.graphics();
            let SchGraphicDetail::Rectangle { from, to, .. } = &graphics[0].detail else {
                panic!("re-solved auto-sized rectangle expected");
            };
            assert_eq!(to.x.raw() - from.x.raw(), mil(80).raw());
            assert_eq!(to.y.raw() - from.y.raw(), mil(40).raw());
        }
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &lone_intent)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn image_sources_lower_to_asset_ops_with_missing_asset_block() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("logo.altium-image");
        std::fs::write(&bundle, b"bundle body one").unwrap();
        let source = ImageAssetSource {
            path: "logo.altium-image".to_owned(),
            declaring_source: Some(temp.path().join("library.schlib-spec")),
        };
        let image_item = |source_value: &ImageAssetSource| {
            let mut image = graphic_intent_of(GraphicKind::Image);
            image.fields.file_name = set("logo.png".to_owned());
            image.fields.source = FieldIntent::Set(spanned(Some(source_value.clone())));
            image.fields.to = set(CoordPoint::new(mil(100), mil(60)));
            let mut component = component_intent("Component_1");
            component.graphics = items(vec![image]);
            partial_intent(vec![component])
        };
        let intent = image_item(&source);

        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        // No captured asset provided → block naming the provider.
        let error = SchLibPatch::from_intent(&input, &intent).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("SchLibIntentAssets")),
            "{error}"
        );

        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&source).unwrap());
        let patch = SchLibPatch::from_intent_with_assets(&input, &intent, &assets).unwrap();
        assert!(matches!(
            &patch.image_asset_ops()[0],
            SchLibImageAssetOp::Insert { index: 0, name, .. } if name == "logo.png"
        ));
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let component = expected.component(0).unwrap();
            let graphics = component.graphics();
            assert!(matches!(
                &graphics[0].detail,
                SchGraphicDetail::Image { file_name, embed_image: true, .. }
                    if *file_name == "logo.png"
            ));
            assert_eq!(
                component.captured_image_asset_names().collect::<Vec<_>>(),
                ["logo.png"]
            );
        }
        assert_eq!(
            doc.image_resources().next().unwrap().payload,
            b"bundle body one"
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &intent, &assets)
                .unwrap()
                .is_empty(),
            "an already-stored identical asset elaborates to no op"
        );

        // A changed bundle becomes a whole-resource Replace under the stored
        // name.
        std::fs::write(&bundle, b"bundle body two").unwrap();
        let mut updated_assets = SchLibIntentAssets::new();
        updated_assets.insert(CapturedImageAsset::capture(&source).unwrap());
        let patch = SchLibPatch::from_intent_with_assets(
            &doc.snapshot().unwrap(),
            &intent,
            &updated_assets,
        )
        .unwrap();
        assert!(matches!(
            &patch.image_asset_ops()[0],
            SchLibImageAssetOp::Replace { index: 0, name, .. } if name == "logo.png"
        ));
        assert_eq!(
            patch.replacements().len(),
            0,
            "the record itself is a no-op"
        );
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            doc.image_resources().next().unwrap().payload,
            b"bundle body two"
        );

        // Clearing the source never deletes the stored asset.
        let mut cleared = graphic_intent_of(GraphicKind::Image);
        cleared.fields.source = FieldIntent::Clear(Span::new(0, 1));
        let mut component = component_intent("Component_1");
        component.graphics = items(vec![cleared]);
        let clear_intent = partial_intent(vec![component]);
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &clear_intent).unwrap();
        assert!(patch.image_asset_ops().is_empty());
        assert!(patch.is_empty(), "clear alone changes nothing");
        assert_eq!(doc.image_resources().len(), 1);
    }

    /// Builds an authored image-graphic intent item: kind Image with the
    /// given optional `file_name`, the given `source`, and a creation box.
    fn image_graphic(file_name: Option<&str>, source: &ImageAssetSource) -> GraphicIntent {
        let mut image = graphic_intent_of(GraphicKind::Image);
        if let Some(name) = file_name {
            image.fields.file_name = set(name.to_owned());
        }
        image.fields.source = FieldIntent::Set(spanned(Some(source.clone())));
        image.fields.to = set(CoordPoint::new(mil(100), mil(60)));
        image
    }

    fn image_intent(graphics: Vec<GraphicIntent>) -> SchLibIntent {
        let mut component = component_intent("Component_1");
        component.graphics = items(graphics);
        partial_intent(vec![component])
    }

    /// Seeds `seeded_library` with one embedded image record whose stored
    /// asset is named "logo.png" with the given bundle body, returning the
    /// document and the bundle's source (whose file remains on disk in
    /// `temp`).
    fn library_with_stored_logo(temp: &std::path::Path, body: &[u8]) -> (SchLib, ImageAssetSource) {
        std::fs::write(temp.join("logo.altium-image"), body).unwrap();
        let source = ImageAssetSource {
            path: "logo.altium-image".to_owned(),
            declaring_source: Some(temp.join("library.schlib-spec")),
        };
        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&source).unwrap());
        let mut doc = seeded_library();
        let intent = image_intent(vec![image_graphic(Some("logo.png"), &source)]);
        let patch =
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &intent, &assets)
                .unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.image_resources().next().unwrap().payload, body);
        (doc, source)
    }

    #[test]
    fn image_source_without_file_name_resolves_the_addressed_stored_name() {
        // The fallback resource-name path: `source:` authored WITHOUT
        // `file_name` binds to the positionally addressed destination
        // image's STORED name.
        let temp = tempfile::tempdir().unwrap();
        let (mut doc, source) = library_with_stored_logo(temp.path(), b"bundle body one");
        std::fs::write(temp.path().join("logo.altium-image"), b"bundle body two").unwrap();
        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&source).unwrap());
        let intent = image_intent(vec![image_graphic(None, &source)]);
        let patch =
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &intent, &assets)
                .unwrap();
        assert_eq!(patch.image_asset_ops().len(), 1);
        assert!(matches!(
            &patch.image_asset_ops()[0],
            SchLibImageAssetOp::Replace { index: 0, name, .. } if name == "logo.png"
        ));
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(
            doc.image_resources().next().unwrap().payload,
            b"bundle body two"
        );
        assert!(matches!(
            &expected.component(0).unwrap().graphics()[0].detail,
            SchGraphicDetail::Image { file_name, .. } if *file_name == "logo.png"
        ));
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &intent, &assets)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn image_source_empty_file_name_blocks() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("logo.altium-image"), b"body").unwrap();
        let source = ImageAssetSource {
            path: "logo.altium-image".to_owned(),
            declaring_source: Some(temp.path().join("library.schlib-spec")),
        };
        let intent = image_intent(vec![image_graphic(Some(""), &source)]);
        let error =
            SchLibPatch::from_intent(&seeded_library().snapshot().unwrap(), &intent).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("non-empty file_name")),
            "{error}"
        );
    }

    #[test]
    fn image_source_conflicting_bodies_for_one_name_block() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("first.altium-image"), b"first body").unwrap();
        std::fs::write(temp.path().join("second.altium-image"), b"second body").unwrap();
        let declaring = Some(temp.path().join("library.schlib-spec"));
        let first = ImageAssetSource {
            path: "first.altium-image".to_owned(),
            declaring_source: declaring.clone(),
        };
        let second = ImageAssetSource {
            path: "second.altium-image".to_owned(),
            declaring_source: declaring,
        };
        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&first).unwrap());
        assets.insert(CapturedImageAsset::capture(&second).unwrap());
        let intent = image_intent(vec![
            image_graphic(Some("logo.png"), &first),
            image_graphic(Some("logo.png"), &second),
        ]);
        let error = SchLibPatch::from_intent_with_assets(
            &seeded_library().snapshot().unwrap(),
            &intent,
            &assets,
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("conflicting bodies")
                    && detail.contains("first.altium-image")
                    && detail.contains("second.altium-image")),
            "{error}"
        );
    }

    #[test]
    fn case_variant_image_names_resolve_one_pairing_target() {
        // E2 review regression (major): the conflicting-bodies check keys on
        // the RESOLVED pairing target, so case-variant spellings addressing
        // one stored slot can neither bypass the block nor emit two
        // self-overwriting Replace ops.
        let temp = tempfile::tempdir().unwrap();
        let (mut doc, source) = library_with_stored_logo(temp.path(), b"bundle body one");
        std::fs::write(temp.path().join("upper.altium-image"), b"upper body").unwrap();
        let upper = ImageAssetSource {
            path: "upper.altium-image".to_owned(),
            declaring_source: source.declaring_source.clone(),
        };
        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&source).unwrap());
        assets.insert(CapturedImageAsset::capture(&upper).unwrap());

        // Different bodies for the case-variant spellings of one stored
        // slot: hard error, never a silent overwrite.
        let conflicting = image_intent(vec![
            image_graphic(Some("LOGO.png"), &upper),
            image_graphic(Some("logo.png"), &source),
        ]);
        let error =
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &conflicting, &assets)
                .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("conflicting bodies") && detail.contains("logo.png")),
            "{error}"
        );

        // One body under both spellings: exactly ONE Replace op under the
        // STORED spelling (case-insensitive pairing, the native order), and
        // apply → re-elaborate reaches the fixed point.
        std::fs::write(temp.path().join("logo.altium-image"), b"bundle body two").unwrap();
        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&source).unwrap());
        let agreeing = image_intent(vec![
            image_graphic(Some("LOGO.png"), &source),
            image_graphic(Some("logo.png"), &source),
        ]);
        let patch =
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &agreeing, &assets)
                .unwrap();
        assert_eq!(
            patch.image_asset_ops().len(),
            1,
            "one storage slot takes one op"
        );
        assert!(matches!(
            &patch.image_asset_ops()[0],
            SchLibImageAssetOp::Replace { index: 0, name, .. } if name == "logo.png"
        ));
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.image_resources().len(), 1);
        assert_eq!(
            doc.image_resources().next().unwrap().payload,
            b"bundle body two"
        );
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &agreeing, &assets)
                .unwrap()
                .is_empty(),
            "apply → re-elaborate must be empty"
        );
    }

    #[test]
    fn case_variant_new_image_inserts_share_one_slot() {
        // The New-target arm of the per-target dedupe: not-yet-stored
        // case-variant spellings collide on the folded name.
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("first.altium-image"), b"first body").unwrap();
        std::fs::write(temp.path().join("second.altium-image"), b"second body").unwrap();
        let declaring = Some(temp.path().join("library.schlib-spec"));
        let first = ImageAssetSource {
            path: "first.altium-image".to_owned(),
            declaring_source: declaring.clone(),
        };
        let second = ImageAssetSource {
            path: "second.altium-image".to_owned(),
            declaring_source: declaring,
        };
        let mut assets = SchLibIntentAssets::new();
        assets.insert(CapturedImageAsset::capture(&first).unwrap());
        assets.insert(CapturedImageAsset::capture(&second).unwrap());

        let conflicting = image_intent(vec![
            image_graphic(Some("LOGO.png"), &first),
            image_graphic(Some("logo.png"), &second),
        ]);
        let error = SchLibPatch::from_intent_with_assets(
            &seeded_library().snapshot().unwrap(),
            &conflicting,
            &assets,
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("conflicting bodies")),
            "{error}"
        );

        // One body under both spellings: a single Insert named by the
        // first-authored spelling.
        let mut doc = seeded_library();
        let agreeing = image_intent(vec![
            image_graphic(Some("LOGO.png"), &first),
            image_graphic(Some("logo.png"), &first),
        ]);
        let patch =
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &agreeing, &assets)
                .unwrap();
        assert_eq!(patch.image_asset_ops().len(), 1);
        assert!(matches!(
            &patch.image_asset_ops()[0],
            SchLibImageAssetOp::Insert { index: 0, name, .. } if name == "LOGO.png"
        ));
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.image_resources().len(), 1);
        assert_eq!(doc.image_resources().next().unwrap().payload, b"first body");
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent_with_assets(&doc.snapshot().unwrap(), &agreeing, &assets)
                .unwrap()
                .is_empty(),
            "case-insensitive pairing makes the replan empty"
        );
    }

    #[test]
    fn created_components_carry_pins_graphics_and_parts() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();

        // Single-part creation: component-scope pins and graphics land in
        // Part(1).
        let mut single = component_intent("E2_CREATED_SINGLE");
        let mut pin = pin_intent("1");
        pin.name = set("IN".to_owned());
        single.pins = items(vec![pin]);
        let mut rect = graphic_intent_of(GraphicKind::Rectangle);
        rect.fields.to = set(CoordPoint::new(mil(100), mil(100)));
        single.graphics = items(vec![rect]);
        let single_intent = partial_intent(vec![single]);
        let patch = SchLibPatch::from_intent(&input, &single_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let created = expected.component(1).unwrap();
            assert_eq!(created.lib_reference(), "E2_CREATED_SINGLE");
            assert_eq!(created.effective_part_count().unwrap(), 1);
            assert_eq!(created.pins()[0].owner_group, SchOwnerGroup::Part(1));
            assert_eq!(created.graphics()[0].owner_group, SchOwnerGroup::Part(1));
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &single_intent)
                .unwrap()
                .is_empty()
        );

        // Multi-part creation: part blocks grow the blank, component-scope
        // pins land shared (owner 0), part children in their slots.
        let mut multi = component_intent("E2_CREATED_MULTI");
        let mut shared = pin_intent("9");
        shared.name = set("SHARED".to_owned());
        multi.pins = items(vec![shared]);
        let mut part1 = part_intent(1);
        let mut rect = graphic_intent_of(GraphicKind::Rectangle);
        rect.fields.to = set(CoordPoint::new(mil(60), mil(60)));
        part1.graphics = items(vec![rect]);
        let mut part2 = part_intent(2);
        let mut part2_pin = pin_intent("2");
        part2_pin.name = set("P2".to_owned());
        part2.pins = items(vec![part2_pin]);
        multi.parts = items(vec![part1, part2]);
        let multi_intent = partial_intent(vec![multi]);
        let patch = SchLibPatch::from_intent(&doc.snapshot().unwrap(), &multi_intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        {
            let created = expected.component(2).unwrap();
            assert_eq!(created.effective_part_count().unwrap(), 2);
            let owners: Vec<SchOwnerGroup> =
                created.pins().iter().map(|view| view.owner_group).collect();
            assert_eq!(
                owners,
                [SchOwnerGroup::SharedPins, SchOwnerGroup::Part(2)],
                "component-scope pins of a created multi-part component are shared"
            );
            assert_eq!(created.graphics()[0].owner_group, SchOwnerGroup::Part(1));
        }
        save_reopen_equals(&doc, &expected);
        assert!(
            SchLibPatch::from_intent(&doc.snapshot().unwrap(), &multi_intent)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn positional_mirror_intents_are_empty() {
        let doc = multi_part_library_with_swaps("A", "A");
        let input = doc.snapshot().unwrap();
        // A complete mirror of every positional group plus the parts
        // structure elaborates to an empty patch.
        let mut mirror = component_intent("Component_1");
        mirror.part_count = set(PartCount::new(2).unwrap());
        mirror.pins = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![pin_intent("9")],
        );
        let mut part1 = part_intent(1);
        part1.pins = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![pin_intent("1"), pin_intent("2")],
        );
        part1.graphics = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![graphic_intent_of(GraphicKind::Rectangle)],
        );
        let mut part2 = part_intent(2);
        part2.part_swap_group = set("A".to_owned());
        part2.pins = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![pin_intent("3"), pin_intent("4")],
        );
        part2.graphics = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![graphic_intent_of(GraphicKind::Line)],
        );
        mirror.parts = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![part1, part2],
        );
        let intent = partial_intent(vec![mirror]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert!(patch.is_empty(), "a full positional mirror must plan empty");
        assert_eq!(patch.resulting_snapshot().unwrap(), input);
    }

    #[test]
    fn noop_intent_yields_empty_patch_for_every_collection() {
        let doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let component = input.component(0).unwrap();

        // Mirror the complete destination state explicitly: scalars Set to
        // current values, every keyed collection listed completely (complete
        // scopes), maps authored explicitly with the current definers.
        let mut mirror = component_intent("Component_1");
        mirror.description = set(component.description().map(str::to_owned));
        mirror.show_hidden_pins = set(component.show_hidden_pins());
        mirror.parameters = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            component
                .parameters()
                .unwrap()
                .iter()
                .map(|view| ParameterIntent {
                    name: spanned(view.name.to_owned()),
                    text: set(view.text.to_owned()),
                    is_hidden: set(view.is_hidden),
                })
                .collect(),
        );
        mirror.aliases = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            component
                .aliases()
                .map(|alias| spanned(alias.to_owned()))
                .collect(),
        );
        mirror.footprints = scoped(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            component
                .footprint_maps()
                .unwrap()
                .footprints
                .iter()
                .map(|view| FootprintIntent {
                    source: None,
                    model_name: spanned(view.model_name.to_owned()),
                    description: set(Some(view.description.to_owned())),
                    maps: scoped(
                        authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
                        view.pin_pad_maps
                            .iter()
                            .filter(|map| map.pads.len() == 1)
                            .map(|map| map_pair(map.pin, &map.pads[0]))
                            .collect(),
                    ),
                    explicit_maps: true,
                })
                .collect(),
        );
        let intent = intent_of(
            authored_scope(CollectionMode::Complete, CollectionDelete::Deny),
            vec![mirror],
        );
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert!(patch.is_empty(), "a full mirror intent must plan empty");
        assert_eq!(patch.resulting_snapshot().unwrap(), input);

        // The empty partial intent is trivially empty too.
        assert!(
            SchLibPatch::from_intent(&input, &partial_intent(vec![]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn schema_version_and_inconsistent_maps_are_rejected() {
        let input = SchLib::new_blank_ad26().unwrap().snapshot().unwrap();
        let mut wrong = partial_intent(vec![]);
        wrong.schema_version = INTENT_SCHEMA_VERSION + 1;
        let error = SchLibPatch::from_intent(&input, &wrong).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("schema version")),
            "{error}"
        );
        // maps content without explicit_maps is an inconsistent intent.
        let mut component = component_intent("Component_1");
        let mut inconsistent = footprint("FPX");
        inconsistent.maps = items(vec![map_pair("1", "A1")]);
        inconsistent.explicit_maps = false;
        component.footprints = items(vec![inconsistent]);
        let error = SchLibPatch::from_intent(&input, &partial_intent(vec![component])).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("explicit maps body")),
            "{error}"
        );
    }

    #[test]
    fn library_ops_lower_to_one_edit_with_verified_image_assets() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("logo.altium-image");
        std::fs::write(&bundle, b"complete encoded bundle").unwrap();
        let asset = CapturedImageAsset::capture(&ImageAssetSource {
            path: "logo.altium-image".to_owned(),
            declaring_source: Some(temp.path().join("library.schlib-spec")),
        })
        .unwrap();

        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();
        let patch = SchLibPatch::from_library_ops(
            &input,
            [
                SchLibLibraryOp::FontAppend(SchFontAppend {
                    name: "Arial".to_owned(),
                    size: 12,
                    rotation: 0,
                    bold: true,
                    italic: false,
                    underline: false,
                    strikeout: false,
                }),
                SchLibLibraryOp::ImageAsset(SchLibImageAssetOp::Insert {
                    index: 0,
                    name: "logo.png".to_owned(),
                    asset: asset.clone(),
                }),
                SchLibLibraryOp::AliasAdd(SchLibAliasChange {
                    component: "Component_1".to_owned(),
                    alias: "OPS_ALIAS".to_owned(),
                }),
                SchLibLibraryOp::ComponentCreate(SchNewComponent::CloneOf {
                    source: 0,
                    lib_reference: "CLONE_1".to_owned(),
                }),
            ],
        )
        .unwrap();
        assert_eq!(patch.font_appends().len(), 1);
        assert_eq!(patch.image_asset_ops().len(), 1);
        assert_eq!(patch.alias_additions().len(), 1);
        assert_eq!(patch.created_components().count(), 1);
        assert!(!patch.is_empty());
        let expected = patch.resulting_snapshot().unwrap();
        assert_eq!(
            patch.resulting_snapshot().unwrap(),
            expected,
            "cached result is stable despite the clone's fresh unique id"
        );
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.snapshot().unwrap(), expected);
        assert_eq!(expected.component_count(), 2);
        assert_eq!(expected.component(1).unwrap().lib_reference(), "CLONE_1");
        assert_eq!(
            doc.image_resources().next().unwrap().payload,
            b"complete encoded bundle"
        );
        let expected_snapshot = expected.clone();
        save_reopen_equals(&doc, &expected_snapshot);

        // Tampering with the captured bundle after review blocks apply.
        let stale_patch = SchLibPatch::from_library_ops(
            &doc.snapshot().unwrap(),
            [SchLibLibraryOp::ImageAsset(SchLibImageAssetOp::Replace {
                index: 0,
                name: "logo.png".to_owned(),
                asset,
            })],
        )
        .unwrap();
        std::fs::write(&bundle, b"tampered").unwrap();
        let before = doc.snapshot().unwrap();
        let error = stale_patch.apply_in_memory(&mut doc).unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::ImageAsset { .. }),
            "{error}"
        );
        assert_eq!(
            doc.snapshot().unwrap(),
            before,
            "failed apply must not mutate"
        );
    }

    #[test]
    fn library_ops_component_removal_is_explicit_and_alias_aware() {
        let mut doc = seeded_library();
        let input = doc.snapshot().unwrap();

        // Removing the aliased component without releasing its alias is the
        // D1 dangling-redirection hard error, surfaced at construction.
        let error = SchLibPatch::from_library_ops(
            &input,
            [SchLibLibraryOp::ComponentRemove {
                lib_reference: "Component_1".to_owned(),
            }],
        )
        .unwrap_err();
        assert!(matches!(&error, SchLibPatchError::Format { .. }), "{error}");

        // Unknown and duplicate removal names are elaboration errors.
        let error = SchLibPatch::from_library_ops(
            &input,
            [SchLibLibraryOp::ComponentRemove {
                lib_reference: "MISSING".to_owned(),
            }],
        )
        .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("MISSING")),
            "{error}"
        );
        let remove_twice = SchLibLibraryOp::ComponentRemove {
            lib_reference: "Component_1".to_owned(),
        };
        let error = SchLibPatch::from_library_ops(&input, [remove_twice.clone(), remove_twice])
            .unwrap_err();
        assert!(
            matches!(&error, SchLibPatchError::Elaboration { detail, .. }
                if detail.contains("more than once")),
            "{error}"
        );

        // Clone, then remove the clone: restores the input exactly.
        let clone = SchLibPatch::from_library_ops(
            &input,
            [SchLibLibraryOp::ComponentCreate(SchNewComponent::CloneOf {
                source: 0,
                lib_reference: "CLONE_TO_REMOVE".to_owned(),
            })],
        )
        .unwrap();
        clone.apply_in_memory(&mut doc).unwrap();
        let removal = SchLibPatch::from_library_ops(
            &doc.snapshot().unwrap(),
            [SchLibLibraryOp::ComponentRemove {
                lib_reference: "CLONE_TO_REMOVE".to_owned(),
            }],
        )
        .unwrap();
        assert_eq!(removal.removed_components().len(), 1);
        removal.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.snapshot().unwrap(), input);
    }
}
