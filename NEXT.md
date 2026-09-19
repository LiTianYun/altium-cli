# Snapshot-bound plan/apply

Updated: 2026-09-18. Accepted implementation direction. Steps 1–3 (language
front end; SchLib snapshots and indexed patches; saved plans and both
directional planners as library-level APIs) are implemented; transactions
and the CLI surface are not. See [STATUS.md](STATUS.md) for current
behavior.

## Decisions

- No external identity ledger, persistent sync database, last-synchronized
  baseline, or automatic `.altium/` state discovery.
- No tool-generated cross-artifact IDs or embedded identity parameters. Preserve
  native Altium IDs and existing spec annotations as typed document/source data.
- Address objects by artifact, owner path, collection kind, and numeric index in
  a particular typed snapshot. An index is not durable identity.
- Plan produces versioned, self-contained typed patches. Apply executes exactly
  those patches without recompiling intent or running a separate reconciler.
- Exact hashes bind the plan to all reviewed inputs. Any intervening edit,
  including a GUI save or source formatting change, requires replanning.
- Compile and dump are directional. The selected source supplies desired values
  inside declared scope; the destination supplies preserved values outside it.
  There is no automatic historical three-way conflict detection.
- Similarity matching, move detection, and grouping improve the report only.
  They cannot select execution targets, change patches, or resolve conflicts.

Explicit binary design artifacts (such as routing data), saved plans, and
temporary transaction recovery files are permitted. None is a database of
persistent object bindings. Recovery journals are never inputs to later planning.

## Current checkpoint and evidence

The language front end, lossless CST, structured edits, and comment-preserving
dump merge live in `altium-spec-lang`, reexported by `altium-format-spec`.
Import resolution and immutable transitive source capture also live in the
language crate. Step 1 is complete under the revised image asset-reference
scope (image codecs are not a gate; see the
[image import design](docs/proposals/typed-image-resources.md) and
[closure evidence](docs/worklogs/next-image-assets-2026-09-05.md)).

Step 2 (SchLib snapshots and indexed patches) is complete as of 2026-09-17
(increments A1–F, baseline 436fa3c through 031cbf2 plus the F docs pass).
`altium-format` owns complete typed SchLib snapshots with read-only child
projections (`crates/altium-format/src/schlib/snapshot/views.rs`), batch
structural editing of parameters/graphics/pins/footprint chains
(`snapshot/children.rs`), part grow/shrink (`snapshot/parts.rs`), and declared
library-level namespace transitions — component create/clone/remove, aliases,
image assets, font appends — with deterministic `ReferenceContext` recompute
(`snapshot/library.rs`); the strict `replace_components` path still rejects
undeclared context drift. `altium-sync` owns the snapshot-bound address
vocabulary (`src/address.rs` — landed and unit-tested, but a step-3-facing
surface: elaboration does not consume it yet), the class-aware collection
planner (`src/collection.rs`), and full definition-scope `SchLibIntent` →
`SchLibPatch` elaboration (`src/schlib/elaborate.rs`,
`src/schlib/positional.rs`, which implements elaboration's own owner-group
scope targeting): keyed and positional alignment per the table below,
owner-group scoping, field policies, layout/auto-size, part swap-group
fan-out, and image-asset sources. Apply then
re-elaborate is replan-empty at every level. The three-library corpus gate
(Connectors/ICs/STM32F4, 223 components) passes scalar, structural, namespace,
and intent passes with native save/reopen typed equality. Semantics reference:
[docs/spec-lang/reference/schlib-collections.md](docs/spec-lang/reference/schlib-collections.md);
evidence: [step-2 worklog](docs/worklogs/next-children-2026-09-16.md) and
[the A1 contract memo](docs/worklogs/next-child-contracts-2026-09-16.md);
recorded hard errors and gaps: [STATUS.md](STATUS.md).

Step 2 does NOT include saved plans, file transactions, CLI migration, or
dump-direction source lowering — that is step 3+ scope. CLI plan still renders
an ECO while apply recompiles the spec through the legacy executor;
preview/apply agreement is not guaranteed until step 5 migrates the SchLib
slice. There is no executable external state subsystem to migrate or delete.

The recorded SchLib / Altium 26.2 GUI-save experiment (2026-06-21) found:

