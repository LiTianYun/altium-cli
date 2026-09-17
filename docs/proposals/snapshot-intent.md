# Snapshot-bound authored intent

This is the proposed semantic contract for [NEXT.md](../../NEXT.md), steps 1–2.
Existing component/import/expression syntax is reused below. The front end and
typed intent core preserve `inherit`, `reset`, collection declarations, and
supported `null` clearing, and NEXT.md step 2 (completed 2026-09-17) shipped the
SchLib elaboration of this contract (`SchLibIntent` → `SchLibPatch` over
snapshot-bound addresses); saved plans, directional planners, and CLI migration
remain step 3+. The authoritative promoted reference for the shipped SchLib
collection semantics is
[docs/spec-lang/reference/schlib-collections.md](../spec-lang/reference/schlib-collections.md);
where this proposal and that page differ, the reference page and the code win.
This document was amended in place during step-2 increment F (2026-09-17) where
step-2 native evidence contradicted its original statements; the dated evidence
trail is [the A1 contract memo](../worklogs/next-child-contracts-2026-09-16.md)
and its errata section. The legacy compiler's `Option` values, eager defaults,
and additive executor do not establish this contract.

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

Component library references, part numbers, alias names, and footprint model
names are exact parent-scoped natural keys for this contract. Parameter names
are the one keying exception (amended 2026-09-17, step-2 increment F): they
match case-insensitively (ordinal, per-character simple uppercase), because
AD26's `GetParameterByName` compares with `StringComparison.OrdinalIgnoreCase`
(`Altium.Sch.DataModel.Objects/SchDataContainer.cs:633-647`) — two case-variant
spellings are one native parameter. The authored spelling is written on update,
so a bare case-variant mention is a rename (idempotent and reviewable, never
silently ignored); mirror-intent generators must emit destination spellings.
Duplicates under a collection's matching equivalence on either side block a
managed collection, with both locations reported.
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
retain outside-scope fields while taking source order, with one exception
(amended 2026-09-17, step-2 increment F): aliases are membership-only. AD26
stores aliases as a sorted set, so alias order carries no native meaning;
surviving aliases keep destination order, authored order is honored only as
append order, and a reorder-only alias listing elaborates to an empty patch.
Positional item N uses
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
owner; source omissions never reclassify it.

New children take owner parts per the native creation evidence (rewritten
2026-09-16 per [the A1 contract memo](../worklogs/next-child-contracts-2026-09-16.md)
§2; the original "owner zero when the effective resulting part count exceeds
one or explicit part blocks exist" rule was contradicted by AD26 — no native
creation path assigns 0, and 0 is a pins-only wildcard, meaningless for
non-pin graphics):

- Children explicitly authored in `part N` use owner N, validated
  `1 <= N <= effective` at the mutation boundary.
- Component-level pins and graphics on a single-part component use owner 1
  (native-faithful: editor placement assigns the current part, which is 1).
- Component-level pins on a multi-part component use owner 0, the natively
  defined shared-pin slot (the native part filter treats 0 as a pins-only
  wildcard).
- Component-level graphics on a multi-part component are a hard error at
  elaboration: no shared graphics owner exists natively; author the graphic
  inside a part block. Creating any graphic with owner 0 or an out-of-range
  owner is a hard error at the mutation boundary.
- New root-owned parameter and designator records use owner −1, the native
  constructor sentinel.
- New children take owner-part display mode 0, the constructor default; batch
  APIs have no "current display mode" cursor.

Growing a component from one to several parts never reclassifies existing
children (native `AddPart`/`DeletePart` precedent): existing owner-one children
never move into the shared owner-zero pin slot. Positional
alignment is within the resulting source owner's collection, so changing an
authoring context cannot silently retarget children from another owner.
Independent delete/insert patches cannot shift another patch's target. Graphics of
different kinds remain in one ordered graphic collection, not separate type buckets.
A type-changing positional replacement preserves only fields with an explicit
cross-kind typed mapping; if an outside-scope field cannot be preserved, planning
must block rather than discard it.

## Creation metadata, load normalization, and reference namespaces

These rules were resolved during NEXT.md step 2. The dated evidence is
[the A1 contract memo](../worklogs/next-child-contracts-2026-09-16.md); its
errata section marks the corrections already folded into this section, and the
memo itself stays verbatim as evidence.

