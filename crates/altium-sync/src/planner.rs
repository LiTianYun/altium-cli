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
    LoadedSchLib, ManifestRole, PLAN_HASH_ALGORITHM, PlanArtifact, PlanArtifactContents,
    PlanBundlePatch, PlanCollectionKind, PlanDiagnostic, PlanError, PlanHash, PlanScope,
    PlanScopeClass, PlanScopeEntry, PlanScopeSubject, PlanSourcePatch, SavedPlan,
    expected_source_state, load_schlib_hashed,
};
use crate::schlib::dump_source::generate_schlib_spec_source;
use crate::schlib::{SchLibIntentAssets, SchLibPatch};
use altium_spec_lang::ast::{CollectionDelete, CollectionMode};
use altium_spec_lang::import::{capture_imports, capture_imports_with_root_text};
use altium_spec_lang::intent::{
    FieldIntent, GraphicIntent, ImageAssetSource, SchLibIntent, compile_captured_schlib,
};
use base64::Engine as _;

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

/// One successful dump planning: the emitted plan plus its exit-code
/// classification.
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibDumpPlanning {
    pub plan: SavedPlan,
    /// Whether the plan writes anything new (source created/edited, or a
    /// bundle destination that is absent or holds different bytes).
    pub changes: bool,
    /// `PLAN_EXIT_NO_CHANGES` or `PLAN_EXIT_CHANGES`.
    pub exit_code: i32,
}

/// Maps a dump-planning outcome to the step-5 exit-code contract.
pub fn dump_plan_exit_code(outcome: &Result<SchLibDumpPlanning, PlanError>) -> i32 {
    match outcome {
        Ok(planning) => planning.exit_code,
        Err(_) => PLAN_EXIT_BLOCKED,
    }
}

/// A bundle file name derived from the store slot and resource name:
/// deterministic, collision-free (the slot index disambiguates), and safe
/// for any stored name.
fn bundle_file_name(spec_path: &Path, index: usize, name: &str) -> String {
    let stem = spec_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("spec");
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let sanitized = if sanitized.is_empty() {
        "asset".to_owned()
    } else {
        sanitized
    };
    format!("{stem}.asset{index}-{sanitized}.altium-image")
}

