# Snapshot-bound authored intent

This is the proposed semantic contract for [NEXT.md](../../NEXT.md), step 1.
It does not describe a shipped planner. Existing component/import/expression syntax
is reused below. The front end and typed intent core now preserve `inherit`,
`reset`, collection declarations, and supported `null` clearing. These operations
are not yet executable through snapshot plans; the remaining gates are in STATUS.md. The legacy compiler's `Option` values,
eager defaults, and additive executor do not establish this contract.

## Context and authority

A `.schlib-spec` component is a symbol **definition**. Its absent fields inherit
the language's versioned definition defaults. Existing imported references such as
`$symbols.R` resolve definitions; merely importing a file does not schedule its
components for output. Imports must resolve explicitly and deterministically,
including transitive imports; missing files, missing names, cycles, and duplicate
names are errors with source spans and import chains.

A placed component with `symbol: $symbols.R` is an **instance**. Its children are
overrides of that resolved definition. No new `definition`, `extends`, or automatic
base-discovery mechanism is introduced. This instance example uses the existing
SchDoc reference surface to specify how SchLib definitions are consumed; shipping
SchDoc child materialization remains NEXT step 7.

Absence in an instance override preserves an existing destination value. When
creating the instance, there is no destination value to preserve, so absence uses
the imported definition value. It never silently substitutes a default for an
unresolved explicit symbol. An explicit `inherit` on an existing instance selects
the imported value even if its destination value differs.

Definition normalization and writable scope are separate. A normalized definition
has defaults, but omitted fields are not automatically permission to overwrite an
existing library component. By default, only explicitly authored properties and
child entries are writable. New objects use normalized defaults for their remaining
required fields. Existing objects preserve fully typed fields outside scope. A
future explicit field scope can select defaulted fields; it must not be inferred
from a CLI flag, import, or missing property. An authored `reset` is the way to
request a default explicitly with the minimal syntax below.

## Field operations

Preserve these five forms in typed intent until context and scope are known.
Do not encode them as one `Option<T>` or lose the source span.

| Authored form | Definition meaning | Instance override meaning |
| --- | --- | --- |
| Property absent | Normalize to the defined default; no implicit write permission | Preserve existing target; on creation use resolved base |
| Literal or ordinary evaluated expression | Set that typed value | Set that typed value |
| `inherit` | Select the defined default explicitly | Select the resolved imported base field explicitly |
| `reset` | Select the versioned language default explicitly | Select the versioned language default, bypassing the imported base |
| `null` | Clear a clearable field explicitly | Clear a clearable field explicitly, bypassing the imported base |

`inherit` and `reset` are contextual property operations, not ordinary strings or
values for arithmetic, interpolation, `let`, or object spreading. Quoted
`"inherit"` and `"reset"` remain ordinary string values. Nested expression use of
these operations is an error. Null as a general expression remains available,
but a property resolving to null must undergo the same clearability check as a
literal null. Duplicate directly authored properties, including synonyms such as
`text` and `value`, are errors rather than last-write-wins. Object spreading keeps
its existing declared source-order overlay semantics: `...base, length: 40mil`
explicitly overrides the inherited entry, and a later spread may override earlier
entries. This intentional overlay is distinct from repeating a direct property.
Validate duplicate direct keys inside spread-source object literals before evaluation
so evaluation cannot erase a typo. The final overlaid typed field retains its authored Set/Clear meaning;
no hidden historical baseline or extra write scope is inferred.

Defaults belong to a typed field registry, versioned with the plan schema. For the
initial SchLib surface: pin `name` and `hidden_net_name` default to empty strings;
`electrical` to passive; `length` to 25mil; location to `(0mil, 0mil)`;
orientation to zero; `is_hidden` to false; parameter `text` to an empty string and
`is_hidden` to false. Component optional designator and description default to
absence. On creation, part count defaults to the maximum declared positive part
number, or one without parts. On an existing component, an absent part count
preserves the existing count unless an explicitly authored part requires growth,
as specified below. Resetting part count uses the resulting managed and preserved
part slots; shrinking requires explicit collection deletion permission and must
never invalidate ownership.

