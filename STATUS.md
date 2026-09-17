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
altium-sync            (asset capture, snapshot-bound patches, library ops, keyed intent elaboration; positional/E2 elaboration pending)
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
independent completeness review. Steps 2 through 9 remain outstanding.
Step 2 now has format-owned complete typed snapshots and immutable batch component
replacement. Native record fields, Data/Additional separation, global settings,
aliases and image resources remain private and intact. Cross-namespace component
replacement is rejected using the captured native library/font/style/image/alias
context; this does not replace ownership validation or reference relocation.
The sync scalar path applies explicitly addressed descriptions, designators,
component kinds and hidden-pin flags, preserves other typed fields, binds execution
to complete typed input equality,
and publishes only a validated candidate. The shared collection planner checks
original indices, partial preservation, complete order, deletion permission and
conflicting edits. Native Data/Additional ownership assembly remaps references
for designator creation/removal, preserves sibling metadata, and rejects retained
orphans and loader-incompatible ownership. Full field/collection elaboration,
saved-plan serialization, filesystem transactions and CLI migration remain ahead.
Real snapshot checks pass on Connectors, ICs and STM32F4: 223 components retain
complete typed equality through native no-op save/reopen and 223 component
replacements covering all four scalar properties, with empty replans. The ICs check exposed and fixed an unjustified pin-count cache
recalculation; the original AD26 exporter uses the stored data-object value.
See the [snapshot investigation](docs/worklogs/next-snapshots-2026-09-05.md).
Snapshot increment validation: 403 core, 168 legacy spec and 12 sync tests pass;
the CLI builds. Independent reviews passed for each implemented boundary.
Step-2 increment A2 (2026-09-16) added the read-only child projection on
`SchLibComponentSnapshot` — raw-truthful `SchOwnerGroup` classification,
`parts()` with derived swap groups (native `|&|` combined-string split/encode
helpers; untouched pins stay byte-verbatim), root-owned `parameters()`,
`graphics()` with explicit unrepresentable Symbol/Hyperlink variants,
PCBLIB-only `footprint_maps()` with a preserved non-PCBLIB count, `aliases()`,
and captured font/image/style namespace projections — plus the documented load
upgrade flooring native PartCount < 2 to 2 and DisplayModeCount < 1 to 1
(mirroring AD26's after-import event; `effective_part_count()` now always ≥ 1
on success; every floored field is reported via
`SchLib::load_normalizations()` as `SchLegacyCountNormalization` entries,
provenance metadata excluded from equality), `pub(crate)` mutation-boundary
validation helpers (font/owner part/display mode/image reference checks;
style_id authoring hard-errors as unproven), and the public snapshot-bound
address vocabulary in `altium-sync::address`. Contract authority:
[the persisted A1 resolutions memo](docs/worklogs/next-child-contracts-2026-09-16.md);
worklog: [next-children-2026-09-16.md](docs/worklogs/next-children-2026-09-16.md),
including the post-review findings resolution (Image view now carries all
intent axes incl. color/line_width/is_solid).
A2 validation (post-review fixes): 438 format, 18 sync, 362 spec-lang and 168
format-spec tests pass; the CLI builds; the three-library corpus gate passes
with empty replans.
Step-2 increment B (2026-09-16) added public batch structural editing of the
`parameters()` and `graphics()` collections on `SchLibComponentSnapshot`:
`with_children(SchChildBatch)` takes per-collection FINAL ordered sequences of
`Keep(i)`/`Replace(i, init)`/`New(init)` items resolved entirely against the
immutable input projections, applied through one `assemble_records` pass;
deletion is omission plus an explicit `allow_deletion` flag; duplicate slot
references, cross-kind Replace targets (incl. unrepresentable Symbol/Hyperlink
targets), Additional-stream creation, and cross-stream original migration are
hard errors. The documented stream-order rule places non-members verbatim and
the edited collection's finals in its members' original stream positions
(extras after the last member, or at the Data-stream end for empty
collections), so the resulting projections equal the requested sequences
exactly. New records get typed defaults, fresh UniqueIds, owner_part_id per
memo §2 (parameters −1; graphics 1..=effective via
`check_written_owner_part_id`), display mode per §4, fonts via
`check_font_id_exists`/default find-only resolution (a missing default-font
entry hard-errors pending increment D's namespace transitions), image storage
references per §8c; style_id is never authorable (§8b guard wired).
IndexInSheet per memo §6: batch-touched owners get their in-objectList
children renumbered to final 0-based sibling positions (pins count, store
nothing; field objects — Designator and root "Comment" parameters — and the
implementation chain occupy no slots); untouched owners preserve stored values
verbatim. Blank-library reconciliation: corpus evidence confirmed AD26 writes
OWNERPARTID=-1/INDEXINSHEET=-1 on root Designator/Comment records, so
`new_blank_ad26` and `with_designator` creation now write −1/−1 and the
creation path is wired through `check_written_owner_part_id`.
B review fix pass (2026-09-16): Replace with `None` optionals now PRESERVES
the original's stored display mode/font (creation defaults apply only on New);
`New` of the indexed-coordinate kinds (Polyline/Polygon/Bezier) no longer
hard-errors on the missing `LocationCount` key, with all-13-kinds creation +
save/reopen coverage; the parameter-creation missing-default-font error names
the real limitation (no authorable font field until increment D); the
IndexInSheet field-object classification is recorded as a deliberate
owner-conditional narrowing of AD26's name-first import dispatch (AD26 drops
unattached special-named parameters on save — we preserve and number them,
e.g. wild root "Description" parameters); the worklog's INDEXINSHEET=-1
evidence was corrected (−1 is carried only by Designator/Comment field
objects; ordinary root parameters carry sibling ordinals); memo errata (§6
ImplementationList serializes 0, §6 dispatch scoping, §8a ReferenceContext
fonts) are queued for increment F in the contracts worklog.
B validation (post-fix): 451 format, 18 sync, 362 spec-lang and 168
format-spec tests pass; the CLI builds; the corpus gate structural pass now
covers insert/reorder/recolor-at-exact-index, a per-component graphic reorder
round-trip, and authorized removal of both probes (parameter + rectangle) with
save/reopen typed equality and projection equality on all 223 components of
the three gate libraries.
Step-2 increment C (2026-09-16) extended the batch engine to `pins()`
(`SchChildBatch.pins` with `SchPinEdit::{Keep, Replace, New}` over the full A2
pin projection, same final-sequence/stream-order/deletion/atomicity
semantics): `SchPinInit` covers the `SchPinView` surface; Replace preserves
every unaddressed field including the sidecar-backed ones
(defined/selected functions, package length, propagation delay, frac coords,
custom text positioning, symbol line width, long descriptions, pair swap);
combined swap strings follow memo §5 rule 3 (both halves `None` → byte
verbatim incl. wild `"|&|"`, either half addressed → native split/re-encode);
new pins get typed wire defaults with NO UniqueId (the binary pin body carries
none — documented deviation) and pin owner parts accept 0 or 1..=effective
(−1 forbidden); authored `hidden_net_name` values without a verified
serialization path hard-error (`check_pin_hidden_net_name_write`).
Additional-stream pins freeze the whole collection: while any Additional pin
member exists the final pin sequence must be positional (Additional members
strictly Keep, Data members Keep or field Replace, no membership change), so
pin insert/delete/reorder — Data-only permutations included — is a hard error
(Additional pin sidecars unproven); Keep-in-place flows through. Binary pin
`owner_part_id` values beyond the signed-16 body field hard-error instead of
truncating, at serialization, at the pin mutation boundary, and as a
part-growth cap (`with_parts_appended` bounds native count at `i16::MAX`).
`all_pin_count` stays hands-off per memo §7 (verbatim through every edit; no
recompute path exists; `stored_all_pin_count()` projects it for verification).
Sidecar consistency needs no new recompute layer — `serialize_pin_sidecars`
enumerates final Data pin encounter order at save time; targeted tests prove
all nine sidecar streams stay consistent under insert/remove/reorder.
Parts ops landed on `SchLibComponentSnapshot`: `with_parts_appended(n)`
(growth never reclassifies children; composes with a following
`with_children`, whose projections are index-identical) and
`with_parts_removed(&[slots], allow_descendant_deletion)` (trailing slots
only, ≥ 1 effective part remains; part-owned children plus ownership
descendants deleted only with authorization, else the error lists them; the
memo §3 rule 4 `current_part_id` decrement is returned as a typed
`SchCurrentPartIdAdjustment` in `SchPartsShrink`, never silent; deleted
children are reported with a typed `SchRecordType` kind; native count =
effective + 1 maintained on the record and the header index entry; display
modes untouched; wild stored `current_part_id` blocks both ops until
explicitly resolved; recorded deviations from native `AddPart`/`DeletePart` —
no current-part move on append, out-of-range owners preserved verbatim where
native would renumber — live in the parts.rs module docs and the worklog).
`check_custom_display_mode_name_index` stays dead by design (no C path writes
display-mode-adjacent state) with a C→D note. New-pin creation keeps typed
wire defaults (show flags false, wire-zero formal type) — a recorded
deviation from native `SchDataPin.SetDefault` with a queued user decision.
C validation (post-fix): 465 format, 18 sync, 362 spec-lang and 168
format-spec tests pass; the CLI builds; `cargo fmt --check` clean; the corpus
gate adds a pin structural pass (insert probe pin STEP2C_{i} into part 1,
reorder, authorized removal, with pins() projection equality, save/reopen
typed equality, and verbatim AllPinCount at every stage) AND a part
structural pass (append one empty trailing slot, save/reopen, remove it with
an empty expected deletion list and the typed current-part adjustment
asserted, save/reopen, restore equality modulo reported adjustments) — all
checks true on all three gate libraries, exit 0.
Step-2 increment D1 (2026-09-16) added library-level namespace transitions:
`SchLibSnapshot::with_library_edit(SchLibEdit)` (module
`schlib/snapshot/library.rs`) is the DECLARED-transition variant of component
replacement — the strict `replace_components` path still hard-errors on any
undeclared context change. One `SchLibEdit` batch carries the final ordered
component sequence (`SchLibComponentEdit::{Keep, ReplaceWith, New}`; omission
is removal under `allow_component_removal`, listed otherwise; `ReplaceWith`
requires input-context equality, no rename, no alias change), the
context-independent new-component factory (`SchNewComponent::CloneOf` — deep
copy under a new name with fresh component UniqueId and cleared alias state —
and `::Blank` reusing the extracted `blank_ad26_component` shape, validating
font id 1), per-name alias ops (`SchAliasOp::{Add, Remove}`; removes before
adds so Remove+Add retargets; removal of an aliased component blocks unless
the same edit releases/retargets every alias), ordered image-asset ops
(`SchImageAssetEdit` wrapping the existing SchLib resource operations, plus
the memo §8c rule: references that resolved against the input asset set must
still resolve against the final set — referenced-asset removal/rename blocks,
orphan removal and wild dangling references stay legal), and typed font-table
appends (`SchFontAppend`, all native axes, positional ids, duplicate-axis and
non-sequential-table rejections). The resulting `ReferenceContext` is always
RECOMPUTED from the assembled final library (deterministic: fresh UniqueIds
never enter the context; no-op edits are context-and-state identical). The
three alias representations stay synchronized (header index aliases, root
`AliasList` comma-join for touched components, global Redirections preserved
verbatim or rebuilt in header order; self-alias storage sharing supported);
>31-char names ride the existing SectionKeys save path; header weight applies
the canonical cardinality delta to the stored value (input convention kept).
This resolves the recorded B font limitation (memo errata item 3: fonts stay
in the context; appends are declared transitions) — `with_children` errors
now point at `with_library_edit`. `SchFontEntryView` grew the style axes.
D1 validation: 470 format, 18 sync, 362 spec-lang and 168 format-spec tests
pass; the CLI builds; `cargo fmt --check` clean; the corpus gate adds a
namespace pass (alias STEP2D_0 add/remove with restore-to-input equality;
clone of component 0 under a 42-char SectionKeys name with save/reopen typed
equality and full projection-fingerprint equality vs the source minus
name/aliases; authorized clone removal restoring the original snapshot
exactly) — all checks true on all three gate libraries, exit 0.
The D1 review fix pass (2026-09-16) hardened the namespace boundary: final
lib_ref/alias uniqueness in `with_library_edit` is now checked on the
storage-namespace form (illegal-char sanitize + the CFB directory namespace's
case-insensitive fold), so case-variant names/aliases — including case-variant
"self-aliases", which the save path does not storage-share — are rejected at
the declared boundary instead of failing later inside `SchLib::save`;
`build_section_keys` became order- and case-robust (short names claim their
storage identities before any truncated key is generated; `generate_unique_key`
folds its used-set), so long-name truncation collisions save correctly in any
header order. The §8c guard was strengthened to per-pairing-target semantics
(each embedded reference resolves to its concrete input asset — first exact,
then first case-insensitive — which must survive name-intact; duplicate-name
siblings are no substitute), sharing one `resolve_image_asset_reference`
helper with the creation-time check; `SchFontAppend::native_default()` now
uses the shared validation constants. Alias `Remove` resolution releases a
removed component's alias even when a New component re-takes the name in the
same edit. Plan D item 1's sync-side `SchLibPatch` library-level op variants
were re-scoped to increment E by the 2026-09-16 orchestrator decision
(recorded in the plan and worklog; elaboration consumes the D1 format API).
Post-fix validation: 474 format, 18 sync tests pass; CLI builds; corpus gate
all-true, exit 0.
Step-2 increment D2 (2026-09-17) extended the batch engine to the footprint/
implementation chain: `SchChildBatch.footprints` with
`SchFootprintEdit::{Keep, Replace, New}` over the A2
`footprint_maps().footprints` projection (PCBLIB implementations only); each
member is the Implementation record plus its whole owned subtree, handled as
a unit, and the serialized compatibility-container chain (ImplementationList
→ Implementation → ImplementationMap → MapDefiner, ParameterList) is
preserved as serialized, never flattened. Non-PCBLIB implementations
(SIM/PCB3D/SI/…) are OUTSIDE the collection: preserved verbatim in their
original relative slots under every footprint edit including complete
replacement with deletion authorized (the red-team [12] SIM-preservation test
exists); reordering permutes only footprint slots. `SchFootprintInit` covers
model_name/description/is_current/pin_pad_maps (`Option`; `None` =
unaddressed → definers preserved verbatim on Replace); Replace preserves
everything unaddressed (unique_id, datafile links, GUIDs, stored model_type
casing, the ParameterList subtree); creation materializes the full native
chain per decompiled-source + fixture-byte evidence — shared
ImplementationList reused (created only when none exists; AD26 emits exactly
one per component unconditionally), Implementation with model_type PCBLIB,
fresh UniqueId, IndexInSheet −1, owner_part_id 0, and the native single
default datafile link (entity = model name, kind "PCBLib"), plus one
ImplementationMap and one ParameterList (both emitted unconditionally by
AD26; fixture 45/46/48 counts are 1:1:1) and one MapDefiner per pin-pad map.
MapDefiners were reclassified as objectList children (native 0-based map
ordinals via the shared memo-§6 renumbering; wild-byte verified) and
ParameterList-owned parameters as non-objectList (−1) content. Duplicate pin
designators are rejected (AD26 silently drops case-insensitive duplicate
definers on load). is_current: the native single-current-per-model-type
invariant (AD26's importer repairs it silently at load) is enforced at the
mutation boundary as a no-new-divergence rule — a batch may neither break a
coherent input nor author an active current claim into an incoherent final
state; pre-existing wild states (keyless ISCURRENT ⇒ zero currents — two of
the three gate fixtures, lucashudson-Connectors and STM32F4) pass through
verbatim when no current value is written and are resolvable in the same
batch. Additional-stream chain records freeze the
collection to the all-positional-Keep no-op (native exporter cannot emit
them; zero corpus occurrences). Footprint subtrees containing pins are hard
errors; subtree records that are graphics members conflict when both
collections are edited in one batch.
D2 validation: 481 format, 18 sync, 362 spec-lang and 168 format-spec tests
pass; the CLI builds; `cargo fmt --check` clean; the corpus gate adds a
footprint structural pass (per component: first-footprint description
replace, PROBE_STEP2D2 add with a pin-pad map from an existing pin,
authorized removal + description restore to exact input equality, with
projection equality and the non-PCBLIB count invariant at every stage) — all
checks true on all 223 components of the three gate libraries, exit 0.
Step-2 increment E1 (2026-09-17) delivered the sync-side keyed elaboration
plus the deferred D1 library-level op surface. The crate-private collection
planner is now class-aware (`CollectionClass::{Keyed, Positional}` derived
totally from `CollectionKind`): partial scope is "updates in original slots +
appends" for BOTH classes — interior insertion is a hard error at one named
enforcement point (`reject_partial_interior_insert`) and partial removal is
inexpressible by construction (`IndexedEdit::Remove` was deleted: the
ratified contract gives it no legal authorization since `delete: allow`
requires `mode: complete`, where removal is omission); complete positional
requests must pair item N with original destination N
(`PositionalCompleteMisalignment`), while keyed complete reorders freely.
The formerly dead planner surface (Insert appends, Complete/FinalItem,
`materialize`, `items`, `removed`) is production-consumed by elaboration and
altium-sync builds with zero warnings. `SchLibPatch` gained: a cached
reviewed result (computed once — creations carry fresh UniqueIds, so
recomputation would diverge from review), `from_library_ops`
(`SchLibLibraryOp`: alias add/remove, image-asset insert/replace/remove with
`CapturedImageAsset` payloads re-verified hash/path at construction AND
before every apply, font appends, component create Blank/CloneOf and
explicit by-name remove) lowering to exactly ONE `SchLibEdit` per
application, and `from_intent(input, &SchLibIntent)` — definition-scope
keyed elaboration: components keyed by exact lib_reference (partial = update
matched + append-create in source order; complete = final membership/order,
omission removes only under delete:allow with the D1 authorization flag and
auto-releases the removed aggregate's aliases; renames never inferred),
parameters keyed case-insensitively per the native
`GetParameterByName`/OrdinalIgnoreCase comparison (authored spelling wins on
update), aliases keyed exactly (membership semantics; surviving order stays
destination order per the D1 sorted-set deviation), footprints keyed by
exact model_name (`source` is provenance, never a key; `explicit_maps=false`
⇒ definers unaddressed/preserved; explicit maps elaborate positionally
through the planner incl. `materialize`; an authored maps entry addressing a
multi-pad destination definer is a hard error — native MapDefiners may carry
several pads while a pin-pad intent pair expresses one, so narrowing is never
silent, while unaddressed tails preserve all pads and complete+allow shorter
lists remove definers explicitly; `is_current` is not intent-expressible —
preserved on update, first-created-on-empty rule on create; whole-set
replacement takes two patches: empty via complete+allow, then author),
scalars via the existing FieldPolicies with Absent-never-writes on
matched AND created components. Every collection scope in a hand-built
intent must be self-consistent: non-default mode/delete without a
declaration span, or partial+`delete: allow` (contract-illegal), hard-error
at the `from_intent` boundary instead of being silently dropped. Duplicate keys on either side of a managed
collection block with both locations; unmanaged collections never touch wild
duplicates. Application order is the
D1 two-step: one library edit first, then component content replacements
captured against its result (replacement indices address the
post-library-edit sequence). Inspection surface: replacements with
before/after snapshots + `scalar_changes()` deltas, created/removed
components, alias/image/font op accessors — the exact-indexed-diff
foundation, no rendering. No-op detection at every level makes
apply→re-elaborate replan-empty.
E1 validation (post findings-fix pass 2026-09-17: multi-pad definer guard +
programmatic scope validation): 481 format, 35 sync, 362 spec-lang and 168
format-spec tests pass; the CLI builds; altium-sync builds with zero
warnings; `cargo fmt --check` clean; the corpus gate adds an intent pass (per file: partial
parameters probe STEP2E on component 0 with apply/save-reopen equality and
replan-empty; partial append of Blank STEP2E_PROBE with one parameter and one
alias, replan-empty; COMPLETE components intent built from the final library
minus the probe with delete:allow removing exactly the probe and restoring
the pre-probe state) — all checks true on all three gate libraries, exit 0.
Step-2 increment E2 (2026-09-17) completed `SchLibIntent` elaboration
(`crates/altium-sync/src/schlib/positional.rs` + the reworked `elaborate.rs`;
the E1 staging errors are gone, created components included). Pins/graphics
elaborate POSITIONALLY per owner group (memo §1/§2): `part N` → Part(N);
component scope → Part(1) single-part / SharedPins(0) multi-part for pins
(matched components crossing the single/multi boundary while managing
component-scope pins hard-error; created components map by resulting count)
and Part(1)-or-hard-error for graphics (no shared graphics owner exists
natively). Authored item K addresses FILTERED index K of its group (the
recorded B/C filtered→full mapping obligation, resolved with a multi-part
isolation test); partial scope updates the prefix + appends overflow with
owner forced to the group (append order: component scope, then parts
ascending); complete scope defines THAT group's membership/order only —
every other group, and the structurally unaddressable
PartIndependent/OutOfRange/owner-0-graphic owners, preserve verbatim.
Unrepresentable graphic KINDS (Symbol/Hyperlink/other) block a complete
scope always and a partial scope only when addressed (tail fine); cross-kind
positional replacement blocks naming both kinds. Pin fields elaborate
through their policies (location via `resolve_pin_location` with independent
x/y axis preservation; hidden_net_name is stored-or-empty — an unsatisfiable
Set hard-errors with the C rationale; swap halves per memo §5 rule 3 —
unauthored prefixes stay byte-verbatim). `inherit` on a native
pin/graphic/part field hard-errors citing step-7 SchDoc scope (layout
metadata keeps its documented inherit≡reset defaults). Parts: `part_count`
through its contextual policy; partial growth = max(destination, highest
authored); complete scope re-checks the contiguous 1..=n prefix, agreement,
≥1 slot, and lowers trailing shrink to `with_parts_removed` under
delete:allow with the typed report surfaced via
`SchLibPatch::parts_shrinks()` (removed slots, deleted descendants,
current_part_id adjustment). `PartIntent::part_swap_group` fans out over the
FINAL sequence's non-owner-0 pins of that part (destination `parts()` Mixed
projection blocks; contradicting pin-level authoring blocks; equal stored
prefixes stay byte-verbatim). Pin placements and auto-sized boxes evaluate
per owner through `evaluate_layout` with effective geometry
authored-else-destination-else-default; component-owner geometry feeds part
units; outputs override pin location/orientation and box corners before
no-op comparison (fixed point on re-elaboration). Image graphics with
authored `source:` resolve caller-captured bundles via the new
`SchLibIntentAssets` provider (`from_intent_with_assets`; `from_intent` =
empty provider; elaboration never sources bundle bytes from the filesystem —
a missing capture blocks and the captured bytes are authoritative — but it
re-verifies each required capture's path resolution and hash against disk
via `verify_unchanged`, so bundles must still exist unchanged at plan time),
lower to whole-resource insert/replace ops in the SAME library edit
that precedes component content (resource name = authored file_name else the
addressed destination image's stored name, never a basename derivation;
stored-payload equality skips the op for replan-empty; conflicting bodies
for one resolved pairing target block — the target, exact-then-
case-insensitive against the stored assets else the case-folded new-insert
name, is the resource identity, so case-variant spellings of one slot can
never emit self-overwriting duplicate ops; clear/reset never deletes
assets), and
`SchLibSnapshot::image_assets()` (altium-format) exposes the stored
name+payload store for that pairing.
E2 validation (post-review, 2026-09-17): 481 format, 54 sync, 362
spec-lang, 168 format-spec tests pass; the CLI checks;
altium-sync builds with zero warnings; `cargo fmt --check` clean; the corpus
gate adds an intent-E2 pass per file (positional partial pin rename of
component 0's first pin through its owner group with exact restore;
part-block graphic add + complete-scope removal on the first eligible
multi-part component — ICs and STM32F4 run it, Connectors records
skipped=1 with no multi-part components; part growth with a probe pin in
the new slot + complete-parts removal with the shrink report asserted
through patch inspection) — all checks true on all three gate libraries,
exit 0.
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