/// Plans a dump of `target_path` (the `.SchLib`, read-only) into
/// `spec_path`: an existing spec receives hash-bound structured edits
/// computed by the comment-preserving dump merge (`plan_merge_dump` — the
/// exact edits `merge_dump` would apply, captured instead of executed); a
/// missing spec produces a `SourceCreate` embedding the complete generated
/// text. Embedded image-store assets are staged as `.altium-image` bundle
/// destinations beside the spec. The resulting source is compiled and
/// elaborated against the captured library AT PLANNING and must produce an
/// EMPTY patch (the directional convergence law) — a generator or merge
/// inconsistency blocks the plan instead of emitting a divergent one.
///
/// Read-once (§S7), recheck-before-emission (§S8), frozen outputs (§S9),
/// explicit in-place declaration (§S10: an existing spec is the ONE
/// `InPlaceTarget`), and §S11 collision rejection all match the compile
/// planner. Exit codes: 0 no changes, 2 changes, 1 error/blocking.
pub fn plan_dump_schlib(
    target_path: &Path,
    spec_path: &Path,
    plan_path: Option<&Path>,
) -> Result<SchLibDumpPlanning, PlanError> {
    // 1. Load the library once (§S7: hash and parse the same buffer).
    let loaded: LoadedSchLib = load_schlib_hashed(target_path)?;
    let input = loaded
        .library
        .snapshot()
        .map_err(|source| PlanError::Format {
            context: format!("capturing target snapshot of '{}'", target_path.display()),
            source,
        })?;

    // 2. Generate the fresh dump text (deterministic; skips recorded).
    let generated = generate_schlib_spec_source(&input).map_err(|source| PlanError::Patch {
        context: format!("generating dump source for '{}'", target_path.display()),
        source,
    })?;
    let mut diagnostics: Vec<PlanDiagnostic> = generated
        .skips
        .iter()
        .map(|skip| PlanDiagnostic {
            message: format!("dump generation: {skip}"),
        })
        .collect();

    // 3. Existing spec? Capture it (root + transitive imports read once).
    let spec_exists = match std::fs::symlink_metadata(spec_path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(PlanError::Io {
                context: format!("probing spec path '{}'", spec_path.display()),
                source,
            });
        }
    };

    let mut artifacts: Vec<PlanArtifact> = Vec::new();
    let source_patch;
    let result_text;
    let existing_graph;
    let existing_text;
    let mut changes;
    if spec_exists {
        let graph = capture_imports(spec_path).map_err(|source| PlanError::Spec {
            context: format!("capturing spec imports of '{}'", spec_path.display()),
            source,
        })?;
        let root_text = graph
            .sources()
            .get(graph.root_path())
            .ok_or_else(|| PlanError::Manifest {
                context: format!("capturing spec source '{}'", spec_path.display()),
                detail: "captured graph is missing its own root source".to_owned(),
            })?
            .source()
            .to_owned();
        let existing_tree =
            altium_spec_lang::SpecTree::parse(root_text.clone()).map_err(|error| {
                PlanError::InvalidPlan {
                    detail: format!(
                        "existing spec '{}' does not parse as a lossless source: {error}",
                        spec_path.display()
                    ),
                }
            })?;
        let fresh_tree =
            altium_spec_lang::SpecTree::parse(generated.text.clone()).map_err(|error| {
                PlanError::InvalidPlan {
                    detail: format!("generated dump text does not parse: {error}"),
                }
            })?;
        let edits =
            altium_spec_lang::plan_merge_dump(&existing_tree, &fresh_tree).map_err(|source| {
                PlanError::DumpMerge {
                    context: format!("reconciling '{}' with the fresh dump", spec_path.display()),
                    source,
                }
            })?;
        let merged = altium_spec_lang::apply_edits(&existing_tree, &edits).map_err(|source| {
            PlanError::StructuredEdit {
                context: format!(
                    "applying reconcile edits to '{}' in memory",
                    spec_path.display()
                ),
                source,
            }
        })?;
        let merged_text = merged.source().to_owned();
        let encoded =
            altium_spec_lang::encode_spec_edits(&existing_tree, &edits).map_err(|source| {
                PlanError::PlannedEdit {
                    context: format!("persisting reconcile edits for '{}'", spec_path.display()),
                    source,
                }
            })?;
        changes = !encoded.is_empty();

        // Manifest: in-place spec + transitive imports + read-only library.
        artifacts.push(PlanArtifact {
            role: ManifestRole::InPlaceTarget,
            declared_path: spec_path.to_owned(),
            contents: PlanArtifactContents::Bytes(root_text.as_bytes().to_vec()),
        });
        for (path, captured) in graph.sources() {
            if path == graph.root_path() {
                continue;
            }
            artifacts.push(PlanArtifact {
                role: ManifestRole::SpecImport,
                declared_path: path.clone(),
                contents: PlanArtifactContents::Bytes(captured.source().as_bytes().to_vec()),
            });
        }
        let spec_canonical = crate::plan::canonicalize_plan_path(spec_path)?;
        source_patch = PlanSourcePatch::Edit {
            source: crate::plan::path_to_string(
                &spec_canonical,
                "resolving the in-place spec path",
            )?,
            edits: encoded,
            result_hash: PlanHash::of_bytes(merged_text.as_bytes()),
        };
        result_text = merged_text;
        existing_graph = Some(graph);
        existing_text = Some(root_text);
    } else {
        artifacts.push(PlanArtifact {
            role: ManifestRole::Destination,
            declared_path: spec_path.to_owned(),
            contents: PlanArtifactContents::ExpectedAbsent,
        });
        let spec_canonical = crate::plan::canonicalize_plan_path(spec_path)?;
        source_patch = PlanSourcePatch::Create {
            destination: crate::plan::path_to_string(
                &spec_canonical,
                "resolving the new spec path",
            )?,
            text: generated.text.clone(),
            result_hash: PlanHash::of_bytes(generated.text.as_bytes()),
        };
        result_text = generated.text.clone();
        existing_graph = None;
        existing_text = None;
        changes = true;
    }
    artifacts.push(PlanArtifact {
        role: ManifestRole::TargetDocument,
        declared_path: target_path.to_owned(),
        contents: PlanArtifactContents::Bytes(loaded.bytes.clone()),
    });

    // 4. Convergence proof at planning: the RESULTING source (with its
    //    captured imports) compiles, and its elaboration against the
    //    captured library is an EMPTY patch. Authored image sources in the
    //    resulting source are captured exactly as the compile planner does.
    let result_graph =
        capture_imports_with_root_text(spec_path, &result_text).map_err(|source| {
            PlanError::Spec {
                context: format!(
                    "capturing the planned resulting source of '{}'",
                    spec_path.display()
                ),
                source,
            }
        })?;
    let compiled = compile_captured_schlib(&result_graph).map_err(|source| PlanError::Spec {
        context: format!(
            "compiling the planned resulting source of '{}'",
            spec_path.display()
        ),
        source,
    })?;
    let mut assets = SchLibIntentAssets::new();
    let mut captured_assets: Vec<CapturedImageAsset> = Vec::new();
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
        artifacts.push(PlanArtifact {
            role: ManifestRole::ImageAssetBundle,
            declared_path: captured.requested_path().to_owned(),
            contents: PlanArtifactContents::Bytes(captured.bytes().to_vec()),
        });
        captured_assets.push(captured.clone());
        assets.insert(captured);
    }
    let patch = SchLibPatch::from_intent_with_assets(&input, &compiled.root, &assets).map_err(
        |source| PlanError::Patch {
            context: format!(
                "elaborating the planned resulting source of '{}' against '{}'",
                spec_path.display(),
                target_path.display()
            ),
            source,
        },
    )?;
    if !patch.is_empty() {
        return Err(PlanError::InvalidPlan {
            detail: format!(
                "dump convergence failure: the planned resulting source of '{}' does not \
                 elaborate to an empty patch against '{}' ({} replacement(s), {} \
                 creation(s), {} removal(s)); this is a dump-generation/merge \
                 inconsistency — report it",
                spec_path.display(),
                target_path.display(),
                patch.replacements().len(),
                patch.created_components().count(),
                patch.removed_components().len(),
            ),
        });
    }
    let expected = expected_source_state(&compiled.root);
    let scope = scope_summary(&compiled.root);

    // 5. Stage every embedded image-store asset as a bundle destination.
    let mut bundles: Vec<PlanBundlePatch> = Vec::new();
    for (index, resource) in input.image_assets().enumerate() {
        let file_name = bundle_file_name(spec_path, index, resource.name);
        let declared = spec_path
            .parent()
            .map(|parent| parent.join(&file_name))
            .unwrap_or_else(|| PathBuf::from(&file_name));
        let payload_hash = PlanHash::of_bytes(resource.payload);
        let contents = match std::fs::symlink_metadata(&declared) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                changes = true;
                PlanArtifactContents::ExpectedAbsent
            }
            Ok(_) => {
                let existing = std::fs::read(&declared).map_err(|source| PlanError::Io {
                    context: format!(
                        "reading existing bundle destination '{}'",
                        declared.display()
                    ),
                    source,
                })?;
                if existing != resource.payload {
                    changes = true;
                    diagnostics.push(PlanDiagnostic {
                        message: format!(
                            "bundle destination '{}' holds different bytes and will be \
                             overwritten (declared-overwrite hash precondition)",
                            declared.display()
                        ),
                    });
                }
                PlanArtifactContents::Bytes(existing)
            }
            Err(source) => {
                return Err(PlanError::Io {
                    context: format!("probing bundle destination '{}'", declared.display()),
                    source,
                });
            }
        };
        artifacts.push(PlanArtifact {
            role: ManifestRole::Destination,
            declared_path: declared.clone(),
            contents,
        });
        let canonical = crate::plan::canonicalize_plan_path(&declared)?;
        bundles.push(PlanBundlePatch {
            destination: crate::plan::path_to_string(&canonical, "resolving a bundle destination")?,
            resource_name: resource.name.to_owned(),
            content_hash: PlanHash {
                algorithm: PLAN_HASH_ALGORITHM.to_owned(),
                hex: payload_hash.hex,
                byte_len: payload_hash.byte_len,
            },
            base64: base64::engine::general_purpose::STANDARD.encode(resource.payload),
        });
    }

    // 6. §S8 change-detection recheck, immediately before plan assembly.
    if let Some(graph) = &existing_graph {
        graph
            .verify_unchanged()
            .map_err(|source| PlanError::ChangedDuringPlanning {
                artifact: spec_path.display().to_string(),
                detail: source.to_string(),
            })?;
    }
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

    // 7. Assemble (manifest capture, §S11, the lowering-time replay dry run)
    //    and classify.
    let plan = SavedPlan::dump_from_parts(
        scope,
        artifacts,
        plan_path,
        diagnostics,
        source_patch,
        bundles,
        expected,
        existing_text.as_deref(),
    )?;
    Ok(SchLibDumpPlanning {
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

    // ── Dump direction (increment C) ─────────────────────────────────────

    use crate::apply::apply_saved_dump_plan;
    use crate::plan::{PlanPatches, PlanSourcePatch};
    use altium_spec_lang::intent::compile_captured_schlib as compile_graph;

    /// Saves the fixture-free "collection zoo" dump snapshot to disk.
    fn write_seeded_library(temp: &Path) -> PathBuf {
        let snapshot = crate::schlib::dump_source::tests::seeded_snapshot();
        let path = temp.join("seeded.SchLib");
        snapshot.materialize().unwrap().save(&path).unwrap();
        path
    }

    /// The directional convergence law: the on-disk source compiles (with
    /// captured imports) and elaborates against the on-disk library to an
    /// EMPTY patch.
    fn assert_converges(spec_path: &Path, library_path: &Path) {
        let graph = capture_imports(spec_path).unwrap();
        let compiled = compile_graph(&graph).unwrap();
        let input = SchLib::open(library_path).unwrap().snapshot().unwrap();
        let patch = SchLibPatch::from_intent(&input, &compiled.root).unwrap();
        assert!(
            patch.is_empty(),
            "dump result must elaborate to an empty patch"
        );
    }

    #[test]
    fn dump_create_plan_round_trips_and_converges() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = write_seeded_library(temp.path());
        let spec_path = temp.path().join("seeded.schlib-spec");
        let plan_path = temp.path().join("dump-plan.json");

        let planned = plan_dump_schlib(&library_path, &spec_path, Some(&plan_path)).unwrap();
        assert!(planned.changes);
        assert_eq!(planned.exit_code, PLAN_EXIT_CHANGES);
        assert!(matches!(
            planned.plan.patches,
            PlanPatches::Dump {
                source: PlanSourcePatch::Create { .. },
                ..
            }
        ));
        // Determinism: a second planning is byte-identical (nothing is
        // minted in a dump plan).
        let again = plan_dump_schlib(&library_path, &spec_path, Some(&plan_path)).unwrap();
        assert_eq!(
            planned.plan.to_json_bytes().unwrap(),
            again.plan.to_json_bytes().unwrap()
        );

        // Serialize → strict decode → apply.
        let decoded =
            crate::plan::SavedPlan::from_json_bytes(&planned.plan.to_json_bytes().unwrap())
                .unwrap();
        // The compile-apply entry point rejects the dump direction.
        let error = crate::apply::apply_saved_plan(&decoded)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not Compile"), "{error}");
        let library_bytes = std::fs::read(&library_path).unwrap();
        let applied = apply_saved_dump_plan(&decoded).unwrap();
        assert!(applied.created);
        assert_eq!(applied.edits_applied, 0);
        assert_eq!(
            std::fs::read(&library_path).unwrap(),
            library_bytes,
            "dump apply must not touch the .SchLib"
        );
        assert_converges(&spec_path, &library_path);

        // Replanning against the created source is a zero-change plan.
        let replanned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        assert!(!replanned.changes);
        assert_eq!(replanned.exit_code, PLAN_EXIT_NO_CHANGES);
        let PlanPatches::Dump {
            source: PlanSourcePatch::Edit { edits, .. },
            ..
        } = &replanned.plan.patches
        else {
            panic!("expected an Edit patch, got {:?}", replanned.plan.patches);
        };
        assert!(edits.is_empty());
    }

    #[test]
    fn dump_edit_plan_preserves_comments_crlf_annotations_and_converges() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = write_seeded_library(temp.path());
        let spec_path = temp.path().join("seeded.schlib-spec");
        let created = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        apply_saved_dump_plan(&created.plan).unwrap();

        // Author around the generated source: CRLF everywhere, a seeded
        // comment, an annotation, and one WRONG property value the dump must
        // correct.
        let text = std::fs::read_to_string(&spec_path).unwrap();
        assert!(text.contains("show_hidden_pins: false"));
        let tampered = text
            .replace("show_hidden_pins: false", "show_hidden_pins: true")
            .replace(
                "component \"Component_1\" {",
                "// keep me: authored comment\n#[annotation(id = \"AAAA1111\")]\ncomponent \"Component_1\" {",
            )
            .replace('\n', "\r\n");
        std::fs::write(&spec_path, &tampered).unwrap();

        let planned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        assert!(planned.changes);
        assert_eq!(planned.exit_code, PLAN_EXIT_CHANGES);
        let decoded =
            crate::plan::SavedPlan::from_json_bytes(&planned.plan.to_json_bytes().unwrap())
                .unwrap();
        let applied = apply_saved_dump_plan(&decoded).unwrap();
        assert!(!applied.created);
        assert!(applied.edits_applied >= 1);

        let merged = std::fs::read_to_string(&spec_path).unwrap();
        // Byte-level checks around the edited region: the seeded comment and
        // annotation survive with CRLF endings, the value is corrected, and
        // no bare LF was introduced anywhere.
        assert!(merged.contains("// keep me: authored comment\r\n"));
        assert!(merged.contains("#[annotation(id = \"AAAA1111\")]\r\n"));
        assert!(merged.contains("show_hidden_pins: false"));
        assert!(!merged.contains("show_hidden_pins: true"));
        assert!(
            !merged.replace("\r\n", "").contains('\n'),
            "CRLF style must be preserved for every edit"
        );
        assert_converges(&spec_path, &library_path);

        // No-op replan after the edit: exit 0 with zero edits.
        let replanned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        assert_eq!(replanned.exit_code, PLAN_EXIT_NO_CHANGES);
    }

    #[test]
    fn dump_apply_rejects_stale_source_tampered_locators_and_state_drift() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = write_seeded_library(temp.path());
        let spec_path = temp.path().join("seeded.schlib-spec");
        let created = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        apply_saved_dump_plan(&created.plan).unwrap();

        // Force an edit-bearing plan.
        let text = std::fs::read_to_string(&spec_path).unwrap();
        std::fs::write(
            &spec_path,
            text.replace("show_hidden_pins: false", "show_hidden_pins: true"),
        )
        .unwrap();
        let planned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        assert!(planned.changes);

        // 1. Stale source: one byte changed after planning.
        let mut stale = std::fs::read(&spec_path).unwrap();
        stale.push(b' ');
        std::fs::write(&spec_path, &stale).unwrap();
        let error = apply_saved_dump_plan(&planned.plan)
            .unwrap_err()
            .to_string();
        assert!(error.contains("content hash mismatch"), "{error}");
        stale.pop();
        std::fs::write(&spec_path, &stale).unwrap();

        // 2. Tampered locator: a node path off the tree fails rebinding.
        let mut value: serde_json::Value =
            serde_json::from_slice(&planned.plan.to_json_bytes().unwrap()).unwrap();
        let edits = &mut value["patches"]["Dump"]["source"]["Edit"]["edits"];
        edits[0]["SetProperty"]["target"]["node_path"] = serde_json::json!([99, 99]);
        let tampered =
            crate::plan::SavedPlan::from_json_bytes(value.to_string().as_bytes()).unwrap();
        let error = apply_saved_dump_plan(&tampered).unwrap_err().to_string();
        assert!(error.contains("does not resolve"), "{error}");

        // 3. Retargeted locator: pointing the edit at a DIFFERENT block of
        //    the right kind fails the exact-text re-validation.
        let mut value: serde_json::Value =
            serde_json::from_slice(&planned.plan.to_json_bytes().unwrap()).unwrap();
        let target =
            &mut value["patches"]["Dump"]["source"]["Edit"]["edits"][0]["SetProperty"]["target"];
        target["expected_text"] = serde_json::json!("component \"Other\" {}");
        let tampered =
            crate::plan::SavedPlan::from_json_bytes(value.to_string().as_bytes()).unwrap();
        let error = apply_saved_dump_plan(&tampered).unwrap_err().to_string();
        assert!(
            error.contains("different source text") || error.contains("no longer matches"),
            "{error}"
        );

        // 4. Expected-state drift: a tampered expected state is caught by
        //    the deterministic recompile check BEFORE any write.
        let before = std::fs::read(&spec_path).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&planned.plan.to_json_bytes().unwrap()).unwrap();
        value["patches"]["Dump"]["expected_state"]["components"] = serde_json::json!([]);
        let tampered =
            crate::plan::SavedPlan::from_json_bytes(value.to_string().as_bytes()).unwrap();
        let error = apply_saved_dump_plan(&tampered).unwrap_err().to_string();
        assert!(error.contains("different typed state"), "{error}");
        assert_eq!(
            std::fs::read(&spec_path).unwrap(),
            before,
            "expected-state rejection must precede every write"
        );

        // The intact plan still applies.
        apply_saved_dump_plan(&planned.plan).unwrap();
        assert_converges(&spec_path, &library_path);
    }

    #[test]
    fn dump_deletes_out_of_library_blocks_and_reports_multibyte_fragments() {
        let temp = tempfile::tempdir().unwrap();
        // A description longer than the report's inline summary budget whose
        // 120th byte falls INSIDE a two-byte character: 118 ASCII bytes plus
        // the opening quote put 'µ' across bytes 119-120 of the rendered
        // property value.
        let description = format!("{}µ{}", "a".repeat(118), "b".repeat(40));
        let seeded = crate::schlib::dump_source::tests::seeded_snapshot();
        let described = seeded
            .component(0)
            .unwrap()
            .with_description(Some(description.clone()))
            .unwrap();
        let library_path = temp.path().join("seeded.SchLib");
        seeded
            .replace_component(0, described)
            .unwrap()
            .materialize()
            .unwrap()
            .save(&library_path)
            .unwrap();

        let spec_path = temp.path().join("seeded.schlib-spec");
        let created = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        apply_saved_dump_plan(&created.plan).unwrap();

        // Author a stale description and a component the library does not
        // have: the dump must correct the one and delete the other.
        let text = std::fs::read_to_string(&spec_path).unwrap();
        assert!(text.contains(&description));
        let tampered = format!(
            "{}\ncomponent \"EXTRA\" {{\n    designator: \"X?\"\n}}\n",
            text.replace(&description, "stale")
        );
        std::fs::write(&spec_path, &tampered).unwrap();

        let planned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        let PlanPatches::Dump {
            source: PlanSourcePatch::Edit { edits, .. },
            ..
        } = &planned.plan.patches
        else {
            panic!("an authored-but-absent component must plan source edits");
        };
        assert!(
            edits
                .iter()
                .any(|edit| matches!(edit, altium_spec_lang::PlannedSpecEdit::DeleteBlock { .. })),
            "the out-of-library component must plan a persisted DeleteBlock"
        );
        let long_value = edits
            .iter()
            .find_map(|edit| match edit {
                altium_spec_lang::PlannedSpecEdit::SetProperty { value, .. }
                    if value.len() > 120 =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .expect("the corrected description exceeds the summary budget");
        assert!(
            !long_value.is_char_boundary(120),
            "the regression is only meaningful when the cut splits a character: {long_value}"
        );

        // The report is a pure function: it must truncate on a character
        // boundary rather than panicking on the byte index.
        let report = crate::report::report(&planned.plan).unwrap();
        let rendered = crate::report::render_plan_report(&report);
        assert!(rendered.contains('…'), "{rendered}");
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.op == crate::report::ReportOp::Delete),
            "the delete must be an expanded report entry:\n{rendered}"
        );

        let applied = apply_saved_dump_plan(&planned.plan).unwrap();
        assert!(applied.edits_applied >= 2);
        let merged = std::fs::read_to_string(&spec_path).unwrap();
        assert!(!merged.contains("EXTRA"), "{merged}");
        assert!(merged.contains(&description));
        assert_converges(&spec_path, &library_path);
    }

    /// Pins the recorded dump limitation against an import-bearing source:
    /// the merge compares property VALUE TEXT, so an authored expression is
    /// literalized even when it evaluates to the unchanged value, and blocks
    /// the fresh dump does not emit (import, let) are deleted. The manifest
    /// still records and re-verifies the import.
    #[test]
    fn dump_against_an_import_bearing_spec_literalizes_and_deletes_authored_blocks() {
        let temp = tempfile::tempdir().unwrap();
        let seeded = crate::schlib::dump_source::tests::seeded_snapshot();
        let described = seeded
            .component(0)
            .unwrap()
            .with_description(Some("shared description".to_owned()))
            .unwrap();
        let library_path = temp.path().join("seeded.SchLib");
        seeded
            .replace_component(0, described)
            .unwrap()
            .materialize()
            .unwrap()
            .save(&library_path)
            .unwrap();

        let spec_path = temp.path().join("seeded.schlib-spec");
        let created = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        apply_saved_dump_plan(&created.plan).unwrap();

        // Re-author the generated source around an import whose binding
        // evaluates to exactly the library's description.
        std::fs::write(
            temp.path().join("shared.schlib-spec"),
            "let shared_desc = \"shared description\"\n",
        )
        .unwrap();
        let text = std::fs::read_to_string(&spec_path).unwrap();
        let authored = format!(
            "import \"shared.schlib-spec\"\n\n{}",
            text.replace(
                "description: \"shared description\"",
                "description: shared_desc",
            )
        );
        std::fs::write(&spec_path, &authored).unwrap();

        let planned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        // The import is a manifest entry, hash-verified and round-tripping.
        let decoded =
            crate::plan::SavedPlan::from_json_bytes(&planned.plan.to_json_bytes().unwrap())
                .unwrap();
        let import_entry = decoded
            .manifest
            .iter()
            .find(|entry| entry.role == ManifestRole::SpecImport)
            .expect("the transitive import is a manifest entry");
        assert!(import_entry.declared_path.ends_with("shared.schlib-spec"));
        assert!(matches!(
            import_entry.precondition,
            PlanPrecondition::Hash(_)
        ));

        // Recorded limitation: the semantically converged expression still
        // plans as a change, because the merge compares value TEXT.
        assert!(planned.changes);
        assert_eq!(planned.exit_code, PLAN_EXIT_CHANGES);
        let PlanPatches::Dump {
            source: PlanSourcePatch::Edit { edits, .. },
            ..
        } = &decoded.patches
        else {
            panic!("expected source edits");
        };
        let literalizes = edits.iter().any(|edit| {
            matches!(
                edit,
                altium_spec_lang::PlannedSpecEdit::SetProperty { key, value, .. }
                    if key == "description" && value.contains("shared description")
            )
        });
        let deletes_authored_block = edits
            .iter()
            .any(|edit| matches!(edit, altium_spec_lang::PlannedSpecEdit::DeleteBlock { .. }));
        assert!(
            literalizes,
            "the authored expression is literalized (recorded limitation): {edits:#?}"
        );
        assert!(
            deletes_authored_block,
            "blocks the fresh dump does not emit are deleted (recorded limitation): {edits:#?}"
        );

        apply_saved_dump_plan(&decoded).unwrap();
        let merged = std::fs::read_to_string(&spec_path).unwrap();
        assert!(merged.contains("description: \"shared description\""));
        assert!(!merged.contains("shared_desc"));
        assert_converges(&spec_path, &library_path);
    }

    #[test]
    fn dump_merge_ambiguity_blocks_planning() {
        let temp = tempfile::tempdir().unwrap();
        let library_path = write_seeded_library(temp.path());
        let spec_path = temp.path().join("seeded.schlib-spec");
        let created = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        apply_saved_dump_plan(&created.plan).unwrap();

        // Duplicate a pin block in the authored source: the merge cannot
        // key the group and planning blocks with the ambiguity.
        let text = std::fs::read_to_string(&spec_path).unwrap();
        let pin_line = text
            .lines()
            .find(|line| line.trim_start().starts_with("pin \"1\""))
            .unwrap()
            .to_owned();
        let duplicated = text.replace(&pin_line, &format!("{pin_line}\n{pin_line}"));
        std::fs::write(&spec_path, &duplicated).unwrap();
        let error = plan_dump_schlib(&library_path, &spec_path, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ambiguous"), "{error}");
    }

    #[test]
    fn dump_stages_image_bundles_with_absence_and_overwrite_preconditions() {
        let temp = tempfile::tempdir().unwrap();
        let mut library = SchLib::new_blank_ad26().unwrap();
        library
            .insert_image_resource(0, "logo one.png".into(), vec![7, 8, 9, 10])
            .unwrap();
        let library_path = temp.path().join("bundled.SchLib");
        library.save(&library_path).unwrap();
        let spec_path = temp.path().join("bundled.schlib-spec");

        let planned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        let bundle_entry = planned
            .plan
            .manifest
            .iter()
            .find(|entry| entry.declared_path.ends_with(".altium-image"))
            .expect("bundle destination staged");
        assert_eq!(bundle_entry.role, ManifestRole::Destination);
        assert_eq!(bundle_entry.precondition, PlanPrecondition::Absent);
        assert!(bundle_entry.declared_path.contains("logo_one.png"));
        let applied = apply_saved_dump_plan(&planned.plan).unwrap();
        assert_eq!(applied.bundles_written, 1);
        let bundle_path = PathBuf::from(&bundle_entry.declared_path);
        assert_eq!(std::fs::read(&bundle_path).unwrap(), vec![7, 8, 9, 10]);

        // Second planning: bundle content already present → hash
        // precondition, no changes at all.
        let replanned = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        assert_eq!(replanned.exit_code, PLAN_EXIT_NO_CHANGES);
        let applied = apply_saved_dump_plan(&replanned.plan).unwrap();
        assert_eq!(applied.bundles_written, 0);

        // Tampered bundle file: declared overwrite (hash precondition +
        // diagnostic), and apply restores the exact store body.
        std::fs::write(&bundle_path, [1, 2]).unwrap();
        let overwrite = plan_dump_schlib(&library_path, &spec_path, None).unwrap();
        assert_eq!(overwrite.exit_code, PLAN_EXIT_CHANGES);
        assert!(
            overwrite
                .plan
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("overwritten")),
            "{:?}",
            overwrite.plan.diagnostics
        );
        let applied = apply_saved_dump_plan(&overwrite.plan).unwrap();
        assert_eq!(applied.bundles_written, 1);
        assert_eq!(std::fs::read(&bundle_path).unwrap(), vec![7, 8, 9, 10]);
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