| Injection point | Recorded GUI-save result |
| --- | --- |
| FileHeader document block, unknown key | Dropped |
| Component RECORD=1 header, unknown key | Dropped despite its native UniqueID |
| Pie RECORD=9 primitive, unknown key | Dropped |
| Real user parameter RECORD=41, Name/Text | Preserved; normalized and given a native UniqueID |

This is historical evidence, not a newly repeated GUI test. It does not establish
preservation of hidden parameters, arbitrary streams, or other formats. The new
workflow does not depend on custom metadata surviving a GUI save.

Native IDs are not uniformly absent: PcbLib has typed UniqueID sidecar support,
and PcbDoc rebuilds its pad-ID section. Conversely, PcbDoc high-level names such
as `track_3` are synthesized from positions. Do not equate an API `id` with a
persistent file-format identifier. Evidence: `api/pcbdoc_read.rs`,
`api/pcbdoc_write.rs`, and `pcblib/sidecar.rs` in `altium-format/src/`.

## Boundaries

`altium-format` owns strict parsing, serialization, native references, ownership,
format upgrades, typed public APIs, and invariant validation. All file-format
internals remain private. Expand typed APIs before using a missing operation;
unsupported data must error, never pass through as undecoded bytes, except for
the explicitly authorized image asset boundary below.

`altium-spec-lang` owns CST, diagnostics, parser, formatter, authored intent,
imports, type-checking, and normalization. It depends on `altium-format-types`,
not the document or sync crates. It produces typed intent.

`altium-sync` owns Altium-shaped intent elaboration, concrete snapshots,
addresses, directional diff, plans, source/document patches, reports, and checked
transaction execution. It depends on both language and document layers. This
avoids a cycle between intent compilation and snapshot types. Start with concrete
SchLib types; a universal provider framework is not a prerequisite.

`altium-cli` owns arguments, explicit paths, exit codes, and output. Each migrated
domain calls one planner and one saved-plan executor. Existing ephemeral
`altium spec sync` is a separate spec-to-spec workflow; keep it working.

AutoPCB owns canonical routing data, routing algorithms, transactions, and route
certificates; Altium CLI owns typed native import/export. Follow
[the integration boundary](docs/autopcb-routing-integration.md).

## Addressing and collection semantics

An address is `(artifact, owner_path, collection_kind, index)`. Use typed collection
kinds and validated index wrappers. Reuse domain types; discuss missing domain
types before adding them to `altium-format-types`.

Every address refers to the immutable input snapshot, not an incrementally mutated
collection. Each domain defines its typed projection order and ownership. Display
sorting and native CFB record numbers do not define this order.

For collections with language-defined natural keys, match exact parent-scoped
keys (component name, footprint name, pad designator). Duplicate/ambiguous keys
block the operation. Native-ID matching requires an explicit domain contract.
Otherwise, source item N describes destination item N: positional alignment is
the declared semantics, not a guess about historical identity.

- A complete, explicitly managed collection determines its final order and length.
  Removing destination-only entries requires source-declared deletion permission.
- A partial collection preserves unspecified destination entries. Omission does
  not authorize truncation. Define partial/complete collection syntax before
  shipping collection deletion.
- Reordering/inserting may produce a noisy positional diff. Similarity cannot
  remap objects, annotations, comments, or overrides during mutation.
- Source annotation IDs remain source metadata; they cannot bind an otherwise
  unidentifiable Altium object across GUI edits.

Prefer replacing complete high-level aggregates (SchLib component, PcbLib
footprint) with typed ordered children. Resolve all addressed inputs first, then
build final collections from the immutable snapshot; multiple deletes/inserts
must never shift another patch's target. Reject overlapping/contradictory patches.
Preserve/rebuild native references and ownership through typed APIs. Reopen and
validate insertion, deletion, and reorder cases before a domain can ship.

### SchLib collection classes (step 2, shipped)

The complete reference is
[docs/spec-lang/reference/schlib-collections.md](docs/spec-lang/reference/schlib-collections.md).
Implementation: `collection_class` in `crates/altium-sync/src/collection.rs`,
owner-group scoping in `crates/altium-sync/src/schlib/positional.rs`,
projections in `crates/altium-format/src/schlib/snapshot/views.rs`.

