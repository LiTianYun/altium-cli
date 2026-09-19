//! Compile-direction planner entry over real files (step-3 increment B;
//! design authority `/tmp/step3/design.md` §S7–§S10, §3 exit codes).
//!
//! [`plan_compile_schlib`] produces a [`SavedPlan`] from a real spec source
//! (with transitive imports via the existing capture API), a real target
//! `.SchLib`, and an explicitly declared output mode
//! ([`SchLibPlanOutput::InPlace`] | [`SchLibPlanOutput::Alternate`]).
//! Every input is read exactly ONCE and the same buffer is both hashed and
//! parsed (§S7): spec sources/imports through `capture_imports`' retained
//! text, the target through [`load_schlib_hashed`], and image bundles
//! through [`CapturedImageAsset`]. Immediately before plan emission the §S8
//! change-detection recheck re-verifies every captured input against disk
//! (`CapturedSourceGraph::verify_unchanged` compares exact contents, which
//! subsumes a hash comparison; assets re-verify path resolution and content
//! hash; the target is re-read and re-hashed) — drift aborts planning with
//! [`PlanError::ChangedDuringPlanning`].
//!
//! Exit-code contract (memo §3, consumed by step 5): `0` for an emitted plan
//! with no changes, `2` for an emitted plan with changes, `1` for any error
//! or blocking diagnostic (elaboration failures are blocking and surface as
//! `Err`; the plan's `diagnostics` carry only non-blocking notes).

use std::path::{Path, PathBuf};

use crate::image_asset::{CapturedImageAsset, requested_bundle_path};
use crate::plan::{
    LoadedSchLib, ManifestRole, PlanArtifact, PlanArtifactContents, PlanCollectionKind, PlanError,
    PlanHash, PlanScope, PlanScopeClass, PlanScopeEntry, PlanScopeSubject, SavedPlan,
    load_schlib_hashed,
};
use crate::schlib::{SchLibIntentAssets, SchLibPatch};
use altium_spec_lang::ast::{CollectionDelete, CollectionMode};
use altium_spec_lang::import::capture_imports;
use altium_spec_lang::intent::{
    FieldIntent, GraphicIntent, ImageAssetSource, SchLibIntent, compile_captured_schlib,
};

/// Exit code for an emitted plan containing no changes.
pub const PLAN_EXIT_NO_CHANGES: i32 = 0;
/// Exit code for any error or blocking diagnostic (no plan emitted).
pub const PLAN_EXIT_BLOCKED: i32 = 1;
/// Exit code for an emitted, applicable plan containing changes.
pub const PLAN_EXIT_CHANGES: i32 = 2;

/// The explicitly declared output mode of a compile planning (memo §S9/§S10).
/// Nothing is ever inferred: `InPlace` records the target as ONE
/// `InPlaceTarget` manifest artifact under one hash precondition; `Alternate`
/// records a read-only `TargetDocument` plus a `Destination` with its own
/// independent precondition (expected absence — the path must not exist in
/// any form at planning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchLibPlanOutput {
    InPlace,
    Alternate(PathBuf),
}

/// One successful compile planning: the emitted plan plus its exit-code
/// classification.
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibCompilePlanning {
    pub plan: SavedPlan,
    /// Whether the plan contains any executable change.
    pub changes: bool,
    /// `PLAN_EXIT_NO_CHANGES` or `PLAN_EXIT_CHANGES`.
    pub exit_code: i32,
}

/// Maps a planning outcome to the step-5 exit-code contract: 0 no-change,
/// 2 changes, 1 error-or-blocking-diagnostic.
pub fn plan_exit_code(outcome: &Result<SchLibCompilePlanning, PlanError>) -> i32 {
    match outcome {
        Ok(planning) => planning.exit_code,
        Err(_) => PLAN_EXIT_BLOCKED,
    }
}

