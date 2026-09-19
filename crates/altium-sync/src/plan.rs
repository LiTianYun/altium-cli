//! Saved-plan container: versioned, self-contained JSON plans with a hashed
//! artifact manifest (step-3 increment A; design authority
//! `/tmp/step3/design.md` §1 S1–S11, §2, §4A; NEXT.md "Saved-plan contract").
//!
//! One plan file is one JSON document. It embeds domain/direction/scope, the
//! resolved artifact manifest (BLAKE3 preconditions or expected absence),
//! typed component Create/Replace/Delete patches carrying complete
//! before/after state ([`altium_format::api::ComponentStatePayload`]), the
//! library-namespace ops with embedded image payloads (base64 next to the
//! capture-time BLAKE3 hash — the authorized opaque image-asset boundary),
//! and non-blocking diagnostics. Apply (increment B) needs the plan plus the
//! manifest-listed artifacts and nothing else: no recompilation, no re-read
//! of untracked imports, no fresh minting (creation `UniqueId`s are frozen
//! inside the embedded after-states).
//!
//! Versioning: [`PLAN_SCHEMA_VERSION`] (this crate) gates plan structure,
//! patch semantics, address vocabulary, and manifest rules;
//! `SCHLIB_STATE_FORMAT_VERSION` (altium-format) gates the component-state
//! payload. Decode accepts exactly the compiled-in values and rejects
//! everything else with a regenerate message. Unknown fields are version
//! skew, never ignorable (`deny_unknown_fields` throughout).
//!
//! Determinism: emission is a pure function of the plan value (stable struct
//! field order, compact JSON, no wall-clock timestamps). Two plannings over
//! identical inputs differ only in freshly allocated `UniqueId` fields of
//! created components; [`mask_unique_ids`] is the display/test normalizer
//! that erases exactly those for comparison. It is never an execution input.
//!
//! # Manifest capture seam (increment B)
//!
//! [`PlanArtifact`] deliberately carries the artifact BYTES (or an expected
//! absence), not just a path: the caller reads each input exactly once and
//! both hashes and parses that same buffer (§S7 "hash and parse the same
//! captured contents"). The in-memory test path passes programmatic bytes;
//! increment B's planner binds real files by loading them through
//! [`load_schlib_hashed`] (documents), `CapturedSource` texts (specs and
//! imports), and `CapturedImageAsset` bytes (bundles), then handing those
//! same buffers here. No rework of this type is needed to bind real files —
//! only callers change.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schlib::{SchLibImageAssetOp, SchLibPatch, SchLibPatchError};
use altium_format::api::{ComponentStatePayload, SCHLIB_STATE_FORMAT_VERSION, SchFontAppend};
use altium_format_types::SchRecordType;
use base64::Engine as _;

/// Exact supported saved-plan schema version (design memo §S1). Bumped on
/// any change to plan structure, patch semantics, addresses, or manifest
/// rules; older plans then fail decode with a regenerate message.
pub const PLAN_SCHEMA_VERSION: u32 = 1;

/// The manifest's hash algorithm of record (design memo §S5, ratified Q2).
pub const PLAN_HASH_ALGORITHM: &str = "blake3";

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error(
        "unsupported saved-plan schema version {found} (supported: {supported}); \
         regenerate the plan with the current tool"
    )]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error(
        "unsupported component state format version {found} (supported: {supported}); \
         regenerate the plan with the current tool"
    )]
    UnsupportedStateFormatVersion { found: u32, supported: u32 },
    #[error("{context}: {detail}")]
    Manifest { context: String, detail: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    Json {
        context: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{context}: {source}")]
    Patch {
        context: String,
        #[source]
        source: SchLibPatchError,
    },
    #[error("{context}: {source}")]
    Format {
        context: String,
        #[source]
        source: altium_format::AltiumFormatError,
    },
    #[error("{context}: {detail}")]
    ImagePayload { context: String, detail: String },
}

// ── Manifest ─────────────────────────────────────────────────────────────

/// Role of one manifest artifact (design memo §S5/§S9/§S10). Routing
/// artifacts (step 8) must NOT be added here without the §S12 context-check
/// rule: numeric references inside them are checked against their
/// board/net/layer context at planning, never accepted merely for being in
/// range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ManifestRole {
    /// The root authored spec source.
    SpecSource,
    /// A transitive spec import.
    SpecImport,
    /// An imported `.altium-image` bundle.
    ImageAssetBundle,
    /// A read-only native document input (the `read A, write B` case).
    TargetDocument,
    /// An output path, with its own precondition (absence, or a declared
    /// overwrite hash) independent of any read-only target.
    Destination,
    /// An explicitly declared in-place input/output: ONE artifact protected
    /// by ONE hash precondition (design memo §S10). Nothing is inferred at
    /// apply; the plan states the declared mode.
    InPlaceTarget,
}

impl ManifestRole {
    fn name(self) -> &'static str {
        match self {
            Self::SpecSource => "SpecSource",
            Self::SpecImport => "SpecImport",
            Self::ImageAssetBundle => "ImageAssetBundle",
            Self::TargetDocument => "TargetDocument",
            Self::Destination => "Destination",
            Self::InPlaceTarget => "InPlaceTarget",
        }
    }
}

/// One collision-resistant content hash: algorithm recorded per entry so a
/// future switch is explicit in the data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanHash {
    pub algorithm: String,
    pub hex: String,
    pub byte_len: u64,
}

impl PlanHash {
    fn of_bytes(bytes: &[u8]) -> Self {
        Self {
            algorithm: PLAN_HASH_ALGORITHM.to_owned(),
            hex: blake3::hash(bytes).to_hex().to_string(),
            byte_len: bytes.len() as u64,
        }
    }

