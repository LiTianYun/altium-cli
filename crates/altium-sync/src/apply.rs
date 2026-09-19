//! Checked apply of compile-direction saved plans (step-3 increment B;
//! design authority `/tmp/step3/design.md` §2 apply-side verification order;
//! NEXT.md "Checked apply and recovery" steps 1–4 IN MEMORY through a plain
//! save + reopen typed-equality check).
//!
//! [`apply_saved_plan`] takes ONLY the plan — no path parameters exist to
//! override the frozen manifest paths (§S9) — and never recompiles intent,
//! re-reads untracked imports, or mints new identifiers (§S13): sources and
//! imports are hash-verified (bytes read and hashed) but never re-parsed,
//! and every installed state is a decoded payload whose `UniqueId`s were
//! frozen at planning.
//!
//! Verification order (memo §2): manifest re-resolution + §S11 re-check +
//! every hash/absence precondition → open the target FROM THE HASHED BYTES
//! → capture the immutable input snapshot → decode embedded image payloads
//! (their hashes were already verified at plan decode) →
//! resolve every patch address against it (component addresses through
//! `crate::address`) → per-patch decoded-before typed equality
//! ([`PlanError::StaleComponentState`], the saved-plan analogue of
//! `apply_in_memory`'s `StaleInput`) → execute → verify produced components
//! equal the decoded after-states and library invariants hold → save to the
//! frozen destination → reopen → typed-equality check.
//!
//! # Execution shape (the D1 two-step, reconstructed from the plan)
//!
//! The plan's patches replay through the same two-phase composition the
//! planner used, each phase ONE `SchLibEdit`:
//!
//! 1. The library-level namespace transition: font appends, image-asset ops
//!    (bodies from the hash-verified embedded payloads), the component
//!    membership change (kept input slots in ascending order — the canonical
//!    shape lowering enforces — with deletions omitted under explicit
//!    authorization and creations appended as `FromState` installs), and the
//!    alias ops. A created component is seeded with its INTERMEDIATE state
//!    (the paired replacement's `before` payload) so the planner's two-step
//!    is reproduced exactly; `AliasAdd` ops whose target is a created
//!    component are subsumed by that state install (the payload's index
//!    already carries the alias — the increment-A reconstruction note) and
//!    are dropped after verifying the alias is indeed present in the
//!    payload.
//! 2. Content after-states: one `ReplaceWithState` per replacement against
//!    the intermediate snapshot (whose slot aliases now equal the
//!    after-state's, so the alias-change rejection holds exactly as it did
//!    at planning).
//!
//! # Step-4 boundary
//!
//! NEXT.md "Checked apply and recovery" steps 1–3 run here in memory; the
//! write is step 4's slice ONLY up to "serialize, reopen, validate against
//! the planned result": a plain `SchLib::save` to the frozen destination
//! followed by a reopen typed-equality check. Staging beside the
//! destination, the durable recovery journal, commit-by-rename, and
//! interrupted-transaction recovery (steps 4's staging plus 5–7) are step-4
//! scope and are NOT provided here — a failure between save and reopen
//! leaves no journal to recover from.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::address::ComponentIndex;
use crate::plan::{
    ManifestEntry, ManifestRole, PLAN_SCHEMA_VERSION, PlanComponentPatch, PlanDirection, PlanError,
    PlanHash, PlanLibraryOp, PlanPatches, PlanPrecondition, SavedPlan, canonicalize_plan_path,
    check_manifest_collisions, path_to_string,
};
use altium_format::SchLib;
use altium_format::api::{
    ComponentStatePayload, SCHLIB_STATE_FORMAT_VERSION, SchAliasOp, SchFontAppend,
    SchImageAssetEdit, SchLibComponentEdit, SchLibEdit, SchLibSnapshot, SchNewComponent,
};

/// Outcome of one checked apply: the destination actually written plus the
/// executed patch counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPlan {
    pub destination: PathBuf,
    pub created: usize,
    pub replaced: usize,
    pub deleted: usize,
    pub library_ops: usize,
}

fn invalid(detail: impl Into<String>) -> PlanError {
    PlanError::InvalidPlan {
        detail: detail.into(),
    }
}

fn format_error(
    context: impl Into<String>,
) -> impl FnOnce(altium_format::AltiumFormatError) -> PlanError {
    let context = context.into();
    move |source| PlanError::Format { context, source }
}

/// The manifest's verified read: the target's hashed bytes and the frozen
/// destination path.
struct VerifiedManifest {
    target_bytes: Vec<u8>,
    destination: PathBuf,
}

