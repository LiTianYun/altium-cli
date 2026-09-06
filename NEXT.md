# Snapshot-bound plan/apply

Updated: 2026-09-05. Accepted implementation direction; the replacement engine
is not implemented yet. See [STATUS.md](STATUS.md) for current behavior.

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
dump merge now live in `altium-spec-lang`, reexported by `altium-format-spec`.
Import resolution and immutable transitive source capture also live in the
language crate. Collection scope and contextual operations parse into explicit
AST/CST forms. The typed SchLib intent core compiles captured imports and retains
field operations, collection scope, source provenance, and spread origins. Full
step 1 is still open for axis-level field intent, complete auto-size constraints,
graphic policies/image resources, and generated footprint-pad imports. Retained
anchor dependencies now have an evaluator over effective target/base geometry. CLI plan still renders an ECO while apply recompiles
the spec and invokes a different executor; preview/apply agreement is not guaranteed.
`altium-sync` contains a dependency scaffold only. There is
no executable external state subsystem to migrate or delete.

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
unsupported data must error, never pass through as undecoded bytes.

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
resolve findings before proceeding. Step 1 front-end/import extraction is implemented; full intent semantics
remain outstanding. None of the numbered steps below is complete.

1. **Language boundary and full SchLib intent.** Move/decouple CST, diagnostics,
   parser, and formatter into `altium-spec-lang`; port imports and SchLib intent
   passes. Specify absence, overrides, and collection scope. Gate: existing source
   preservation tests plus deterministic examples for imports, defaults,
   Set/Inherit/Reset/Clear, invalid keys, and partial/complete collections.
2. **SchLib snapshots and indexed patches.** Implement complete elaboration and
   projection, addresses, aggregate replacement, and scope checks. Cover components,
   parts, pins, parameters, aliases, footprint maps, graphics, and swap groups.
   Gate: materialize/project equality, ownership validation, multi-insert/delete/
   reorder cases, and hard errors for unsupported content or unauthorized deletion.
3. **Saved plans and both directional planners.** Implement schema/version,
   manifests, source/document lowering, and exact reporting. Gate: deterministic
   semantic plans, complete preconditions, no-op/create/update/delete examples,
   preserved source comments/expressions/CRLF/annotations, no heuristic execution.
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