/// Plans a compile of `spec_path` against `target_path`, freezing the
/// declared output and (optionally) checking the plan's own output path for
/// §S11 collisions. See the module documentation for the read-once, recheck,
/// and exit-code contracts.
pub fn plan_compile_schlib(
    spec_path: &Path,
    target_path: &Path,
    output: SchLibPlanOutput,
    plan_path: Option<&Path>,
) -> Result<SchLibCompilePlanning, PlanError> {
    // 1. Capture the spec graph: each source read once, hashed and parsed
    //    from the same retained text.
    let graph = capture_imports(spec_path).map_err(|source| PlanError::Spec {
        context: format!("capturing spec imports of '{}'", spec_path.display()),
        source,
    })?;
    let compiled = compile_captured_schlib(&graph).map_err(|source| PlanError::Spec {
        context: format!("compiling captured spec '{}'", spec_path.display()),
        source,
    })?;

    // 2. Load the target once (§S7: hash and parse the same buffer).
    let loaded: LoadedSchLib = load_schlib_hashed(target_path)?;
    let input = loaded
        .library
        .snapshot()
        .map_err(|source| PlanError::Format {
            context: format!("capturing target snapshot of '{}'", target_path.display()),
            source,
        })?;

    // 3. Capture every authored image bundle exactly once (deduplicated by
    //    the pure requested-path arithmetic, before any read).
    let mut assets = SchLibIntentAssets::new();
    let mut captured_assets: Vec<CapturedImageAsset> = Vec::new();
    let mut bundle_artifacts: Vec<PlanArtifact> = Vec::new();
    for source in collect_intent_image_sources(&compiled.root) {
        let requested = requested_bundle_path(source).map_err(|error| PlanError::ImageAsset {
            context: format!("resolving authored image bundle '{}'", source.path),
            source: error,
        })?;
        if assets.get(&requested).is_some() {
            continue;
        }
        let captured =
            CapturedImageAsset::capture(source).map_err(|error| PlanError::ImageAsset {
                context: format!("capturing authored image bundle '{}'", source.path),
                source: error,
            })?;
        bundle_artifacts.push(PlanArtifact {
            role: ManifestRole::ImageAssetBundle,
            declared_path: captured.requested_path().to_owned(),
            contents: PlanArtifactContents::Bytes(captured.bytes().to_vec()),
        });
        captured_assets.push(captured.clone());
        assets.insert(captured);
    }

    // 4. Elaborate (blocking diagnostics surface as errors here).
    let patch = SchLibPatch::from_intent_with_assets(&input, &compiled.root, &assets).map_err(
        |source| PlanError::Patch {
            context: format!(
                "elaborating '{}' against '{}'",
                spec_path.display(),
                target_path.display()
            ),
            source,
        },
    )?;

    // 5. Manifest artifacts: spec source, transitive imports, image bundles,
    //    then the declared target/destination shape (§S9/§S10).
    let root_source =
        graph
            .sources()
            .get(graph.root_path())
            .ok_or_else(|| PlanError::Manifest {
                context: format!("capturing spec source '{}'", spec_path.display()),
                detail: "captured graph is missing its own root source".to_owned(),
            })?;
    let mut artifacts = vec![PlanArtifact {
        role: ManifestRole::SpecSource,
        declared_path: spec_path.to_owned(),
        contents: PlanArtifactContents::Bytes(root_source.source().as_bytes().to_vec()),
    }];
    for (path, source) in graph.sources() {
        if path == graph.root_path() {
            continue;
        }
        artifacts.push(PlanArtifact {
            role: ManifestRole::SpecImport,
            declared_path: path.clone(),
            contents: PlanArtifactContents::Bytes(source.source().as_bytes().to_vec()),
        });
    }
    artifacts.extend(bundle_artifacts);
    match &output {
        SchLibPlanOutput::InPlace => artifacts.push(PlanArtifact {
            role: ManifestRole::InPlaceTarget,
            declared_path: target_path.to_owned(),
            contents: PlanArtifactContents::Bytes(loaded.bytes.clone()),
        }),
        SchLibPlanOutput::Alternate(destination) => {
            artifacts.push(PlanArtifact {
                role: ManifestRole::TargetDocument,
                declared_path: target_path.to_owned(),
                contents: PlanArtifactContents::Bytes(loaded.bytes.clone()),
            });
            artifacts.push(PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: destination.clone(),
                contents: PlanArtifactContents::ExpectedAbsent,
            });
        }
    }

    // 6. §S8 change-detection recheck, immediately before emission. This is
    //    race DETECTION, not an atomicity claim.
    graph
        .verify_unchanged()
        .map_err(|source| PlanError::ChangedDuringPlanning {
            artifact: spec_path.display().to_string(),
            detail: source.to_string(),
        })?;
    for captured in &captured_assets {
        captured
            .verify_unchanged()
            .map_err(|source| PlanError::ChangedDuringPlanning {
                artifact: captured.canonical_path().display().to_string(),
                detail: source.to_string(),
            })?;
    }
    let reread = std::fs::read(target_path).map_err(|source| PlanError::Io {
        context: format!("re-reading target '{}'", target_path.display()),
        source,
    })?;
    if PlanHash::of_bytes(&reread) != loaded.content_hash {
        return Err(PlanError::ChangedDuringPlanning {
            artifact: target_path.display().to_string(),
            detail: "target document content hash changed since it was loaded".to_owned(),
        });
    }

    // 7. Lower into the saved plan and classify.
    let plan = SavedPlan::compile_from_schlib_patch(
        &patch,
        scope_summary(&compiled.root),
        artifacts,
        plan_path,
        Vec::new(),
    )?;
    let changes = !patch.is_empty();
    Ok(SchLibCompilePlanning {
        plan,
        changes,
        exit_code: if changes {
            PLAN_EXIT_CHANGES
        } else {
            PLAN_EXIT_NO_CHANGES
        },
    })
}