    fn verify_bytes(&self, bytes: &[u8], context: &str) -> Result<(), PlanError> {
        if self.algorithm != PLAN_HASH_ALGORITHM {
            return Err(PlanError::ImagePayload {
                context: context.to_owned(),
                detail: format!(
                    "unsupported hash algorithm '{}' (supported: {PLAN_HASH_ALGORITHM})",
                    self.algorithm
                ),
            });
        }
        let actual = Self::of_bytes(bytes);
        if actual.hex != self.hex || actual.byte_len != self.byte_len {
            return Err(PlanError::ImagePayload {
                context: context.to_owned(),
                detail: format!(
                    "content hash mismatch: recorded {} ({} bytes), actual {} ({} bytes)",
                    self.hex, self.byte_len, actual.hex, actual.byte_len
                ),
            });
        }
        Ok(())
    }
}

/// The precondition protecting one manifest entry at apply time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanPrecondition {
    /// The artifact's exact captured content hash.
    Hash(PlanHash),
    /// The path must not exist in ANY form — verification is
    /// `symlink_metadata`, so a dangling symlink occupying the path also
    /// fails (design memo §S6).
    Absent,
}

/// One resolved manifest artifact. Paths are recorded as authored
/// (`declared_path`) and symlink-resolved (`canonical_path`; for absent
/// files: the canonicalized parent joined with the file name). Output paths
/// are frozen here — apply takes only the plan (§S9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub role: ManifestRole,
    pub declared_path: String,
    pub canonical_path: String,
    pub precondition: PlanPrecondition,
}

/// Caller-captured contents of one artifact at planning time (the §S7 seam;
/// see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanArtifactContents {
    /// The exact bytes the planner read once, hashed AND parsed from the
    /// same buffer.
    Bytes(Vec<u8>),
    /// A new output: recorded as `Absent` after verifying nothing occupies
    /// the path right now.
    ExpectedAbsent,
}

/// One artifact submitted to manifest capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanArtifact {
    pub role: ManifestRole,
    pub declared_path: PathBuf,
    pub contents: PlanArtifactContents,
}

/// The single shared canonicalization applied to every manifest entry and
/// the plan output path (design memo §S11): full `canonicalize` for paths
/// that exist in any form (a dangling symlink is an occupying entry and
/// fails resolution with context), canonicalized parent + file name for
/// absent ones.
fn canonicalize_plan_path(path: &Path) -> Result<PathBuf, PlanError> {
    let io = |context: String, source: std::io::Error| PlanError::Io { context, source };
    match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::canonicalize(path).map_err(|source| {
            io(
                format!("resolving existing plan artifact path '{}'", path.display()),
                source,
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let file_name = path.file_name().ok_or_else(|| PlanError::Manifest {
                context: format!("resolving absent plan artifact path '{}'", path.display()),
                detail: "path has no file name component".to_owned(),
            })?;
            let parent = match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let parent = std::fs::canonicalize(parent).map_err(|source| {
                io(
                    format!(
                        "resolving parent directory '{}' of absent path '{}'",
                        parent.display(),
                        path.display()
                    ),
                    source,
                )
            })?;
            Ok(parent.join(file_name))
        }
        Err(source) => Err(io(
            format!("probing plan artifact path '{}'", path.display()),
            source,
        )),
    }
}

fn path_to_string(path: &Path, context: &str) -> Result<String, PlanError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| PlanError::Manifest {
            context: context.to_owned(),
            detail: format!("path '{}' is not valid UTF-8", path.display()),
        })
}

/// Captures and validates the manifest: canonicalization, BLAKE3 hashing of
/// the caller-provided bytes, absence records, and every §S11 rejection —
/// cross-artifact aliases (equal canonical paths under different roles),
/// duplicate destinations (and duplicate entries generally), and plan-path
/// collision with any entry.
fn build_manifest(
    artifacts: Vec<PlanArtifact>,
    plan_path: Option<&Path>,
) -> Result<Vec<ManifestEntry>, PlanError> {
    let mut entries = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        let context = format!(
            "manifest {} entry '{}'",
            artifact.role.name(),
            artifact.declared_path.display()
        );
        let canonical = canonicalize_plan_path(&artifact.declared_path)?;
        let precondition = match artifact.contents {
            PlanArtifactContents::Bytes(bytes) => {
                PlanPrecondition::Hash(PlanHash::of_bytes(&bytes))
            }
            PlanArtifactContents::ExpectedAbsent => {
                if artifact.role != ManifestRole::Destination {
                    return Err(PlanError::Manifest {
                        context,
                        detail: format!(
                            "expected absence is only valid for Destination entries, not {}",
                            artifact.role.name()
                        ),
                    });
                }
                match std::fs::symlink_metadata(&artifact.declared_path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        PlanPrecondition::Absent
                    }
                    Ok(_) => {
                        return Err(PlanError::Manifest {
                            context,
                            detail: "output path was declared new, but something already \
                                     occupies it"
                                .to_owned(),
                        });
                    }
                    Err(source) => return Err(PlanError::Io { context, source }),
                }
            }
        };
        entries.push(ManifestEntry {
            role: artifact.role,
            declared_path: path_to_string(&artifact.declared_path, &context)?,
            canonical_path: path_to_string(&canonical, &context)?,
            precondition,
        });
    }
    let plan_canonical = plan_path
        .map(|path| {
            canonicalize_plan_path(path)
                .and_then(|canonical| path_to_string(&canonical, "resolving the plan output path"))
        })
        .transpose()?;
    check_manifest_collisions(&entries, plan_canonical.as_deref())?;
    Ok(entries)
}