Clear is allowed for component optional `description` and `designator` (typed
absence), pin `name`, `hidden_net_name`, and swap-group assignments (empty strings),
and parameter `text` (empty string). Empty string Set and optional-field Clear are
distinct even when a particular serialization normalizes them. Clear does not
remove the pin, parameter, or component. Required identifiers, coordinates,
electrical type, length, orientation, booleans, and part count cannot be cleared.
Other fields require an explicit typed clear/default policy before support; absence
of a policy is a blocking error, never a best-effort omission. Required identifiers
have no reset default. `inherit` on an instance lacking a resolved base errors.

For example, these property forms fit existing block/property syntax, but their
new operation semantics are proposed:

```text
component R {
    description: null
    pin 1 { name: "A", length: reset }
    parameter Value { text: "10k" }
}
```

On an existing target this clears R's description, sets pin 1's name, and explicitly
sets its length to 25mil. Other pin fields remain unchanged. On creation, remaining
fields receive the definition defaults. Pins align positionally as defined below;
the name `1` is the desired designator, not inferred durable identity.

Given an imported definition with `pin 1 { length: 40mil }`, an instance override
`pin 1 { length: inherit }` selects 40mil, `length: reset` selects 25mil, and
`length: null` is an error. An absent `length` preserves a preexisting 60mil target
value and uses 40mil on creation. The enclosing reference is the ordinary
`symbol: $symbols.R` form, not a generated linking ID.

## Collection scope syntax

Proposed syntax keeps existing child blocks and adds one metadata declaration per
collection at its owner. The declaration does not contain or duplicate its items:

```text
collection components { mode: partial, delete: deny }
component R {
    collection pins { mode: complete, delete: allow }
    pin 1 { name: "A" }
    pin 2 { name: "B" }

    collection parameters { mode: partial, delete: deny }
    parameter Value { text: "10k" }
}
```

Without a declaration a collection is partial and deletion is denied. Allowed
mode values are `partial` and `complete`; deletion values are `deny` and `allow`.
The only legal combination with `delete: allow` is `mode: complete`. Omitted mode
or deletion properties receive `partial` and `deny` respectively. Unknown keys,
values, duplicate metadata declarations, or metadata at the wrong owner error.
CLI scope can only tighten these permissions. Tightening that prevents a requested
change blocks planning; it does not silently yield a misleading successful plan.

Collections at the library owner are `components`; at a component they are
`parts`, `pins`, `parameters`, `aliases`, `footprints`, and `graphics`; at a part
they are `pins` and `graphics`; at a footprint map they are `maps`. Swap-group
blocks declare scoped bindings, not standalone native records or deletable native
collections. Their assignments on pins are fields with Set/Inherit/Reset/Clear.

Component library references, part numbers, parameter names, alias names, and
footprint model names are exact parent-scoped natural keys for this contract.
Duplicates on either side block a managed collection, with both locations reported.
Pins and graphics align positionally, within their typed owner collection, without
matching on designators, annotations, bindings, values, or native IDs. Map pairs
are positional within each footprint map. A future native-ID matching rule requires
a separate domain contract and schema version.

A partial keyed collection updates matched entries in their original slots and
appends new entries in source order; unmentioned entries retain order and values.
A partial positional collection with N authored entries describes indices 0 through
N-1 and preserves the remaining destination tail; entries past the destination
length append in source order. It does not support sparse index selection or
interior insertion. Such edits require a complete collection.

A complete collection determines final order and membership from its authored
items, including an empty list when no children occur. Surviving keyed entries
retain outside-scope fields while taking source order. Positional item N uses
original destination N for preserved fields. Removing any destination-only entry
requires `delete: allow`; otherwise planning errors even if every remaining entry
is valid. A complete collection does not make every field on its children writable.

To delete all aliases explicitly:

```text
component R {
    collection aliases { mode: complete, delete: allow }
}
```

Deleting a component requires a complete library `components` declaration allowing
deletion and omitting that component. Renaming a keyed component is a delete plus
create and requires the same permission.

