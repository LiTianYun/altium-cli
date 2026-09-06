# Greenfield, brownfield, and directional plan/apply

Greenfield describes a design authored in specs; brownfield describes an existing
Altium design being edited through specs. Either workflow can use compile
(spec → document) or dump (document → spec). Command direction selects authority
for the requested operation, independently of how components are materialized.

The accepted replacement design is [NEXT.md](../../../NEXT.md).
It uses addresses bound to exact input snapshots and no persistent synchronization
state or embedded tool identity. These are planned semantics, not a claim that
the replacement CLI or inline-child materialization has shipped.

## Authoring workflows

A greenfield spec can import library definitions and instantiate symbols. GUI
edits can be folded into source by explicitly planning the dump direction:

```text
spec → compile/apply → Altium GUI edit → dump/plan → reviewed source edits
```

A brownfield workflow starts with an existing document and its concrete contents:

```text
Altium document → dump → source edits → compile/plan → reviewed document edits
```

There is no remembered previous synchronization state. If both files changed,
the chosen direction determines the desired managed values and the plan exposes
the destination changes. A changed input after planning invalidates the saved
plan. Do not promise automatic detection or merging of historical concurrent edits.

## Inline component children

Materialization depends on the explicit symbol reference, not inferred authority:

| Component form | Planned materialization |
| --- | --- |
| Inline children without a symbol reference | Materialize the concrete pins, graphics, parameters, and other children directly |
| Resolvable explicit `symbol: $lib.Name` | Use the imported symbol as base and apply declared overrides |
| Explicit unresolved symbol reference | Hard error; do not fall back to inline-only materialization |

An imported definition remains authored source. Dump may represent concrete
differences as overrides, including whole ordered child collections. If source
edits cannot preserve imports, expressions, and meaning, report an unsupported
operation instead of silently flattening or rewriting the definition.

### Current fail-fast boundary

Inline `pin`, `graphic`, `part`, and `footprint_map` blocks inside a SchDoc
component parse, but materialization is not implemented.
`compiler.rs::compile_schdoc_component` rejects them. Keep that error until
typed materialization and save/reopen validation exist.

## Addressing and source preservation

Objects without reliable keys use their parent/collection/index in the captured
typed snapshot. Named collections may use exact scoped keys under their domain
contract. Native IDs are preserved where present, but are not assumed universal.

Indices can change after insertions, deletions, reorders, or GUI saves. An exact
input hash protects a reviewed plan from being applied to a different snapshot.
It does not prove that two objects across edited snapshots share an identity.

Existing spec annotations remain supported. A source annotation identifies authored
source metadata; it cannot establish a binding to an Altium primitive that has no
corresponding durable field. Do not introduce hidden component parameters,
file-level mode markers, or external identity maps for this workflow.

Structured CST edits preserve comments, formatting, expressions, and existing
annotations according to explicit matching/positional semantics. Similarity
heuristics may improve the report only; reassigning a comment or override is a
source mutation and cannot be driven by a display heuristic.

## Artifacts and transactions

A plan lists every file it would modify. Compile normally writes the document;
dump normally writes the spec. PcbDoc dump may also write a typed binary routing
artifact so bulk copper need not appear as thousands of source lines.

All sources, imports, design artifacts, and destinations have hash/absence
preconditions. Apply stages, serializes, reopens, and validates all outputs before
committing with a temporary recovery journal. This provides recovery across files,
not a claim of perfect multi-file atomicity. No source linking-annotation write or
embedded identity write is required as a side effect.