/// The §S11 rejection set over already-canonicalized entries. Re-run
/// verbatim on decode (and again at apply, where resolution is recomputed).
fn check_manifest_collisions(
    entries: &[ManifestEntry],
    plan_canonical_path: Option<&str>,
) -> Result<(), PlanError> {
    for (position, entry) in entries.iter().enumerate() {
        for earlier in &entries[..position] {
            if earlier.canonical_path == entry.canonical_path {
                let detail = if earlier.role == entry.role {
                    if entry.role == ManifestRole::Destination {
                        format!(
                            "duplicate destination: '{}' and '{}' resolve to the same \
                             path '{}'",
                            earlier.declared_path, entry.declared_path, entry.canonical_path
                        )
                    } else {
                        format!(
                            "duplicate {} entry: '{}' and '{}' resolve to the same \
                             path '{}'",
                            entry.role.name(),
                            earlier.declared_path,
                            entry.declared_path,
                            entry.canonical_path
                        )
                    }
                } else {
                    format!(
                        "cross-artifact alias: {} '{}' and {} '{}' resolve to the same \
                         path '{}' (declare a single InPlaceTarget for an intentional \
                         in-place input/output)",
                        earlier.role.name(),
                        earlier.declared_path,
                        entry.role.name(),
                        entry.declared_path,
                        entry.canonical_path
                    )
                };
                return Err(PlanError::Manifest {
                    context: "validating the plan manifest".to_owned(),
                    detail,
                });
            }
        }
        if plan_canonical_path == Some(entry.canonical_path.as_str()) {
            return Err(PlanError::Manifest {
                context: "validating the plan manifest".to_owned(),
                detail: format!(
                    "the plan file path collides with {} entry '{}' (resolved '{}')",
                    entry.role.name(),
                    entry.declared_path,
                    entry.canonical_path
                ),
            });
        }
    }
    Ok(())
}

// ── Scope summary ────────────────────────────────────────────────────────

/// Whether a recorded collection scope was authored partial or complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanScopeClass {
    Partial,
    Complete,
}

/// The managed collection a scope line addresses — the serializable mirror
/// of the closed `altium_spec_lang::ast::CollectionKind` vocabulary (the
/// same pattern `PlanScopeClass` uses for `CollectionMode`), so the
/// versioned plan schema carries a typed collection name, not a free
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanCollectionKind {
    Components,
    Parts,
    Pins,
    Parameters,
    Aliases,
    Footprints,
    Graphics,
    Maps,
}

/// What a recorded scope line applies to: the library's top-level
/// components collection, or one component's managed child collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanScopeSubject {
    Library,
    Component { lib_reference: String },
}

/// One recorded source-declared scope line: report/diagnostic metadata, NOT
/// an execution input (the patches already encode the outcome).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanScopeEntry {
    pub subject: PlanScopeSubject,
    pub collection: PlanCollectionKind,
    pub class: PlanScopeClass,
    pub delete_allowed: bool,
}

/// Recorded source-declared scope summary (design memo §S4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanScope {
    pub entries: Vec<PlanScopeEntry>,
}

// ── Patches ──────────────────────────────────────────────────────────────

/// Address of one component patch target: the position in the patch's final
/// (post-library-edit) component sequence for replaces, the input position
/// for deletes — always paired with the component's `lib_reference` so
/// review and apply can cross-check both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanComponentAddress {
    pub index: usize,
    pub lib_reference: String,
}

/// Serialized mirror of one reported `current_part_id` adjustment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanCurrentPartIdAdjustment {
    pub before: i32,
    pub after: i32,
}

/// Serialized mirror of one deleted child record from a parts shrink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRemovedPartChild {
    pub child_index: usize,
    pub kind: SchRecordType,
    pub owner_part_id: i32,
}

/// Serialized mirror of a typed parts-shrink report carried by a component
/// replacement (deletes are never hidden behind counts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanPartsShrink {
    pub removed_parts: Vec<i32>,
    pub deleted_children: Vec<PlanRemovedPartChild>,
    pub current_part_id_adjustment: Option<PlanCurrentPartIdAdjustment>,
}

/// One typed component patch with embedded complete before/after state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanComponentPatch {
    /// A created component. Its allocated native `UniqueId`s are frozen
    /// inside `after` (native output data, not synchronization bindings).
    Create {
        lib_reference: String,
        after: ComponentStatePayload,
    },
    /// A replaced component, addressed in the final (post-library-edit)
    /// component sequence, with the exact before state it replaces.
    Replace {
        address: PlanComponentAddress,
        before: ComponentStatePayload,
        after: ComponentStatePayload,
        parts_shrink: Option<PlanPartsShrink>,
    },
    /// A deleted component, addressed by its input position, with the
    /// complete destroyed state embedded for review and recovery.
    Delete {
        address: PlanComponentAddress,
        before: ComponentStatePayload,
    },
}

/// One embedded image-asset payload: the complete encoded body (base64 —
/// the authorized opaque boundary), its capture-time BLAKE3 hash, and the
/// canonical source path it was captured from. Decode re-verifies the hash
/// against the decoded bytes; a mismatch is a hard error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanImagePayload {
    pub source_path: String,
    pub content_hash: PlanHash,
    pub base64: String,
}

impl PlanImagePayload {
    fn from_asset(asset: &crate::image_asset::CapturedImageAsset) -> Result<Self, PlanError> {
        Ok(Self {
            source_path: path_to_string(
                asset.canonical_path(),
                "embedding captured image asset path",
            )?,
            content_hash: PlanHash {
                algorithm: PLAN_HASH_ALGORITHM.to_owned(),
                hex: blake3::Hash::from_bytes(*asset.content_hash())
                    .to_hex()
                    .to_string(),
                byte_len: asset.bytes().len() as u64,
            },
            base64: base64::engine::general_purpose::STANDARD.encode(asset.bytes()),
        })
    }

    /// Decodes and hash-verifies the embedded body.
    pub fn decoded_bytes(&self) -> Result<Vec<u8>, PlanError> {
        let context = format!(
            "decoding embedded image payload from '{}'",
            self.source_path
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.base64)
            .map_err(|error| PlanError::ImagePayload {
                context: context.clone(),
                detail: format!("invalid base64: {error}"),
            })?;
        self.content_hash.verify_bytes(&bytes, &context)?;
        Ok(bytes)
    }
}

