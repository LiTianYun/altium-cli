# SchLib collection semantics

Authoritative reference for the shipped SchLib keyed/positional alignment,
owner-group scoping, deletion permission, load normalizations, and
unsupported-content hard errors — the NEXT.md step-2 surface. Promoted
2026-09-17 (step-2 increment F) from
[the A1 contract memo](../../worklogs/next-child-contracts-2026-09-16.md)
(dated evidence; its errata corrections are folded in here) and the
[step-2 worklog](../../worklogs/next-children-2026-09-16.md).

Implementation (validate this page against the code; the code wins):

- Projections and owner groups: `crates/altium-format/src/schlib/snapshot/views.rs`
  (`SchOwnerGroup`, `parts()`, `pins()`, `parameters()`, `graphics()`,
  `aliases()`, `footprint_maps()`).
- Structural editing: `crates/altium-format/src/schlib/snapshot/children.rs`
  (`with_children`), `snapshot/parts.rs` (`with_parts_appended`,
  `with_parts_removed`), `snapshot/library.rs` (`with_library_edit`);
  mutation-boundary checks in `crates/altium-format/src/schlib/validation.rs`.
- Class-aware planning: `collection_class` and `CollectionClass` in
  `crates/altium-sync/src/collection.rs`.
- Intent elaboration: `crates/altium-sync/src/schlib/elaborate.rs` and
  `crates/altium-sync/src/schlib/positional.rs` (elaboration's owner-group
  scope targeting lives in `positional.rs`). The snapshot-bound address
  vocabulary in `crates/altium-sync/src/address.rs` is landed and
  unit-tested but step-3-facing: elaboration does not consume it yet (saved
  plans are its intended consumer; its module docs record the divergence
  risk against `positional.rs`).

The intent syntax itself is specified in
[snapshot intent syntax](../language/snapshot-intent.md) and the semantic
contract in
[the snapshot-intent proposal](../../proposals/snapshot-intent.md); block-level
authoring forms are in [`schlib` blocks](../blocks/schlib.md).

## Collection classes and keys

| Collection | Owner | Class | Key / alignment rule |
| --- | --- | --- | --- |
| `components` | library | keyed | exact `lib_reference`; renames are never inferred (rename = delete + create under deletion permission) |
| `parts` | component | keyed (structural) | numeric slot `1..=effective`; the projection contains every slot including empty ones; complete scope must describe a contiguous prefix starting at 1 |
| `parameters` | component | keyed | name, **case-insensitive** (ordinal per-character simple uppercase — AD26 `GetParameterByName` uses `StringComparison.OrdinalIgnoreCase`, `SchDataContainer.cs:633-647`); the authored spelling is written on update, so a bare case-variant mention is a rename; root-owned records only (implementation-owned parameters are outside the collection) |
| `aliases` | component | keyed | exact string; **membership-only** — AD26 stores a sorted set, so order is never authored; surviving aliases keep destination order, authored order is append order, reorder-only listings elaborate to empty |
| `footprints` | component | keyed | exact `model_name`; membership = PCBLIB implementations only (case-insensitive model type); `source:` is provenance, never a matching key; non-PCBLIB implementations (SIM/PCB3D/SI/…) are outside-collection preserved content |
| `pins` | component / part | positional | per owner-part group: authored item K addresses filtered index K of the scope's owner group |
| `graphics` | component / part | positional | per owner-part group; one ordered collection across all graphic kinds, never per-kind buckets |
| `maps` | footprint | positional | pin-pad definer order within one footprint; one authored pair per definer |

**Swap groups are not a collection.** All swap state is per-pin fields:
`SwapIdPin`, `PairSwapID`, and the combined `SwapIdPart` prefix optionally
followed by `"|&|" + SwapIdPartPin` (`SWAP_ID_PART_INFO_SEPARATOR`,
`crates/altium-format/src/sch_records.rs`; native
`SchPin.cs:31 SwapIdPartInfoSeparator`). The part-level `part_swap_group` is a
derived projection — the unanimous prefix across the part's non-owner-0 pins
over all display modes; non-unanimous values project a typed `Mixed` state —
and a part-level write fans out to every non-owner-0 pin of that part,
preserving each pin's own suffix. Writing against a `Mixed` projection is a
hard error; untouched pins keep their stored combined string byte-verbatim
(wild encodings, bare separators, and mojibake separators included — no
repair exists). A pin-level authored `part_swap_group` contradicting its
part's fan-out target is a hard error; equal values compose.

## Owner-group mapping

Children group by the raw native `owner_part_id` (`SchOwnerGroup` in
`views.rs`): `PartIndependent` (−1), `SharedPins` (0 — natively a pins-only
wildcard), `Part(n)` for `1..=effective`, `OutOfRange` for everything else.
Classification is total and raw-truthful; nothing is repaired or filtered at
capture. Matching is by raw owner id, so growth reclassifies a previously
`OutOfRange(n)` group into `Part(n)` without touching any record.

Scope-to-group mapping for authored intents (single-part = effective count 1,
i.e. native `PARTCOUNT` 2):