| Collection | Class | Key / alignment |
| --- | --- | --- |
| components | keyed | exact `lib_reference`; renames never inferred |
| parts | keyed (structural) | numeric slot `1..=effective`; complete scope = contiguous prefix from 1; trailing shrink only, under deletion permission |
| parameters | keyed | name, case-insensitive (native `GetParameterByName` is OrdinalIgnoreCase); authored spelling written on update; root-owned records only |
| aliases | keyed | exact string; membership-only (AD26 stores a sorted set — order is never authored) |
| footprints | keyed | exact `model_name`, PCBLIB implementations only; `source:` is provenance, never a key |
| pins | positional | per owner-part group (filtered projection index K) |
| graphics | positional | per owner-part group; one ordered collection across kinds |
| maps | positional | within each footprint (pin-pad definer order) |

Swap groups are not a collection: they are scoped pin-field bindings
(`SwapIdPin`, `PairSwapID`, and the combined `SwapIdPart |&| SwapIdPartPin`
string), with the part-level `part_swap_group` a derived unanimity projection
over the part's non-owner-0 pins plus fan-out on write; a `Mixed` projection
blocks writes. Owner-group mapping: `part N` → owner N; component scope →
owner 1 on single-part components and the shared owner-0 pin slot on
multi-part ones; multi-part component-scope graphics are a hard error (no
shared graphics owner exists natively).

## Direction, scope, and intent

Compile resolves the spec and imports and computes desired target state. Dump
projects the document and computes structured edits to existing source, or creates
new source. No state remembers which side changed: if both were edited before
planning, direction selects authority within scope and the report shows replacements.
Edits after planning invalidate the plan.

Retain explicit authored meanings: literal = Set, `null` = Clear, contextual
`inherit` and `reset` = inheritance/reset. Absence at instance scope leaves the
target unchanged; absence in a definition inherits the defined default. Define
precedence, legal clear/reset operations, and contextual errors with SchLib examples
before implementing these forms. Unsupported is never an absence meaning.

Source-declared scope selects writable entities, collections, and fields. Deletes
require explicit source opt-in; CLI options may only tighten scope. Outside-scope
content is fully parsed and typed and deliberately preserved. Unsupported
representation inside scope blocks planning instead of claiming convergence.

Greenfield/brownfield describe authoring context, not metadata policies. SchDoc
inline children without a symbol reference materialize directly. A resolvable
explicit symbol supplies an imported base plus overrides; an unresolved explicit
symbol errors. Dump preserves imports and authored expressions, representing
differences as overrides or complete positional child collections where possible.
If that cannot preserve authored meaning, block rather than silently flatten.
Keep existing inline-child errors until materialization works.

## Imported image assets

Treat image contents as opaque imported assets, per the user's 2026-09-05 scope
change. Keep Storage framing, resource identities, references, placement, and
display flags typed. Preserve the complete native image body, including preview
and original, without decoding or re-encoding it. This exception is only for
image assets; the strict parsing rule still applies to all other document data.

Initially dump/import explicit `.altium-image` bundles containing the exact
decompressed resource body. Standalone PNG/JPEG conversion and preview generation
are deferred. Planning captures asset bytes and hashes; saved plans contain the
payload and apply never refreshes it. Dump stages assets and spec together.
Use whole-resource replacement, explicit scope, and reference validation.
See the [import contract and staged gates](docs/proposals/typed-image-resources.md).

## PcbDoc bulk routing

Do not require thousands of routed segments to appear in authored spec text.
The intended split is readable board/net/rule/placement intent plus an explicitly
referenced, typed binary routing artifact:

```text
routing {
    solution: "main.routes"
}
```

Current evidence (2026-09-05):

- This syntax compiles to `RoutingSpec.solution` in Altium CLI today, but its
  executor does not load the file. Dump still emits individual tracks/arcs/vias.
- The sibling AutoPCB repository implements `autopcb-routes`, including binary
  save/load for typed segments, vias, and pour geometry. It also has JSON inspection.
- Its current route model is not a complete arbitrary-PcbDoc serialization:
  native arcs and some Altium attributes are not represented. Numeric net/layer
  IDs need an explicit mapping to the board they belong to.