/// Serialized mirror of one font-table append.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanFontAppend {
    pub name: String,
    pub size: i32,
    pub rotation: i32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

impl PlanFontAppend {
    fn from_op(append: &SchFontAppend) -> Self {
        Self {
            name: append.name.clone(),
            size: append.size,
            rotation: append.rotation,
            bold: append.bold,
            italic: append.italic,
            underline: append.underline,
            strikeout: append.strikeout,
        }
    }
}

/// One library-namespace op, serialized from the `SchLibLibraryOp`
/// vocabulary (component create/remove lower to
/// [`PlanComponentPatch::Create`]/[`PlanComponentPatch::Delete`] instead).
/// Emitted in the D1 application order: font appends, image-asset ops (in
/// relative order), alias removes, alias adds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanLibraryOp {
    FontAppend(PlanFontAppend),
    ImageAssetInsert {
        index: usize,
        name: String,
        payload: PlanImagePayload,
    },
    ImageAssetReplace {
        index: usize,
        name: String,
        payload: PlanImagePayload,
    },
    ImageAssetRemove {
        index: usize,
    },
    AliasRemove {
        component: String,
        alias: String,
    },
    AliasAdd {
        component: String,
        alias: String,
    },
}

/// Direction-specific patch payloads. Dump-direction source patches are
/// increment C (design memo §S16); adding them extends this enum under the
/// same schema gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlanPatches {
    Compile {
        library_ops: Vec<PlanLibraryOp>,
        components: Vec<PlanComponentPatch>,
    },
}

/// One non-blocking planning note carried by the plan (blocking diagnostics
/// prevent plan emission entirely).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDiagnostic {
    pub message: String,
}

/// The plan's domain (one variant in step 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum PlanDomain {
    Schlib,
}

/// The planning direction the patches were computed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum PlanDirection {
    Compile,
    Dump,
}

// ── The plan document ────────────────────────────────────────────────────

/// One self-contained saved plan (design memo §S4). Field order is the
/// serialization order; do not reorder without bumping
/// [`PLAN_SCHEMA_VERSION`]. [`SavedPlan::from_json_bytes`] is the decode
/// contract: exact versions, unknown fields rejected, §S11 re-checked, and
/// every embedded image payload hash-verified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPlan {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    pub state_format_version: u32,
    pub domain: PlanDomain,
    pub direction: PlanDirection,
    pub scope: PlanScope,
    pub manifest: Vec<ManifestEntry>,
    pub patches: PlanPatches,
    pub diagnostics: Vec<PlanDiagnostic>,
}

fn deserialize_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let found = u32::deserialize(deserializer)?;
    if found != PLAN_SCHEMA_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported saved-plan schema version {found} (supported: \
             {PLAN_SCHEMA_VERSION}); regenerate the plan with the current tool"
        )));
    }
    Ok(found)
}

impl SavedPlan {
    /// Lowers a reviewed compile-direction [`SchLibPatch`] into a saved
    /// plan: component creates/replaces/deletes with complete embedded
    /// before/after state (creation `UniqueId`s frozen from the cached
    /// reviewed result), library-namespace ops with embedded image
    /// payloads, the captured manifest, and the recorded scope summary.
    ///
    /// `artifacts` is the caller-provided §S7 capture set (see the module
    /// documentation for the increment-B seam); `plan_path` is the frozen
    /// output path of the plan file itself, checked for collisions when
    /// known.
    pub fn compile_from_schlib_patch(
        patch: &SchLibPatch,
        scope: PlanScope,
        artifacts: Vec<PlanArtifact>,
        plan_path: Option<&Path>,
        diagnostics: Vec<PlanDiagnostic>,
    ) -> Result<Self, PlanError> {
        let manifest = build_manifest(artifacts, plan_path)?;

        // Library-namespace ops in D1 application order.
        let mut library_ops = Vec::new();
        for append in patch.font_appends() {
            library_ops.push(PlanLibraryOp::FontAppend(PlanFontAppend::from_op(append)));
        }
        for op in patch.image_asset_ops() {
            library_ops.push(match op {
                SchLibImageAssetOp::Insert { index, name, asset } => {
                    PlanLibraryOp::ImageAssetInsert {
                        index: *index,
                        name: name.clone(),
                        payload: PlanImagePayload::from_asset(asset)?,
                    }
                }
                SchLibImageAssetOp::Replace { index, name, asset } => {
                    PlanLibraryOp::ImageAssetReplace {
                        index: *index,
                        name: name.clone(),
                        payload: PlanImagePayload::from_asset(asset)?,
                    }
                }
                SchLibImageAssetOp::Remove { index } => {
                    PlanLibraryOp::ImageAssetRemove { index: *index }
                }
            });
        }
        for change in patch.alias_removals() {
            library_ops.push(PlanLibraryOp::AliasRemove {
                component: change.component.clone(),
                alias: change.alias.clone(),
            });
        }
        for change in patch.alias_additions() {
            library_ops.push(PlanLibraryOp::AliasAdd {
                component: change.component.clone(),
                alias: change.alias.clone(),
            });
        }

        // Component patches: creates (final order), replaces, deletes.
        let mut components = Vec::new();
        let result = patch.result_snapshot();
        for position in patch.created_component_positions() {
            let after = result
                .component(position)
                .map_err(|source| PlanError::Format {
                    context: format!(
                        "reading created component at final position {position} out of the \
                         reviewed result"
                    ),
                    source,
                })?
                .encode_state();
            components.push(PlanComponentPatch::Create {
                lib_reference: after.lib_reference().to_owned(),
                after,
            });
        }
        for replacement in patch.replacements() {
            let before = replacement.before().encode_state();
            let after = replacement.after().encode_state();
            let parts_shrink = patch
                .parts_shrinks()
                .iter()
                .find(|shrink| shrink.component == before.lib_reference())
                .map(|shrink| PlanPartsShrink {
                    removed_parts: shrink.removed_parts.clone(),
                    deleted_children: shrink
                        .deleted_children
                        .iter()
                        .map(|child| PlanRemovedPartChild {
                            child_index: child.child_index,
                            kind: child.kind,
                            owner_part_id: child.owner_part_id,
                        })
                        .collect(),
                    current_part_id_adjustment: shrink.current_part_id_adjustment.map(
                        |adjustment| PlanCurrentPartIdAdjustment {
                            before: adjustment.before,
                            after: adjustment.after,
                        },
                    ),
                });
            components.push(PlanComponentPatch::Replace {
                address: PlanComponentAddress {
                    index: replacement.index(),
                    lib_reference: before.lib_reference().to_owned(),
                },
                before,
                after,
                parts_shrink,
            });
        }
        let input = patch.input_snapshot();
        for removal in patch.removed_components() {
            let before = input
                .component(removal.input_index)
                .map_err(|source| PlanError::Format {
                    context: format!(
                        "reading removed component '{}' (input slot {}) out of the input \
                         snapshot",
                        removal.lib_reference, removal.input_index
                    ),
                    source,
                })?
                .encode_state();
            components.push(PlanComponentPatch::Delete {
                address: PlanComponentAddress {
                    index: removal.input_index,
                    lib_reference: removal.lib_reference.clone(),
                },
                before,
            });
        }

        Ok(Self {
            schema_version: PLAN_SCHEMA_VERSION,
            state_format_version: SCHLIB_STATE_FORMAT_VERSION,
            domain: PlanDomain::Schlib,
            direction: PlanDirection::Compile,
            scope,
            manifest,
            patches: PlanPatches::Compile {
                library_ops,
                components,
            },
            diagnostics,
        })
    }

