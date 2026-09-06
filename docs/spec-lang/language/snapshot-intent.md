# Snapshot intent syntax

The language front end parses the following forms for NEXT.md's replacement
workflow. The legacy CLI `plan` and `apply` reject collection declarations and
contextual field operations; they cannot execute the new intent contract yet.

```text
collection components { mode: partial, delete: deny }
component R {
    collection pins { mode: complete, delete: allow }
    description: null
    pin 1 { name: "A", length: reset }
}
```

Collection declarations occur beside the children they govern. The root supports
`components`; components support `parts`, `pins`, `parameters`, `aliases`,
`footprints`, and `graphics`; parts support `pins` and `graphics`; footprint maps
support `maps`. Mode defaults to `partial` and deletion to `deny`.
`delete: allow` requires `mode: complete`. Unknown kinds/properties/values,
wrong owners, and duplicate declarations fail parsing. Collection metadata does
not itself authorize replacing every field on a child.

`inherit` and `reset` have explicit expression nodes and are legal only as direct
property operations. They cannot appear as ordinary arithmetic, let values,
interpolations, or values inside spread objects. Quoted strings and ordinary
property/member names remain text. `null` remains an ordinary expression; its
clearability depends on the typed intent field, not the syntax parser.

Lossless CST access and structured property edits preserve collection declarations,
comments, and CRLF. Canonical formatting supports these forms but retains its
existing narrower comment-preservation behavior; use CST edits for existing source.

`altium_spec_lang::import::capture_imports` captures the root and every transitive
source as immutable UTF-8 text plus its parsed AST, canonical paths, ordered import
edges, and dependency order. It compares captured contents and path resolutions
again before returning. `verify_unchanged()` checks them later without refreshing
the capture. This detects observed edits, not an atomic filesystem snapshot or an
exclusive writer lock. Hashing, saved plans, and checked writes belong to later
NEXT.md steps.

The evolving semantic contract is in
[the proposal](../../proposals/snapshot-intent.md); the current implementation
boundary and remaining gates are tracked in [STATUS.md](../../../STATUS.md).

## Typed SchLib intent API

`altium_spec_lang::intent::compile_schlib` compiles an import-free AST;
`compile_captured_schlib` evaluates the immutable captured graph without filesystem
reads. Imported definitions and constants remain available through explicit
references; imported components are not scheduled as root output. Definition
references are typed, so an ordinary object cannot impersonate a footprint.

Field intent retains `Absent`, `Set`, `Inherit`, `Reset`, and `Clear`, with spans.
Collection permissions remain independent of child fields. Component, part, pin,
parameter, alias, footprint-map, and graphic intents are typed. Field policies
resolve definition defaults and instance target/base precedence for supported
fields; they do not grant writes to omitted fields. Existing annotation data and
imported footprint source references remain source metadata, not native bindings.

Spread expressions retain direct-property origins through evaluation. Later spread
entries preserve the established override order; duplicate direct assignments to
the same field (including typed synonyms such as parameter text/value) error.
Null supplied by a spread keeps its Clear meaning. Objects are not globally
rewritten to canonical field names.

Use the read-only diagnostic example to compile from captured contents twice and
check equal results:

```sh
cargo run -p altium-spec-lang --example check_schlib_intent -- design.schlib-spec
```

This is a front-end API, not the replacement CLI planner. Full step 1 still needs
graphic default and clear policies and typed image-resource support.
Unsupported image_data fails explicitly. Graphic fields are intent, not proof of
a native mapping: line-width enum conversion and fields without a native counterpart
must be resolved before materialization. Later steps must verify ownership,
materialize/project equality, scope enforcement, and native serialization.

Pin coordinates retain either a paired `at`/`location` operation or independent
`x` and `y` operations. `resolve_pin_location` resolves effective coordinates
without granting write scope: an omitted axis preserves the target value, or uses
the resolved instance base or definition default when creating a pin. Instance
`inherit` selects the base axis; `reset` selects zero. Coordinates cannot be
cleared. Paired coordinates, independent axes, and anchored placement are mutually
exclusive, including when supplied through spreads.

Captured PcbLib imports export explicit and generated pad names for SchLib
footprint mappings. Rows, columns, and numeric/alphanumeric grids preserve naming,
skip, perimeter, and explicit generated-pad override semantics. Bound box edges
support forward references. Invalid generator properties, counts, coordinates,
and duplicate names fail contextually; expansion is limited to one million
generated positions per footprint before allocation. Compilation uses captured
contents and retains the referenced definition's source path. This is a pad-name
projection, not full PcbLib intent or native footprint materialization.

## Retained layout dependencies

Anchored pins retain typed source-owner/ordinal references, edge, sequencing,
gap, side, and offset. Bound box graphics retain auto-size directives. These are
source references, not native object identities. Compiling an anchor does not
turn an absent pin length/location or box corner into a Set operation.

`evaluate_layout` receives effective pin and box geometry after a planner has
resolved target/base/default values. A preserved 60mil pin therefore uses 60mil
for placement even when its source omits length; creation may use the 25mil
definition default. Parts inherit component graphic bindings with ordinary lexical
shadowing. Source ranges, cycles, overflows, and contradictory placements error.

Owner-local auto-size solves the minimum feasible integer width and height for
start/end/center roots, before/after chains, branches, offsets, and effective
absolute-pin dependencies. It preserves an explicit corner and respects minimum
dimensions, margins, and representable pin coordinates. Infeasible constraints
fail with source context. Omitted padding uses each chain's root gap; explicit
padding applies to all chains, with reset/inherit selecting the 20mil default.
Parts treat inherited component boxes as fixed effective geometry; they do not
resize a shared box from part-local pins. Snapshot scope integration remains a
later elaboration gate.
