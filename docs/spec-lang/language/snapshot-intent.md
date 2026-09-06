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