/// Re-resolves every manifest path, re-runs the §S11 collision checks, and
/// verifies every hash/absence precondition. Spec sources, imports, and
/// image bundles are read and HASHED only — never re-parsed (§S13).
fn verify_manifest(manifest: &[ManifestEntry]) -> Result<VerifiedManifest, PlanError> {
    let mut target: Option<(ManifestRole, Vec<u8>)> = None;
    let mut destination: Option<PathBuf> = None;
    let mut in_place = 0usize;
    let mut target_documents = 0usize;
    let mut destinations = 0usize;
    for entry in manifest {
        let declared = Path::new(&entry.declared_path);
        let context = format!(
            "manifest {} entry '{}'",
            role_name(entry.role),
            entry.declared_path
        );
        let actual = canonicalize_plan_path(declared)?;
        let actual_string = path_to_string(&actual, &context)?;
        if actual_string != entry.canonical_path {
            return Err(PlanError::PathResolutionChanged {
                role: role_name(entry.role).to_owned(),
                declared: entry.declared_path.clone(),
                expected: entry.canonical_path.clone(),
                actual: actual_string,
            });
        }
        match &entry.precondition {
            PlanPrecondition::Hash(hash) => {
                let bytes = std::fs::read(declared).map_err(|source| PlanError::Io {
                    context: context.clone(),
                    source,
                })?;
                let actual_hash = PlanHash::of_bytes(&bytes);
                if actual_hash != *hash {
                    return Err(PlanError::PreconditionFailed {
                        context,
                        detail: format!(
                            "content hash mismatch: recorded {} ({} bytes), actual {} ({} \
                             bytes)",
                            hash.hex, hash.byte_len, actual_hash.hex, actual_hash.byte_len
                        ),
                    });
                }
                match entry.role {
                    ManifestRole::InPlaceTarget => {
                        in_place += 1;
                        target = Some((entry.role, bytes));
                        destination = Some(actual);
                    }
                    ManifestRole::TargetDocument => {
                        target_documents += 1;
                        target = Some((entry.role, bytes));
                    }
                    ManifestRole::Destination => {
                        // A declared-overwrite destination: hash verified;
                        // the write goes here.
                        destinations += 1;
                        destination = Some(actual);
                    }
                    ManifestRole::SpecSource
                    | ManifestRole::SpecImport
                    | ManifestRole::ImageAssetBundle => {}
                }
            }
            PlanPrecondition::Absent => {
                match std::fs::symlink_metadata(declared) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(PlanError::PreconditionFailed {
                            context,
                            detail: "output path was recorded as absent, but something now \
                                     occupies it"
                                .to_owned(),
                        });
                    }
                    Err(source) => return Err(PlanError::Io { context, source }),
                }
                if entry.role != ManifestRole::Destination {
                    return Err(invalid(format!(
                        "absence precondition on non-Destination manifest entry '{}'",
                        entry.declared_path
                    )));
                }
                destinations += 1;
                destination = Some(actual);
            }
        }
    }
    check_manifest_collisions(manifest, None)?;
    let valid_shape = (in_place == 1 && target_documents == 0 && destinations == 0)
        || (in_place == 0 && target_documents == 1 && destinations == 1);
    if !valid_shape {
        return Err(invalid(format!(
            "the manifest must declare exactly one InPlaceTarget, or exactly one \
             TargetDocument with one Destination; found {in_place} InPlaceTarget, \
             {target_documents} TargetDocument, {destinations} Destination"
        )));
    }
    let (_, target_bytes) = target.ok_or_else(|| invalid("no target artifact in manifest"))?;
    let destination = destination.ok_or_else(|| invalid("no destination artifact in manifest"))?;
    Ok(VerifiedManifest {
        target_bytes,
        destination,
    })
}

fn role_name(role: ManifestRole) -> &'static str {
    match role {
        ManifestRole::SpecSource => "SpecSource",
        ManifestRole::SpecImport => "SpecImport",
        ManifestRole::ImageAssetBundle => "ImageAssetBundle",
        ManifestRole::TargetDocument => "TargetDocument",
        ManifestRole::Destination => "Destination",
        ManifestRole::InPlaceTarget => "InPlaceTarget",
    }
}

/// Outcome of the in-memory execution core: the produced snapshot plus the
/// executed patch counts.
pub(crate) struct ExecutedPlan {
    pub(crate) result: SchLibSnapshot,
    pub(crate) created: usize,
    pub(crate) replaced: usize,
    pub(crate) deleted: usize,
    pub(crate) library_ops: usize,
}