| Authoring scope | Pins | Graphics |
| --- | --- | --- |
| `part N` block | `Part(N)` (`1 <= N <= effective`) | `Part(N)` |
| component scope, single-part | `Part(1)` | `Part(1)` |
| component scope, multi-part | `SharedPins` (owner 0, the native shared-pin slot) | **hard error** — no shared graphics owner exists natively; author the graphic inside a part block |

Boundary-crossing error: a matched component whose intent changes the
effective count across the single/multi-part boundary while managing
component-scope pins is a hard error — the mapping would silently retarget the
authored items between `Part(1)` and `SharedPins` mid-patch. Created
components map by the resulting count alone. Two scopes addressing one group
(component scope plus `part 1` on a single-part component) are a hard error.

`PartIndependent` (root Designator/Parameter text), `OutOfRange` owners, and
owner-0 graphics can never appear inside a targetable group's filtered
projection: their preservation is structural, byte-verbatim through any patch.

New-child owner assignment follows the same mapping (memo §2, native
evidence): `part N` children get owner N; component-level pins/graphics on a
single-part component get owner 1; component-level pins on a multi-part
component get owner 0; new root parameter/designator records get owner −1
(constructor sentinel); new children get owner-part display mode 0. Part
growth never reclassifies existing children.

## Partial and complete scope per class

Without a declaration a collection is unmanaged (partial, deletion denied) and
elaboration never touches wild content in it. Declarations must be
self-consistent: non-default mode/deletion without a declaration span, and the
contract-illegal `partial` + `delete: allow` combination, hard-error at the
`from_intent` boundary.

**Keyed partial**: matched entries update in their original slots; unmatched
authored entries append (components: create) in source order; omission
preserves. A sparse Replace index set is a legitimate product of keyed
matching; interior insertion is not expressible.

**Keyed complete**: authored items define final membership and order (free
reorder); omission removes only under `delete: allow`, and removal is
expressed by omission — there is no partial-removal form at all
(`IndexedEdit::Remove` does not exist).

**Positional partial**: authored item K addresses filtered index K of the
scope's owner group; the tail is preserved; overflow items append with the
owner forced to the group's id (append order: component scope first, then
parts ascending). Interior insertion and sparse index selection are hard
errors at one named enforcement point (`reject_partial_interior_insert`,
`collection.rs`).

**Positional complete**: defines that owner group's final membership and order
only; item N must pair with original destination N for preserved fields
(`PositionalCompleteMisalignment` otherwise — substitution in place and
appends are legal, re-pairing is not); every other group keeps its slots
verbatim.

**Parts (structural)**: partial growth = resulting effective count is
`max(destination, highest authored part number)`; an explicit `part_count`
must accommodate it (lowering under partial is an error). Complete scope
re-checks the authored numbers as the contiguous prefix `1..=n`, requires
`part_count` agreement and `n >= 1`, and authorizes trailing shrink only under
`delete: allow` — which is also the descendant-deletion authorization. Shrink
reports removed slots, every deleted descendant, and the `current_part_id`
adjustment (per removed slot, decrement only when > 1 — the native
`DeletePart` rule) through `SchLibPatch::parts_shrinks()`; growth composes
before child batches, shrink's outcome snapshot is the input of any following
batch (never fused with pre-deletion indices). Two batch-door collection
invariants are mirrored on the shrink door and are not curable by the
deletion authorization: the Additional-stream pin freeze and the
single-current footprint no-break rule (see the hard-error table).

**Maps**: `explicit_maps == false` means unaddressed — definers preserved
verbatim (including wild multi-pad definers and wild base-field state); an
explicit maps body elaborates positionally (partial prefix + preserved tail;
complete final list, shrink under `delete: allow`); a final list equal to the
destination collapses to no rebuild.

## Deletion permission

Deletion is never implicit. At the intent boundary the only legal
authorization is `mode: complete, delete: allow`, and removal is omission from
the complete listing. At the format boundary every deleting operation carries
its own explicit flag (`allow_deletion` on `SchChildBatch`,
`allow_component_removal` on `SchLibEdit`, `allow_descendant_deletion` on
`with_parts_removed`); unauthorized deletion hard-errors listing every doomed
member (index, kind, owner). Removing a component releases its aliases in the
same intent-elaborated library edit (the removal authorization covers the
aggregate); the imperative `from_library_ops` surface instead requires
explicit alias release. Deleting a record that still owns retained dependents
is an assembly orphan error. Asset lifecycle is separate: clearing an image
`source` never deletes a storage asset; asset removal is an explicit
library-level op, blocked while any record's concrete pairing target it is.

## Unsupported-content hard errors

Every class below fails fast with context (component, collection, index/key)
and loads/preserves the wild state verbatim when merely present — validation
runs at the mutation/elaboration boundaries, never at capture:

