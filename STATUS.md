# Codebase Status

Updated: 2026-09-05 (plan/apply direction; older corpus results remain dated below)

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
altium-sync            (scaffold: snapshots, indexed patches, directional plan/apply)
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
origins. It compiles captured transitive values without reopening imports. Full
step 1 remains open for typed image resources.
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
source content. Typed resources must cover both representations and metadata;
the inventory does not establish complete codec or native-source understanding.
Fresh original managed-assembly inspection now verifies the appended-class
serialization contract and the 0/10/30/50mil pen-width table. Graphic intent accepts
only exact physical widths after checked Coord conversion; renderer and intent
share the verified constants. Endpoint sizes remain named-only because they use
a separate semantic table. The [proposed shared image boundary](docs/proposals/typed-image-resources.md)
is awaiting the required new-domain-type discussion; no codec/types have been added.
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
recompiles/runs the separate executor. Saved-plan execution, precondition checks,
typed snapshot patches, and recovery transactions are not implemented. The
plan/apply checkmarks below indicate legacy command availability, not these
guarantees. Next: finish language/SchLib intent separation, then the complete
SchLib snapshot → plan → checked apply slice before migrating other domains.

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