Implementation requirements:

1. Reuse the AutoPCB artifact boundary; do not invent a second routing authority.
   Define the exact supported schema, board context, units, net/layer mapping,
   native attributes, and import/export contract before wiring the bridge.
2. Keep all routing content fully typed. Extend the owning typed schema where
   needed or reject unsupported externalization. No raw native payload sidecar.
   Scope-limited edits may preserve fully parsed routing in the target document
   when routing is outside scope; never claim a complete export in that case.
3. A full PcbDoc dump stages spec plus routing artifact together. Compile loads
   the explicitly named file, validates references, and plans replacement of the
   managed routing collections. Missing/invalid files are errors; never silently
   ignore `routing.solution` or append duplicate copper on repeat apply.
4. Define the interaction with inline hand-authored copper: each managed collection
   must have one authoritative representation. Reject overlapping inline/artifact
   ownership until a deterministic explicit composition rule exists.
5. Hash routing artifacts as inputs/outputs just like source/imports/documents.
   Freeze typed route patch data in the saved plan; apply must not refresh it from
   a subsequently changed artifact. Dump's spec and binary outputs share a transaction.
6. Report counts and semantic changes by net/layer, with expanded indexed detail.
   Offer an explicit text inspection/export path; bulk text is not the default
   authored representation. Bound memory and report-pairing cost on large boards.

The artifact stores current design geometry, not object-history mappings or a
last-synchronized baseline. File encoding does not alter strict typed parsing rules.

## Saved-plan contract

Use self-contained JSON with an exact supported `schema_version`. Versioning
covers patch semantics, snapshot projection/order, addresses, and normalization.
Unsupported versions fail with a request to regenerate.

The plan embeds domain/direction/scope, a resolved artifact manifest, typed
before/after state, typed Create/Replace/Delete or source patches, and diagnostics.
Use collision-resistant byte hashes (such as SHA-256) for every source, transitive
import, referenced binary design artifact, and destination read or written.
Record expected absence for new outputs.

Hash and parse the same captured contents. Detect changes during planning.
Freeze output paths; an alternate output has its own precondition independent of
a read-only target. Permit an explicitly declared in-place input/output as one
artifact protected by one precondition. Reject accidental cross-artifact aliases,
duplicate destinations, and any plan-file collision with an input or output;
resolve symlinks consistently. Numeric references inside routing artifacts must be
checked against their board/net/layer context, not accepted because they are in range.

At apply, do not recompile intent, reread untracked imports, rerun a router, or mint
new random semantic values. Allocate required native creation IDs at planning.
They are native output data, not synchronization bindings.

For a new source file embed validated text and its typed expected state; for
existing source save structured edits whose locators are bound to the source hash.
Large machine-readable plan payloads may contain all frozen geometry; report
summaries and authored spec text need not. Binary plan packaging can be considered
later without changing the contract; it is not required for the first SchLib slice.

## Reporting

Derive the report from actual patches and typed before/after values. First ship
an exact indexed diff. Later add deterministic, bounded display heuristics:
exact-value alignment, likely move/edit pairing, grouping, and repetitive summaries.
Label inferred pairings, retain old/new indices, and expose every executable change
in expanded output. Never hide deletes, blocking errors, or changed values behind
tolerances. Turning heuristics off must leave patches and after-state identical.

## Checked apply and recovery

1. Decode strictly; reject bad versions, diagnostics, contradictory patches, and
   invalid addresses before artifact writes.
2. Verify every input hash/absence precondition, parse captured contents, and
   resolve all addresses against immutable snapshots.
3. Execute typed patches in memory; verify planned after-state and all invariants.
4. Stage every output beside its destination; serialize, reopen, project, and
   validate against the planned result. Only documented typed upgrades are allowed.
5. Prepare a durable recovery journal for this transaction's explicit destinations,
   stages, and previous files. Recheck inputs immediately before commit.
6. Commit by renames and record progress durably. Keep recovery material until
   every output is committed, then clean it up.
7. Detect interrupted transactions and support completing or restoring the full
   file set. Never overwrite a subsequent unrelated edit during recovery. Remove
   the journal after resolution; later plans never consume it as sync state.