/// Executes a decoded compile-direction plan against its manifest-verified
/// target and writes the frozen destination. See the module documentation
/// for the complete contract and the step-4 boundary.
pub fn apply_saved_plan(plan: &SavedPlan) -> Result<AppliedPlan, PlanError> {
    // 1. Structural gate: exact versions and direction, before any read.
    if plan.schema_version != PLAN_SCHEMA_VERSION {
        return Err(PlanError::UnsupportedSchemaVersion {
            found: plan.schema_version,
            supported: PLAN_SCHEMA_VERSION,
        });
    }
    if plan.state_format_version != SCHLIB_STATE_FORMAT_VERSION {
        return Err(PlanError::UnsupportedStateFormatVersion {
            found: plan.state_format_version,
            supported: SCHLIB_STATE_FORMAT_VERSION,
        });
    }
    if plan.direction != PlanDirection::Compile {
        return Err(invalid(
            "apply_saved_plan executes compile-direction plans; this plan's direction is \
             not Compile",
        ));
    }

    // 2. Manifest re-resolution, §S11 re-check, every precondition.
    let verified = verify_manifest(&plan.manifest)?;

    // 3. Open the target FROM THE HASHED BYTES and capture the immutable
    //    input snapshot.
    let library = SchLib::from_bytes(&verified.target_bytes).map_err(format_error(
        "parsing the target from its hash-verified bytes",
    ))?;
    let input = library
        .snapshot()
        .map_err(format_error("capturing the target snapshot"))?;

    // 4–11. The shared execution core (also run as the lowering-time dry
    // run, so an emitted plan is guaranteed executable).
    let executed = execute_compile_patches(plan, &input)?;

    // 12. Materialize (library invariants), save to the frozen destination,
    //     reopen, and check typed equality. Plain save: staging/journal/
    //     commit-by-rename are step 4 (module docs).
    let produced = executed
        .result
        .materialize()
        .map_err(format_error("materializing the produced library"))?;
    produced
        .save(&verified.destination)
        .map_err(format_error(format!(
            "saving the produced library to '{}'",
            verified.destination.display()
        )))?;
    let reopened = SchLib::open(&verified.destination)
        .map_err(format_error(format!(
            "reopening '{}'",
            verified.destination.display()
        )))?
        .snapshot()
        .map_err(format_error("capturing the reopened snapshot"))?;
    if reopened != executed.result {
        return Err(PlanError::ApplyVerification {
            context: format!("reopened '{}'", verified.destination.display()),
            detail: "reopened typed state differs from the produced result".to_owned(),
        });
    }

    Ok(AppliedPlan {
        destination: verified.destination,
        created: executed.created,
        replaced: executed.replaced,
        deleted: executed.deleted,
        library_ops: executed.library_ops,
    })
}