    /// Deterministic compact JSON emission: a pure function of the plan
    /// value — stable field order, no timestamps. Two emissions of the same
    /// plan are byte-identical.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, PlanError> {
        serde_json::to_vec(self).map_err(|source| PlanError::Json {
            context: "serializing saved plan".to_owned(),
            source,
        })
    }

    /// Strict decode: exact `schema_version` and `state_format_version`
    /// (regenerate message otherwise), unknown fields rejected at every
    /// level, the §S11 manifest checks re-run on the decoded entries, and
    /// every embedded image payload hash-verified against its decoded
    /// bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, PlanError> {
        let plan: Self = serde_json::from_slice(bytes).map_err(|source| PlanError::Json {
            context: "decoding saved plan".to_owned(),
            source,
        })?;
        if plan.state_format_version != SCHLIB_STATE_FORMAT_VERSION {
            return Err(PlanError::UnsupportedStateFormatVersion {
                found: plan.state_format_version,
                supported: SCHLIB_STATE_FORMAT_VERSION,
            });
        }
        check_manifest_collisions(&plan.manifest, None)?;
        let PlanPatches::Compile { library_ops, .. } = &plan.patches;
        for op in library_ops {
            if let PlanLibraryOp::ImageAssetInsert { payload, .. }
            | PlanLibraryOp::ImageAssetReplace { payload, .. } = op
            {
                payload.decoded_bytes()?;
            }
        }
        Ok(plan)
    }
}

// ── §S7 helper ───────────────────────────────────────────────────────────

/// A SchLib loaded from exactly one read of its file: the hash and the
/// parse both consumed `bytes` (never two racing reads of the path).
#[derive(Debug)]
pub struct LoadedSchLib {
    pub library: altium_format::SchLib,
    pub bytes: Vec<u8>,
    pub content_hash: PlanHash,
}

/// Reads the file once, hashes that buffer, and parses the SAME buffer via
/// the in-memory CFB reader path (design memo §S7). Increment B's planner
/// builds its `TargetDocument`/`InPlaceTarget` manifest artifacts from the
/// returned bytes.
pub fn load_schlib_hashed(path: &Path) -> Result<LoadedSchLib, PlanError> {
    let bytes = std::fs::read(path).map_err(|source| PlanError::Io {
        context: format!("reading SchLib '{}'", path.display()),
        source,
    })?;
    let content_hash = PlanHash::of_bytes(&bytes);
    let library =
        altium_format::SchLib::from_bytes(&bytes).map_err(|source| PlanError::Format {
            context: format!("parsing SchLib '{}' from its hashed bytes", path.display()),
            source,
        })?;
    Ok(LoadedSchLib {
        library,
        bytes,
        content_hash,
    })
}

// ── Display/test normalizer ──────────────────────────────────────────────