Parts are a special structural collection: the typed component represents a part
count and children with numeric owners, not a stored vector of part objects. Its
projection contains every slot `1..=part_count`, including empty slots, in ascending
numeric order. There is no implicit renumbering or reordering of owners. A complete
parts declaration must therefore describe a contiguous prefix starting at one;
omitting an interior slot while retaining a higher slot is an error. Removing a
trailing slot requires `mode: complete, delete: allow`, reduces the count, and
deletes that slot's descendants, all of which the report expands. At least one
slot must remain. To empty an interior slot while keeping its numeric owner, retain
that part and explicitly clear its child collections with complete deletion scope.
A child scope requiring preservation of a descendant contradicts deletion of its
part and blocks planning. Explicitly lowering `part_count` without the same
complete parts/deletion scope is an error, even when the removed slots are empty.

An explicitly authored `part 3` on a component with count two and omitted
`part_count` authorizes structural growth to three. More generally, absent count
becomes the maximum of the preserved count and the highest authored part number
under partial scope; creation uses at least one. Intermediate new slots are empty.
This is a declared consequence of authoring a part, not blanket field scope:
existing children and owners remain unchanged. Under complete parts scope, the
contiguous authored prefix determines the resulting count. An explicit part count
must agree with complete scope and must accommodate all authored and preserved
owners under partial scope, otherwise planning errors. CLI scope that forbids
count growth blocks the requested part insertion rather than silently omitting it.

All indices refer to the immutable, fully parsed input projection. The projection
uses typed API collection order, filtered by owner part, not display sorting or
CFB record offsets. Projection preserves each existing child's native numeric
owner; source omissions never reclassify it. New component-level children use owner
part zero if the effective resulting part count exceeds one or explicit part
blocks exist, otherwise owner part one. Thus `part_count: 2` gives new top-level
children owner zero even without part blocks. Children explicitly authored in
`part N` use owner N. Growing a component from one to several parts does not move
existing owner-one children into the shared owner-zero collection. Positional
alignment is within the resulting source owner's collection, so changing an
authoring context cannot silently retarget children from another owner.
Independent delete/insert patches cannot shift another patch's target. Graphics of
different kinds remain in one ordered graphic collection, not separate type buckets.
A type-changing positional replacement preserves only fields with an explicit
cross-kind typed mapping; if an outside-scope field cannot be preserved, planning
must block rather than discard it.

## Required checks before shipping

The implementation must test absent versus all four explicit operations on both
existing and new objects; inherited 40mil versus reset 25mil; clear versus empty
Set on optional strings; invalid required-field clears; missing defaults and bases;
unknown keys and duplicate synonyms; transitive import errors; empty complete
collections; denied deletion; multiple insertion/deletion/reordering cases; and
parts with shared and local ownership. Partial prefix updates must preserve tails.
Repeated planning after apply must be empty in the same direction.

Dump must preserve imports, comments, annotations, and authored expressions when
representable by structured edits and explicit overrides. If source meaning cannot
be preserved, it must block. Source annotations are source metadata only. Native
IDs remain typed native data; neither annotations nor generated IDs may become
cross-artifact execution bindings. Reports derive from actual typed patches and
show replaced values and deleted descendants without similarity-based retargeting.

## Graphic policy frontier

The implemented graphic policies use the legacy `graphic_from_spec` constructors in
`crates/altium-format-spec/src/executor.rs` supply defaults, but those fallback
values do not prove valid creation geometry. Arc, elliptical-arc, and pie end
angles default to typed absence, not 360 degrees; intent reset/clear now preserves
that distinction. Native line widths use `PenWidth`, while
the old compiler exposes coordinates and the old executor ignores them. Polyline
`closed` and image `show_border` have no direct field in the current public graphic
API. The new intent compiler rejects those properties and dimensioned pen widths;
named typed pen widths are supported. Font defaults require validation against the
destination font table. These policy choices are checked against current Rust
code, not a new independent AD26 source verification.

The old `image_data` compiler field has no corresponding public image-resource
mutation boundary and is ignored by its executor. The replacement intent compiler
rejects it. A complete implementation needs typed resource APIs and strict image
validation; no raw payload retention can fill that gap.
