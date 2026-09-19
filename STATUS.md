# Codebase Status

Updated: 2026-09-17 (NEXT.md step 2 complete; older corpus results remain dated below)

## Workspace Overview

Rust workspace for reading, writing, querying, and rendering Altium Designer files.

```
altium-format-types    (domain types, enums, constants — zero deps)
altium-format-derive   (proc macros: FromParams, ToParams, OpsSchema, OpsEnum)
altium-format          (core: parsing, serialization, high-level API, rendering infra)
  ├→ altium-format-query       (AQL query language engine)
  ├→ altium-format-render-svg  (SVG rendering backend)
  └→ altium-format-render-png  (PNG rasterization via resvg)
altium-format-spec     (declarative spec language for Altium documents — being replaced)
altium-spec-lang       (lossless CST, parser, formatter, expression evaluation)
altium-sync            (asset capture, snapshot-bound addresses/patches, library ops, full SchLib intent elaboration)
altium-cli             (CLI binary)
```

## Plan/apply rebuild (design simplified, 2026-09-05)

[NEXT.md](NEXT.md) is the accepted implementation plan: collection/index addresses
bound to exact input snapshots, self-contained saved plans, directional compile
and dump, and checked apply with temporary transaction recovery. There is no
external identity ledger, persistent baseline, automatic sync-state directory,
or new embedded binding metadata. Report heuristics may group likely edits/moves
but cannot change execution. Without historical state, direction selects authority;
the workflow cannot detect independent edits to both sides since a previous sync.

Implemented foundation: the lossless CST, AST/parser, diagnostics, formatter,
expression evaluation, trivia, structured source edits, and comment-preserving
dump merge now live in document-independent `altium-spec-lang`. Compatibility
reexports preserve existing `altium-format-spec` callers. Import graph resolution
also lives in the language crate with contextual transitive errors. The new
immutable capture API retains exact root/transitive text, ASTs, canonical paths,
ordered edges, and dependency order, and rechecks content/path resolutions.
[Collection/contextual-operation syntax](docs/spec-lang/language/snapshot-intent.md)
is parsed and preserved by the CST; legacy commands explicitly reject collection
scope instead of ignoring it. The typed SchLib intent core now preserves field operations/spans, collection
scope, typed definition references, imported source provenance, and nested spread
origins. It compiles captured transitive values without reopening imports.
Step 1 is complete under the revised image asset-reference scope, following
independent completeness review. Step 2 is complete (2026-09-17, section
below); steps 3 through 9 remain outstanding.

### NEXT.md step 2 — SchLib snapshots and indexed patches (complete, 2026-09-17)

Step 2 is complete: format-owned typed SchLib snapshots, batch structural
editing, declared namespace transitions, and full definition-scope
`SchLibIntent` → `SchLibPatch` elaboration over snapshot-bound addresses and
scope checks. The authoritative collection-semantics reference is
[docs/spec-lang/reference/schlib-collections.md](docs/spec-lang/reference/schlib-collections.md),
promoted from [the A1 contract memo](docs/worklogs/next-child-contracts-2026-09-16.md);
the full increment history and every review trail is in
[docs/worklogs/next-children-2026-09-16.md](docs/worklogs/next-children-2026-09-16.md).
Increment history, brief (baseline 436fa3c through 031cbf2, each with an
independent review and fix pass): 2026-09-05 foundation (complete typed
snapshots, immutable batch component replacement with captured
`ReferenceContext` drift rejection, scalar patches, ownership assembly);
A1/A2 evidence memo + read-only child projections, owner groups, reported
load normalization, address vocabulary; B parameters/graphics batch editing;
C pins + part grow/shrink; D1 library-level namespace transitions; D2
footprint/implementation-chain editing; E1 keyed elaboration + sync-side
library ops; E2 positional elaboration, parts, layout, swap fan-out,
image-asset sources; F gate review + docs promotion.

**What works** (all snapshot-bound, atomic against immutable inputs):