**IndexInSheet.** A new in-objectList child is written with the 0-based
position in its owner's final child sequence (pins and Additional-destined
subtree roots count as siblings). Component field objects — the designator and
the root parameter named `Comment` — serialize −1. The synthetic
ImplementationList wrapper serializes wire 0, not −1: its constructor calls
`SetIndexInSheetForSave(0)` (`Objects/SchDataImplementationList.cs:9-14`), and
gate-fixture bytes show bare `|RECORD=44` blocks with the key absent
(absent ≡ 0). When a batch structurally touches an owner's child sequence, all
of that owner's in-objectList children are renumbered to final 0-based sibling
positions; untouched owners preserve stored values verbatim — a deliberate
deviation from AD26's renumber-everything-on-save that keeps no-op save
equality. AD26's import attach (`SchDataParameter.UpdateOwner`,
`Objects/SchDataParameter.cs:128-196`) dispatches on parameter NAME alone
(DefaultNet / HiddenNetName / PinSelectionMemory / Comment / Description /
PinUniqueId) with owner-interface guards inside the branches; a special-named
parameter whose owner lacks the matching interface (for example a root
`Description` — `SchDataComponent` is not an `ISchDataDescriptionOwner`)
attaches nowhere and AD26 silently drops it on its next save. We never
replicate that drop: only owner/name combinations AD26 actually consumes (root
`Comment`; the pin-owned specials) are exempt from sibling numbering, and
preserved records AD26 would drop are numbered as ordinary siblings.
IndexInSheet is never an address, never identity, and never validated at
capture.

**Load normalization (reported, never silent).** Native `PartCount < 2`
(including wire-absent) floors to 2 and `DisplayModeCount < 1` floors to 1 at
SchLib load, mirroring AD26's after-import event; every floored field is
reported via `SchLib::load_normalizations()`. `CurrentPartID` is never touched
at load — no native clamp exists — and a stored value outside `[1, native)`
blocks part mutations until explicitly resolved, never a silent clamp.

**Fonts.** Every font id our APIs write must reference an existing font-table
entry; new text-bearing children resolve the native default (Times New Roman,
size 10) find-only against the captured table. Fonts are part of the captured
`ReferenceContext`; a font-table append is a library-level namespace transition
declared via `SchLibEdit::font_appends`, which recomputes the resulting
snapshot's context. (This corrects the memo §8a rule-3 parenthetical "fonts are
not part of ReferenceContext".)

**style_id.** Authoring or modifying style_id references or style tables
through any step-2 API is a hard error with context: the managed V5 format
neither reads, writes, nor validates StyleID, and the native (Delphi)
style-table implementation has not been reverse engineered. Loaded values are
preserved verbatim. Upgrade path: ghidra-cli decompilation (project altium26)
of the Delphi binary consuming STYLEID/STYLEIDCOUNT on the SchLib load/save
path — specifically its import lookup, bounds handling, and dangling-id
behavior — or discovery of a managed reader.

**Embedded images.** A created or retargeted embedded image record must
reference a storage asset that is either preexisting or inserted by a PRIOR
library-level image-asset op whose result snapshot the component edit is taken
from — component content edits run against a snapshot's captured context, so
"inserted by the same batch" is not expressible; same-edit composition applies
only to new components created within the library edit itself. (This corrects
the memo §8c rule-2 "same batch's library-level image-asset op" wording.)
Intent elaboration folds the two steps into one patch: asset ops ride the
single library edit that precedes component content. Removing or renaming an
image record's concrete pairing target (first exact, then first
case-insensitive name match — the native pairing order) is blocked while the
record survives; orphan assets and wild dangling references stay legal and are
preserved verbatim.

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
API. The new intent compiler rejects those properties. Named pen widths and exact
physical widths of 0/10/30/50mil are supported, with that table now verified against
the original AD26 managed assembly. Font defaults require validation against the
destination font table; font-table appends are a library-level namespace
transition declared via `SchLibEdit::font_appends`, and fonts are part of the
captured `ReferenceContext` (see the reference-namespace section above).
Other creation policy choices remain checked against
current Rust code, without independent AD26 verification of every default.

The old `image_data` compiler field has no corresponding public image-resource
mutation boundary and is ignored by its executor. The replacement intent compiler
rejects it. The typed replacement shipped in step 2: image graphics author a
`source:` bundle reference, elaborated through caller-captured asset bytes into
whole-resource library-level ops on the same patch's library edit, under the
two-step embedded-image reference rule above. Codec decoding stays outside
scope per the image-asset boundary in [NEXT.md](../../NEXT.md).