/// The in-memory execution core (module docs steps: image-payload decode,
/// patch partitioning, address resolution, before-equality, the two-phase
/// D1 reconstruction, and after-state/membership verification). Shared by
/// [`apply_saved_plan`] and the lowering-time dry run in `crate::plan`
/// (which guarantees at PLANNING time that the emitted plan is executable
/// and reproduces the reviewed result — e.g. a `FromState` install of a
/// cloned wild component fails here with the install's own message,
/// rejecting the plan before emission instead of at apply).
pub(crate) fn execute_compile_patches(
    plan: &SavedPlan,
    input: &SchLibSnapshot,
) -> Result<ExecutedPlan, PlanError> {
    let PlanPatches::Compile {
        library_ops,
        components,
    } = &plan.patches;

    // Decode every embedded image payload (hash-verified bodies).
    let mut image_bodies: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for (position, op) in library_ops.iter().enumerate() {
        if let PlanLibraryOp::ImageAssetInsert { payload, .. }
        | PlanLibraryOp::ImageAssetReplace { payload, .. } = op
        {
            image_bodies.insert(position, payload.decoded_bytes()?);
        }
    }

    // Partition and cross-check the component patches.
    let mut creates: Vec<(&str, &ComponentStatePayload)> = Vec::new();
    let mut replaces: Vec<(usize, &str, &ComponentStatePayload, &ComponentStatePayload)> =
        Vec::new();
    let mut deletes: Vec<(usize, &str, &ComponentStatePayload)> = Vec::new();
    for patch in components {
        match patch {
            PlanComponentPatch::Create {
                lib_reference,
                after,
            } => {
                if lib_reference != after.lib_reference() {
                    return Err(invalid(format!(
                        "creation of '{lib_reference}' embeds an after-state for '{}'",
                        after.lib_reference()
                    )));
                }
                if creates
                    .iter()
                    .any(|(name, _)| *name == lib_reference.as_str())
                {
                    return Err(invalid(format!(
                        "component '{lib_reference}' is created more than once"
                    )));
                }
                creates.push((lib_reference.as_str(), after));
            }
            PlanComponentPatch::Replace {
                address,
                before,
                after,
                parts_shrink: _,
            } => {
                if address.lib_reference != before.lib_reference()
                    || address.lib_reference != after.lib_reference()
                {
                    return Err(invalid(format!(
                        "replacement of '{}' embeds states for '{}'/'{}'",
                        address.lib_reference,
                        before.lib_reference(),
                        after.lib_reference()
                    )));
                }
                if replaces.iter().any(|(index, name, _, _)| {
                    *index == address.index || *name == address.lib_reference.as_str()
                }) {
                    return Err(invalid(format!(
                        "component '{}' (final slot {}) is replaced more than once",
                        address.lib_reference, address.index
                    )));
                }
                replaces.push((address.index, address.lib_reference.as_str(), before, after));
            }
            PlanComponentPatch::Delete { address, before } => {
                if address.lib_reference != before.lib_reference() {
                    return Err(invalid(format!(
                        "deletion of '{}' embeds a before-state for '{}'",
                        address.lib_reference,
                        before.lib_reference()
                    )));
                }
                if deletes.iter().any(|(index, name, _)| {
                    *index == address.index || *name == address.lib_reference.as_str()
                }) {
                    return Err(invalid(format!(
                        "component '{}' (input slot {}) is deleted more than once",
                        address.lib_reference, address.index
                    )));
                }
                deletes.push((address.index, address.lib_reference.as_str(), before));
            }
        }
    }

    // Resolve and verify every deletion against the input snapshot
    // (component addresses resolve through crate::address).
    for (index, name, before) in &deletes {
        let resolved =
            ComponentIndex::resolve(input, *index).map_err(|source| PlanError::Address {
                context: format!("resolving deletion of '{name}' at input slot {index}"),
                source,
            })?;
        let component = input
            .component(resolved.get())
            .map_err(format_error(format!("reading input component #{index}")))?;
        if component.lib_reference() != *name {
            return Err(PlanError::StaleComponentState {
                component: (*name).to_owned(),
                detail: format!(
                    "input slot {index} holds '{}' instead",
                    component.lib_reference()
                ),
            });
        }
        if component.encode_state() != **before {
            return Err(PlanError::StaleComponentState {
                component: (*name).to_owned(),
                detail: "typed state differs from the plan's recorded before-state".to_owned(),
            });
        }
    }

    // Reconstruct the canonical final sequence and validate every
    //    replacement address against it.
    let deleted_slots: BTreeSet<usize> = deletes.iter().map(|(index, _, _)| *index).collect();
    let kept: Vec<usize> = (0..input.component_count())
        .filter(|slot| !deleted_slots.contains(slot))
        .collect();
    let input_names: Vec<&str> = input.component_lib_references().collect();
    let mut final_index_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (rank, &slot) in kept.iter().enumerate() {
        if final_index_of.insert(input_names[slot], rank).is_some() {
            return Err(invalid(format!(
                "target library holds two components named '{}'",
                input_names[slot]
            )));
        }
    }
    let created_by_name: BTreeMap<&str, &ComponentStatePayload> = creates
        .iter()
        .map(|(name, after)| (*name, *after))
        .collect();
    for (ordinal, (name, _)) in creates.iter().enumerate() {
        if final_index_of.insert(name, kept.len() + ordinal).is_some() {
            return Err(invalid(format!(
                "created component '{name}' collides with a kept component of that name"
            )));
        }
    }
    for (index, name, _, _) in &replaces {
        match final_index_of.get(*name) {
            Some(expected) if expected == index => {}
            Some(expected) => {
                return Err(invalid(format!(
                    "replacement of '{name}' is addressed at final slot {index}, but the \
                     reconstructed final sequence places it at {expected}"
                )));
            }
            None => {
                return Err(invalid(format!(
                    "replacement targets '{name}', which is neither a kept nor a created \
                     component of this plan"
                )));
            }
        }
    }

    // Phase 1: the ONE library-level SchLibEdit.
    let replace_before_of = |name: &str| -> Option<&ComponentStatePayload> {
        replaces
            .iter()
            .find(|(_, candidate, _, _)| *candidate == name)
            .map(|(_, _, before, _)| *before)
    };
    let mut edit = SchLibEdit::default();
    for (position, op) in library_ops.iter().enumerate() {
        match op {
            PlanLibraryOp::FontAppend(append) => edit.font_appends.push(SchFontAppend {
                name: append.name.clone(),
                size: append.size,
                rotation: append.rotation,
                bold: append.bold,
                italic: append.italic,
                underline: append.underline,
                strikeout: append.strikeout,
            }),
            PlanLibraryOp::ImageAssetInsert { index, name, .. } => {
                edit.image_assets.push(SchImageAssetEdit::Insert {
                    index: *index,
                    name: name.clone(),
                    payload: image_bodies
                        .remove(&position)
                        .ok_or_else(|| invalid("missing decoded image payload"))?,
                });
            }
            PlanLibraryOp::ImageAssetReplace { index, name, .. } => {
                edit.image_assets.push(SchImageAssetEdit::Replace {
                    index: *index,
                    name: name.clone(),
                    payload: image_bodies
                        .remove(&position)
                        .ok_or_else(|| invalid("missing decoded image payload"))?,
                });
            }
            PlanLibraryOp::ImageAssetRemove { index } => {
                edit.image_assets
                    .push(SchImageAssetEdit::Remove { index: *index });
            }
            PlanLibraryOp::AliasRemove { component, alias } => {
                edit.alias_ops.push(SchAliasOp::Remove {
                    component: component.clone(),
                    alias: alias.clone(),
                });
            }
            PlanLibraryOp::AliasAdd { component, alias } => {
                if let Some(payload) = created_by_name.get(component.as_str()) {
                    // Subsumed by the FromState install (increment-A note):
                    // the created state's index already carries the alias.
                    if !payload.aliases().any(|candidate| candidate == alias) {
                        return Err(invalid(format!(
                            "alias '{alias}' is added to created component '{component}', \
                             but the embedded after-state does not carry it"
                        )));
                    }
                } else {
                    edit.alias_ops.push(SchAliasOp::Add {
                        component: component.clone(),
                        alias: alias.clone(),
                    });
                }
            }
        }
    }
    if !creates.is_empty() || !deletes.is_empty() {
        let mut slots: Vec<SchLibComponentEdit> = kept
            .iter()
            .map(|&slot| SchLibComponentEdit::Keep(slot))
            .collect();
        for (name, after) in &creates {
            // Seed with the planner's INTERMEDIATE state (the paired
            // replacement's before payload); a creation with no content
            // replacement is already final.
            let seed = replace_before_of(name).unwrap_or(after).clone();
            slots.push(SchLibComponentEdit::New(SchNewComponent::FromState(seed)));
        }
        edit.components = Some(slots);
        edit.allow_component_removal = !deletes.is_empty();
    }
    let has_library_edit = edit != SchLibEdit::default();
    let intermediate: SchLibSnapshot = if has_library_edit {
        input.with_library_edit(edit).map_err(format_error(
            "applying the plan's library-level namespace transition",
        ))?
    } else {
        input.clone()
    };

    // Per-replacement decoded-before typed equality against the
    //    intermediate snapshot (addresses resolve through crate::address).
    for (index, name, before, _) in &replaces {
        let resolved = ComponentIndex::resolve(&intermediate, *index).map_err(|source| {
            PlanError::Address {
                context: format!("resolving replacement of '{name}' at final slot {index}"),
                source,
            }
        })?;
        let component = intermediate
            .component(resolved.get())
            .map_err(format_error(format!(
                "reading intermediate component #{index}"
            )))?;
        if component.lib_reference() != *name {
            return Err(PlanError::StaleComponentState {
                component: (*name).to_owned(),
                detail: format!(
                    "final slot {index} holds '{}' instead",
                    component.lib_reference()
                ),
            });
        }
        if component.encode_state() != **before {
            return Err(PlanError::StaleComponentState {
                component: (*name).to_owned(),
                detail: "typed state differs from the plan's recorded before-state".to_owned(),
            });
        }
    }

    // Phase 2: install every after-state through ReplaceWithState.
    let result = if replaces.is_empty() {
        intermediate
    } else {
        let mut slots: Vec<SchLibComponentEdit> = (0..intermediate.component_count())
            .map(SchLibComponentEdit::Keep)
            .collect();
        for (index, _, _, after) in &replaces {
            slots[*index] = SchLibComponentEdit::ReplaceWithState(*index, (*after).clone());
        }
        intermediate
            .with_library_edit(SchLibEdit {
                components: Some(slots),
                ..Default::default()
            })
            .map_err(format_error("installing the plan's after-states"))?
    };

    // Verify the produced components against the decoded after-states
    //     and the final membership.
    if result.component_count() != kept.len() + creates.len() {
        return Err(PlanError::ApplyVerification {
            context: "final component count".to_owned(),
            detail: format!(
                "expected {} components, produced {}",
                kept.len() + creates.len(),
                result.component_count()
            ),
        });
    }
    for (index, name, _, after) in &replaces {
        let produced = result
            .component(*index)
            .map_err(format_error(format!("reading produced component #{index}")))?;
        if produced.encode_state() != **after {
            return Err(PlanError::ApplyVerification {
                context: format!("replaced component '{name}'"),
                detail: "produced state differs from the plan's after-state".to_owned(),
            });
        }
    }
    for (ordinal, (name, after)) in creates.iter().enumerate() {
        let position = kept.len() + ordinal;
        let produced = result.component(position).map_err(format_error(format!(
            "reading produced component #{position}"
        )))?;
        if produced.lib_reference() != *name || produced.encode_state() != **after {
            return Err(PlanError::ApplyVerification {
                context: format!("created component '{name}'"),
                detail: "produced state differs from the plan's after-state".to_owned(),
            });
        }
    }
    for (_, name, _) in &deletes {
        if result
            .component_lib_references()
            .any(|candidate| candidate == *name)
        {
            return Err(PlanError::ApplyVerification {
                context: format!("deleted component '{name}'"),
                detail: "component still present in the produced library".to_owned(),
            });
        }
    }

    Ok(ExecutedPlan {
        result,
        created: creates.len(),
        replaced: replaces.len(),
        deleted: deletes.len(),
        library_ops: library_ops.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_asset::CapturedImageAsset;
    use crate::plan::{PlanArtifact, PlanArtifactContents, PlanScope, load_schlib_hashed};
    use crate::schlib::{SchLibAliasChange, SchLibImageAssetOp, SchLibLibraryOp, SchLibPatch};
    use altium_spec_lang::ast::{CollectionDelete, CollectionMode};
    use altium_spec_lang::diagnostic::{Span, Spanned};
    use altium_spec_lang::intent::{
        Collection, CollectionScope, ComponentIntent, FieldIntent, INTENT_SCHEMA_VERSION,
        ImageAssetSource, ParameterIntent, PartIntent, PartNumber, SchLibIntent,
    };

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
    fn part_item(number: i32) -> PartIntent {
        PartIntent {
            number: spanned(PartNumber::new(number).expect("positive part number")),
            part_swap_group: FieldIntent::Absent,
            pins: Collection::default(),
            graphics: Collection::default(),
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

    /// Component_1 grown to two parts plus a VICTIM clone, saved to disk.
    fn base_library(temp: &Path) -> PathBuf {
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
        let path = temp.join("base.SchLib");
        library.save(&path).unwrap();
        path
    }

    /// Create (PLAN_PROBE with parameter + alias) + replace (Component_1
    /// description + parts shrink 2 -> 1) + delete (VICTIM) + alias op.
    fn full_intent() -> SchLibIntent {
        let mut existing = blank_component_intent("Component_1");
        existing.description = authored(Some("applied by the plan".to_owned()));
        existing.parts = Collection {
            scope: complete_scope(),
            items: vec![part_item(1)],
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
        intent_of(complete_scope(), vec![existing, probe])
    }

    fn alternate_artifacts(library_path: &Path, destination: &Path) -> Vec<PlanArtifact> {
        let loaded = load_schlib_hashed(library_path).unwrap();
        vec![
            PlanArtifact {
                role: ManifestRole::TargetDocument,
                declared_path: library_path.to_owned(),
                contents: PlanArtifactContents::Bytes(loaded.bytes),
            },
            PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: destination.to_owned(),
                contents: PlanArtifactContents::ExpectedAbsent,
            },
        ]
    }

    fn plan_of(
        library_path: &Path,
        destination: &Path,
        intent: &SchLibIntent,
    ) -> (SavedPlan, SchLibPatch) {
        let input = SchLib::open(library_path).unwrap().snapshot().unwrap();
        let patch = SchLibPatch::from_intent(&input, intent).unwrap();
        let plan = SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            alternate_artifacts(library_path, destination),
            None,
            Vec::new(),
        )
        .unwrap();
        (plan, patch)
    }

    /// Serialize -> strict decode, the round trip every applied plan takes.
    fn reload(plan: &SavedPlan) -> SavedPlan {
        SavedPlan::from_json_bytes(&plan.to_json_bytes().unwrap()).unwrap()
    }

    #[test]
    fn create_update_delete_plan_applies_with_reopen_equality_and_replan_empty() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("out.SchLib");
        let intent = full_intent();
        let (plan, patch) = plan_of(&library_path, &destination, &intent);
        let expected = patch.resulting_snapshot().unwrap();

        let applied = apply_saved_plan(&reload(&plan)).unwrap();
        assert_eq!(
            applied.destination,
            destination.canonicalize().unwrap_or(destination.clone())
        );
        assert_eq!(
            (applied.created, applied.replaced, applied.deleted),
            (1, 2, 1),
            "create + created-content replace + existing replace is 2 replaces? \
             counts: {applied:?}"
        );

        // Save/reopen equality: the written destination holds the reviewed
        // result exactly (apply also re-checked this internally).
        let written = SchLib::open(&destination).unwrap().snapshot().unwrap();
        assert_eq!(written, expected);
        let component = written.component(0).unwrap();
        assert_eq!(component.description(), Some("applied by the plan"));
        assert_eq!(component.effective_part_count().unwrap(), 1);
        let probe = written.component(1).unwrap();
        assert_eq!(probe.lib_reference(), "PLAN_PROBE");
        assert!(probe.aliases().any(|alias| alias == "PLAN_ALIAS"));
        assert!(
            probe
                .parameters()
                .unwrap()
                .iter()
                .any(|view| view.name == "PLAN_PARAM")
        );
        assert!(
            !written
                .component_lib_references()
                .any(|name| name == "VICTIM")
        );

        // Replan against the applied output is empty.
        assert!(
            SchLibPatch::from_intent(&written, &intent)
                .unwrap()
                .is_empty()
        );

        // The target input file itself was not modified (alternate output).
        let untouched = SchLib::open(&library_path).unwrap().snapshot().unwrap();
        assert_eq!(untouched.component_count(), 2);
    }

    #[test]
    fn noop_plan_applies_writing_an_identical_library() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("noop-out.SchLib");
        // An empty partial intent plans no changes.
        let intent = intent_of(CollectionScope::default(), Vec::new());
        let (plan, patch) = plan_of(&library_path, &destination, &intent);
        assert!(patch.is_empty());
        let applied = apply_saved_plan(&reload(&plan)).unwrap();
        assert_eq!(
            (
                applied.created,
                applied.replaced,
                applied.deleted,
                applied.library_ops
            ),
            (0, 0, 0, 0)
        );
        let written = SchLib::open(&destination).unwrap().snapshot().unwrap();
        let input = SchLib::open(&library_path).unwrap().snapshot().unwrap();
        assert_eq!(written, input);
    }

    #[test]
    fn in_place_plan_overwrites_the_single_declared_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let loaded = load_schlib_hashed(&library_path).unwrap();
        let input = loaded.library.snapshot().unwrap();
        let mut update = blank_component_intent("Component_1");
        update.description = authored(Some("updated in place".to_owned()));
        let intent = intent_of(CollectionScope::default(), vec![update]);
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        let plan = SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            vec![PlanArtifact {
                role: ManifestRole::InPlaceTarget,
                declared_path: library_path.clone(),
                contents: PlanArtifactContents::Bytes(loaded.bytes),
            }],
            None,
            Vec::new(),
        )
        .unwrap();
        let applied = apply_saved_plan(&reload(&plan)).unwrap();
        assert_eq!(applied.destination, library_path.canonicalize().unwrap());
        assert_eq!(
            SchLib::open(&library_path).unwrap().snapshot().unwrap(),
            expected
        );
        // Replan against the rewritten in-place file is empty.
        assert!(
            SchLibPatch::from_intent(&expected, &intent)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn alias_and_content_change_on_one_component_reconstructs_the_two_step() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("alias-out.SchLib");
        let mut update = blank_component_intent("Component_1");
        update.description = authored(Some("alias + content".to_owned()));
        update.aliases = Collection {
            scope: CollectionScope::default(),
            items: vec![spanned("C1_ALIAS".to_owned())],
        };
        let intent = intent_of(CollectionScope::default(), vec![update]);
        let (plan, patch) = plan_of(&library_path, &destination, &intent);
        // The plan carries BOTH the alias op and the content replacement.
        let PlanPatches::Compile {
            library_ops,
            components,
        } = &plan.patches;
        assert_eq!(library_ops.len(), 1);
        assert_eq!(components.len(), 1);
        apply_saved_plan(&reload(&plan)).unwrap();
        let written = SchLib::open(&destination).unwrap().snapshot().unwrap();
        assert_eq!(written, patch.resulting_snapshot().unwrap());
        let component = written.component(0).unwrap();
        assert_eq!(component.description(), Some("alias + content"));
        assert!(component.aliases().any(|alias| alias == "C1_ALIAS"));
    }

    #[test]
    fn stale_target_expected_absent_and_payload_tamper_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("stale-out.SchLib");
        let (plan, _) = plan_of(&library_path, &destination, &full_intent());
        let plan = reload(&plan);

        // Target byte-changed (a GUI save between plan and apply).
        let mut bytes = std::fs::read(&library_path).unwrap();
        bytes.push(0);
        std::fs::write(&library_path, &bytes).unwrap();
        let error = apply_saved_plan(&plan).unwrap_err().to_string();
        assert!(error.contains("content hash mismatch"), "{error}");
        bytes.pop();
        std::fs::write(&library_path, &bytes).unwrap();

        // Expected-absent output now occupied.
        std::fs::write(&destination, b"occupied").unwrap();
        let error = apply_saved_plan(&plan).unwrap_err().to_string();
        assert!(error.contains("occupies"), "{error}");
        std::fs::remove_file(&destination).unwrap();

        // Plan-file tamper of a patch payload: a valid-but-different before
        // state decodes fine and is caught by before-equality, naming the
        // component.
        let mut tampered: serde_json::Value =
            serde_json::from_slice(&plan.to_json_bytes().unwrap()).unwrap();
        let components = tampered["patches"]["Compile"]["components"]
            .as_array_mut()
            .unwrap();
        let replace = components
            .iter_mut()
            .find_map(|patch| patch.get_mut("Replace"))
            .unwrap();
        replace["before"]["index"]["description"] = serde_json::json!("TAMPERED");
        replace["before"]["component"]["component"]["component_description"] =
            serde_json::json!("TAMPERED");
        let tampered_plan = SavedPlan::from_json_bytes(tampered.to_string().as_bytes()).unwrap();
        let error = apply_saved_plan(&tampered_plan).unwrap_err().to_string();
        assert!(error.contains("stale input"), "{error}");
        assert!(error.contains("before-state"), "{error}");

        // A tamper that breaks the payload contract is caught at decode.
        let mut broken: serde_json::Value =
            serde_json::from_slice(&plan.to_json_bytes().unwrap()).unwrap();
        broken["patches"]["Compile"]["components"][0]["Create"]["after"]["surprise"] =
            serde_json::json!(1);
        let error = SavedPlan::from_json_bytes(broken.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("surprise"), "{error}");

        // The untampered plan still applies after the target is restored.
        apply_saved_plan(&plan).unwrap();
    }

    #[test]
    fn edited_spec_source_and_transitive_import_are_rejected_at_apply() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("spec-out.SchLib");
        let spec = temp.path().join("root.schlib-spec");
        let import = temp.path().join("shared.schlib-spec");
        std::fs::write(&import, "let shared_desc = \"from the import\"").unwrap();
        std::fs::write(
            &spec,
            "import \"shared.schlib-spec\"\ncomponent Component_1 { description: shared_desc }",
        )
        .unwrap();
        let planned = crate::planner::plan_compile_schlib(
            &spec,
            &library_path,
            crate::planner::SchLibPlanOutput::Alternate(destination.clone()),
            None,
        )
        .unwrap();
        let plan = reload(&planned.plan);

        // Transitive import edited.
        std::fs::write(&import, "let shared_desc = \"edited import\"").unwrap();
        let error = apply_saved_plan(&plan).unwrap_err().to_string();
        assert!(error.contains("shared.schlib-spec"), "{error}");
        assert!(error.contains("content hash mismatch"), "{error}");
        std::fs::write(&import, "let shared_desc = \"from the import\"").unwrap();

        // Root spec source edited.
        std::fs::write(
            &spec,
            "import \"shared.schlib-spec\"\ncomponent Component_1 { description: \"x\" }",
        )
        .unwrap();
        let error = apply_saved_plan(&plan).unwrap_err().to_string();
        assert!(error.contains("root.schlib-spec"), "{error}");
        assert!(error.contains("content hash mismatch"), "{error}");
    }

    #[test]
    fn changed_image_bundle_is_rejected_at_apply() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("image-out.SchLib");
        let bundle = temp.path().join("logo.altium-image");
        std::fs::write(&bundle, b"reviewed image body").unwrap();
        let asset = CapturedImageAsset::capture(&ImageAssetSource {
            path: "logo.altium-image".to_owned(),
            declaring_source: Some(temp.path().join("library.schlib-spec")),
        })
        .unwrap();
        let input = SchLib::open(&library_path).unwrap().snapshot().unwrap();
        let patch = SchLibPatch::from_library_ops(
            &input,
            [
                SchLibLibraryOp::ImageAsset(SchLibImageAssetOp::Insert {
                    index: 0,
                    name: "logo.png".to_owned(),
                    asset: asset.clone(),
                }),
                SchLibLibraryOp::AliasAdd(SchLibAliasChange {
                    component: "Component_1".to_owned(),
                    alias: "IMG_ALIAS".to_owned(),
                }),
            ],
        )
        .unwrap();
        let mut artifacts = alternate_artifacts(&library_path, &destination);
        artifacts.push(PlanArtifact {
            role: ManifestRole::ImageAssetBundle,
            declared_path: bundle.clone(),
            contents: PlanArtifactContents::Bytes(asset.bytes().to_vec()),
        });
        let plan = SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            artifacts,
            None,
            Vec::new(),
        )
        .unwrap();
        let plan = reload(&plan);

        std::fs::write(&bundle, b"changed image body!").unwrap();
        let error = apply_saved_plan(&plan).unwrap_err().to_string();
        assert!(error.contains("logo.altium-image"), "{error}");
        assert!(error.contains("content hash mismatch"), "{error}");

        // Restored bundle applies; the store holds the reviewed body and
        // the alias landed.
        std::fs::write(&bundle, b"reviewed image body").unwrap();
        apply_saved_plan(&plan).unwrap();
        let written = SchLib::open(&destination).unwrap();
        let resource = written.image_resources().next().unwrap();
        assert_eq!(resource.name, "logo.png");
        assert_eq!(resource.payload, b"reviewed image body");
    }

    #[cfg(unix)]
    #[test]
    fn retargeted_symlink_input_is_rejected_at_apply() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let twin = temp.path().join("twin.SchLib");
        std::fs::copy(&library_path, &twin).unwrap();
        let link = temp.path().join("link.SchLib");
        std::os::unix::fs::symlink(&library_path, &link).unwrap();
        let destination = temp.path().join("link-out.SchLib");
        let (plan, _) = plan_of(&link, &destination, &full_intent());
        let plan = reload(&plan);

        // Retarget to a byte-identical copy: the hash still matches, but
        // the RESOLUTION changed — explicit rejection (memo §S11).
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&twin, &link).unwrap();
        let error = apply_saved_plan(&plan).unwrap_err().to_string();
        assert!(error.contains("now resolves to"), "{error}");
        assert!(error.contains("replan"), "{error}");
    }

    #[test]
    fn wrong_direction_plans_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let destination = temp.path().join("dir-out.SchLib");
        let (plan, _) = plan_of(&library_path, &destination, &full_intent());
        let mut value: serde_json::Value =
            serde_json::from_slice(&plan.to_json_bytes().unwrap()).unwrap();
        value["direction"] = serde_json::json!("dump");
        let dump_plan = SavedPlan::from_json_bytes(value.to_string().as_bytes()).unwrap();
        let error = apply_saved_plan(&dump_plan).unwrap_err().to_string();
        assert!(error.contains("direction"), "{error}");
    }

    #[test]
    fn reordering_patches_cannot_be_lowered_into_plans() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = base_library(temp.path());
        let input = SchLib::open(&library_path).unwrap().snapshot().unwrap();
        // Complete components scope listing the two components REVERSED:
        // a legal patch whose keep order the plan schema cannot represent.
        let intent = intent_of(
            complete_scope(),
            vec![
                blank_component_intent("VICTIM"),
                blank_component_intent("Component_1"),
            ],
        );
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        let error = SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            alternate_artifacts(&library_path, &temp.path().join("reorder-out.SchLib")),
            None,
            Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("reorders kept components"), "{error}");
    }
}