/// Erases every `unique_id` field value in a JSON tree (the ONLY
/// nondeterministic content of a plan: freshly allocated creation ids).
/// This is a comparison/display utility for the determinism gate and
/// review tooling — never an execution input.
pub fn mask_unique_ids(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if key == "unique_id" && entry.is_string() {
                    *entry = serde_json::Value::String(String::new());
                } else {
                    mask_unique_ids(entry);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                mask_unique_ids(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_asset::CapturedImageAsset;
    use crate::schlib::{SchLibAliasChange, SchLibImageAssetOp, SchLibLibraryOp};
    use altium_format::SchLib;
    use altium_format::api::{SchLibComponentEdit, SchLibEdit, SchNewComponent};
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

    /// A library with `Component_1` grown to two parts plus a `VICTIM`
    /// clone, saved to disk so manifests can bind real bytes.
    fn base_library(temp: &Path) -> (PathBuf, SchLib) {
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
        let final_snapshot = cloned.replace_component(0, grown).unwrap();
        let library = final_snapshot.materialize().unwrap();
        let path = temp.join("base.SchLib");
        library.save(&path).unwrap();
        let reopened = SchLib::open(&path).unwrap();
        (path, reopened)
    }

    /// One intent producing create + replace (with parts shrink) + delete +
    /// an alias library op, matching the full compile lowering surface.
    fn full_intent() -> SchLibIntent {
        let mut existing = blank_component_intent("Component_1");
        existing.description = authored(Some("plan lowering probe".to_owned()));
        existing.parts = Collection {
            scope: complete_scope(),
            items: vec![part_item(1)],
        };
        let mut probe = blank_component_intent("PLAN_PROBE");
        probe.parameters = Collection {
            scope: CollectionScope::default(),
            items: vec![ParameterIntent {
                name: spanned("PLAN_PARAM".to_owned()),
                text: authored("created by the plan test".to_owned()),
                is_hidden: FieldIntent::Absent,
            }],
        };
        probe.aliases = Collection {
            scope: CollectionScope::default(),
            items: vec![spanned("PLAN_ALIAS".to_owned())],
        };
        SchLibIntent {
            schema_version: INTENT_SCHEMA_VERSION,
            components: Collection {
                scope: complete_scope(),
                items: vec![existing, probe],
            },
        }
    }

    fn scope_summary() -> PlanScope {
        PlanScope {
            entries: vec![PlanScopeEntry {
                subject: PlanScopeSubject::Library,
                collection: PlanCollectionKind::Components,
                class: PlanScopeClass::Complete,
                delete_allowed: true,
            }],
        }
    }

    fn manifest_artifacts(temp: &Path, library_path: &Path) -> Vec<PlanArtifact> {
        let loaded = load_schlib_hashed(library_path).unwrap();
        vec![
            PlanArtifact {
                role: ManifestRole::TargetDocument,
                declared_path: library_path.to_owned(),
                contents: PlanArtifactContents::Bytes(loaded.bytes),
            },
            PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: temp.join("out.SchLib"),
                contents: PlanArtifactContents::ExpectedAbsent,
            },
        ]
    }

    fn full_plan(temp: &Path) -> (SavedPlan, SchLibPatch) {
        let (library_path, library) = base_library(temp);
        let input = library.snapshot().unwrap();
        let patch = SchLibPatch::from_intent(&input, &full_intent()).unwrap();
        let plan = SavedPlan::compile_from_schlib_patch(
            &patch,
            scope_summary(),
            manifest_artifacts(temp, &library_path),
            Some(&temp.join("plan.json")),
            vec![PlanDiagnostic {
                message: "non-blocking note".to_owned(),
            }],
        )
        .unwrap();
        (plan, patch)
    }

    #[test]
    fn lowering_carries_complete_typed_patches_and_survives_strict_decode() {
        let temp = tempfile::tempdir().unwrap();
        let (plan, patch) = full_plan(temp.path());
        assert_eq!(plan.schema_version, PLAN_SCHEMA_VERSION);
        assert_eq!(plan.state_format_version, SCHLIB_STATE_FORMAT_VERSION);
        assert_eq!(plan.domain, PlanDomain::Schlib);
        assert_eq!(plan.direction, PlanDirection::Compile);
        assert_eq!(plan.diagnostics.len(), 1);
        assert_eq!(plan.manifest.len(), 2);
        assert!(matches!(
            plan.manifest[0].precondition,
            PlanPrecondition::Hash(ref hash) if hash.algorithm == PLAN_HASH_ALGORITHM
        ));
        assert_eq!(plan.manifest[1].precondition, PlanPrecondition::Absent);

        let PlanPatches::Compile {
            library_ops,
            components,
        } = &plan.patches;
        assert_eq!(
            library_ops.as_slice(),
            &[PlanLibraryOp::AliasAdd {
                component: "PLAN_PROBE".to_owned(),
                alias: "PLAN_ALIAS".to_owned(),
            }]
        );
        // Create + two replaces (the created component's children apply as
        // a content replacement after the creating library edit) + delete.
        assert_eq!(components.len(), 4);

        // Create: after-state equals the reviewed result's read-out.
        let result = patch.resulting_snapshot().unwrap();
        let created_position = patch.created_component_positions()[0];
        let PlanComponentPatch::Create {
            lib_reference,
            after,
        } = &components[0]
        else {
            panic!("expected Create first, got {:?}", components[0]);
        };
        assert_eq!(lib_reference, "PLAN_PROBE");
        assert_eq!(
            *after,
            result.component(created_position).unwrap().encode_state()
        );

        // Replaces: typed before/after equality via the payload; the
        // parts-shrink report is carried on the shrunk component.
        assert_eq!(patch.replacements().len(), 2);
        for (position, replacement) in patch.replacements().iter().enumerate() {
            let PlanComponentPatch::Replace {
                address,
                before,
                after,
                parts_shrink,
            } = &components[1 + position]
            else {
                panic!(
                    "expected Replace at {}, got {:?}",
                    1 + position,
                    components[1 + position]
                );
            };
            assert_eq!(address.lib_reference, replacement.before().lib_reference());
            assert_eq!(address.index, replacement.index());
            assert_eq!(*before, replacement.before().encode_state());
            assert_eq!(*after, replacement.after().encode_state());
            if address.lib_reference == "Component_1" {
                let shrink = parts_shrink.as_ref().expect("parts shrink carried");
                assert_eq!(shrink.removed_parts, vec![2]);
                assert!(shrink.deleted_children.is_empty());
            } else {
                assert_eq!(address.lib_reference, "PLAN_PROBE");
                assert!(parts_shrink.is_none());
            }
        }

        // Delete: complete destroyed state embedded, addressed by input slot.
        let input = library_input(temp.path());
        let PlanComponentPatch::Delete { address, before } = &components[3] else {
            panic!("expected Delete last, got {:?}", components[3]);
        };
        assert_eq!(address.lib_reference, "VICTIM");
        assert_eq!(
            *before,
            input.component(address.index).unwrap().encode_state()
        );

        // Strict decode round-trip: typed equality of the whole plan.
        let bytes = plan.to_json_bytes().unwrap();
        let decoded = SavedPlan::from_json_bytes(&bytes).unwrap();
        assert_eq!(decoded, plan);
    }

    fn library_input(temp: &Path) -> altium_format::api::SchLibSnapshot {
        SchLib::open(temp.join("base.SchLib"))
            .unwrap()
            .snapshot()
            .unwrap()
    }

    #[test]
    fn emission_is_byte_deterministic_and_id_masked_across_constructions() {
        let temp = tempfile::tempdir().unwrap();
        let (plan, _) = full_plan(temp.path());
        assert_eq!(
            plan.to_json_bytes().unwrap(),
            plan.to_json_bytes().unwrap(),
            "two emissions of one plan must be byte-identical"
        );

        // A second planning over identical inputs differs ONLY in freshly
        // allocated UniqueIds of the created component.
        let (library_path, library) = (temp.path().join("base.SchLib"), library_input(temp.path()));
        let patch = SchLibPatch::from_intent(&library, &full_intent()).unwrap();
        let second = SavedPlan::compile_from_schlib_patch(
            &patch,
            scope_summary(),
            manifest_artifacts(temp.path(), &library_path),
            Some(&temp.path().join("plan.json")),
            vec![PlanDiagnostic {
                message: "non-blocking note".to_owned(),
            }],
        )
        .unwrap();
        let first_bytes = plan.to_json_bytes().unwrap();
        let second_bytes = second.to_json_bytes().unwrap();
        assert_ne!(
            first_bytes, second_bytes,
            "fresh creation UniqueIds must differ across plannings"
        );
        let mut first_masked: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
        let mut second_masked: serde_json::Value = serde_json::from_slice(&second_bytes).unwrap();
        mask_unique_ids(&mut first_masked);
        mask_unique_ids(&mut second_masked);
        assert_eq!(
            first_masked, second_masked,
            "plans must be identical modulo allocated UniqueIds"
        );
    }

    #[test]
    fn decode_rejects_version_skew_and_unknown_fields_at_both_layers() {
        let temp = tempfile::tempdir().unwrap();
        let (plan, _) = full_plan(temp.path());
        let mut value: serde_json::Value =
            serde_json::from_slice(&plan.to_json_bytes().unwrap()).unwrap();

        // Plan schema version.
        let mut skew = value.clone();
        skew["schema_version"] = serde_json::json!(PLAN_SCHEMA_VERSION + 1);
        let error = SavedPlan::from_json_bytes(skew.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("regenerate"), "{error}");

        // Plan-level state format version.
        let mut skew = value.clone();
        skew["state_format_version"] = serde_json::json!(SCHLIB_STATE_FORMAT_VERSION + 1);
        let error = SavedPlan::from_json_bytes(skew.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("regenerate"), "{error}");

        // Payload-level state format version (checked by altium-format's
        // own deserializer).
        let mut skew = value.clone();
        skew["patches"]["Compile"]["components"][0]["Create"]["after"]["state_format_version"] =
            serde_json::json!(SCHLIB_STATE_FORMAT_VERSION + 1);
        let error = SavedPlan::from_json_bytes(skew.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("regenerate"), "{error}");

        // Unknown fields: plan level and payload level.
        let mut unknown = value.clone();
        unknown["surprise"] = serde_json::json!(1);
        let error = SavedPlan::from_json_bytes(unknown.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("surprise"), "{error}");
        value["patches"]["Compile"]["components"][0]["Create"]["after"]["surprise"] =
            serde_json::json!(1);
        let error = SavedPlan::from_json_bytes(value.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("surprise"), "{error}");
    }

    fn empty_patch(library: &SchLib) -> SchLibPatch {
        SchLibPatch::component_properties(&library.snapshot().unwrap(), []).unwrap()
    }

    fn lower_with_artifacts(
        library: &SchLib,
        artifacts: Vec<PlanArtifact>,
        plan_path: Option<&Path>,
    ) -> Result<SavedPlan, PlanError> {
        SavedPlan::compile_from_schlib_patch(
            &empty_patch(library),
            PlanScope::default(),
            artifacts,
            plan_path,
            Vec::new(),
        )
    }

    #[test]
    fn manifest_rejects_every_s11_collision_class_with_context() {
        let temp = tempfile::tempdir().unwrap();
        let (library_path, library) = base_library(temp.path());
        let bytes = std::fs::read(&library_path).unwrap();

        // Duplicate destinations through different spellings of one path.
        let error = lower_with_artifacts(
            &library,
            vec![
                PlanArtifact {
                    role: ManifestRole::Destination,
                    declared_path: temp.path().join("out.SchLib"),
                    contents: PlanArtifactContents::ExpectedAbsent,
                },
                PlanArtifact {
                    role: ManifestRole::Destination,
                    declared_path: temp.path().join("./out.SchLib"),
                    contents: PlanArtifactContents::ExpectedAbsent,
                },
            ],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("duplicate destination"), "{error}");
        assert!(error.contains("out.SchLib"), "{error}");

        // Cross-artifact alias: input and output resolving to one path.
        let error = lower_with_artifacts(
            &library,
            vec![
                PlanArtifact {
                    role: ManifestRole::TargetDocument,
                    declared_path: library_path.clone(),
                    contents: PlanArtifactContents::Bytes(bytes.clone()),
                },
                PlanArtifact {
                    role: ManifestRole::Destination,
                    declared_path: temp.path().join("./base.SchLib"),
                    contents: PlanArtifactContents::Bytes(bytes.clone()),
                },
            ],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cross-artifact alias"), "{error}");
        assert!(error.contains("InPlaceTarget"), "{error}");

        // Duplicate non-destination entries are accidental aliases too.
        let error = lower_with_artifacts(
            &library,
            vec![
                PlanArtifact {
                    role: ManifestRole::SpecImport,
                    declared_path: library_path.clone(),
                    contents: PlanArtifactContents::Bytes(bytes.clone()),
                },
                PlanArtifact {
                    role: ManifestRole::SpecImport,
                    declared_path: temp.path().join("./base.SchLib"),
                    contents: PlanArtifactContents::Bytes(bytes.clone()),
                },
            ],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("duplicate SpecImport"), "{error}");

        // Plan-file collision with a manifest entry.
        let error = lower_with_artifacts(
            &library,
            vec![PlanArtifact {
                role: ManifestRole::InPlaceTarget,
                declared_path: library_path.clone(),
                contents: PlanArtifactContents::Bytes(bytes.clone()),
            }],
            Some(&temp.path().join("./base.SchLib")),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("plan file path collides"), "{error}");

        // Expected-absent output that something already occupies.
        let error = lower_with_artifacts(
            &library,
            vec![PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: library_path.clone(),
                contents: PlanArtifactContents::ExpectedAbsent,
            }],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("already occupies"), "{error}");

        // Expected absence is a Destination-only precondition.
        let error = lower_with_artifacts(
            &library,
            vec![PlanArtifact {
                role: ManifestRole::SpecSource,
                declared_path: temp.path().join("missing.schlib-spec"),
                contents: PlanArtifactContents::ExpectedAbsent,
            }],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("only valid for Destination"), "{error}");

        // Decode re-runs the checks on the stored manifest.
        let (mut plan, _) = full_plan(temp.path());
        plan.manifest.push(plan.manifest[0].clone());
        let error = SavedPlan::from_json_bytes(&plan.to_json_bytes().unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_aliases_are_resolved_and_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let (library_path, library) = base_library(temp.path());
        let bytes = std::fs::read(&library_path).unwrap();
        let alias = temp.path().join("alias.SchLib");
        std::os::unix::fs::symlink(&library_path, &alias).unwrap();
        let error = lower_with_artifacts(
            &library,
            vec![
                PlanArtifact {
                    role: ManifestRole::TargetDocument,
                    declared_path: library_path,
                    contents: PlanArtifactContents::Bytes(bytes.clone()),
                },
                PlanArtifact {
                    role: ManifestRole::Destination,
                    declared_path: alias.clone(),
                    contents: PlanArtifactContents::Bytes(bytes),
                },
            ],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cross-artifact alias"), "{error}");

        // A dangling symlink occupies its path: it can be neither an
        // expected-absent output nor a resolvable artifact.
        let dangling = temp.path().join("dangling.SchLib");
        std::os::unix::fs::symlink(temp.path().join("nowhere"), &dangling).unwrap();
        let error = lower_with_artifacts(
            &library,
            vec![PlanArtifact {
                role: ManifestRole::Destination,
                declared_path: dangling,
                contents: PlanArtifactContents::ExpectedAbsent,
            }],
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("already occupies") || error.contains("resolving"),
            "{error}"
        );
    }

    #[test]
    fn image_payloads_embed_verify_and_reject_tampering() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("logo.altium-image");
        std::fs::write(&bundle, b"complete encoded image body").unwrap();
        let asset = CapturedImageAsset::capture(&ImageAssetSource {
            path: "logo.altium-image".to_owned(),
            declaring_source: Some(temp.path().join("library.schlib-spec")),
        })
        .unwrap();
        let (library_path, library) = base_library(temp.path());
        let input = library.snapshot().unwrap();
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
        let plan = SavedPlan::compile_from_schlib_patch(
            &patch,
            PlanScope::default(),
            manifest_artifacts(temp.path(), &library_path),
            None,
            Vec::new(),
        )
        .unwrap();
        let PlanPatches::Compile { library_ops, .. } = &plan.patches;
        let PlanLibraryOp::ImageAssetInsert { name, payload, .. } = &library_ops[0] else {
            panic!("expected image insert first, got {:?}", library_ops[0]);
        };
        assert_eq!(name, "logo.png");
        assert_eq!(
            payload.decoded_bytes().unwrap(),
            b"complete encoded image body"
        );
        assert_eq!(
            payload.content_hash.hex,
            blake3::Hash::from_bytes(*asset.content_hash())
                .to_hex()
                .to_string()
        );
        assert!(payload.source_path.ends_with("logo.altium-image"));
        assert!(matches!(
            &library_ops[1],
            PlanLibraryOp::AliasAdd { component, alias }
                if component == "Component_1" && alias == "IMG_ALIAS"
        ));

        // Decode round-trip preserves the ops; tampering with either the
        // body or the recorded hash is a decode-time hard error.
        let bytes = plan.to_json_bytes().unwrap();
        assert_eq!(SavedPlan::from_json_bytes(&bytes).unwrap(), plan);
        let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        tampered["patches"]["Compile"]["library_ops"][0]["ImageAssetInsert"]["payload"]["base64"] =
            serde_json::json!(base64::engine::general_purpose::STANDARD.encode(b"different body"));
        let error = SavedPlan::from_json_bytes(tampered.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("hash mismatch"), "{error}");
        let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        tampered["patches"]["Compile"]["library_ops"][0]["ImageAssetInsert"]["payload"]["base64"] =
            serde_json::json!("not!base64");
        let error = SavedPlan::from_json_bytes(tampered.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid base64"), "{error}");
    }

    #[test]
    fn load_schlib_hashed_parses_the_hashed_buffer() {
        let temp = tempfile::tempdir().unwrap();
        let (library_path, library) = base_library(temp.path());
        let loaded = load_schlib_hashed(&library_path).unwrap();
        assert_eq!(loaded.library, library);
        assert_eq!(loaded.content_hash, PlanHash::of_bytes(&loaded.bytes));
        assert_eq!(loaded.content_hash.algorithm, PLAN_HASH_ALGORITHM);
        let error = load_schlib_hashed(&temp.path().join("missing.SchLib"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing.SchLib"), "{error}");
    }
}