Ordinary renames do not provide multi-file atomicity or exclude an Altium writer.
Document the exclusive-write window and detect races where possible; do not claim
cross-process atomic compare-and-swap. Read-only planning creates no recovery files.
Transaction backups are not undecoded payloads carried through the semantic model.

## Planned CLI

These commands are not implemented yet:

```text
altium compile plan design.schlib-spec --target design.SchLib --out plan.json
altium compile apply --plan plan.json
altium dump plan design.SchLib --spec design.schlib-spec --out plan.json
altium dump apply --plan plan.json
```

Exit codes: 0 for no-change planning/successful apply, 2 for an applicable plan with
changes, 1 for an error or blocking diagnostic. Direction-specific apply rejects
the wrong plan direction. Report writes by artifact.

Convenience modes must route through the same planner/executor. Each completed
domain retires its legacy reconciliation/execution route; no silent fallback.
Until then, distinguish legacy commands from the new surface. Project `--all`
plans and stages every affected artifact before any destination is committed.

## Implementation steps and gates

Complete an independent agent completeness review after every numbered step;
resolve findings before proceeding. Steps 1 through 3 are complete; steps 4
through 9 remain outstanding. Next: step 4 (checked transaction executor).

1. **Language boundary and full SchLib intent — complete.** Move/decouple CST, diagnostics,
   parser, and formatter into `altium-spec-lang`; port imports and SchLib intent
   passes. Specify absence, overrides, and collection scope. Gate: existing source
   preservation tests plus deterministic examples for imports, defaults,
   Set/Inherit/Reset/Clear, invalid keys, and partial/complete collections.
2. **SchLib snapshots and indexed patches — complete (2026-09-17).** Every
   gate item landed and was independently reviewed per increment (worklog:
   `docs/worklogs/next-children-2026-09-16.md`). Elaboration and projection:
   `SchLibIntent` → `SchLibPatch` in `crates/altium-sync/src/schlib/elaborate.rs`
   over the read-only projections in
   `crates/altium-format/src/schlib/snapshot/views.rs`. Owner-group scope
   targeting for elaboration: `crates/altium-sync/src/schlib/positional.rs`;
   the snapshot-bound address vocabulary
   (`crates/altium-sync/src/address.rs`) is landed and unit-tested but not
   yet consumed by elaboration — saved plans (step 3) are its intended
   consumer (its module docs record the divergence risk against
   `positional.rs`). Aggregate replacement:
   `replace_components` (`schlib/snapshot.rs`), `with_library_edit`
   (`snapshot/library.rs`), `with_children` (`snapshot/children.rs`),
   `with_parts_appended`/`with_parts_removed` (`snapshot/parts.rs`). Scope
   checks: the class-aware planner in `crates/altium-sync/src/collection.rs`
   plus `validate_scope` in `elaborate.rs`. Coverage: components, parameters,
   aliases, footprint maps keyed and pins/graphics/maps positional
   (`elaborate.rs`); parts structural (`snapshot/parts.rs`,
   `resolve_parts_structure` in `positional.rs`); swap groups as pin-field
   bindings with part-level projection (`views.rs` `parts()`) and fan-out
   (`positional.rs` `part_swap_fanouts`/`apply_part_swap_fanouts`). Gate
   evidence: materialize/project equality and multi-insert/delete/reorder in
   the default `--lib` suites (482 format, 54 sync tests; every step-2 test
   is fixture-free — the 11 ungated legacy CFB/IntLib tests recorded in
   STATUS.md predate step 2) and the three-library corpus example
   (`crates/altium-sync/examples/check_schlib_snapshots.rs`, all passes,
   empty replans); ownership validation in `assemble_records`
   (`crates/altium-format/src/schlib/ownership.rs`); unsupported-content and
   unauthorized-deletion hard errors enumerated in
   [docs/spec-lang/reference/schlib-collections.md](docs/spec-lang/reference/schlib-collections.md).
   Deliberately out of step-2 scope: saved plans, transactions, CLI
   migration, and dump-direction lowering (step 3+); recorded
   unsupported-content classes (style_id, Additional-stream structural
   edits, owner-0 single-part pins, multi-pad definer addressing,
   `is_current` authoring) stay hard errors or preserved-only until their
   evidence or address forms land.