/// Every authored image-asset source in the intent (component-scope and
/// part-block graphics), in authoring order, duplicates included — the
/// caller deduplicates by requested bundle path.
fn collect_intent_image_sources(intent: &SchLibIntent) -> Vec<&ImageAssetSource> {
    fn from_graphics<'a>(items: &'a [GraphicIntent], out: &mut Vec<&'a ImageAssetSource>) {
        for graphic in items {
            if let FieldIntent::Set(spanned) = &graphic.fields.source
                && let Some(source) = &spanned.node
            {
                out.push(source);
            }
        }
    }
    let mut sources = Vec::new();
    for component in &intent.components.items {
        from_graphics(&component.graphics.items, &mut sources);
        for part in &component.parts.items {
            from_graphics(&part.graphics.items, &mut sources);
        }
    }
    sources
}

/// The recorded source-declared scope summary (memo §S4): report/diagnostic
/// metadata, never an execution input. The library components scope is
/// always recorded; per-component collection scopes are recorded when
/// DECLARED (their span is present), with part-block pin/graphic scopes
/// folded into the component's `Pins`/`Graphics` kinds; exact duplicates are
/// recorded once. Footprint pin-pad map lists are positional payloads of
/// their footprint entries, not separately scoped collections, so no `Maps`
/// entry is recorded.
fn scope_summary(intent: &SchLibIntent) -> PlanScope {
    fn class_of(mode: CollectionMode) -> PlanScopeClass {
        match mode {
            CollectionMode::Partial => PlanScopeClass::Partial,
            CollectionMode::Complete => PlanScopeClass::Complete,
        }
    }
    let mut entries = vec![PlanScopeEntry {
        subject: PlanScopeSubject::Library,
        collection: PlanCollectionKind::Components,
        class: class_of(intent.components.scope.mode),
        delete_allowed: intent.components.scope.delete == CollectionDelete::Allow,
    }];
    for component in &intent.components.items {
        let subject = PlanScopeSubject::Component {
            lib_reference: component.name.node.clone(),
        };
        let mut push = |collection: PlanCollectionKind,
                        scope: &altium_spec_lang::intent::CollectionScope| {
            if scope.span.is_none() {
                return;
            }
            let entry = PlanScopeEntry {
                subject: subject.clone(),
                collection,
                class: class_of(scope.mode),
                delete_allowed: scope.delete == CollectionDelete::Allow,
            };
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        };
        push(PlanCollectionKind::Pins, &component.pins.scope);
        push(PlanCollectionKind::Parameters, &component.parameters.scope);
        push(PlanCollectionKind::Aliases, &component.aliases.scope);
        push(PlanCollectionKind::Footprints, &component.footprints.scope);
        push(PlanCollectionKind::Graphics, &component.graphics.scope);
        push(PlanCollectionKind::Parts, &component.parts.scope);
        for part in &component.parts.items {
            push(PlanCollectionKind::Pins, &part.pins.scope);
            push(PlanCollectionKind::Graphics, &part.graphics.scope);
        }
    }
    PlanScope { entries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::apply_saved_plan;
    use crate::plan::{ManifestRole, PlanPrecondition};
    use altium_format::SchLib;

    fn write_library(temp: &Path) -> PathBuf {
        let path = temp.join("target.SchLib");
        SchLib::new_blank_ad26().unwrap().save(&path).unwrap();
        path
    }

    fn write_specs(temp: &Path) -> PathBuf {
        std::fs::write(
            temp.join("shared.schlib-spec"),
            "let shared_desc = \"planned description\"",
        )
        .unwrap();
        let root = temp.join("root.schlib-spec");
        std::fs::write(
            &root,
            "import \"shared.schlib-spec\"\n\
             component Component_1 { description: shared_desc }\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn plans_from_real_files_with_exit_codes_and_manifest_roles() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = write_library(temp.path());
        let spec_path = write_specs(temp.path());
        let destination = temp.path().join("out.SchLib");
        let plan_path = temp.path().join("plan.json");

        let planned = plan_compile_schlib(
            &spec_path,
            &library_path,
            SchLibPlanOutput::Alternate(destination.clone()),
            Some(&plan_path),
        )
        .unwrap();
        assert!(planned.changes);
        assert_eq!(planned.exit_code, PLAN_EXIT_CHANGES);
        assert_eq!(plan_exit_code(&Ok(planned.clone())), PLAN_EXIT_CHANGES);

        // Manifest: SpecSource + SpecImport + TargetDocument + Destination,
        // with the destination recorded expected-absent.
        let roles: Vec<ManifestRole> = planned
            .plan
            .manifest
            .iter()
            .map(|entry| entry.role)
            .collect();
        assert_eq!(
            roles,
            vec![
                ManifestRole::SpecSource,
                ManifestRole::SpecImport,
                ManifestRole::TargetDocument,
                ManifestRole::Destination,
            ]
        );
        assert_eq!(
            planned.plan.manifest[3].precondition,
            PlanPrecondition::Absent
        );
        // Recorded scope summary: the library components collection.
        assert_eq!(planned.plan.scope.entries.len(), 1);
        assert_eq!(
            planned.plan.scope.entries[0].collection,
            PlanCollectionKind::Components
        );
        assert_eq!(planned.plan.scope.entries[0].class, PlanScopeClass::Partial);
        assert!(!planned.plan.scope.entries[0].delete_allowed);

        // The plan applies through the saved-plan pipeline end to end.
        let decoded =
            crate::plan::SavedPlan::from_json_bytes(&planned.plan.to_json_bytes().unwrap())
                .unwrap();
        apply_saved_plan(&decoded).unwrap();
        let written = SchLib::open(&destination).unwrap().snapshot().unwrap();
        assert_eq!(
            written.component(0).unwrap().description(),
            Some("planned description")
        );

        // Replanning against the applied output is a no-change plan (exit 0).
        let replanned = plan_compile_schlib(
            &spec_path,
            &destination,
            SchLibPlanOutput::Alternate(temp.path().join("out2.SchLib")),
            None,
        )
        .unwrap();
        assert!(!replanned.changes);
        assert_eq!(replanned.exit_code, PLAN_EXIT_NO_CHANGES);

        // Errors classify as exit 1.
        let missing = plan_compile_schlib(
            &temp.path().join("missing.schlib-spec"),
            &library_path,
            SchLibPlanOutput::InPlace,
            None,
        );
        assert!(missing.is_err());
        assert_eq!(plan_exit_code(&missing), PLAN_EXIT_BLOCKED);
    }

    #[test]
    fn in_place_mode_records_one_artifact_and_applies_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = write_library(temp.path());
        let spec_path = write_specs(temp.path());
        let planned =
            plan_compile_schlib(&spec_path, &library_path, SchLibPlanOutput::InPlace, None)
                .unwrap();
        assert_eq!(planned.exit_code, PLAN_EXIT_CHANGES);
        let roles: Vec<ManifestRole> = planned
            .plan
            .manifest
            .iter()
            .map(|entry| entry.role)
            .collect();
        assert_eq!(
            roles,
            vec![
                ManifestRole::SpecSource,
                ManifestRole::SpecImport,
                ManifestRole::InPlaceTarget,
            ]
        );
        apply_saved_plan(&planned.plan).unwrap();
        assert_eq!(
            SchLib::open(&library_path)
                .unwrap()
                .snapshot()
                .unwrap()
                .component(0)
                .unwrap()
                .description(),
            Some("planned description")
        );
        // Replanning in place is now a no-change plan.
        let replanned =
            plan_compile_schlib(&spec_path, &library_path, SchLibPlanOutput::InPlace, None)
                .unwrap();
        assert_eq!(replanned.exit_code, PLAN_EXIT_NO_CHANGES);
    }
}