| Class | Rule | Where enforced |
| --- | --- | --- |
| `style_id` | Authoring or modifying style_id references or style tables is a hard error ("style_id semantics unproven — native (Delphi) style-table implementation not yet reverse engineered"); preservation is verbatim. Upgrade path: ghidra decompilation of the Delphi STYLEID consumer. | `check_style_id_write`, `validation.rs` |
| Additional-stream pins | While any pin member lives in the Additional stream, the pin collection is frozen positional: strict Keep of Additional members, Keep or field-Replace of Data members, no membership change; insert/delete/reorder hard-error (Additional pin sidecars unproven). The freeze holds through every door: a part shrink whose deletion set contains a pin hard-errors under the same condition, regardless of deletion authorization. | `children.rs`; `parts.rs` (shrink mirror) |
| Additional-stream implementation chains | While any chain-kind or member-subtree record lives in the Additional stream, the only accepted footprints edit is the all-positional-Keep no-op. | `children.rs` |
| `hidden_net_name` | The binary pin body has no field and the pin-owned Parameter storage form is preserved-only, so only the stored value or the empty default may be written; anything else hard-errors (an unsatisfiable intent `Set` errors at elaboration with the same rationale). | `check_pin_hidden_net_name_write`, `validation.rs`; `elaborate.rs` |
| Multi-part component-level graphics | No shared graphics owner exists natively (owner 0 is a pins-only wildcard): component-scope graphics on a multi-part component hard-error at elaboration; creating any graphic with owner 0 or out-of-range owner errors at mutation. | `positional.rs`; `check_written_owner_part_id`, `validation.rs` |
| Multi-pad definer addressing | Native MapDefiners may carry several pads while a pin-pad intent pair expresses exactly one; an authored maps entry positionally addressing a multi-pad destination definer hard-errors naming footprint/position/pin/pad count. Unaddressed tails preserve all pads; complete + `delete: allow` with a shorter list removes one explicitly. | `elaborate.rs` |
| Symbol / Hyperlink / other unrepresentable graphic kinds | Always block a complete scope over their group; block a partial scope only when the authored prefix would address one (tail preservation is fine); cross-kind positional replacement blocks naming both kinds. | `positional.rs`, `children.rs` |
| Owner-0 pins on single-part components | Unaddressable by any scope (component scope maps to `Part(1)` there); preserved-only until a dedicated shared-pin address form exists. Tools must record the skip, never silently pass. | `positional.rs` (recorded consequence) |
| Instance-scope `inherit` | `inherit` on a native pin/graphic/part field at definition scope hard-errors citing NEXT.md step 7 (it resolves against an imported instance base, which SchLib definition elaboration does not have). Exceptions with documented contracts: layout metadata (inherit ≡ reset ≡ versioned default), `part_count`'s contextual policy, and the four component scalars (inherit ≡ reset — flagged for a consistency decision). | `elaborate.rs` |
| Boundary crossing / scope conflicts | Single/multi-part boundary crossing while managing component-scope pins; two scopes addressing one owner group; inconsistent programmatic scope declarations. | `elaborate.rs`, `positional.rs` |
| Wild `current_part_id` | A stored value outside `[1, native)` blocks part mutations until explicitly resolved — never a silent clamp. | `parts.rs` |
| Swap `Mixed` projection | Part-level swap-group writes against a non-unanimous pin prefix state block; no silent overwrite of a distinction the file holds. | `views.rs` projection; `positional.rs` fan-out (`part_swap_fanouts` / `apply_part_swap_fanouts`) |

## Load normalizations

Exactly one class of load-time normalization exists, mirroring AD26's
after-import event, and it is always reported, never silent
(`SchLib::load_normalizations()`, returning typed
`SchLegacyCountNormalization` entries excluded from document equality):

- native `PartCount < 2` (including wire-absent) floors to 2, on both the
  component record and the write-only header index entry;
- `DisplayModeCount < 1` floors to 1, with `custom_display_mode_names` padded
  to the count so the upgraded state is a save/reopen fixed point.

`CurrentPartID` is never touched at load (no native clamp exists). Everything
else is capture-forbidden: out-of-range owners (the corpus carries records
with `OWNERPARTID == PARTCOUNT`), wild swap encodings, dangling font/style/
image references, and orphan storage assets load and round-trip verbatim.

## `is_current` (footprints)

The native invariant is one current implementation per model type; AD26's
importer silently repairs wild states at load
(`ValidateImplementationsCurrentState` runs on the SchLib V5 import path). Our
rules:

- Capture is verbatim — no load repair, no report (two of the three gate
  fixtures store no `ISCURRENT` key anywhere and must round-trip unchanged).
- The mutation boundary blocks only NEW divergence: a batch may neither break
  a coherent input's single-current state nor author an active current claim
  into an incoherent final state; pre-existing wild states pass through when
  no current value is written, and are resolvable in the same batch by
  authoring a coherent assignment. The no-break half is mirrored on the
  part-shrink door (`parts.rs`): a shrink whose deletion set would leave a
  coherent input's retained PCBLIB implementations without exactly one
  current member hard-errors.
- `is_current` is not intent-expressible: matched footprints preserve the
  stored value; a created footprint is current only when the destination
  PCBLIB collection was empty and it is the first authored one. Whole-set
  replacement therefore takes two patches: empty the collection
  (complete + `delete: allow`, zero entries), then author the new set.