3. **Saved plans and both directional planners — complete (2026-09-18).**
   Schema/version: `PLAN_SCHEMA_VERSION`/`SCHLIB_STATE_FORMAT_VERSION`/
   `SPEC_EDIT_FORMAT_VERSION`, exact-match decode with regenerate messages
   (`crates/altium-sync/src/plan.rs`, `altium-format` state payload,
   `altium-spec-lang/src/cst/planned.rs`). Manifests: BLAKE3/absence
   preconditions with read-once capture, planning-drift recheck, frozen
   outputs, explicit in-place declaration, and alias/duplicate/plan-path
   collision rejection re-run at decode and apply. Document lowering:
   `compile_from_schlib_patch` + `plan_compile_schlib` + `apply_saved_plan`
   (typed Create/Replace/Delete with complete embedded before/after state,
   lowering-time execution dry run). Source lowering:
   `plan_dump_schlib` + `apply_saved_dump_plan` (hash-bound persisted
   structured edits via `PlannedSpecEdit` locators, `SourceCreate` with
   embedded validated text + typed expected state, staged image bundles,
   planning-time convergence proof: the resulting source elaborates to an
   empty patch). Exact reporting: `report.rs` per-artifact entries in both
   directions, deletes always expanded, no heuristics. Gate evidence per
   clause (determinism, complete preconditions, no-op/create/update/delete,
   comments/expressions/CRLF/annotations preservation, no heuristic
   execution): the step-3 closure section of
   [docs/worklogs/next-plans-2026-09-18.md](docs/worklogs/next-plans-2026-09-18.md),
   backed by fixture-free suites and the three-library corpus gate
   (56/32/135, exit 0, incl. the dump convergence/preservation stage).
   The comments/CRLF/annotations half of clause 4 is met; authored
   EXPRESSION preservation is NOT — the legacy merge compares property value
   text and literalizes every expression it touches, even one evaluating to
   the unchanged value. Recorded residuals live in the same closure section
   (v1 keep-order restriction, index-less alias ops, dump-source
   completeness guards, expression literalization and deletion of spec
   content outside the fresh dump).
4. **Checked transaction executor.** Implement staging, reopen validation, durable
   commit, and recovery. Gate: source/import/target/routing changes and newly
   occupied outputs reject stale plans; path aliases/overlapping patches error;
   injected failures at each commit boundary recover without silent partial results.
5. **Ship full SchLib CLI slice.** Wire both directions, remove SchLib's old
   execution path, update operational docs/STATUS. Gate: apply equals the planned
   semantic state; same-direction replanning is empty; supported roundtrips preserve
   authored meaning; no persistent synchronization state or embedded linking IDs.
6. **PcbLib, then PrjPcb.** Repeat steps 1–5 and independent review for each domain.
   PcbLib needs complete pads/graphics and long-name SectionKeys. PrjPcb needs a
   typed public mutation boundary and all-artifact project transaction behavior.
7. **SchDoc.** Complete inline children, imported overrides, document parameters,
   ownership/connectivity rebuilding, and both-direction acceptance laws. Preserve
   fail-fast errors until each path is complete; review before moving on.
8. **PcbDoc and binary routing bridge.** Define the artifact fidelity/context
   contract above with AutoPCB; implement typed bulk import/export, complete board/
   layer/primitive snapshots, and native-reference rebuilding. Gate: no-op routed
   board preservation, spec-plus-artifact roundtrips, stale-artifact and bad-net/
   layer errors, no duplicate copper on reapply, bounded large-board handling.
   Creation only ships when initialization invariants and typed APIs support it.
9. **Report readability.** Add pairing/grouping using representative noisy diffs;
   retain exact expanded output. Gate: display settings cannot change patches or
   hide changes. This step may follow SchLib; it need not wait for PcbDoc and is
   never a prerequisite for correct execution.

Use fixture-free unit and programmatic-document tests. Run targeted checks first,
then broader default suites appropriate to changed modules. Do not run
`test-fixtures` or `proptest` without explicit authorization. When corpus testing
is authorized, use semantic CFB comparisons and distinguish documented upgrades
from unexplained loss. Keep contextual errors and strict no-opaque-data rules.