- Capture and materialize: complete typed snapshots of every gate-corpus
  SchLib (lucashudson-Connectors, ICs, arthurbenemann-STM32F4 — 223
  components) with native save/reopen typed equality; the wider `data/schlib`
  fixture set is exercised by open/validate only (feature-gated), not by the
  snapshot equality gate. Capture never validates wild values
  — out-of-range owners, wild swap encodings, dangling font/image references,
  and orphan storage assets load and round-trip verbatim. The one load-time
  exception is reported, never silent: native PartCount < 2 floors to 2 and
  DisplayModeCount < 1 to 1 (mirroring AD26's after-import event), surfaced
  via `SchLib::load_normalizations()`.
- Read-only projections on `SchLibComponentSnapshot`
  (`crates/altium-format/src/schlib/snapshot/views.rs`): parts with derived
  swap-group projections, pins, root-owned parameters, graphics with explicit
  unrepresentable variants, aliases, PCBLIB-only footprint maps, captured
  font/image/style namespaces, and raw-truthful `SchOwnerGroup`
  classification (`PartIndependent`/`SharedPins`/`Part(n)`/`OutOfRange`).
- Structural editing: `with_children` (parameters, graphics, pins, footprint
  chains — final ordered Keep/Replace/New sequences over the input
  projections, one `assemble_records` pass, stream-order and IndexInSheet
  policies per the promoted reference), `with_parts_appended`/
  `with_parts_removed` (trailing-only shrink with typed deletion and
  `current_part_id` reports), and `with_library_edit` (component
  create/clone/remove, aliases with three synchronized representations,
  whole-resource image assets, font appends) with the resulting
  `ReferenceContext` always recomputed; strict `replace_components` still
  rejects undeclared context drift. Deletion always requires explicit
  authorization and unauthorized deletion errors list the doomed members.
- Sync-side elaboration (`altium-sync`): the snapshot-bound address
  vocabulary (`src/address.rs` — landed and unit-tested, but step-3-facing:
  elaboration does not consume it yet; its module docs record the divergence
  risk against `positional.rs`'s own scope targeting), the class-aware
  collection planner
  (`src/collection.rs`: keyed = components/parts/parameters/aliases/
  footprints, positional = pins/graphics/maps; partial = in-slot updates plus
  appends with interior insertion a hard error; complete = final
  membership/order with deletion only under `delete: allow`), owner-group
  positional scoping (`src/schlib/positional.rs`), field policies
  (Set/Clear/Inherit/Reset/Absent), layout/auto-size evaluation, part
  swap-group fan-out, image-asset sources (`from_intent_with_assets` with
  hash/path re-verified captures), and imperative library ops
  (`from_library_ops`). Patches carry a cached reviewed result plus
  inspection accessors (the exact-indexed-diff foundation), and apply
  followed by re-elaboration is replan-empty at every level.

**Recorded unsupported-content hard errors** (fail-fast with context;
triggers, rationale, and enforcement points in the reference page): style_id
authoring (Delphi style tables not reverse engineered); Additional-stream pin
or implementation-chain structural edits (positional freeze; Additional
sidecars unproven; the pin freeze and the single-current footprint invariant
are mirrored on the part-shrink door, so neither can be bypassed through a
part removal's deletion set); non-stored `hidden_net_name` writes (no proven
serialization path); component-scope graphics on multi-part components (no
native shared owner); multi-pad definer addressing (a pin-pad intent pair
expresses one pad); Symbol/Hyperlink/other unrepresentable graphic kinds
under a complete scope or when addressed; owner-0 shared pins on single-part
components (unaddressable, preserved-only); `inherit` on native
pin/graphic/part fields at definition scope (instance bases are step 7);
single/multi-part boundary crossing while managing component-scope pins;
authoring `is_current` incoherence (no-new-divergence rule); wild stored
`current_part_id` blocking part ops until explicitly resolved.

**Step 2 does NOT include** (step 3+ scope): saved plans (schema, manifests,
hashes, frozen payloads), filesystem transactions and recovery, CLI migration
(plan/apply still run the legacy ECO/executor path — preview/apply agreement
is not guaranteed until step 5), and dump-direction source lowering
(document → spec structured edits through the new planner). Elaboration is
compile-direction and in-memory only.

Step-2 validation at the final code state: 482 altium-format `--lib`, 54
altium-sync, 362 altium-spec-lang and 168 altium-format-spec tests pass; the
CLI builds; altium-sync builds with zero warnings; the three-library corpus
gate (lucashudson-Connectors 56, ICs 32, arthurbenemann-STM32F4 135
components) passes all scalar/structural/namespace/intent checks with native
save/reopen typed equality and empty replans, exit 0. Open questions queued
for the user are collected in the step-2 closure section of
[the step-2 worklog](docs/worklogs/next-children-2026-09-16.md).

### NEXT.md step 3 — saved plans (in progress; increment A complete, 2026-09-18)

Increment A of the step-3 saved-plan design
([docs/worklogs/next-plans-2026-09-18.md](docs/worklogs/next-plans-2026-09-18.md);
design memo ratified with Q1 = versioned public payload, Q2 = BLAKE3) is
complete:

- **Versioned component-state payload** (`altium-format`):
  `ComponentStatePayload` + `SCHLIB_STATE_FORMAT_VERSION = 1` — a deliberate
  public serialization contract carrying the COMPLETE typed component state
  (root record, both Data/Additional record sequences with owner addresses,
  all sidecar-backed typed pin fields, index entry incl. aliases; no raw
  blobs — image bodies live in the library store, not in component state).
  Serde impls stay inside `altium-format` (`deny_unknown_fields` on the
  payload and every nested record type; the serde derives on the internal
  record model and on `altium-format-types` sch/common/coord/color types
  exist solely to serve this contract). Decode of any other version fails
  with a regenerate message. Decoded-state install forms
  `SchLibComponentEdit::ReplaceWithState` and `SchNewComponent::FromState`
  flow through `with_library_edit` with NO fresh minting, through the shared
  final-namespace/ownership validation and `ReferenceContext` recompute, and
  through a state-install validation pass (`validate_state_install`) that
  re-establishes authoring-time reference/consistency checks on decoded
  payloads (font and embedded-image references, index/root description and
  `AliasList` agreement, `FromState` alias legality) — with a per-slot
  wild-inheritance carve-out so wild content loaded libraries carry keeps
  round-tripping while FRESH dangling references from tampered payloads are
  rejected. `ReplaceWith`'s process-local `ReferenceContext` drift check has
  no state-form equivalent; the saved-plan manifest hash plus before/after
  typed equality at apply (increment B) subsume it.
  Round-trip law (tested over a programmatic collection zoo — all eight
  collection classes incl. parameters — and the corpus):
  `decode(encode(c))` installed via `ReplaceWithState` is a typed no-op.
- **Saved-plan container** (`altium-sync/src/plan.rs`): `SavedPlan`
  (`PLAN_SCHEMA_VERSION = 1`, `state_format_version`, domain, direction,
  scope summary, manifest, patches, diagnostics), `ManifestEntry` roles
  (SpecSource/SpecImport/ImageAssetBundle/TargetDocument/Destination/
  InPlaceTarget) with BLAKE3 `Hash{algorithm,hex,byte_len}` or `Absent`
  preconditions (absence via `symlink_metadata`), one shared
  canonicalization function, and every §S11 rejection (cross-artifact
  aliases, duplicate destinations/entries, plan-path collision, symlink
  resolution) at plan construction AND re-run on strict decode.
  Deterministic compact JSON emission (byte-identical re-emission; identical
  modulo freshly allocated creation `UniqueId`s across plannings, via the
  `mask_unique_ids` display/test normalizer). Embedded image payloads are
  base64 bodies hash-guarded by the capture-time BLAKE3 hash; a decode-time
  mismatch is a hard error.
- **Lowering**: `SavedPlan::compile_from_schlib_patch` lowers a reviewed
  `SchLibPatch` to Create/Replace/Delete component patches with complete
  embedded before/after payloads (creation ids frozen from the cached
  reviewed result), `parts_shrink` reports carried on replacements, and
  library-namespace ops in D1 application order. Manifest capture takes the
  caller-provided artifact byte set (`PlanArtifact`), the documented §S7
  seam increment B binds real files through.
- **§S7 helper**: `load_schlib_hashed` reads a SchLib file once and hashes +
  parses the same buffer (via `SchLib::from_bytes`).

The increment-A independent review's findings (1 major — the state-install
reference-validation gap above — plus minors: index desync checks, zoo
parameters coverage, `BoundingBox` invariant-bypassing serde removed,
non-finite `SchAngle` rejected at the parse boundary, typed
`PlanScopeSubject`/`PlanCollectionKind` in the plan scope) are resolved; see
the findings-resolution section of the worklog.

Validation at this state: 490 altium-format `--lib` (482 step-2 + 8 new), 61
altium-sync (54 + 7 new), 362 altium-spec-lang, 168 altium-format-spec tests
pass; CLI checks; altium-sync builds with zero warnings; the three-library
corpus gate passes with the new saved-plan pass (per-component payload
round-trip no-op, plan strict-decode equality, byte-determinism across two
plannings). Increments B (checked in-memory apply, exact report, address
unification), C (dump-direction source patches), and D (closure/evidence)
remain.

Full image codecs were removed from the gate by the user's 2026-09-05 scope change.
Graphic field policies now cover all 13 kinds against the existing typed Rust
API/creation defaults, including optional arc endpoints and named PenWidth values.
Inapplicable properties fail explicitly. Font/resource existence and valid creation
geometry still require elaboration checks. Anchored pins/auto-sized boxes retain typed dependencies; the layout
evaluator uses effective target/base/default geometry without erasing absence.
Independent pin x/y operations preserve omitted target/base axes and distinguish
inheritance from reset; conflicting paired/axis/anchor coordinates fail explicitly.
Owner-local auto-size solves centered/branching chains with checked integer
constraints, explicit corner preservation, and effective absolute-pin references.
Parts continue to treat inherited component boxes as fixed geometry.
Axis/layout validation: 340 language tests passed, followed by 17 solver tests
including the final interval-intersection regression; 168 legacy spec tests and
CLI compilation passed. Both deltas passed independent review. The five real
SchLib-derived intent dumps again compile deterministically with unchanged counts.
Captured footprint imports now export pad names from rows, columns, and grids,
including skip/perimeter naming and explicit generated-pad overrides. Counts,
coordinates, generator properties, and name collisions are checked. This exports
the SchLib mapping namespace; it does not materialize a native PcbLib footprint.
Seven fixture-free generated-import tests passed, including captured transitive
imports after source deletion and signed edge-coordinate overflow cases. The
implementation passed independent review.
Final graphic-policy validation: 353 language tests, 168 legacy spec tests, CLI
compilation, and the five real SchLib-derived deterministic intent checks pass.
Independent review found no blockers in the bounded graphic policy change.
The [embedded-image inventory](docs/worklogs/next-images-2026-09-05.md) found
58 resources in 13 real SchLib files: 16 BMP-only resources and 42 BMP previews
with appended PNG/JPEG/EMF source objects. Preview-only decoding would discard
source content. The revised import design preserves the complete encoded image
body as an opaque asset, under an explicit user-authorized exception; codecs are
deferred. The inventory does not establish codec validity or visual equivalence.
Fresh original managed-assembly inspection now verifies the appended-class
serialization contract and the 0/10/30/50mil pen-width table. Graphic intent accepts
only exact physical widths after checked Coord conversion; renderer and intent
share the verified constants. Endpoint sizes remain named-only because they use
a separate semantic table. The [revised image import design](docs/proposals/typed-image-resources.md)
supersedes the shared codec proposal. Image-only `source` intent now retains
Set/Absent/Inherit/Reset/Clear and the containing declaration's canonical source
path. Compilation performs no asset I/O. The native API exposes ordered borrowed
resources and explicit indexed insert/replace/remove, checking serialization
constraints before mutation and preserving duplicates and complete asset bodies.
`altium-sync::image_asset` captures exact bytes plus BLAKE3 hashes, and checks
content and canonical-path preconditions without refreshing captured data.
These are building blocks: integrated dump/import, reference validation, saved
plan serialization, and transactional application remain unimplemented.
Final validation: 362 language, 396 core, 168 legacy spec and 3 sync tests pass.
The CLI builds at the final code state.
All 58 image bodies across 13 real SchLib libraries preserve exact ordered names
and bytes through export/capture/replacement/native save/reopen. A real self-alias
save failure was fixed by sharing the component storage while retaining its
Redirection stream; the malformed outer-framing negative example still errors.
These checks do not establish whole-document semantic convergence or image codec
validity. See the [closure worklog](docs/worklogs/next-image-assets-2026-09-05.md).
Verified physical-width validation: 354 language tests and 168 legacy spec tests
pass; CLI compilation passes. The bounded conversion/rendering change passed
independent review. No fixture-feature or property tests were run.
CFB wrapper/tracking unit tests now create temporary containers instead of
depending on the missing legacy `data/BlankSchlibComponent.SchLib` file. Their
stream-content and consumption assertions are preserved or strengthened; all
eight targeted tests and independent review pass. This does not validate native
SchLib semantics.
Embedded-object envelopes now require complete zlib streams with no trailing
compressed input, lossless Windows-1252 identifiers, and checked identifier/length
serialization. The inner compressed length uses the verified full signed 32-bit
field; unsupported oversized outer blocks fail before writing. Storage validates
its header, optional record sentinel, entry count, and binary block formats, with
stream/block/resource error context. Existing duplicate entries remain ordered.
Both boundary changes passed independent review. The combined default library
suite passes 385 tests, legacy spec passes 168, and the CLI builds. All 13 real
image-bearing SchLibs still validate; an altered text-tagged resource block that
previously passed now fails at `/Storage block #1`. This is envelope validation,
not typed image payload support or native roundtrip acceptance.
Oversized embedded-object framing now follows the verified AD26 extension through
32 MiB minus one byte. A checked D0 prefix selects the extended length; ordinary
framing remains intact otherwise, including empty binary blocks followed by
D0-looking frame headers. Embedded envelope validation remains strict, and lazy
framing iterators terminate after errors. Generic block writers are unchanged.
Independent review and all 392 default core-library tests pass, including an
actual compressed payload above 16 MiB followed by another entry. All 168 legacy
spec tests pass, the CLI builds, and all 13 real image-bearing libraries still
validate while the corrupted block-format variant fails. The large-payload test
is synthetic; those 13 real files do not establish oversized-image codec support.
Coordinate literals and dimensional float conversions now reject nonfinite/overflow
values; signed-minimum division, negation, and abs return errors instead of panics. `altium-sync` remains a dependency scaffold. No state subsystem
existed to remove. Obsolete design notes and scaffold identity
plans were removed, including the unused `altium-sync` random-ID dependency.
Existing native Altium IDs, spec annotations, and ephemeral spec-to-spec sync
remain supported; they are not a cross-artifact identity database.

Validation of the front-end extraction: 258 language and 176 legacy spec unit
tests pass; `cargo check -p altium-format-spec --features altium-apply` passes.
The subsequent import extraction passes 18 targeted resolver tests. Default CLI
`cargo test` passes with zero runnable tests (downstream compilation only).
Independent review found no code blockers for these bounded migrations; this does
not complete the full intent gate. All five test-data repositories are cloned
under ignored `data/` directories. A [21-file CLI smoke run](docs/worklogs/next-corpus-2026-09-05.md)
validated/saved 12 files, all with unresolved semantic CFB differences; 9 files
rejected unsupported content. This is not a roundtrip acceptance gate.

Syntax/capture validation: 286 language tests and 168 legacy spec tests pass.
Independent reviews covered captured source provenance, contextual operations,
CST edits, and legacy rejection guards; findings were fixed before commit.

Typed-intent validation: all 309 language tests (including 23 intent tests),
168 legacy spec tests, and the CLI compile check pass after independent review.
Core findings were fixed before commit. The [five-library captured-intent check](docs/worklogs/next-intent-2026-09-05.md)
spans the SchLib corpus by size and compiles identical intent twice per capture.
It covers 7,260 components, 41,528 pins, and 27,464 graphics; this is front-end
coverage, not binary preservation or a completed step 1 gate.

Retained-layout/numeric validation: 331 language tests, 168 legacy spec tests,
and the CLI compile check pass. Independent module/integration/numeric reviews
passed after shadowing, containment, and signed-minimum arithmetic fixes. The
five-library captured corpus still passes deterministically. A formerly accepted
`length: 999999999999mil` now returns a source-located ArithmeticOverflow error.
Auto-size is a bounded edge-chain evaluator: unsupported containment errors, and
part layouts treat inherited component boxes as fixed effective geometry.

Current CLI execution architecture is unchanged: `plan` renders the legacy ECO and `apply`
recompiles/runs the separate executor. Typed snapshot patches exist in-memory
(step 2), but saved-plan execution, precondition manifests, and recovery
transactions are not implemented. The
plan/apply checkmarks below indicate legacy command availability, not these
guarantees. Next: NEXT.md step 3 — saved plans and both directional planners —
then the checked transaction executor and the SchLib CLI slice.

PcbDoc bulk routing is planned as an explicitly referenced typed binary design
artifact, separate from synchronization state. AutoPCB has `.routes` binary I/O;
Altium currently only compiles `routing.solution` and does not load it on apply.
Dump still emits tracks/arcs/vias as text. The bridge needs a fidelity and net/layer
mapping contract before arbitrary routed boards can roundtrip through that artifact.

## Document Type Support

| Document   | Ext     | Parse | Serialize | High-Level API | Spec | Query | Render    | CLI validate | CLI save-as | CLI new |
|------------|---------|-------|-----------|----------------|------|-------|-----------|--------------|-------------|---------|
| **SchLib** | .SchLib | ✅    | ✅        | ✅ Full CRUD   | ✅   | ✅    | ✅ SVG/PNG | ✅           | ✅          | ✅      |
| **SchDoc** | .SchDoc | ✅    | ✅        | ✅ Read/Write  | ✅   | ✅    | ✅ SVG/PNG | ✅           | ✅          | ✅      |
| **PcbLib** | .PcbLib | ✅    | ✅        | ✅ Full CRUD   | ✅   | ✅    | ✅ SVG/PNG | ✅           | ✅          | ✅      |
| **PcbDoc** | .PcbDoc | ✅    | ✅        | ✅ Read/Write  | ✅   | ✅    | ❌        | ✅           | ✅          | ❌      |
| **PrjPcb** | .PrjPcb | ✅    | ✅        | ✅ Read-only   | ✅   | ❌    | ❌        | ✅           | ✅          | ✅      |
| **IntLib** | .IntLib | ✅    | ❌        | ❌ Read-only   | ❌   | ❌    | ❌        | ✅           | ✅ dump     | ❌      |

## CLI Command Matrix

| Command       | SchLib | SchDoc | PcbLib | PcbDoc | PrjPcb | IntLib |
|---------------|--------|--------|--------|--------|--------|--------|
| `new`         | ✅     | ✅     | ✅     | ❌     | ✅     | ❌     |
| `validate`    | ✅     | ✅     | ✅     | ✅     | ✅     | ✅     |
| `save-as`     | ✅     | ✅     | ✅     | ✅     | ✅     | ❌     |
| `get version` | ✅     | ❌     | ✅     | ❌     | ❌     | ❌     |
| `render`      | ✅     | ✅     | ✅     | ❌     | ❌     | ❌     |
| `query`       | ✅     | ✅     | ✅     | ✅     | ❌     | ❌     |
| `info`        | ✅     | ✅     | ✅     | ✅     | ❌     | ❌     |
| `plan/apply`  | ✅     | ✅     | ✅     | ✅     | ✅     | ❌     |
| `dump`        | ✅     | ✅     | ✅     | ✅     | ✅     | ✅     |
| `cfb *`       | ✅     | ✅     | ✅     | ✅     | n/a    | ✅     |

Additional commands: `spec sync` (forward/diff/dry-run), `cfb ls/dump/blocks/diff/cat`.
Spec files use `.schlib-spec`, `.pcblib-spec`, `.schdoc-spec`, `.pcbdoc-spec`, and `.prjpcb-spec`.

Default `cargo test --workspace` is intended to be fixture-free. Most tests
that read `data/` are gated by `test-fixtures`, but 11 legacy CFB/IntLib tests
remain ungated and fail with `No such file or directory` when fixture repos are
absent. This is a test-infrastructure defect, not a parser failure.

## Lossless Spec Updates (2026-06-21)

The spec crate now has a lossless structured CST for every current spec domain,
a typed accessor layer, and typed source edits: `InsertBlock`, `DeleteBlock`,
`SetProperty`, `RemoveProperty`, and `SetAnnotation`. Edits reparse before
returning, reject stale source IDs, preserve CRLF/LF style, and leave bytes
outside the edited syntax ranges unchanged.

`altium dump` no longer uses the CLI's AST-span text splice. Existing specs are
updated recursively through typed CST edits, preserving matched ordering,
authored comments/formatting, and prior annotation IDs. Source IDs are
authoritative; identityless records use exact matching followed by guarded
same-header ordinal matching. Malformed existing source, ambiguous identities,
and non-name header changes that cannot be preserved are hard errors rather
than overwrite/canonicalization fallbacks. PcbLib footprint load failures now
abort dump instead of producing an incomplete `// ERROR` spec.

## Per-Document Notes

### SchLib — Most Complete
All schematic record types parsed. Full CRUD API. 9 per-component sidecar streams. Complete roundtrip with semantic CFB diff verification. Spec dump/compile/plan/apply/reconcile all working.

### SchDoc
All 40+ record types parsed. Flat OWNERINDEX → nested tree conversion. UniqueId-based field preservation on save. Spec supports sheet metadata, all object types, `net`/`power` blocks, pin connections (`pin X -> #NET`), and SchLib import references.

### PcbLib
8 primitive types (Pad, Via, Track, Arc, Text, Fill, Region, ComponentBody). 6 sidecar stream types. Complete roundtrip. PadStack/PcbContour shared types with PcbDoc. Spec supports pad templates and spread operators.

### PcbDoc
18+ section types parsed. DRC engine with 39 rule classes and 38 violation classes. V2 API: LayerStack, RuleParams, PadStack, BoardGeometry, BoardConnectivity. 94/96 V6 test files passing. PrimitiveParameters (BOM data) pipeline complete.

### PrjPcb
INI-style format with indexed sections. Complete roundtrip. Read-only high-level API (internal write exists but not surfaced).

### IntLib
Read-only. Decompresses embedded SchLib/PcbLib from CFB. Dump produces separate `.schlib-spec` and `.pcblib-spec` files.

## Spec Dump/Apply Roundtrip (2026-06-09 audit)

Criterion: `dump → apply (new doc) → validate → re-dump` produces an identical
spec (modulo random annotation IDs) across the `data/` fixture corpora.

| Document | Result | Remaining failures |
|----------|--------|--------------------|
| SchLib   | 121/126 | 4 parameter `%UTF8%` whitespace-trim upgrade artifacts; 1 duplicate component storage name on apply |
| PcbLib   | 33/40  | 5 parser gaps on originals (Parameters streams, binary misread, SectionKeys pascal length); 2 long-footprint-name apply failures (SectionKeys not written on apply). Zero roundtrip diffs. |
| SchDoc   | 67/1226 | 278 parser gaps on originals (`IGNOREONLOAD`, `OWNERINDEXADDITIONALLIST`, …); 881 blocked on the inline-children design gap below |
| PcbDoc   | dump+plan runs on 79/132 | 36 corrupt/non-CFB fixtures; 15 `Dimensions6 TEXTX` parse gaps; 2 V5 files. `plan` reports false ADDs for board primitives (reconciler has no spec↔document identity matching yet). |

Invariants now enforced:
- `Coord` Display only emits mm when the printed string re-parses (via the
  393,701 units/mm conversion) to the identical coordinate; mils are always exact.
- Unknown graphic property keys are compile errors (sch + pcb paths) instead of
  silently dropped geometry.
- Dump emission keys match the grammar (rectangle `from`/`to`, polyline
  `vertices`, fill `corner1`/`corner2`, via `at`, component_body
  `height`/`model`/`outline` with typed `arc(...)` segments, pad
  `mid_*`/`bot_*`/`hole_shape`/`slot_size`, `is_solid: false` emission,
  explicit pad sizes, `part_count`, sheet `style:`).

**Design gap (decision RESOLVED 2026-06-20)**: SchDoc spec dump emits inline
component children (pins, graphics, parameters), but `apply` was designed for
SchLib-import authoring (`symbol: $lib.Name` + `pin X -> #NET`), not
full-fidelity sheet reconstruction. The planned materialization contract (clarified
2026-09-05) is independent of authority: absent symbol reference means concrete
inline children; a resolvable explicit symbol supplies an imported base plus
overrides; an unresolved explicit symbol is an error. See
`docs/spec-lang/explanation/greenfield-vs-brownfield.md`.
**Fail-fast fix landed**: inline `pin`/`graphic`/`part`/`footprint_map` blocks
inside a SchDoc component previously parsed then silently dropped at compile;
they now hard-error (`compiler.rs::compile_schdoc_component`) until
materialization is implemented. Sheet-level document `parameter` blocks are
still not applied.

## Known Issues

**Moderate:**
- PrjPcb has no public write API (internal write exists)
- PcbDoc rendering not supported (no SVG/PNG)

**Minor:**
- PcbLib `apply` cannot create footprints whose names exceed the 31-char CFB
  storage limit (SectionKeys mapping not generated on apply)
- SchLib parameter values with non-ASCII chars get whitespace-trimmed via the
  `%UTF8%` duplicate entry on save (matches documented Altium upgrade
  behavior; verify against decompiled reader precedence)
- Spec language does not yet capture (silently normalized on apply): pin
  `show_name`/`show_designator`, schematic graphic colors/line widths, pad
  `corner_radius_pct`/`inner_layers`/`slot_rotation`, component_body
  `standoff_height`/3D color/opacity, text_frame `word_wrap`/`clip_to_rect`,
  image `embed_image`/`keep_aspect`, PCB region `holes`/`kind` (PcbLib path)

**Fail-fast audit gaps (spec crate, 2026-06-20 — open):**
- `compile_pad` / pin compilation have no unknown-key rejection (unlike sch/pcb
  graphics, which got `KNOWN_KEYS` allow-lists). Misspelled/unsupported pad/pin
  keys are silently dropped. Fix: add allow-lists; validate against the fixture
  corpus before merge.
- Placement DSL "parse-later" no-ops (forbidden by CARDINAL RULE): unknown
  placement config keys (`compiler.rs` ~2491), `minimize` non-`wirelength`
  objectives + `subject_to` hints (~2528), and `SeparateDecl` (~2538) are
  accepted then dropped. Should hard-error or emit an explicit "unsupported"
  diagnostic.
- Domain compilers skip parsed top-level declarations that belong to another
  document domain (`compile_schdoc` / `compile_pcbdoc`). Mixed-domain specs must
  hard-error instead of silently dropping declarations.
- PcbLib dump skips regions with empty outlines, silently changing the graphic
  count in the generated spec.
- PcbDoc class compilation filters non-string `members` values out of arrays;
  malformed members must return a type error instead of disappearing.
- Reconciler under-reporting: documents/graphics/polygon-props/placement-rotation
  marked "unchanged for now" (`reconciler.rs` 327/1235/2135/2482) → `plan` can
  show no change while `apply` mutates (plan/apply disagreement).
- `lexer::is_keyword` flagged dead-code despite a call in
  `dump::quote_entity_name` — investigate reachability of the keyword-quote guard.
- 11 `altium-format` CFB/IntLib unit tests read absent `data/` files without the
  required `test-fixtures` gate, so default workspace tests are not currently
  fixture-free.
- PcbDoc: 2/96 V6 files failing (EmbeddedFonts and WideStrings edge cases)
- PcbDoc V5 format not supported (2 test files deferred)
- PcbDoc spec dumps currently omit layer stack and board geometry blocks until the spec compiler supports applying them.
- SVG clip regions not applied
- `get version` only works for SchLib/PcbLib
- `apply --report-json` flag accepted but unused
- SOURCEUNIQUEID not populated from SchDoc for new components during apply

## Roundtrip Known Differences (Acceptable)

All document types: font name buffer zero-fill (vs Altium heap garbage), boolean normalization (non-zero → 0x01).

PcbLib-specific: text WideStrings upgrade, via format upgrade (ext_size 42→45), SharedUnion NUL terminator.

PcbDoc-specific: pad sub4 format upgrade (171→172 bytes), via section 4/5 always written, Rules6 tier2 serialization, param key ordering, duplicate param deduplication.
