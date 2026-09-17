# Step-2 child collections worklog — 2026-09-16

Non-authoritative worklog for the step-2 child-collection increments (plan
`/tmp/step2/plan.md`, baseline 436fa3c). Contract authority for these
increments is the persisted A1 resolutions memo,
`docs/worklogs/next-child-contracts-2026-09-16.md` (cited below as "memo §N").

## Increment A2 — projection, addresses, boundary validation (implemented)

### Public read-only child projection (altium-format)

New module `crates/altium-format/src/schlib/snapshot/views.rs`, exported
through `schlib`, `api`, and the snapshot module:

- `SchOwnerGroup` — raw-truthful owner classification per memo §1:
  `PartIndependent` (−1), `SharedPins` (0; natively a pins-only wildcard),
  `Part(n)` for `1..=effective`, `OutOfRange(raw)` for everything else
  (including negatives below −1). `classify(owner_part_id, native_part_count)`
  is total; nothing is repaired or filtered at projection time.
- `SchLibComponentSnapshot::parts()` — effective slots `1..=effective` with a
  derived `SchPartSwapGroup` per memo §5 rule 1: the unanimous `SwapIdPart`
  prefix of the part's non-owner-0 pins across all display modes; typed
  `None`/`Group`/`Mixed` states (unanimous empty prefix and no-eligible-pins
  both project `None`).
- `pins()`, `parameters()`, `graphics()`, `aliases()`, `footprint_maps()`,
  plus minimal namespace projections `captured_fonts()` (id/name/size),
  `captured_image_asset_names()`, `captured_display_styles_present()`.
  - `parameters()` = ROOT-owned Parameter records only (owner resolves to the
    component root); implementation-owned parameters are excluded even under
    name collision (test:
    `parameters_are_root_owned_only_excluding_implementation_owned_same_name`).
  - `graphics()` = one ordered collection including ALL graphic-like kinds;
    `Symbol`, `Hyperlink`, and any other non-child-collection kind appear as
    explicit intent-unrepresentable variants (`is_intent_representable()`).
  - `footprint_maps()` = PCBLIB implementations only (case-insensitive model
    type, matching the existing api layer), flattened through the native
    44→45→46→47 chain, with an explicit
    `preserved_non_pcblib_implementations` count.
  - Views borrow strings/slices and copy small scalars; no native record type
    appears in any public signature.
- Child encounter order is Data record order followed by Additional record
  order. The plan text said "Data encounter order"; Additional-stream children
  are included because they exist in the typed model (programmatically today)
  and hiding them from the ordered projection would misreport membership. The
  corpus carries zero Additional-stream records, so both readings coincide on
  every real fixture.

### Swap combined-string helpers (memo §5 rule 3)

`crates/altium-format/src/sch_records.rs`: the Rust `SchPin::swap_id_part`
field holds the raw COMBINED native string (verified against
`SchPin.cs:31,152-175` and `SchDataExporterLibraryV5.cs:393-409` — the
PinWideText `SwapIDPart` key carries `GetSwapIdPartAndPartPin()`). Pure
helpers `split_swap_id_part_and_part_pin` / `encode_swap_id_part_and_part_pin`
implement the native first-ordinal-`|&|` split and the emit-separator-only-
with-nonempty-suffix concat. Split distinguishes `None` (no separator) from
`Some("")` (literal separator, empty half) so untouched pins are never
re-encoded; mojibake separators (`?&?…`) parse as ordinary prefixes (memo §5
rule 4). Tests: `swap_split_and_encode_follow_native_rules`,
`literal_separator_swap_strings_survive_projection_and_save_reopen_verbatim`.
The `"|&|"` constant is local to altium-format for now — FLAGGED for promotion
into `altium-format-types::constants::pin` after user discussion.

### Load normalization (memo §3 rule 1, §4 rule 1)

`upgrade_legacy_part_display_counts` in `crates/altium-format/src/schlib.rs`,
applied once per component at the end of `SchLib::parse_from_cfb`, documented
as a format upgrade mirroring AD26 `SchComponent.HandleAfterImportEvent`:
native `PartCount < 2` (including wire-absent) floors to 2 on both the record
and the write-only header index entry; `DisplayModeCount < 1` floors to 1 with
`custom_display_mode_names` padded to the count so the upgraded state is a
save/reopen fixed point. `CurrentPartID` is untouched (memo §3 rule 3 — no
native clamp exists). The upgrade is REPORTED, never silent (memo §3 rule 1 /
§4 rule 1): every floored field is recorded as a
`SchLegacyCountNormalization { lib_reference, field, before, after }` entry
queryable via `SchLib::load_normalizations()` (fields: `PartCount`,
`HeaderIndexPartCount`, `DisplayModeCount`, `CustomDisplayModeNames`). The
report is provenance metadata: excluded from `SchLib` equality (manual
`PartialEq` with a compile-guarding destructure) so the upgraded state remains
a save/reopen fixed point, and never serialized. The corpus has zero such
files; synthetic coverage in `legacy_part_and_display_counts_upgrade_at_load`
asserts the exact report for legacy files, its emptiness after re-save, and
its emptiness for modern counts. `new_blank_ad26` now writes
`DisplayModeCount = 1` with one empty custom name (a native-faithful blank —
AD26 floors on load — and required for blank fixed-point equality).
`effective_part_count()`'s contract is now "success is always ≥ 1"; the error
path was NOT removed because it is not dead — programmatic in-crate state can
still bypass the load upgrade, and that state hard-errors (never a silent
clamp). The old `part_count=1 → Ok(0)` behavior is gone; test updated.

### Mutation-boundary validation helpers (memo §1/§2/§4/§8)

New `pub(crate)` module `crates/altium-format/src/schlib/validation.rs`,
staged for the B/C/D engines and NOT wired into capture/parse paths:

- `check_font_id_exists` (§8a rule 2; now also the production check inside
  `with_designator` creation, replacing the inline equivalent) and
  `find_or_create_default_font` (Times New Roman 10, rotation 0, no styles;
  §8a rule 3; refuses non-sequential font tables rather than renumbering).
- `check_written_owner_part_id` with `WrittenOwnerPartClass::{Pin, Graphic,
  RootText}` (§1/§2: pins accept 0 or `1..=effective`; graphics accept
  `1..=effective` only; root text accepts −1 only).
- `check_written_display_mode` and `check_custom_display_mode_name_index`
  (§4 rules 3/5). Rule 4 (shrink must not orphan) needs batch context and
  lands with the B/C engines.
- `check_embedded_image_reference` (§8c rule 2, check-only; exact then
  case-insensitive match mirroring the native pairing pass; external-path
  images exempt).
- `check_style_id_write` — hard error with the §8b message ("style_id
  semantics unproven — native (Delphi) style-table implementation not yet
  reverse engineered") for any authored style change; verbatim preservation
  passes.

Capture stays permissive: regression test
`wild_owner_values_snapshot_fine_and_classify_raw_truthfully` snapshots a
component carrying the corpus wild class (`OWNERPARTID == native PARTCOUNT`)
plus part-0 pins and out-of-range classification is projected, not rejected.

### altium-sync address vocabulary (plan A2 §1)

New public module `crates/altium-sync/src/address.rs`:
`SchLibChildAddress { component, owner: SchLibChildOwner, collection:
altium_spec_lang::ast::CollectionKind, index }` resolving against a
`SchLibSnapshot` into `ResolvedSchLibChildAddress` with validated newtypes
(`ComponentIndex`, `ChildIndex`, `FootprintIndex`). The owner path encodes the
part slot per memo §1: `ComponentScope` maps pins to `Part(1)` (single-part)
or `SharedPins` (multi-part); component-scope graphics on a multi-part
component are a hard error ("author the graphic inside a part block", memo
§2); `Part(n)` requires `n ≤ effective`; `Maps` nest under a
`Footprint(index)` owner. Indices bind into the per-owner-group filtered
projection and to the exact input snapshot (typed-state equality). Tests cover
single-part resolution end to end and the pure owner-group mapping for the
multi-part cases (the public API cannot construct a multi-part fixture yet).

### Verification (all default suites, fixture-free)

- `cargo test -p altium-format --lib` — 437 passed, 0 failed.
- `cargo test -p altium-sync` — 18 passed, 0 failed (6 pre-existing
  `collection.rs` dead-code warnings, documented in the A2 code maps).
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- Corpus gate: `cargo run -p altium-sync --example check_schlib_snapshots --
  data/schlib/lucashudson-Connectors.SchLib data/schlib/ICs.SchLib
  data/schlib/arthurbenemann-STM32F4.SchLib` — all checks true for all three
  files, exit 0.

### Known items deferred / flagged out of A2

- `with_designator` creation and `new_blank_ad26` still write
  `owner_part_id = 0` on new root Designator/Comment records; memo §2 says new
  root text takes −1. Pre-existing behavior, owned by increment B (creation
  rules) — changing it now would alter serialized blank output outside A2
  scope.
- SchLib Data streams do not reopen `Hyperlink` (RECORD=226) records
  (pre-existing dispatcher gap; zero corpus occurrences). The projection
  handles in-memory Hyperlinks; parser completeness is a separate item.
- The SchDoc load path does not apply the §3/§4 count floors (AD26's
  after-import event is document-wide); step-2 scope is SchLib.
- `"|&|"` separator constant and any new domain types await user discussion
  before entering `altium-format-types`.

## Increment A2 — review findings resolution (2026-09-16)

Independent review returned 7 findings (two reviewers duplicated the
load-normalization one). Dispositions, each verified against the code first:

1. **Image graphic view omitted `color` / `line_width` / `is_solid`
   (major) — CONFIRMED, FIXED.** The intent policies
   (`altium-spec-lang/src/intent/graphics.rs` `field_policies`) mark all three
   authorable for Image, and the native `SchImage` record carries them;
   `SchGraphicDetail::Image` projected only geometry/asset fields. Added the
   three fields to the variant and the `graphic_view` projection; new test
   `image_graphic_projects_all_intent_axes` asserts every Image axis
   end-to-end through snapshot projection. The other twelve representable
   kinds were re-checked axis-by-axis against `field_policies` during review
   and are complete.
2. **Load normalization was silent; memo §3 rule 1 / §4 rule 1 mandate a
   REPORTED normalization (major, reported twice) — CONFIRMED, FIXED** by
   adding the reporting surface now rather than deferring: public
   `SchLegacyCountNormalization` / `SchNormalizedCountField` (in
   `altium_format::schlib`, re-exported via `api`), populated by
   `upgrade_legacy_part_display_counts` for every field it changes and
   queryable via `SchLib::load_normalizations()`. See the amended "Load
   normalization" section above for the equality-exclusion rationale. This
   also un-silences the acknowledged deviation-6 half (a header index entry
   floored while the record was already modern now yields a
   `HeaderIndexPartCount` entry). E's report expansion (memo consumption
   table) can consume this surface instead of inventing one.
3. **`cargo fmt --check` failures (minor) — CONFIRMED, FIXED.** Violations
   were confined to A2-touched files (`schlib.rs`, `snapshot/views.rs`,
   `validation.rs`); `cargo fmt -p altium-format`/`-p altium-sync` applied,
   `--check` now clean on both crates, no unrelated files touched.
4. **Blank-creation `owner_part_id = 0` on root Designator/Comment vs memo §2
   `−1`, interlocked with `check_written_owner_part_id` (minor) — CONFIRMED,
   DEFERRED TO B (reviewer's own recommended fix).** Binding obligation
   recorded here for increment B: change the creation default to −1 in the
   SAME change that wires `check_written_owner_part_id` into
   creation/replacement flows (the helper rejects 0 for
   `WrittenOwnerPartClass::RootText`, so wiring it without changing the
   default would make blank-created libraries fail their own validation),
   with a blank-library save/reopen test covering the new bytes and the
   `parameters()` owner-group projection (`PartIndependent`).
5. Duplicate of 2 — same fix.
6. **`"|&|"` constant lives in altium-format, not
   `altium-format-types::constants` (minor) — CONFIRMED, STANDS AS FLAGGED.**
   Plan A2 forbids unilateral types-crate additions; the promotion (to
   `constants::pin`, citing `SchPin.cs:31` `SwapIdPartInfoSeparator`) requires
   the user discussion already flagged above and must be resolved before
   increment C consumes the swap helpers. Not resolvable inside this fix pass.
7. **`usize::try_from(effective).unwrap_or(0)` silent-default conversion in
   `address.rs` (minor) — CONFIRMED, FIXED.** Replaced with an `expect`
   naming the broken invariant (`effective_part_count()` guarantees ≥ 1 on
   success), so an impossible state surfaces as the internal bug it is
   instead of being mislabeled `IndexOutOfRange` on a count of 0.

Post-fix verification: `cargo test -p altium-format --lib` — 438 passed,
0 failed, 0 warnings; `cargo test -p altium-sync` — 18 passed, 0 failed;
`cargo check -p altium-cli` — clean; `cargo fmt --check` clean on
altium-format and altium-sync; corpus gate re-run — all checks true for all
three files, exit 0.

## Increment B — structural editing of parameters + graphics (implemented)

Contract authority: memo §1/§2/§4/§6/§8 (cited as "memo §N"). New module
`crates/altium-format/src/schlib/snapshot/children.rs`, exported through
`schlib`, `api`, and the snapshot module.

### API shape

- `SchLibComponentSnapshot::with_children(SchChildBatch) -> Result<Self>` —
  one batch over both collections, applied through a SINGLE `assemble_records`
  pass; the input snapshot is immutable and errors leave no partial state.
- `SchChildBatch { parameters: Option<Vec<SchParameterEdit>>, graphics:
  Option<Vec<SchGraphicEdit>>, allow_deletion: bool }` — `None` leaves a
  collection untouched; `Some(vec![])` deletes every member (authorization
  required).
- `SchParameterEdit::{Keep(usize), Replace(usize, SchParameterInit),
  New(SchParameterInit)}` and `SchGraphicEdit::{Keep, Replace, New}` with
  `SchGraphicInit { owner_part_id, owner_part_display_mode: Option<i32>,
  shape: SchGraphicShape }`. `SchGraphicShape` mirrors the 13 representable
  `SchGraphicDetail` kinds field-for-field (owned data; angles as `f64`;
  `font_id: Option<i32>` on Label/TextFrame). `SchParameterInit` =
  name/text/is_hidden. `SchParameterInit::from_view` /
  `SchGraphicInit::from_view` copy the addressable surface out of the A2
  views (`from_view` errors for unrepresentable kinds and for owner groups
  other than `Part(n)`).
- OPTIONAL-FIELD SEMANTICS (post-B review fix): `owner_part_display_mode:
  None` and `font_id: None` (Label/TextFrame) mean "unaddressed" — on `New`
  they resolve the creation defaults (mode 0; the native default font found in
  the existing table), on `Replace` they PRESERVE the original record's stored
  value verbatim (wild out-of-range values included; nothing is validated for
  a value we do not write). An absent optional never resets a stored value.
  `from_view` populates both explicitly. Test:
  `replace_with_unaddressed_optionals_preserves_display_mode_and_font`.
- INDEX SPACE DECISION: `Keep`/`Replace` indices are positions in the FULL A2
  projections of the input snapshot (`parameters()[i]` / `graphics()[i]`),
  because deliverable 2's projection-equality gate is stated over those views
  and one final sequence per collection needs one index space. This deviates
  from the plan's "per-owner-part addressed per A2 rule" wording; the
  deviation is flagged for orchestrator/user acceptance. BINDING OBLIGATION ON
  INCREMENT E: sync-side per-owner-part positional addresses MUST be resolved
  through the owner_group-filtered → full-projection mapping (the views'
  `owner_group` field gives the injective mapping), with a test proving the
  mapping on a multi-part component. **RESOLVED by increment E2
  (2026-09-17):** `crates/altium-sync/src/schlib/positional.rs` resolves
  every positional pin/graphic address through exactly that mapping, proven
  on one multi-part component across two tests:
  `multi_part_positional_edits_touch_only_their_group` (part-2 and shared
  owner-0 group edits, part 1 asserted untouched by fingerprint) and
  `complete_scope_defines_only_its_group` (a part-1 complete-scope edit,
  parts 2 and shared asserted untouched); see the E2 section below.
- Rejections (all hard errors with component/collection/index context):
  duplicate references to one original slot (Keep+Keep, Keep+Replace, …);
  out-of-range indices; omission without `allow_deletion` (message lists the
  omitted member indices); Replace target kind ≠ replacement kind (no
  cross-kind mapping — includes any Replace of Symbol/Hyperlink/Other);
  deleting a record that still owns retained dependents (assembly orphan
  check); creating records in the Additional stream; final arrangements that
  would migrate an original between streams (assembly check). Child Component
  insertion is not expressible through the API surface at all (and stays
  rejected inside `assemble_records`).
- Existing `api::Graphic`/`Parameter` types were NOT reused for inits: they
  carry caller-supplied `unique_id` (conflicts with the fresh-UniqueId
  creation rule), lack `owner_part_display_mode`, and use `SchAngle`; the new
  init types mirror the A2 views exactly, which the projection-equality gate
  is defined against.

### Stream-order rule (documented on the module, tested directly)

Within each stream, non-members keep their original relative order. The edited
collection's final sequence occupies its members' original stream positions in
encounter order (Data slots, then Additional slots); extras are placed
contiguously right after the last original member in that member's stream; a
previously empty collection appends extras at the END of the Data stream
(parameters before graphics when both append there). Deletions vacate the
trailing member slots. The resulting A2 projection of the edited collection
equals the requested final sequence exactly — asserted in unit tests
(`batch_insert_reorder_replace_projects_exactly…`,
`stream_order_rule_places_extras_after_last_member_or_at_data_end`) and per
component in the corpus example.

### IndexInSheet policy (memo §6) as implemented

Touched owner = an owner whose ordered child-identity sequence (original
addresses / batch-local new ids, both streams in encounter order) differs
between input and output. For every touched owner, ALL its in-objectList
children are renumbered to final 0-based sibling positions; pins count as
siblings but carry no stored field. Untouched owners preserve stored values
verbatim (deliberate deviation from AD26's renumber-everything, per memo §6
rule 2). The objectList-membership classification (sources re-read in the
post-B fix pass; the original wording "mirrors the native attach mechanics
verified this increment" overstated the match — see the recorded deviation
below):

- Designator records: designator field object → never in an objectList.
- Root-owned Parameter records named "Comment" (ordinal case-insensitive):
  comment field object. New ones are created with IndexInSheet −1.
- Pin-owned parameters named DefaultNet/HiddenNetName/PinSelectionMemory/
  PinUniqueId: consumed as pin fields, never objectList children.
- ImplementationList/Implementation/ImplementationMap/MapDefiner/
  ParameterList: the compatibility chain never participates in sibling
  numbering (implementations are Add-then-Removed before AD26 numbers
  siblings, so they neither receive numbers nor occupy slots) — and stays
  byte-identical because its owners are never touched by a B batch.
- Everything else occupies a slot and (non-pins) gets the ordinal stored.

RECORDED DEVIATION (name-dispatch narrowing; post-B review). AD26's import
attach, `SchDataParameter.UpdateOwner`
(Altium.Sch.DataModel.Objects/SchDataParameter.cs:128-196), selects its
special branches by NAME ALONE — DefaultNet, HiddenNetName,
PinSelectionMemory, Comment, Description, PinUniqueId — and never calls
`base.UpdateOwner` from those branches; when the resolved owner lacks the
matching interface the parameter attaches NOWHERE. `SchDataComponent`
implements `ISchDataCommentOwner` but NOT `ISchDataDescriptionOwner`
(SchDataComponent.cs:19) and its `GetFieldObjects` yields only
designator+comment (SchDataComponent.cs:842-859), so a root-owned Parameter
named "Description" (or a pin-owned "Comment", etc.) is in no objectList and
is silently DROPPED by AD26 on its next save (`FillChildrenListForSave`
iterates only the objectList; `ProcessFieldObjects` only the field objects —
SchDataContainer.cs:709-731, 385). We must never replicate that silent drop
(cardinal rule), so `occupies_object_list_slot` is deliberately
owner-conditional — NARROWER than the name-only dispatch: only combinations
AD26 actually consumes (root "Comment"; pin-owned specials) are exempt from
numbering, and preserved records AD26 would drop are numbered as ordinary
siblings when their owner is touched. Wild-file precedent supports numbering
them: editor-attached root "Description" parameters carry sibling ordinals in
the corpus (Custom.SchLib `RECORD=41|INDEXINSHEET=3|OWNERPARTID=-1|…|NAME=
Description`; kmilo17pet-PIC16_Part1.SchLib INDEXINSHEET=4 — raw-byte
verified 2026-09-16). Functional exposure is nil on load (both AD26's SchLib
importer and ours ignore IndexInSheet), and our own save/reopen stays a fixed
point. A memo §6 correction is queued for increment F (see the errata section
appended to `next-child-contracts-2026-09-16.md`).

Caveats recorded: a Replace that renames a parameter to/from "Comment" changes
its field-object classification for numbering; the stale stored index on a
renamed-to-Comment parameter is preserved (the importer ignores IndexInSheet;
AD26's next save writes −1 for field objects). Root-owned parameters named
"Description" (and the other special names under non-consuming owners) are
numbered as ordinary siblings per the deviation above.

### Blank-library reconciliation (deliverable 5)

CORRECTED 2026-09-16 (post-B review): the paragraph originally shipped here
overclaimed the INDEXINSHEET evidence ("every AD26-written root Designator and
root Parameter carries INDEXINSHEET=-1", citing the §1 census figures 74,510 +
1,603,004). That sentence was false as written: the 74,510/1,603,004 census is
the memo §1 **OWNERPARTID** distribution only, and INDEXINSHEET=-1 is carried
ONLY by the component FIELD OBJECTS — Designator records and root Parameters
named "Comment". Re-verified raw dumps of all component Data streams
(2026-09-16, this fix pass):

- ICs.SchLib (32 components): 32 RECORD=34, all `INDEXINSHEET=-1`; 89
  RECORD=41 of which exactly 32 (one Comment per component) carry
  `INDEXINSHEET=-1` — the other 57 root parameters carry 0-based sibling
  ordinals (7 with the key absent ⇒ 0, 50 at 1..10).
- arthurbenemann-STM32F4.SchLib (135 components): 135 RECORD=34 all at −1;
  3,858 RECORD=41 of which 135 (the Comments) are −1 and 3,723 carry ordinals
  (≥ 51 — pins occupy the lower sibling slots).
- lucashudson-Connectors.SchLib (56 components, legacy key casing
  `IndexInSheet`/`OwnerPartId`): 56 RECORD=34 all at −1; 100 RECORD=41 of
  which 56 (the Comments) are −1, 42 omit the key (⇒ 0) and 2 carry 1.

The scoped claim fully supports the shipped code (which was already correct —
only Designator/Comment field objects are created with −1; ordinary root
parameters are numbered siblings per memo §6): `new_blank_ad26` and
`with_designator` creation write owner_part_id −1 (memo §2 ctor sentinel,
SchDataGraphicalObject.cs:34-39; corpus-dominant on root text with recorded
wild tails at ≥1, memo §1) AND index_in_sheet −1 for the Designator/Comment
field objects specifically (memo §6 rule 1), wired through
`check_written_owner_part_id(RootText, …)` in the same change (A2 review
finding 4 obligation). No repo fixture or test asserted 0 as native truth —
the old 0 was our creation artifact. Test:
`blank_library_root_text_uses_field_object_sentinels_and_reopens_exactly`
(field asserts, serialized `|OWNERPARTID=-1|`/`|INDEXINSHEET=-1|` keys,
save/reopen fixed point, `parameters()` owner group `PartIndependent`,
creation-path sentinels).

### Creation defaults and reference validation

New records: typed wire defaults (`from_params` on an empty collection — the
indexed-coordinate kinds Polyline/Polygon/Bezier additionally require the
`LocationCount` key seeded at construction, a creation defect found and fixed
in the post-B review with coverage for all 13 kinds in
`creation_covers_all_thirteen_representable_kinds`), fresh
`generate_unique_id()` where the kind carries one (Pie has none), parameters →
owner_part_id −1 + display mode 0, graphics → validated `owner_part_id`
(`1..=effective`) and `owner_part_display_mode` (`0 ≤ m <
display_mode_count`, default 0 on New; preserved on Replace when `None`).
Fonts: authored ids via `check_font_id_exists`; unspecified fonts on New
resolve the native default (Times New Roman 10) via
`find_or_create_default_font` run against a PROBE COPY of the captured table —
if resolution would append an entry, that is a library-level namespace change
that `replace_components`' ReferenceContext equality check must not absorb
silently, so it hard-errors naming the increment-D dependency (the D
namespace-transition ops will recompute the context). RECORDED LIMITATION:
`SchParameterInit` mirrors `SchParameterView` and exposes no font field, so in
a library whose captured table lacks the native default entry, parameter
creation is impossible until increment D lands; the parameter-path hard error
names exactly that (it no longer suggests authoring a font_id that parameters
cannot express). All three gate fixtures carry Times New Roman 10 as font 1,
so the corpus structural pass exercises the find path. Embedded image records
(New/Replace) validate `file_name` against captured storage assets
(`check_embedded_image_reference`, §8c rule 2); external-path images exempt.
style_id: no init exposes one; `check_style_id_write` is wired into the
New/Replace paths as a guard (§8b), and Replace preserves the original value
(with unique_id, index_in_sheet subject to §6, and all other non-addressed
fields) verbatim.

### Test evidence (fixture-free, default suite)

`crates/altium-format/src/schlib/snapshot/children.rs` tests (10):
insert+reorder+replace with exact projection equality, untouched-record
verbatim preservation, save/reopen typed equality, weight delta +2, and no-op
batch identity; touched-owner renumbering with untouched-owner (parent line
child) and field-object preservation; deletion authorization / duplicate slots
/ range errors / retained-dependent orphan rejection with weight delta −1;
cross-kind and unrepresentable Replace rejection plus from_view round-trip
recolor; creation validation matrix (owner part 0/out-of-range/−1, display
mode bound, dangling font, missing-default-font D-dependency error, image
references, "comment"-named new parameter gets −1/fresh UniqueId); dual-stream
shifts (Data parameter insert shifts an Additional-to-Data parent owner 3→4,
descendant Additional-to-Additional links survive, both streams save/reopen
with typed equality); Additional-stream creation and cross-stream migration
rejection on a collection spanning both streams; stream-order extras
placement (after last member vs Data-stream end); two sequential batches
(insert then authorized removal) restoring the original projections.
The ownership-worklog items "child Component insert" and "Data→Additional
links" remain covered by the existing `ownership.rs` tests (the public B
surface cannot express either).

### Corpus example (structural pass)

`crates/altium-sync/examples/check_schlib_snapshots.rs` gains a structural
pass per file (scalar pass unchanged), reworked in the post-B review to cover
the plan's full "parameter/graphic insert/remove/reorder" list and to make the
recolor check non-vacuous. For EVERY component:

1. Insert parameter `STEP2B_{i}` (text set, hidden) + a new part-1 rectangle,
   reverse the parameters, recolor the first replaceable graphic (via
   `SchGraphicInit::from_view`); native save/reopen; typed snapshot equality;
   per-component projection equality (parameter tuples, graphic kind
   sequences, probe rectangle owner group/display mode/UniqueId). The recolor
   is verified AT ITS EXACT INDEX: the reopened view's `from_view` init must
   equal the submitted recolored init field-for-field (the appended probe
   rectangle can no longer satisfy the check).
2. GRAPHIC REORDER: swap the probe rectangle (last member) with the last
   original graphic member — they occupy adjacent stream slots, so the swap is
   safe on any ownership graph — save/reopen, typed equality plus the swapped
   kind-sequence projection per component.
3. Authorized removal of BOTH probes (the `STEP2B_{i}` parameter and the probe
   rectangle, located and shape-verified at its post-swap index); save/reopen,
   typed equality, probe-parameter absence, and the graphics kind sequence
   restored to the original input's per component.

JSON fields: `structural_replaced_colors`, `structural_insert_reorder_equal`,
`structural_projection_equal`, `structural_graphic_reorder_equal`,
`structural_removal_equal`; any false counts as a failure → nonzero exit.
Gate run: all three files true on all checks (56/32/135 components, all
recolored), exit 0.

### Verification (all default suites, fixture-free; post-fix-pass numbers)

- `cargo test -p altium-format --lib` — 451 passed, 0 failed (448 at the
  initial B report; +3 fix-pass tests, see the findings resolution below).
- `cargo test -p altium-sync` — 18 passed, 0 failed (6 pre-existing
  `collection.rs` dead-code warnings, unchanged).
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check` — clean on altium-format and altium-sync.
- Corpus gate with structural checks (incl. graphic reorder/removal) — all
  true, exit 0 (see above).

## Increment B — review findings resolution (2026-09-16)

Independent review returned 13 findings (three groups of duplicates).
Dispositions, each verified against the code/sources/fixture bytes first:

1. **Worklog INDEXINSHEET=-1 evidence false as written (major; reported three
   times) — CONFIRMED, FIXED (evidence-record defect; code was correct).**
   Re-dumped every component Data stream of the three gate fixtures: only
   Designators and root "Comment" parameters carry INDEXINSHEET=-1; ordinary
   root parameters carry 0-based sibling ordinals (ICs 32 of 89 R41 at −1,
   others at 0..10; STM32F4 135 of 3,858, others ≥51; Connectors 56 of 100,
   42 key-absent ⇒ 0, 2 at 1); the 74,510/1,603,004 census figures are the
   memo §1 OWNERPARTID distribution only. Rewrote the reconciliation section
   above with the scoped claim and per-fixture counts; also fixed the same
   overclaim in the `blank_library_root_text…` test comment and tightened the
   `new_blank_ad26` comment in `schlib.rs` (OWNERPARTID −1 is corpus-dominant
   with recorded wild tails, not universal). STATUS.md's narrower phrasing was
   already accurate.
2. **`occupies_object_list_slot` narrower than the cited AD26 dispatch; root
   "Description" numbered (major) + "narrowing not recorded" (minor) —
   CONFIRMED as an evidence-record defect; code behavior RETAINED as a
   documented deviation.** Verified in the decompiled sources:
   `SchDataParameter.UpdateOwner` is name-first; `SchDataComponent` is not an
   `ISchDataDescriptionOwner`; unattached specials are dropped by AD26 on
   resave (objectList-only save walk). Verified in fixture bytes: wild root
   "Description" parameters are NUMBERED siblings (Custom.SchLib
   INDEXINSHEET=3, kmilo17pet-PIC16_Part1 INDEXINSHEET=4). Decision: keep the
   owner-conditional classification — replicating the name-only dispatch would
   mean mirroring numbering for records AD26 does not even re-emit, and our
   cardinal rule forbids the drop; numbering preserved records as ordinary
   siblings matches the wild-file (editor-attached) precedent. The "verified
   … mirrors the native attach mechanics" wording was corrected, the deviation
   and the root-"Description" caveat are recorded in the IndexInSheet section
   above and in the module docs, and a memo §6 correction is queued for F in
   the errata section of `next-child-contracts-2026-09-16.md`.
3. **Corpus structural pass omitted graphic remove/reorder (minor) —
   CONFIRMED, FIXED.** The pass now runs a per-component graphic reorder
   round-trip (adjacent-slot swap of the probe rectangle with the last
   original member) and removes the probe rectangle (shape-verified at its
   post-swap index) in the authorized-removal batch, restoring and asserting
   the original kind sequences. New gate field
   `structural_graphic_reorder_equal`; all three files pass.
4. **Recolor verification vacuous (appended probe satisfied the `any`)
   (minor) — CONFIRMED, FIXED.** The check now compares the reopened view at
   the exact replaced index against the submitted recolored init
   field-for-field. Also extended fixture-free Replace coverage beyond
   Rectangle: `replace_round_trips_polyline_and_label_views` (vertex-list and
   text kinds).
5. **NEW DEFECT FOUND WHILE FIXING 4 (not reported by any reviewer):
   `SchGraphicEdit::New` hard-errored for Polyline/Polygon/Bezier** —
   `from_params` on an empty collection requires the `LocationCount` key
   (`MissingParam`). Fixed by seeding the count key at construction;
   `creation_covers_all_thirteen_representable_kinds` now creates every
   representable kind, asserts exact projection round-trip, and save/reopens
   with typed equality (proving no other kind has a missing-required-param
   creation failure).
6. **Replace with `None` optionals silently reset stored display mode/font
   (minor; reported twice) — CONFIRMED, FIXED.** `None` now means
   "unaddressed": creation defaults on New, preserve-the-original on Replace
   (no validation runs for a value we do not write — wild stored values stay
   capture-forbidden). Documented on the module, the init fields, and tested
   (`replace_with_unaddressed_optionals_preserves_display_mode_and_font`).
7. **Parameter creation impossible without the default font + misleading
   error advice (minor) — CONFIRMED, FIXED/RECORDED.** The advice string is
   now caller-specific: the parameter path names the real limitation
   (`SchParameterInit` has no font field; creation impossible in such
   libraries until increment D) instead of suggesting an unauthorable
   font_id. `SchParameterInit` deliberately does NOT grow a font field — it
   mirrors `SchParameterView`, which exposes none. Limitation recorded in the
   creation section above. The pre-existing api-layer `default_base()`
   sentinels (`api/sch_common.rs:540`: owner_part_id 0 / index_in_sheet 0,
   used by `api/schlib_write.rs` creation) are confirmed present, outside B's
   snapshot scope — QUEUED as an explicit alignment-or-deprecation decision
   for the user/increment F (not just an open question).
8. **Memo §8a rule 3 "fonts are not part of ReferenceContext" contradicted by
   the code; discrepancy not queued (minor) — CONFIRMED, RECORDED.**
   `ReferenceContext` does carry fonts (`schlib/snapshot.rs:25-32`) and
   `replace_components` enforces context equality (`snapshot.rs:179`); B's
   fail-closed find-only resolution stands. The memo/code contradiction and
   the D obligation (own the declared font-table append or move fonts out of
   the context) are queued in the errata section of
   `next-child-contracts-2026-09-16.md`.
9. **Keep/Replace index space deviates from the plan's per-owner-part wording
   (minor) — CONFIRMED, RECORDED; needs orchestrator/user acceptance.** The
   full-projection index space stands (deliverable-2 gate is defined over the
   full views); a BINDING OBLIGATION on increment E (filtered→full mapping +
   multi-part test) is now recorded in the API-shape section above.
10. **`String::from_utf8_lossy` in a test assertion (minor) — CONFIRMED,
    FIXED** with strict `std::str::from_utf8(...).expect(...)`.

Memo errata queued for increment F (recorded in
`next-child-contracts-2026-09-16.md`): §6 ImplementationList wrapper
serializes INDEXINSHEET absent ⇒ 0, not −1 (ctor `SetIndexInSheetForSave(0)`;
gate-fixture bytes show `|RECORD=44` with no keys at all); §6 field-object
classification scoping (name-first import dispatch, owner-interface guards,
AD26's silent drop of unattached specials); §8a ReferenceContext parenthetical.

## Increment C — structural pin editing + part grow/shrink (implemented)

Contract authority: memo §1/§2/§3/§4/§5/§6/§7 incl. the errata section.
Extends `crates/altium-format/src/schlib/snapshot/children.rs` (pins in the
B batch engine) and adds `crates/altium-format/src/schlib/snapshot/parts.rs`
(part ops), both exported through `schlib`, `api`, and the snapshot module.

### Pins in the batch engine

- `SchChildBatch` grows `pins: Option<Vec<SchPinEdit>>`;
  `SchPinEdit::{Keep(usize), Replace(usize, SchPinInit), New(SchPinInit)}`
  indices are positions in the FULL A2 `pins()` projection of the immutable
  input — same final-sequence, stream-order (extras after the last pin
  member; empty membership appends at the Data end after parameters and
  graphics extras), deletion-authorization, and atomicity semantics as B.
- `SchPinInit` mirrors the `SchPinView` surface: owner_part_id (0 =
  SharedPins wildcard or 1..=effective; −1 forbidden — memo §1, via
  `check_written_owner_part_id(Pin, …)`), `owner_part_display_mode:
  Option<i32>` with B's unaddressed semantics (§4 bound + a u8 range check
  for the binary field), designator, name, electrical, length, location,
  orientation, is_hidden, hidden_net_name, swap_group (SwapIdPin),
  `part_swap_group`/`part_pin_swap_suffix` (`Option<String>` halves of the
  combined string), pair_swap_group (PairSwapID). `from_view` populates
  everything explicitly (view suffix `None` maps to authored `Some("")` —
  identical under the native concat rule) and errors for
  PartIndependent/OutOfRange owners.
- SWAP ENCODING (memo §5 rule 3): both halves `None` on Replace → the stored
  combined string is preserved BYTE-VERBATIM (wild `"|&|"` included; test
  `pin_swap_halves_preserve_or_reencode_per_the_native_rule`); either half
  addressed → split via `split_swap_id_part_and_part_pin`, unaddressed half
  preserved through the split, re-encode via
  `encode_swap_id_part_and_part_pin` (now consumed; `#[allow(dead_code)]`
  removed). Re-encoding normalizes the wild bare-separator form — the native
  concat cannot regenerate it — which is the memo's contract for touched
  halves; `from_view`-driven Replace therefore re-encodes (documented on the
  type).
- Replace preserves every unaddressed field verbatim, explicitly including
  the sidecar-backed typed fields (defined/selected functions, package
  length, propagation delay, fractional coordinates, custom text
  positioning, symbol line width, >254-char descriptions, pair swap) and
  unique_id/default_value/description/color/symbols/show flags.
- NEW-PIN DEVIATION (recorded): new pins get typed wire defaults
  (`parse_text_pin` on an empty collection) with the four IEEE-symbol
  presence flags forced true (SchLib pins serialize binary; text-pin omission
  flags would break the save/reopen fixed point) and NO fresh UniqueId — the
  task text asked for one, but the native binary pin body carries no UniqueId
  field (`parse_binary_pin` zero-initializes, `serialize_binary_pin` writes
  none), the A2 pin view exposes none, and an assigned id would break the
  save/reopen typed-equality gate the same deliverable demands. Documented in
  the module docs and asserted in tests.
- NEW-PIN CREATION-DEFAULT DEVIATION (recorded in the C fix pass): the typed
  wire defaults leave `show_name`/`show_designator` FALSE (PIN_CONGLOMERATE
  wire zero) and the formal type at its wire-zero variant, whereas AD26's
  editor creation default `SchDataPin.SetDefault` (Objects/SchDataPin.cs)
  sets both show flags TRUE and `formalType = eStdLogic_ForcingUnknown`
  (and seeds the combined swap string as the bare `"|&|"` separator, which
  our empty-halves encoding writes as `""`). `SchPinInit`/`SchPinView`
  expose none of these fields, so callers cannot author them. Round-trip
  correctness is unaffected (the values serialize and reopen exactly;
  asserted in `pin_creation_validates_owner_display_and_hidden_net_writes`),
  but a pin created through this engine renders with hidden name/designator
  unlike an editor-created pin. QUEUED USER DECISION: default the show flags
  (and formal type) to the native `SetDefault` values on `New`, or grow
  authored `show_name`/`show_designator` fields on `SchPinInit`/`SchPinView`.
- `hidden_net_name`: the binary pin body has no field and the native storage
  form (pin-owned Parameter "HiddenNetName") is preserved-only content, so a
  written value would be silently lost on save. New helper
  `check_pin_hidden_net_name_write` (validation.rs) allows only the stored
  value or the empty default; anything else hard-errors naming the unproven
  path.
- ADDITIONAL-PIN REJECTION SHAPE (tightened in the C fix pass — see the
  findings-resolution section): while any Additional pin member exists, the
  final pin sequence is DIRECTLY enforced to be positional — every member
  must sit at its original ordinal (Additional members strictly `Keep(j)`,
  Data members `Keep(j)` or field `Replace(j, …)`) and no member may be
  added or deleted; any violation is a hard error naming the frozen
  `pins()[j]`. The original C implementation checked only the Additional
  ordinals, which silently ADMITTED Data-only permutations and balanced
  Data delete+insert (both provably sidecar-safe — sidecars regenerate from
  final Data encounter order — but contradicting the published "only field
  Replace of Data pins remains possible" contract); the guard now enforces
  the documented conservative freeze. Keep-untouched (and batches with
  `pins: None`) flow through unchanged. Rationale unchanged: sidecars are
  serialized from and merged by final DATA pin encounter order only
  (`serialize_pin_sidecars`/`merge_pin_sidecars` both walk `comp.records`),
  and Additional pin sidecars remain unproven (ownership worklog).
- `all_pin_count` (memo §7): hands-off. No code path in the engine reads or
  writes the stored cache; `SchLibComponentSnapshot::stored_all_pin_count()`
  (fields.rs) projects it read-only so callers/tests can verify. Test
  `all_pin_count_stays_verbatim_through_structural_pin_edits` covers the
  stale-positive (17) and zero cases through batches and native save/reopen.
- Sidecar consistency (plan C: NO new recompute layer): the save path already
  serializes all nine sidecar streams from the final Data pin encounter
  order, so structural edits need nothing new. Test
  `pin_batch_edits_keep_all_nine_sidecar_streams_consistent_across_save_reopen`
  builds a fixture that engages ALL NINE streams (asserted by name against
  `serialize_pin_sidecars` output: PinFrac, PinDesc, PinMiscData,
  PinTextData, PinWideText, PinSymbolLineWidth, PinPackageLength,
  PinPropagationDelay, PinFunctionData), then proves insert-at-front +
  reorder + Replace and a second authorized-removal batch keep reopened typed
  equality with the sidecar-backed fields on the right pins.
- Removed while implementing: a drafted i16 body-range check for authored
  pin coordinates is provably unreachable (|i32::MAX| / C_BASE_UNIT = 21,474
  < i16::MAX — every representable Coord fits the binary body plus PinFrac);
  a comment records the arithmetic instead of dead code.

### Parts API (`snapshot/parts.rs`) — design shape

- `with_parts_appended(additional_parts: i32) -> Result<Self>`: appends empty
  trailing effective slots; native count = effective + 1 maintained on BOTH
  the component record and the write-only header index entry; children,
  display_mode/display_mode_count/custom names, and current_part_id are
  untouched (growth never reclassifies, memo §2 — a previously OutOfRange
  owner may classify as a numbered part under the new count, which is raw
  classification, not a record edit). The resulting native count is bounded
  by `i16::MAX` (fix pass): the binary pin body stores `owner_part_id` as
  i16, so a larger slot could never carry a serializable pin owner.
  RECORDED DEVIATION from native `AddPart` (SchComponent.cs:1575-1586):
  `AddPart` ends with `SetCurrentPartID(GetPartCount() - 1)`, moving the
  current part to the new slot; `with_parts_appended` leaves
  `current_part_id` untouched (no interactive part cursor in a batch API;
  asserted in tests).
- `with_parts_removed(removed_parts: &[i32], allow_descendant_deletion: bool)
  -> Result<SchPartsShrink>`: the explicit slot list makes non-trailing
  requests expressible and rejectable — the sorted/deduped set must equal the
  trailing run `effective-k+1..=effective` and ≥ 1 effective part must
  remain. Deletion set = records whose raw `owner_part_id` matches a removed
  slot (the SELECTION half of the native `DeletePart` walk,
  SchComponent.cs:1593-1620) transitively closed over record ownership (a
  descendant with a DIFFERENT part id would otherwise be a retained orphan;
  it is deleted under the same flag and listed — recorded deviation from the
  part-id-only native walk). Two further recorded deviations (fix pass; also
  in the parts.rs module docs): (1) native `DeletePart` DECREMENTS the
  `owner_part_id` of every retained first-level object whose owner exceeds
  the removed slot (the contiguous-renumbering half); under the
  trailing-only restriction the only such records are out-of-range wilds,
  which we preserve BYTE-VERBATIM per memo §1 instead of renumbering
  (tested: an owner-4 record survives a shrink to native 3 unchanged, where
  native would decrement it to 3). (2) Whether native `DeletePart` reaches
  wild ROOT TEXT depends on Parameter/Designator membership in
  `SchDataConsts.SpatialObjectSet`, which the decompile hides in an opaque
  token blob — unverified, so wild-root-text deletion cites OUR memo-§1
  raw-match rule, not a native precedent (flag-authorized and listed either
  way). Unauthorized → hard error listing every doomed record
  (`child[i] Kind (owner_part_id n)`). Owner-0 pins, −1 root text, and
  out-of-range owners are never deleted. Assembly goes through
  `assemble_records` (owner remap) plus the shared memo-§6
  `renumber_touched_owners` (made `pub(super)` for reuse).
- `SchPartsShrink { component, removed_parts, deleted_children:
  Vec<SchRemovedPartChild { child_index /* encounter order */, kind:
  SchRecordType /* typed since the fix pass; was &'static str */,
  owner_part_id }>, current_part_id_adjustment:
  Option<SchCurrentPartIdAdjustment { before, after }> }` — the memo §3
  rule 4 decrement (per removed slot, −1 only when > 1) is applied AND
  returned; a no-op (current 1) reports `None`.
- Memo §3 rule 3: a stored `current_part_id` outside `[1, native)` blocks
  BOTH ops with an "explicitly resolved" error — never a silent clamp.
- COMPOSITION DECISION (part ops + child edits in one call): compose, not
  extend. `with_parts_appended` changes only the two count fields, so a
  following `with_children` resolves against projections index-identical to
  the original input's (tested:
  `part_growth_composes_with_child_batches_creating_children_in_the_new_part`
  — new graphics/pins in the new part, shared-pin wildcard now writable).
  `with_parts_removed` deletes children, so its outcome snapshot is the
  immutable input of any subsequent batch; the report lists deletions and the
  caller re-projects. A deletion-bearing part op is never silently fused with
  child indices resolved pre-deletion.
- `check_custom_display_mode_name_index` stays dead by design: no C path
  writes display-mode-adjacent state; its comment now carries the C→D note
  (increment D owns component-level display-mode/custom-name edits).

### Test evidence (fixture-free, default suite)

New tests in `children.rs` (8): all-nine-sidecar-streams save/reopen
consistency under insert/reorder/replace + authorized removal; all_pin_count
verbatim (stale 17 + zero); multi-collection batch (parameters + graphics +
pins in one call against the immutable input); pin insert shifting an
Additional-to-Data parent owner (3→4) with descendant links surviving
save/reopen; Additional-pin freeze matrix (Replace/reorder/delete/insert all
rejected; Data-pin Replace with Keep-in-place passes; no-op identity); swap
halves preserve-or-re-encode matrix; creation validation (owner −1/2
rejected, 0/1 accepted with projected groups, display bound, unaddressed mode
default, hidden_net_name guard, from_view round-trip); part growth
composition. New tests in `parts.rs` (3): growth without reclassification +
save/reopen + invalid counts; trailing shrink (deleted-children listing incl.
closure descendant and Additional child, reported current 3→2 and 3→1
double-decrement, current-1 no-op, IndexInSheet renumbering vector, kept
projections, weight −4, save/reopen); rejection matrix (unauthorized listing,
non-trailing [2]/[1,3]/[4]/[1], remove-all, empty list, wild current blocks
both ops). Plus `check_pin_hidden_net_name_write` unit coverage in
`validation.rs`.

### Corpus example (pin structural pass)

`check_schlib_snapshots.rs` gains `pin_structural_pass` from a fresh open:
per component (1) append probe pin `STEP2C_{i}` (part 1, Passive, 300 mil)
— save/reopen typed equality, untouched-pin fingerprint equality (complete
`SchPinView` Debug fingerprints), probe `from_view` == submitted init;
(2) swap the probe with the last original pin (adjacent stream slots — the
extra is placed contiguously after the last member, and a pin's owned
children all sit later, so the swap is safe on any ownership graph) —
save/reopen + swapped-projection equality; (3) authorized removal of the
probe (position-verified by designator), restoring the ORIGINAL pins()
fingerprints exactly; the stored AllPinCount is asserted verbatim at every
stage. New JSON fields enforced in `main()`:
`structural_pin_insert_equal`, `structural_pin_reorder_equal`,
`structural_pin_removal_equal`, `structural_pin_projection_equal`,
`structural_all_pin_count_verbatim`.

### Corpus example (part structural pass — added in the C fix pass)

The plan's "pin/part structural round-trips" deliverable originally shipped
pins only; `part_structural_pass` closes the gap. From a fresh open, per
component: (1) `with_parts_appended(1)` — effective count +1, pin
fingerprints AND graphics kind sequences index-identical to the input
(growth never reclassifies), then whole-library save/reopen typed equality;
(2) `with_parts_removed(&[appended_slot], false)` on the reopened grown
snapshot — the empty new slot must report ZERO deletions (so no
authorization is passed), `removed_parts == [slot]`, and the memo §3 rule 4
`current_part_id` adjustment is asserted through the typed report (`None`,
or `before > 1 && after == before - 1` — the decrement fires on ANY slot
removal when the stored current part is > 1, so a grow+shrink cycle does
NOT round-trip `current_part_id` for such components; that is the
documented native rule, not a defect); save/reopen typed equality; (3)
restore check — every component's counts/projections equal the original's,
and every component WITHOUT a reported adjustment equals the original typed
component exactly. New JSON fields enforced in `main()`:
`structural_part_growth_equal`, `structural_part_shrink_equal`,
`structural_part_report_correct`, `structural_part_restore_equal`.

### Verification (all default suites, fixture-free)

- `cargo test -p altium-format --lib` — 463 passed, 0 failed.
- `cargo test -p altium-sync` — 18 passed, 0 failed (6 pre-existing
  `collection.rs` dead-code warnings, unchanged).
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- Corpus gate: all checks (incl. the five pin fields) true for
  lucashudson-Connectors (56), ICs (32), arthurbenemann-STM32F4 (135);
  exit 0.

### Known items deferred / flagged out of C

- The `"|&|"` separator constant still awaits the user discussion before
  promotion into `altium-format-types::constants::pin` (A2 flag; the C swap
  plumbing consumes the local constant through the existing helpers).
- No explicit `current_part_id` resolve/set operation exists; wild stored
  values therefore block part ops until such an op (or E-level policy) lands.
- Additional-stream pins stay frozen until Additional pin sidecars are proven
  (or proven absent) against native evidence.
- E obligation (unchanged from B): sync-side per-owner-part positional pin
  addresses resolve through the owner_group-filtered → full-projection
  mapping.

## Increment C — review findings resolution (2026-09-16)

Independent-review findings against the C implementation, each verified
against code before acting. Post-fix verification numbers at the end.

1. **MAJOR — corpus example omitted the part grow/shrink round-trips**
   (plan increment C: "Extend corpus example with pin/part structural
   round-trips"; only `pin_structural_pass` existed, and the omission was
   recorded nowhere). CONFIRMED and fixed: `part_structural_pass` added to
   `check_schlib_snapshots.rs` (per-component append-one/remove-it with
   save/reopen typed equality at both stages, empty expected deletion list,
   typed `current_part_id` adjustment asserted per memo §3 rule 4, restore
   equality modulo reported adjustments) plus four enforced JSON fields.
   All true on the three gate files, exit 0.
2. **MAJOR — pin `owner_part_id` i16 truncation aperture**: `serialize_binary_pin`
   wrote `pin.owner_part_id as i16` (silent truncation, baseline code) and
   increment C opened reachable paths to out-of-i16 values —
   `with_parts_appended` bounded the count only by i32 overflow, and
   `check_written_owner_part_id(Pin, …)` accepted any id below a wild
   effective count. CONFIRMED (silent-corruption class, cardinal rule) and
   fixed at all three layers: `serialize_binary_pin` now `i16::try_from`
   hard-errors ("refusing to truncate"); `with_parts_appended` caps the
   resulting native count at `i16::MAX` (its error text now names the
   signed-16 range it actually enforces); `check_written_owner_part_id`
   rejects pin ids beyond `i16::MAX` at the mutation boundary. New tests in
   all three files (exact-bound acceptance included).
3. **MINOR ×3 (duplicates) — Additional-pin freeze weaker than documented**:
   the guard checked only that Additional ordinals held `Keep(j)` at final
   position j, so Data-only permutations and balanced Data delete+insert
   PASSED while module docs/worklog/STATUS/report all claimed "only field
   Replace of Data pins remains possible". CONFIRMED (contract mismatch;
   the admitted operations were verified sidecar-safe, so no corruption).
   Resolved by TIGHTENING the guard to the published conservative contract
   (full positional freeze: identical membership, positional Keep/Replace of
   Data members, strict Keep of Additional members, no count change) rather
   than weakening four documents to a subtler ordinal-stability rule that
   later increments would have to plan against; relaxation stays available
   to increment E once Additional sidecars are proven (existing open item).
   New test `data_pin_structural_edits_are_frozen_while_additional_pins_exist`
   locks the permutation/balanced-substitution rejections and the
   positional-Replace pass-through. The `plan_streams` Additional-creation
   error now says "parameters/graphics/pins".
4. **MINOR — stale/wrong doc comments**: `with_children` doc said
   "parameters and graphics" (now names all three collections);
   `raw_owner_part_id`'s comment claimed `None` for nested Components while
   that arm returns `Some`, and its `unwrap_or(0)` could have fabricated
   owner 0 in the deletion report. CONFIRMED and fixed: the helper now
   returns `Result<i32>` (hard error on the provably-unreachable baseless
   arm, threaded with context — no fabricated default can enter the report).
5. **MINOR — unrecorded native-precedent gaps in the parts ops**
   (`DeletePart`'s owner-renumbering half; `AddPart`'s current-part move;
   the unverifiable SpatialObjectSet claim for wild root text). CONFIRMED
   against SchComponent.cs:1575-1620 and recorded: parts.rs module doc grew
   a "Recorded deviations from native AddPart/DeletePart" section and the
   design-shape bullets above were corrected (the wild-root-text deletion
   now cites the memo-§1 raw-match rule, not a native precedent).
6. **MINOR — new-pin show flags default false vs native SetDefault true**:
   CONFIRMED against Objects/SchDataPin.cs `SetDefault` (showName/
   showDesignator true, formalType eStdLogic_ForcingUnknown, combined swap
   seeded `"|&|"`). Recorded as an explicit creation-default deviation
   (children.rs module doc + the pins section above) with a queued user
   decision (adopt SetDefault values vs. grow authored show-flag fields);
   behavior deliberately unchanged in a fix pass — round-trip correctness
   is unaffected and now pinned by a test assertion.
7. **MINOR — `SchRemovedPartChild.kind` stringly typed**: CONFIRMED and
   fixed — the field is now `SchRecordType` (existing domain type from
   `altium-format-types`; `record_type_for` made `pub(crate)`); the string
   label survives only inside rendered error text via `Display`, and
   `record_kind_label` reverted to `views.rs`-private.

Post-fix verification (all after the last edit):
- `cargo test -p altium-format --lib` — 465 passed, 0 failed.
- `cargo test -p altium-sync` — 18 passed, 0 failed (6 pre-existing
  `collection.rs` dead-code warnings, unchanged).
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- Corpus gate: all checks true — the five pin fields AND the four new part
  fields — for lucashudson-Connectors (56), ICs (32),
  arthurbenemann-STM32F4 (135); exit 0.

## Increment D1 — library-level namespace transitions (implemented)

Contract authority: memo §8a/§8c incl. the errata section (fonts-in-
ReferenceContext contradiction), plan increment D items 1 and 3. New module
`crates/altium-format/src/schlib/snapshot/library.rs`, exported through
`schlib`, `api`, and the snapshot module.

### API shape

- `SchLibSnapshot::with_library_edit(SchLibEdit) -> Result<SchLibSnapshot>` —
  the DECLARED-transition variant of component replacement. The strict path
  is untouched: `replace_components` with an UNDECLARED context change
  remains a hard error (re-asserted in tests), so namespace changes are only
  expressible through this explicit typed batch.
- `SchLibEdit { components: Option<Vec<SchLibComponentEdit>>, alias_ops:
  Vec<SchAliasOp>, image_assets: Vec<SchImageAssetEdit>, font_appends:
  Vec<SchFontAppend>, allow_component_removal: bool }`.
- `SchLibComponentEdit::{Keep(i), ReplaceWith(i, SchLibComponentSnapshot),
  New(SchNewComponent)}` over the FINAL ordered component sequence
  (`None` = component set untouched; omission of an input slot = removal,
  requiring `allow_component_removal` and listing the omitted slots
  otherwise). `ReplaceWith` keeps the strict rules: input-context equality,
  no renames, no alias-list changes (alias changes go through `SchAliasOp`).
- `SchNewComponent::{CloneOf { source, lib_reference }, Blank
  { lib_reference }}` — the context-independent factory. CloneOf deep-copies
  an INPUT component under the new name with a fresh component (root)
  `UniqueId` and cleared alias state (`index.aliases` + root `AliasList`);
  every other axis, child-record unique ids included, is copied verbatim
  ("minus name/UniqueId"). Blank reuses the `new_blank_ad26` component
  shape via the extracted `blank_ad26_component(lib_reference)` factory
  (schlib.rs) and validates that font id 1 exists in the post-append table.
- `SchAliasOp::{Add, Remove} { component: lib_ref, alias }` — addressed by
  name so a Remove can target a component the same edit deletes (releasing
  its alias); removes apply before adds, so Remove+Add retargets an alias.
- `SchImageAssetEdit::{Insert, Replace, Remove}` — ordered ops wrapping the
  existing `SchLib::{insert,replace,remove}_image_resource` (serialization
  constraints validated there, per-op position context added).
- `SchFontAppend` — every native font axis except the positionally assigned
  id; `SchFontAppend::native_default()` is Times New Roman 10.
  `SchFontEntryView` grew the style axes (rotation/bold/italic/underline/
  strikeout) so the captured-table projection covers the append surface.

### Context-recompute rule

The result's `ReferenceContext` is recomputed via `ReferenceContext::capture`
over the assembled final library — never hand-patched — and all component
snapshots taken from the result bind to it (they share the recomputed Arc).
Determinism: the final state is a pure function of (input, edit) except the
fresh `UniqueId`s of New components, which never enter the context — same
edit on same input yields identical contexts always, and identical full
snapshots for New-free edits (both tested). A no-op edit (empty batch or
all-Keep) yields a context-and-state-identical snapshot; any actual state
change compares unequal (replan visibility). Both tested.

### Composition decision (resolves the recorded B font limitation)

Fonts STAY inside `ReferenceContext`; increment D adopted the errata's
"declared font-table append that recomputes the context" option. Within one
edit the application order is: font appends → image-asset ops → component
slots → alias ops (removes, then adds) → namespace/reference validation →
context recapture. Component CONTENT edits (`with_children`) still run
against a snapshot's captured context, so a child needing a namespace
addition is authored in TWO steps: the library edit first, then component
edits taken from the RESULT snapshot ("font appends apply first and
component edits reference the extended table"). The B-era hard errors in
`children.rs` now name `SchLibSnapshot::with_library_edit` as the resolution
path, and the end-to-end flow (library lacking the default font → parameter
creation blocked → declared append → parameter creation resolves the
appended entry → save/reopen equality) is tested
(`font_appends_recompute_context_and_unlock_child_creation`). Inside a
single edit, New Blank components DO see same-edit font appends (fonts apply
first; tested with an empty-font-table library).

### SectionKeys / Redirection handling

- Storage keys need no op surface: `SchLib::save` derives SectionKeys from
  the final header names via the existing `build_section_keys` path; a
  42-character clone name is save/reopened in unit tests AND per corpus file.
- Aliases keep three synchronized representations: header
  `SchLibComponentIndex.aliases` (removals splice, additions append), the
  root record `AliasList` param (recomputed as the comma-join of the final
  vector for alias-touched components only; untouched components stay
  byte-verbatim), and the global `SchLibAlias` Redirection entries —
  preserved verbatim when the ordered header pair sequence is unchanged,
  otherwise rebuilt deterministically in header order (the parse order).
  RECORDED DEVIATION: AD26 keeps aliases in a sorted set
  (`SchDataAliasList`, SortedList; V5 export writes both the header entries
  and `AliasList` comma-text from it — FileFormatV5.cs:2894,
  SchDataExporterLibraryV5.cs:143-150); we preserve loaded order and append,
  never re-sort. The self-alias storage-sharing case round-trips (tested).
- Namespace rules at the mutation boundary: final lib_refs unique; alias
  names globally unique and colliding with no OTHER component's lib_ref
  (self-alias allowed); added aliases non-empty and comma-free (the native
  comma-join cannot represent commas). Component removal with a surviving
  alias is a hard error listing the dangling redirections unless the same
  edit removes/retargets them.
- Image-asset reference rule (memo §8c rules 2-3): an embedded image record
  whose `file_name` resolved against the INPUT asset set (exact, then
  case-insensitive — the native pairing order) must still resolve against
  the FINAL asset set; removal/rename of a referenced asset is blocked,
  orphan-asset removal and preservation of already-dangling references stay
  legal, and removing the referencing record in the SAME edit unblocks the
  asset removal (validation runs over the final state).
- Weight keeps the input's stored convention: the canonical cardinality
  delta (records + aliases per component, plus one root per component)
  between final and input states is applied to the stored value — the
  `replace_components` policy extended with the component-root term; never
  recanonicalized. (A wild historical weight formula that cannot absorb a
  component-cardinality delta fails loudly at final invariant validation;
  all three gate fixtures pass.)

### Memo obligations resolved / still open

- RESOLVED: errata item 3 (§8a rule 3 "fonts are not part of
  ReferenceContext" contradiction) — D adopted the declared append; fonts
  remain in the context; increment F rewrites the memo parenthetical to
  "font-table appends are a library-level namespace transition declared via
  SchLibEdit::font_appends". A dated resolution note was appended to the
  errata entry.
- RESOLVED: the recorded B limitation "parameter creation impossible in a
  library lacking the default font until increment D lands" — see the
  composition section; error text updated.
- OPEN (unchanged): `check_custom_display_mode_name_index` stays dead —
  D1 covers plan-D items 1 and 3 only; component-level display-mode/custom-
  name edits (plan D item 2, implementation chains) remain for the next D
  increment.
- OPEN (unchanged): no explicit `current_part_id` resolve op; Additional-pin
  freeze; the `"|&|"` constant promotion discussion.

### Test evidence (fixture-free)

New tests in `library.rs` (5): empty/all-Keep no-op identity + inequality on
real change; font appends (context recompute, end-to-end child creation on
the extended snapshot, strict-path drift rejection, duplicate/empty-name/
non-sequential rejections); alias ops (three-representation sync asserted on
private state, self-alias save/reopen, weight, removal resync, determinism,
7 rejection paths); clone/blank/removal (deep-copy-minus-name/UniqueId
equality, weight delta, >31-char SectionKeys save/reopen, context
determinism, unauthorized-removal listing, restore-to-input equality, blank
shape + same-edit font composition, dangling-alias block + release +
retarget, slot misuse and duplicate-name rejections incl. clone-under-alias
-name, ReplaceWith context/alias-smuggle rejections); image-asset ops
(orphan removal, referenced-asset remove/rename blocks incl. case-
insensitive, same-name payload replace recomputing the context, same-edit
record-removal unblocking, op-position error context, dangling-reference
preservation).

### Corpus example (namespace pass)

`check_schlib_snapshots.rs` gains `namespace_pass` from a fresh open, per
file: (1) add alias `STEP2D_0` to component 0 — save/reopen typed equality +
alias projected; (2) remove it — save/reopen typed equality AND equality
with the ORIGINAL input snapshot; (3) clone component 0 under the
42-character `STEP2D_CLONE_WITH_A_VERY_LONG_STORAGE_NAME` (SectionKeys) —
save/reopen typed equality + full public-projection fingerprint equality of
the clone vs source (description/designator/kind/hidden-pins/effective
parts/AllPinCount/parts/footprints/parameters/pins/graphics; lib reference
and aliases excluded by design); (4) authorized removal of the clone —
save/reopen typed equality AND restoration to the original input snapshot
exactly. New JSON fields enforced in `main()`:
`namespace_alias_add_equal`, `namespace_alias_remove_equal`,
`namespace_clone_equal`, `namespace_clone_projection_equal`,
`namespace_restore_equal`.

### Verification (all default suites, fixture-free)

- `cargo test -p altium-format --lib` — 470 passed, 0 failed.
- `cargo test -p altium-sync` — 18 passed, 0 failed (6 pre-existing
  `collection.rs` dead-code warnings, unchanged).
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- Corpus gate: all checks true — incl. the five namespace fields — for
  lucashudson-Connectors (56), ICs (32), arthurbenemann-STM32F4 (135);
  exit 0.

## Increment D1 — review findings resolution (2026-09-16)

Fix pass over the independent-review findings; where a statement below
contradicts the D1 section above, this section governs.

1. **[MAJOR, FIXED] Case-insensitive CFB namespace collisions.** The D1
   namespace checks compared raw strings, but the CFB directory namespace is
   case-insensitive (cfb 0.12.1 `compare_names`, per-char simple uppercase),
   so a clone under `COMPONENT_1` beside `Component_1`, a case-variant alias
   of another component's lib_ref, or a case-variant "self-alias" (which the
   save path does NOT storage-share — sharing requires exact equality) all
   passed `with_library_edit` and then failed inside `SchLib::save` with a
   low-level `create_storage` error. Fixed at the declared boundary:
   final lib_ref uniqueness and the global alias/lib_ref name-set check now
   compare **storage-namespace keys** — `cfb_storage_fold(sanitize_cfb_name(
   name))`, where `cfb_storage_fold` (new, `schlib.rs`) applies the
   container's per-character simple-uppercase fold — and collision errors
   name both colliding strings plus the case-insensitivity rationale. The
   exact self-alias remains legal; names colliding only after illegal-char
   sanitize (`A_B` vs `a?b`) are rejected too. Full-name case variants are
   rejected even when >31 chars (one namespace rule, no length special case).
   These stay mutation-boundary policies: wild loaded state captures verbatim
   (`validate_schlib_invariants` unchanged).
   Test: `namespace_collisions_are_rejected_case_insensitively`.
2. **[MINOR, FIXED] Order-dependent SectionKeys truncation collisions.**
   `build_section_key_for_name` registered short-name keys only as it walked
   the header, so a >31-char name truncating onto a LATER short name produced
   two colliding storages and save failed (`AAAA…`×42 before `A`×31), and
   `generate_unique_key`'s used-set was case-sensitive (two long case-variant
   names could truncate to case-colliding keys). `build_section_keys` is now
   two-pass — every no-entry name claims its case-folded storage identity
   first, then keys are generated against the complete claimed set (pending
   names deduplicated, so a long self-alias no longer burns a wasted key) —
   and `generate_unique_key` checks/records case-folded membership. Long
   truncation collisions now save correctly in ANY header order.
   Test: `section_keys_disambiguate_truncation_collisions_in_any_header_order`
   (long-before-short prefix pair + case-variant 32-char pair, save/reopen
   typed equality).
3. **[DEFERRAL RECORDED] Plan D item 1's sync-side `SchLibPatch`
   library-level op variants.** Not implemented in D1 and no longer owed by
   it: the ORCHESTRATOR SCOPE DECISION of 2026-09-16 (recorded in the step-2
   completion plan, increment D item 1) moves the `SchLibPatch` op-variant
   surface to increment E, where intent elaboration consumes the D1 format
   API (`with_library_edit`) — consistent with the redteam finding [3] seam
   split (format-level engine vs sync-side elaboration). Increment E MUST
   add typed library-level ops (alias set changes, image-asset
   insert/replace/remove, component create/remove, font appends) to
   `crates/altium-sync/src/schlib.rs` and elaborate intents through them;
   until then namespace transitions are format-API-only. This supersedes the
   D1 section's "plan D items 1 and 3" claim: D1 delivered item 1's
   FORMAT-LEVEL clauses and item 3.
4. **[MINOR, FIXED] §8c rule 3 strengthened to per-pairing-target
   semantics.** The D1 guard checked name-set resolvability, which allowed
   removing a record's concrete pairing target when a duplicate-name or
   case-variant sibling still satisfied resolution (silent rebinding to a
   different asset body; the store explicitly permits duplicate names).
   `with_library_edit` now tracks every INPUT asset's position through the
   ordered ops and requires each embedded reference's concrete pairing
   target (first exact match, then first case-insensitive — the native
   order) to survive with its name intact. Same-name payload replacement,
   orphan removal, wild dangling preservation, and same-edit
   referencing-record removal all stay legal, and errors now name the input
   pairing position. The resolution logic is one shared helper —
   `validation::resolve_image_asset_reference` — also used by the
   creation-time `check_embedded_image_reference`, eliminating the
   duplicated exact/case-insensitive logic (drift-class fix); likewise
   `SchFontAppend::native_default()` now uses the shared
   `DEFAULT_FONT_NAME`/`DEFAULT_FONT_SIZE` constants.
   Tests: duplicate-name + insert-shift assertions in
   `image_asset_ops_wrap_schlib_resources_with_reference_validation`,
   pairing-order assertions in the validation unit test.
5. **[MINOR, FIXED] Shadowed-name alias release.** `SchAliasOp::Remove`
   resolved against final components first and reached a removed component's
   aliases only when NO final component carried the name, so the
   content-reset flow (remove aliased `X` + New clone under `X` in one edit)
   could not release the alias and the dangling check made the edit
   inexpressible. Resolution is now: the final component actually CARRYING
   the alias, then the removed input component of that name — unambiguous
   because a New slot cannot carry an alias while removes apply (additions
   come after). Module doc updated; test:
   `removed_component_aliases_release_when_a_new_component_retakes_the_name`
   (release, retarget, and both not-present error paths).
6. **[MINOR, FIXED] Missing success-path/two-step image-asset test.**
   Added `image_asset_insert_unlocks_two_step_record_creation`: creation
   referencing a missing asset blocked on the input context → declared
   `SchImageAssetEdit::Insert` recomputes the context → `with_children` on
   the RESULT snapshot creates the embedded image record → save/reopen typed
   equality → the inserted asset is now a protected pairing target (removal
   blocked). This is the image analogue of the font two-step test, closing
   the untested half of the §8c rule 2 composition.
7. **[MINOR, FIXED] Untested `with_library_edit` rename rejection.** Added a
   `ReplaceWith(0, component(1))` assertion (same context, different
   lib_ref) covering the "renaming is unsupported (clone under the new
   name…)" branch in `resolve_component_edit`.
8. **[MINOR, FIXED] Memo §8c rule 2 "same batch" wording.** The memo's
   "inserted by the same batch's library-level image-asset op" is not
   expressible under the adopted composition (record creation runs through
   `with_children` against a captured context); the supported path is the
   two-step flow. A dated errata item (4) was appended to the contracts memo
   assigning increment F the §8c rule 2 rewrite alongside the §8a one; the
   D1 module doc states the two-step path explicitly.

Post-fix verification (verbatim, after the last edit): see the increment's
final report — `cargo test -p altium-format --lib` 474 passed;
`cargo test -p altium-sync` 18 passed; `cargo check -p altium-cli` clean;
corpus gate exit 0, all checks true on all three fixtures.

## Increment D2 — structural editing of the footprint/implementation chain (implemented)

Contract authority: memo §1/§6 incl. errata item 1 (ImplementationList wire
IndexInSheet 0), the ownership worklog's serialized compatibility-container
chain rule (ImplementationList → Implementation → ImplementationMap →
MapDefiner, ParameterList — preserved as serialized, never flattened), and
red-team finding [12]. Extends the B/C batch engine in
`crates/altium-format/src/schlib/snapshot/children.rs`; new types exported
through `schlib`, `api`, and the snapshot module.

### API shape

- `SchChildBatch` grows `footprints: Option<Vec<SchFootprintEdit>>`;
  `SchFootprintEdit::{Keep(usize), Replace(usize, SchFootprintInit),
  New(SchFootprintInit)}` indices are positions in the A2
  `footprint_maps().footprints` projection of the immutable input (PCBLIB
  implementations only, case-insensitive model type — same membership
  predicate as the view). Same final-sequence, deletion-authorization, and
  atomicity semantics as B/C.
- `SchFootprintInit { model_name, description, is_current, pin_pad_maps:
  Option<Vec<SchPinPadMapInit>> }`; `SchPinPadMapInit { pin, pads }` (pin
  designator → ordered pad designators, one MapDefiner each).
  `pin_pad_maps: None` = unaddressed (B/C optional-field semantics): on `New`
  no mappings, on `Replace` the original MapDefiner records are preserved
  verbatim; `Some(maps)` rebuilds the definers. `from_view` populates all
  axes explicitly (so a from_view Replace rebuilds definers; leave the field
  `None` to preserve them). No owner axis exists: the chain hard-codes
  owner_part_id 0 natively (memo §1).
- Each member is the Implementation record PLUS its whole transitively-owned
  subtree (map, definers, parameter list, implementation-owned parameters,
  wild descendants), moved/replaced/deleted as a unit. Deletion removes the
  whole subtree — assembly's retained-orphan check guarantees no orphans —
  and requires `allow_deletion`.
- The batch-plan machinery grew created-record ownership: `PlannedEntry::
  Created { local_id, owner: PlannedOwner::{Root, Original(addr),
  Created(local_id)} }`, resolved to `RecordOwner::New` stream positions by
  `to_items` (creation stays Data-only; the Additional-creation rejection now
  names all four collections).

### Non-PCBLIB implementations are outside-collection content (red-team [12])

SIM/PCB3D/SI/… chains are preserved verbatim in their original relative
stream slots under EVERY footprint edit, including complete replacement with
deletion authorized. The stream plan consumes member subtrees and emits the
final entry assigned to slot m at original member m's Implementation
position; extras (new subtrees) follow the last member-subtree record (or
the Data end when no members existed), so reordering permutes only footprint
slots and interleaved non-PCBLIB siblings plus the shared ImplementationList
keep their positions. The exact red-team [12] test exists:
`complete_footprint_replacement_never_touches_the_sim_chain` (footprints:
`Some(vec![])` + `allow_deletion` on a component holding a full SIM chain —
SIM records verbatim, list preserved, save/reopen typed equality).

### Chain materialization evidence (creation)

All from the decompiled AD26 sources
(`/home/dev/projects/old-altium-cli/AD26-dotnet/Altium.Sch.DataModel/`) and
gate-fixture wire bytes (dumped 2026-09-17):

- **One shared ImplementationList per component, emitted UNCONDITIONALLY**
  by `SchDataComponent.AddToListForSave` (Objects/SchDataComponent.cs:861-885
  creates the synthetic list even with zero implementations). Fixture bytes:
  every gate-fixture component carries exactly one bare `|RECORD=44` (ICs 32
  components / 32 lists / 26 implementations). Creation therefore REUSES the
  list owning the existing implementations (multiple distinct owner lists =
  hard error, wild), else the first bare list, and creates one only when the
  component has none at all — owner root, owner_part_id 0, IndexInSheet 0
  (ctor `SetOwnerPartId(0)` + `SetIndexInSheetForSave(0)`,
  Objects/SchDataImplementationList.cs:9-14). Remove-all does NOT delete the
  list (outside-collection container; AD26 re-emits one regardless).
- **Implementation record**: model_type `PCBLIB` (EngineObjects/
  SchImplementation.cs:31 `ModelTypePCBLIB`), fresh 8-char UniqueId (fixture
  `UNIQUEID=TFUCFNKK` etc.), owner_part_id 0, IndexInSheet −1 (fixture bytes;
  natively via the IndexOf-mismatch — the implementation is Removed from the
  component and Added to the synthetic list, so `SetIndexInSheetForSave(
  IndexOf)` yields −1), and the native single default datafile link
  `{location: "", entity: model_name, kind: "PCBLib"}` — AD26 synthesizes
  exactly that for a link-less PCBLIB implementation in
  `HandleAfterImportEvent` (EngineObjects/SchImplementation.cs:760-766) and
  every corpus footprint carries it (`DATAFILECOUNT=1|MODELDATAFILEENTITY0=
  <model>|MODELDATAFILEKIND0=PCBLib`, location key absent).
- **ImplementationMap and ParameterList are emitted unconditionally per
  implementation**: the map is a ctor-created field object walked by the
  save iterator (SchDataImplementation.cs ctor + GetFieldObjects;
  SchDataContainer.GetIteratedObjectList → ProcessFieldObjects), and
  `SchDataImplementation.AddToListForSave` creates the synthetic
  ParameterList unconditionally. Gate fixtures: RECORD 45/46/48 counts are
  exactly 1:1:1 (26/26/26, 58/58/58, 381/381/381). A new footprint therefore
  materializes Implementation + Map (owner_part_id 0, IndexInSheet 0 per
  ctor; no UniqueId — fixture RECORD=46 carries only OWNERINDEX) + one
  MapDefiner per pin-pad map + ParameterList (0/0), in the native subtree
  order impl→map→definers→parameter list (wire-confirmed in Custom.SchLib).
- **MapDefiners ARE objectList children of their map** and carry 0-based
  sibling ordinals (wild bytes: Custom.SchLib `RECORD=47 …` first definer
  key-absent ⇒ 0, second `INDEXINSHEET=1`). `occupies_object_list_slot` was
  corrected: MapDefiner now occupies a slot, so rebuilt/new definers get
  their ordinals through the shared memo-§6 touched-owner renumbering and
  untouched maps preserve stored values verbatim. In the same pass,
  ParameterList-OWNED parameters were reclassified as non-objectList content
  (they serialize INDEXINSHEET=-1 through the same IndexOf-mismatch — wild
  bytes in Custom.SchLib show `RECORD=48` followed by `RECORD=41|…|
  INDEXINSHEET=-1|OWNERPARTID=-1`); previously a touched ParameterList would
  have renumbered them wrongly (latent — no B/C path ever touched one).
- **Duplicate pin designators rejected** (case-insensitive): AD26's
  `AddMapDefiner` silently DROPS a definer whose designator interface is
  already mapped (Objects/SchDataImplementationMap.cs, OrdinalIgnoreCase
  dictionary), so writing duplicates would be silent data loss on the next
  AD26 load. New-footprint model_name must be non-empty (the native default
  datafile link derives its entity from it); Replace writes the authored
  name verbatim (from_view round-trips of wild empty-named footprints stay
  identity-safe).

### is_current resolution (deliverable 3)

The native model HAS a single-current invariant: exactly one current
implementation per MODEL TYPE, enforced by
`SchDataComponent.ValidateImplementationsCurrentState`
(SchDataComponent.cs:974-1030: a type's sole implementation is forced
current; a count ≠ 1 among several resets all and marks the
first-encountered one) — and it runs ON THE SCHLIB V5 IMPORT PATH
(SchDataImporterLibraryV5.cs:239 per-component, :385 whole-warehouse), i.e.
AD26 silently REPAIRS wild states at load. The wild class is real, and it
covers TWO of the three gate fixtures: lucashudson-Connectors stores no
ISCURRENT key anywhere (58 `RECORD=45` implementations, 0 `ISCURRENT`
occurrences file-wide) and arthurbenemann-STM32F4 likewise (381
implementations, 0 `ISCURRENT`); only ICs is coherent (26 implementations,
26 `ISCURRENT`). Our resolution:

- Capture stays verbatim (no load repair, no report — repairing would
  mutate a third of the gate corpus; the memo sanctions load normalization
  only where it resolved one explicitly).
- Mutation boundary blocks only NEW divergence: a batch may neither break a
  coherent input's single-current state nor author an ACTIVE current claim
  (a Replace changing the stored value, or a New with is_current true) into
  an incoherent final state. Pre-existing wild states pass through verbatim
  when the batch writes no current value (Keeps, value-equal Replaces,
  non-current News) — AD26's load repair of them is unchanged by such an
  edit. Wild states are resolvable in the same batch by authoring
  is_current into a coherent assignment (exactly the repair AD26 would
  apply). The scope is the PCBLIB collection only; non-PCBLIB types are
  outside-collection preserved content.

### Additional-stream chain records (deliverable 5 policy)

REJECTED under the same conservative freeze shape as pins, strengthened to
Keep-only: while any chain-kind record or member-subtree record lives in the
Additional stream, the only accepted footprints edit is the
all-positional-Keep no-op. Evidence for rejection over preservation-with-
edits: the native exporter CANNOT emit chain records into Additional
(implementation kinds are absent from the Additional-root object set,
ownership worklog `SchDataConsts.cs:79-91`, and the component save path
emits the chain through the Data list exclusively), and the corpus carries
zero Additional streams; member subtrees may also span streams via legal
Additional→Data links, which the single-stream slot plan does not model.
Constructible programmatically → covered by
`additional_stream_chain_records_freeze_footprint_edits`.

### Conflict rules

A footprint subtree containing a Pin record is a hard error under any
footprints edit (pin sidecar ordering belongs to the frozen pins contract);
a subtree record that is simultaneously a `graphics()` member conflicts when
BOTH collections are edited in one batch (hard error naming the record);
with graphics untouched, the footprint edit owns the record (subtree
semantics — deletion removes it with the member). Root-owned parameters
cannot overlap footprint scope by construction.

### Test evidence (fixture-free, default suite)

New/extended tests in `children.rs` (8):
`footprint_creation_materializes_full_chain_and_list_rules` (blank component
→ full chain incl. created list with all sentinels asserted field-by-field,
bare-list reuse [the ICs shape], empty-model-name/duplicate-pin/sole-must-
be-current rejections, save/reopen typed equality);
`footprint_creation_alongside_sim_reuses_shared_list_and_leaves_sim_untouched`;
`footprint_replace_rebuilds_definers_and_preserves_unaddressed_state`
(rebuilt definers with fresh 0-based ordinals, unique_id/datafile/GUID/
model_type-casing preservation, unaddressed-pin_pad_maps verbatim
preservation, implementation-parameter survival, coherence rejection);
`complete_footprint_replacement_never_touches_the_sim_chain` (red-team [12]:
remove-all authorized + SIM verbatim + list preserved + weight −8 + the
unauthorized listing; remove-one keeping the sibling);
`footprint_reorder_permutes_only_footprint_slots` (slot permutation with the
SIM chain fixed between slots, restore-to-original equality, all-Keep no-op
identity, duplicate/range rejections);
`additional_stream_chain_records_freeze_footprint_edits`;
`footprint_conflicts_and_wild_current_states` (pin-in-subtree, graphics
conflict + subtree-owned deletion, wild double-current pass-through/active-
claim block, zero-current [STM32F4 shape] pass-through + block + same-batch
resolution); `multi_collection_batch_edits_all_four_collections_against_
immutable_input` (parameters + graphics + pins + footprints in one batch).
SIM-verbatim assertions compare typed records with `owner_index` normalized
(stream-address plumbing that assembly legitimately remaps; parent kinds
stay validated by `assemble_records`).

### Corpus example (footprint structural pass)

`check_schlib_snapshots.rs` gains `footprint_structural_pass` from a fresh
open, per component: (1) replace the first footprint's description
(pin_pad_maps unaddressed, is_current value-preserving) — save/reopen typed
equality + projection equality; (2) add probe footprint `PROBE_STEP2D2`
(one pin-pad map from the first existing pin when the component has pins;
is_current true only when the component had no footprints) — save/reopen +
probe projection; (3) authorized removal of the probe + description restore
— save/reopen AND full equality with the ORIGINAL input snapshot; the
non-PCBLIB implementation count is asserted invariant at every stage. New
JSON fields enforced in `main()`: `structural_footprint_replace_equal`,
`structural_footprint_probe_equal`, `structural_footprint_removal_equal`,
`structural_footprint_projection_equal`,
`structural_footprint_non_pcblib_invariant` (plus the informational
`structural_footprints_replaced`). All 223 gate components run all three
stages — including all 135 STM32F4 components in the zero-current wild
class (56 + 26 + 135 descriptions replaced).

### Verification (all default suites, fixture-free)

- `cargo test -p altium-format --lib` — 481 passed, 0 failed.
- `cargo test -p altium-sync` — 18 passed, 0 failed (6 pre-existing
  `collection.rs` dead-code warnings, unchanged).
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- Corpus gate: all checks — incl. the five footprint fields — true for
  lucashudson-Connectors (56), ICs (32), arthurbenemann-STM32F4 (135);
  exit 0.

### Known items deferred / flagged out of D2

- `MODEL_TYPE_PCBLIB` (`"PCBLIB"`) and `MODEL_DATAFILE_KIND_PCBLIB`
  (`"PCBLib"`) are local consts in children.rs — FLAGGED for promotion into
  `altium-format-types::constants` after user discussion (same class as the
  `"|&|"` swap separator). The promotion must unify EVERY raw literal site
  onto the promoted constants: `schlib/snapshot/views.rs:532` (the A2
  projection membership predicate — the one site that MUST stay in lockstep
  with the engine predicate at children.rs `resolve_footprint_edits`,
  because `SchFootprintEdit` index-space equivalence with
  `footprint_maps().footprints` depends on the two predicates matching),
  `api/sch_common.rs:410` and `:501`, `api/schdoc_write.rs:527`, and
  `api/schlib_write.rs:138`. Predicates are character-identical today
  (`eq_ignore_ascii_case("PCBLIB")`), so no behavior drift exists yet.
- `new_blank_ad26` components carry NO ImplementationList, whereas an
  AD26-saved blank always carries one (unconditional emit). Pre-existing
  blank-factory shape, observed not changed (creation handles both);
  queued as a user decision alongside the api-layer `default_base()` item.
- Replace preserves datafile links verbatim even when model_name changes,
  so a renamed footprint keeps its original link entity (task-mandated
  preservation; AD26 itself only synthesizes a link when NONE exist).
- The E obligations (sync-side footprint addresses through the
  owner_group-filtered mapping; SchLibPatch library-level ops) are
  unchanged.

## Increment D2 — review findings resolution (2026-09-17)

Independent review returned 7 findings (three were the same issue); each was
verified against the code/fixtures before acting. All were minor; none
revealed a behavior defect.

1. **Wrong record count in a test comment (three duplicate findings) —
   CONFIRMED, FIXED.** `children.rs`
   `complete_footprint_replacement_never_touches_the_sim_chain`: the comment
   read "12 records: 2 impl + 2 maps + 1 definer + 2 lists + 1 param" but
   its own enumeration sums to 8 and both adjacent assertions (record-count
   delta and header-weight delta) correctly check 8 (FP_A subtree
   impl+map+definer+ParameterList+parameter = 5, FP_B impl+map+ParameterList
   = 3). Comment corrected to "8 records"; code unchanged. (The "[12]" in
   the test's worklog description is the red-team finding NUMBER, not a
   record count.)
2. **PCBLIB membership literal in views.rs omitted from the promotion flag
   — CONFIRMED, FIXED (docs).** The flagged constants-promotion item above
   now enumerates every raw-literal site — most importantly
   `schlib/snapshot/views.rs:532`, the A2 projection predicate that must
   stay in lockstep with the engine's `MODEL_TYPE_PCBLIB` predicate for
   `SchFootprintEdit` index-space equivalence — plus `api/sch_common.rs:410,
   :501`, `api/schdoc_write.rs:527`, `api/schlib_write.rs:138`. Verified
   character-identical predicates today; no behavior defect.
3. **Zero-current wild-class census understated — CONFIRMED, FIXED (docs).**
   Re-counted fixture bytes: lucashudson-Connectors has 58 `RECORD=45` and
   ZERO `ISCURRENT` occurrences (same wild class as STM32F4's 381/0); ICs is
   26/26. The is_current evidence paragraph above and the children.rs module
   docs now state that two of three gate fixtures are in the wild class. The
   engine's no-new-divergence rule already handled both identically (the
   corpus gate's value-preserving replace stage exercises all 56 Connectors
   components), so this was an evidence-census correction only.
4. **pin_pad_maps rebuild resets wild MapDefiner base fields — CONFIRMED,
   FIXED (docs); optional hard-error DECLINED.** A `Replace` addressing
   `pin_pad_maps: Some(..)` deletes the original definers and rebuilds from
   typed wire defaults carrying only `DesIntf`/`DesImp{i}`; a wild
   non-default `SchPrimitiveBase` field on a replaced definer (e.g.
   `graphically_locked`, `union_index`) is reset, so a `from_view`
   round-trip Replace over such a definer is not byte-identity. This is an
   explicitly authored complete-rebuild axis, `SchMapDefiner` has no other
   typed fields, native definers only ever carry ctor base state (AD26
   creates them fresh via `AddMapDefiner`), and `None` preserves verbatim —
   now stated explicitly in the module docs' Replace bullet. The suggested
   optional hard-error on non-default doomed-definer base state was
   considered and declined for D2: unlike the definer-with-descendants guard
   (which prevents deleting whole RECORDS the axes never addressed), it
   would freeze `pin_pad_maps` editing on hypothetical wild files with no
   escape short of hand-clearing base flags, and the structural comparison
   would need per-field exclusions (owner_index / index_in_sheet ordinals
   are legitimately definer-specific) — a fragile guard against a state
   with zero corpus occurrences. Revisit in F if evidence of the wild class
   appears.
5. **R7/R8 model-value constants local to altium-format — CONFIRMED as the
   already-flagged deliberate deferral; NO code change.** Promoting
   `MODEL_TYPE_PCBLIB`/`MODEL_DATAFILE_KIND_PCBLIB` into
   `altium-format-types::constants/model.rs` requires the queued user
   discussion first (plan standing rule; CLAUDE.md). The flagged item above
   now carries the complete replacement-site list so the post-discussion
   promotion in F unifies all spellings onto a single source of truth.

Post-fix verification (docs + comment-only changes to Rust files):
`cargo test -p altium-format --lib` 481 passed; `cargo test -p altium-sync`
18 passed; `cargo check -p altium-cli` clean; `cargo fmt --check` clean;
corpus gate exit 0 with all checks true on all three fixtures.

## Increment E1 — keyed intent elaboration + library-level ops (implemented, 2026-09-17)

Contract authority: `docs/proposals/snapshot-intent.md` (keyed partial/complete
alignment, §"Collections"), memo §8a/§8c rule 4, the D1 deferral recorded in
the D1 findings resolution (item 3), and the B/C-era E obligations that touch
keyed collections. Positional pins/graphics, parts, layout/auto-size, swap
fan-out, and image-source graphics remain increment E2 and are clean hard
errors (below). New module `crates/altium-sync/src/schlib/elaborate.rs`;
`crates/altium-sync/src/{collection.rs,schlib.rs}` reworked;
`SchLibSnapshot::component_lib_references()` added in altium-format
(cheap key access, no aggregate clones).

### Planner class rules (collection.rs)

- `CollectionClass::{Keyed, Positional}` with a total
  `collection_class(CollectionKind)` mapping (Components/Parts/Parameters/
  Aliases/Footprints keyed; Pins/Graphics/Maps positional);
  `CollectionContext::class()` exposes it and error text names it.
- Partial scope = updates in original slots + appends, for BOTH classes. The
  single named enforcement point `reject_partial_interior_insert` hard-errors
  every `Insert` whose boundary is not the append boundary
  (`PartialInteriorInsert`), citing the contract sentence.
- INTERPRETATION RECORDED (task wording "interior Insert and sparse selection
  are hard errors for BOTH classes"): a sparse `Replace` index set is the
  LEGITIMATE product of keyed matching and of no-op thinning (both classes
  produce them), so the planner cannot treat Replace-sparsity as an error;
  the only sparse placements the request shape can express are interior
  inserts (rejected) and removals. The positional authored-prefix rule
  ("N entries describe indices 0..N-1") is enforceable only where index
  provenance is known — the elaboration boundary — and the E1 positional
  elaborator (footprint maps) produces contiguous prefixes by construction;
  the pins/graphics elaborators inherit this obligation in E2. Documented in
  the module docs.
- DEVIATION RECORDED: `IndexedEdit::Remove` was DELETED, not "activated".
  The ratified contract leaves it no legal constructor — `delete: allow`
  requires `mode: complete`, and complete-mode removal is expressed by
  omission — so partial removal is now inexpressible by construction (the
  strongest form of the required hard error) instead of a permanently dead
  enum variant that would keep a build warning alive.
- New class-specific complete rule: a complete POSITIONAL request must pair
  item N with original destination N (`PositionalCompleteMisalignment`;
  snapshot-intent "Positional item N uses original destination N for
  preserved fields") — substitution-in-place and appends legal, re-pairing
  (`Keep(0)` at final position 1) rejected. Keyed complete reorders freely.
- The formerly dead surface (`Insert` appends, `CollectionRequest::Complete`,
  all of `FinalItem`, `materialize`, `items`, `removed`,
  `InputLengthChanged`) is now production-consumed by elaboration; all six
  known dead-code warnings are gone naturally and `cargo build -p
  altium-sync` is warning-free. No `#[allow(dead_code)]` exists in the crate.

### SchLibPatch library-level ops (the D1 deferral, RESOLVED)

The 2026-09-16 orchestrator decision (D1 findings item 3) obligated E to add
the sync-side op surface; it now exists:

- `SchLibLibraryOp::{AliasAdd, AliasRemove, ImageAsset(SchLibImageAssetOp::
  {Insert, Replace, Remove}), FontAppend(SchFontAppend),
  ComponentCreate(SchNewComponent), ComponentRemove{lib_reference}}` +
  `SchLibPatch::from_library_ops`. Image payloads are
  `image_asset::CapturedImageAsset` values — hash/path-verified at
  construction AND re-verified before every `apply_in_memory` (a tampered
  bundle blocks apply without mutating the target). `ComponentRemove` is
  itself the removal authorization on this imperative surface and does NOT
  auto-release aliases (D1's explicit-release contract; tested); unknown and
  duplicate removal names are errors.
- Lowering: every patch application performs exactly ONE
  `SchLibSnapshot::with_library_edit(SchLibEdit)` (fonts → image ops in
  relative order → component slots → alias removes-then-adds), then component
  content replacements against the RESULT (the D1 two-step order).
  `ComponentReplacement::index` addresses the post-library-edit sequence
  (documented; equals the input slot when membership/order is unchanged).
- `SchLibPatch` now caches its reviewed result at construction
  (`resulting_snapshot()` returns the cache): created components take fresh
  UniqueIds, so recomputing at apply time would silently diverge from the
  reviewed state (stability asserted in tests and exercised by the corpus
  pass). `apply_in_memory` = whole-input `StaleInput` precondition
  (unchanged) + image-asset re-verification + materialize of the cached
  result.
- Inspection surface (exact-indexed-diff foundation, no rendering):
  `replacements()` (each with `before()`/`after()` snapshots and
  `scalar_changes()` — Debug-rendered before/after pairs of the four cheap
  scalars), `created_components()`, `removed_components()`,
  `alias_additions()`/`alias_removals()`, `image_asset_ops()`,
  `font_appends()`, `is_empty()` (now covering ALL op kinds, not just
  replacements).

### Elaboration semantics (`SchLibPatch::from_intent`)

`from_intent(input: &SchLibSnapshot, intent: &SchLibIntent) ->
Result<SchLibPatch, SchLibPatchError>`; schema_version must equal
`INTENT_SCHEMA_VERSION`. Definition scope only; `Absent` never writes — on
matched AND on created components (a created Blank keeps its factory shape
wherever the intent is silent, including its Comment parameter, which is a
real keyed destination entry for the created component's parameters
collection).

- **Components**: keyed by exact `lib_reference`; authored scope enforced by
  the planner (partial = update matched in slot + append-create in source
  order, omission preserves; complete = final membership/order, omission
  removes only under `delete: allow`, lowered to the D1 removal with the
  authorization flag). Renames are never inferred. A removed component's
  aliases are released in the same library edit (the removal authorization
  covers the aggregate; without this, aliased-component removal would be
  inexpressible). Duplicate authored names block; the destination-side check
  exists defensively (library invariants already forbid duplicate lib_refs).
- **Scalars**: the four existing properties via their FieldPolicies through
  the extracted shared `elaborate_component_properties` (the `property!`
  elaboration `component_properties` also uses).
- **Parameters**: keyed CASE-INSENSITIVELY — investigated and resolved:
  AD26's `SchDataContainer.GetParameterByName` compares with
  `StringComparison.OrdinalIgnoreCase` (SchDataContainer.cs:633-647), so two
  case-variant names are ONE native parameter; our fold is per-char simple
  uppercase (ordinal, no locale). The AUTHORED spelling is written on update
  (a real byte edit that re-elaborates to a no-op) — this holds for a BARE
  mention authoring no fields at all: a case-variant bare mention is a
  rename (idempotent, reviewable via `replacements()`, never silently
  ignored), so mirror-intent generators must emit destination spellings
  (recorded consequence; findings resolution below). Unauthored
  text/is_hidden preserve the destination on update and resolve
  `definition()` defaults on creation (text clear/default = empty string per
  the PARAMETER_TEXT registry). Elaborated-equal ⇒ no edit (partial) / Keep
  (complete).
- **Aliases**: keyed by exact string; partial appends unmatched entries;
  complete defines membership with removals under allow. RECORDED
  LIMITATION: authored ORDER is honored only as append order — surviving
  aliases keep destination order because the D1 op surface splices/appends
  and AD26 itself stores a sorted SET (alias order carries no native
  meaning; D1's recorded deviation).
- **Footprints**: keyed by exact `model_name`; `source:
  SourceDefinitionRef` is provenance only, never a matching key.
  `explicit_maps == false` ⇒ `pin_pad_maps: None` (definers preserved
  verbatim); maps content without an explicit body is an inconsistent-intent
  error. Explicit maps elaborate POSITIONALLY through the planner (partial =
  replace indices 0..N-1 + append, tail preserved; complete = final list,
  shrink under `delete: allow`), materialized against the destination-derived
  init list via `CollectionPlan::materialize`; a final list equal to the
  destination collapses back to `None` (no definer rebuild ⇒ wild MapDefiner
  base state untouched on no-op). RECORDED LIMITATION: `is_current` is not
  intent-expressible — matched footprints preserve the stored value
  (value-preserving on the wild zero-current class), a created footprint is
  current only when the destination PCBLIB collection was empty and it is
  the first authored one; a complete replacement removing a coherent
  component's current footprint while keeping or creating others therefore
  hard-errors in D2's coherence guard (E1 staging restriction: keep the
  current footprint, or empty the collection first — complete +
  `delete: allow` with zero authored footprints — and author the new set in
  a follow-up patch against the then-empty collection; single-patch
  whole-set replacement is NOT expressible, per the findings resolution
  below). MULTI-PAD GUARD (findings fix): native MapDefiner records may
  carry several pads while a pin-pad intent pair expresses exactly one, so
  an authored maps entry that positionally addresses a multi-pad
  destination definer is a hard error naming the footprint, position, pin,
  and pad count — never a silent narrowing to one pad; unaddressed tail
  definers are preserved with all pads, and complete + `delete: allow` with
  a shorter authored list removes a multi-pad definer explicitly
  (authorized deletion).
- **Component creation**: unmatched names create `SchNewComponent::Blank`
  with elaborated keyed children (parameters/aliases/footprints) and
  authored scalars; the sole created footprint is current per the native
  rule.
- **Duplicates**: on either side of a MANAGED collection (authored items or
  an authored scope declaration), duplicate keys under the collection's
  matching equivalence block with both positions and both spellings
  reported; unmanaged collections never block on wild duplicates. A
  destination-side duplicate ALIAS is wild loaded state that no public
  mutation path can construct (D1 enforces global alias uniqueness), so the
  destination guard is exercised through the constructible case-variant
  parameter case; the alias guard stands for wild loads.
- **E2 staging restrictions**: authored pins (items OR a scope declaration;
  placement lives inside pins), graphics, parts, or part_count on ANY
  component intent — matched or created — hard-error naming the component,
  the offending collection, and increment E2; the created-component message
  additionally names the creation staging restriction (keyed children only).
- **No-op detection / replan-empty**: per-field (unauthored preserves), per
  child (elaborated == destination ⇒ Keep/no edit), per collection
  (identity plan ⇒ no batch entry), per component (after == before ⇒ no
  replacement), per patch (`is_empty()`); apply → re-elaborate the same
  intent is empty, asserted in unit tests and per corpus file.
- **Errors**: `SchLibPatchError` gained `Spec{context}`,
  `Elaboration{context, detail}`, `ImageAsset{context}`; every
  block/mode/duplicate/unsupported error names the owner (`SchLib` /
  `component 'X'` / `component 'X' footprint 'M'`), the `CollectionKind`,
  and the index/key — the `CollectionError` rendering carries
  owner/collection/index exactly like the A2 address vocabulary.

### Test evidence (fixture-free; `crates/altium-sync`)

collection.rs (9): total class mapping; partial update+append with sparse
Replace legitimacy and repeatability; interior-insert rejection for BOTH
classes at every interior boundary with the append boundary legal; keyed
complete free reorder vs positional position-pairing (aligned
substitution/append pass, three misalignment shapes rejected); equal-value
keyed reorder keeps distinct slots; complete omission permission +
partial-cannot-remove; repeated-slot rejection both modes; out-of-range /
mode-mismatch / input-length-changed context errors; empty-collection
creation + authorized empty final order.

schlib/elaborate.rs (12): keyed partial parameters update (case-variant key,
authored spelling wins, unauthored is_hidden preserved) + append +
save/reopen + replan-empty; complete parameters identity no-op, reorder
under deny, omission deny-blocks/allow-removes matrix; complete component
reorder binding content edits to post-library-edit positions (+
replan-empty); duplicate keys block (destination case-variant parameters
with both spellings reported, unmanaged pass-through, authored parameter
dups, authored alias dups, authored component dups); alias add/remove via
intent (partial append, complete deny-block, complete allow removal
restoring the input exactly); footprint replace via intent (unaddressed
definers preserved verbatim, explicit partial maps rebuild index 0 + tail
preserved, complete maps deny-block/allow-shrink, unmatched-model creation
non-current beside the stored current, all with save/reopen + replan-empty);
component create with keyed children (Blank + Comment destination entry,
alias, sole-current footprint, cached-result stability) + complete remove
under permission restoring the pre-creation state with alias release +
deny-block + partial-omission preservation; E2 hard errors
(pins-items/pins-scope-only/graphics/parts/part_count + created-component
staging message); full-mirror no-op intent empty for EVERY collection;
schema-version and inconsistent-maps rejections; multi-pad definer guard
(addressed positions error with footprint/position/pin/pad-count in both
modes, unaddressed multi-pad tail survives a prefix rebuild with all pads +
save/reopen + replan-empty, complete+allow shorter list removes it
explicitly — findings fix); inconsistent programmatic scopes rejected
(span-less non-default on components/pins/parameters/nested maps, and
partial+allow even with a span; span-less partial+deny default stays
unmanaged — findings fix). from_library_ops (2): one
edit with verified image assets (font+image+alias+clone; cached result
stable; tampered bundle blocks apply atomically); explicit alias-aware
component removal (dangling-alias D1 error at construction,
unknown/duplicate names, clone→remove restore-to-input).

### Corpus example (intent pass)

`check_schlib_snapshots.rs` gains `intent_pass` from a fresh open, per file:
(a) programmatic partial `SchLibIntent` setting parameter STEP2E (text +
is_hidden) on component 0 → `from_intent` → `apply_in_memory` → projection +
native save/reopen typed equality → re-elaborate the same intent → empty;
(b) partial append of NEW component STEP2E_PROBE (Blank, one parameter
STEP2E_P, one alias STEP2E_ALIAS) → apply → projection (name, parameter,
alias) + save/reopen equality → replan-empty; (c) a COMPLETE components
intent constructed from the final library minus the probe with
`delete: allow` → removes exactly the probe (removal list asserted, zero
replacements) → apply + save/reopen equality → result equals the stage-(a)
state exactly. New JSON fields enforced in `main()`:
`intent_parameter_equal`, `intent_parameter_replan_empty`,
`intent_create_equal`, `intent_create_replan_empty`, `intent_remove_equal`,
`intent_restore_equal`.

### Verification (all default suites, fixture-free; re-run after the
findings fix pass, 2026-09-17)

- `cargo test -p altium-format --lib` — 481 passed, 0 failed.
- `cargo test -p altium-sync` — 35 passed, 0 failed; `cargo build -p
  altium-sync` — ZERO warnings (the 6 collection.rs dead-code warnings are
  gone naturally).
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- Corpus gate: all checks — incl. the six intent fields — true for
  lucashudson-Connectors (56), ICs (32), arthurbenemann-STM32F4 (135);
  exit 0.

### Known items deferred / flagged out of E1

- E2 owns: positional pins/graphics elaboration (incl. the B/C obligation to
  resolve per-owner-part positional addresses through the
  owner_group-filtered → full-projection mapping with a multi-part test),
  parts/part_count, layout/auto-size wiring, swap fan-out, image-source
  graphics through the memo §8c rule-4 asset ops, the positional
  authored-prefix (0..N-1) enforcement at the pins/graphics elaboration
  boundary, and the complete-scope block on collections containing
  intent-unrepresentable graphic kinds.
  **RESOLVED by increment E2 (2026-09-17): all of the above landed — see
  the E2 section below. The E1 staging errors (pins/graphics/parts/
  part_count rejection, the created-component keyed-children-only
  restriction) are removed.**
- `is_current` authoring (an intent axis or an explicit resolution op) and
  alias ORDER authoring remain unsupported (recorded limitations above).
  E2 candidate (findings item 4): extend the created-footprint
  `first_new_is_current` rule to fire when no SURVIVING original footprint
  is current (not only on an empty destination), which would make
  single-patch whole-set footprint replacement expressible.
- Multi-pad pin-pad definers are intent-unrepresentable (hard-errored when
  addressed, per the guard above); a multi-pad intent axis is a candidate
  E2/F extension.
- The `"|&|"` constant promotion, `current_part_id` resolve op, and
  Additional-pin freeze items are unchanged.

## Increment E1 — review findings resolution (2026-09-17)

Independent review returned 9 findings (1 major, 8 minor; two were the same
worklog sentence). Each was verified against code before acting; fixes
re-verified by the full default suites + corpus gate (all green, recorded in
the Verification section above, now reflecting the post-fix state).

1. **[MAJOR — FIXED] Multi-pad pin-pad definers silently narrowed by
   authored map pairs.** Confirmed: `SchPinPadMapView.pads` documents that
   native files may carry several pads per definer
   (`crates/altium-format/src/schlib/snapshot/views.rs`), while
   `authored_map_init` builds a single-pad init, so an authored maps entry
   pairing with a multi-pad destination position always compared unequal
   and became a Replace that dropped the extra pads without diagnostic.
   Fixed in `elaborate_maps` (`crates/altium-sync/src/schlib/elaborate.rs`):
   any authored position that pairs with a destination definer whose
   `pads.len() > 1` now hard-errors (both modes) naming the footprint,
   position, pin, and pad count, and pointing at the two legal routes
   (leave the position unaddressed, or remove the definer explicitly via
   complete + `delete: allow` with a shorter list — authorized deletion is
   not a silent drop, so removal stays legal). Module docs + the footprints
   bullet above record the rule beside the Symbol/Hyperlink analogue; new
   test `multi_pad_definers_block_addressing_but_survive_unaddressed_tails`
   covers error (both modes), full-pad tail preservation through a prefix
   rebuild (save/reopen + replan-empty), and explicit shrink-removal.
2. **[FIXED] Inconsistent programmatic `CollectionScope` silently ignored.**
   Confirmed: `managed()` keyed off `span` alone, so a hand-built scope
   like `{mode: complete, delete: allow, span: None}` with no items was
   silently dropped by elaborate_parameters/aliases/footprints AND bypassed
   the E2 rejection for pins/graphics/parts; a span-less non-default maps
   scope under `explicit_maps: false` likewise slipped past the
   inconsistent-FootprintIntent check. Fixed: new `validate_scope` walk at
   the `from_intent` boundary over the whole authored tree (components,
   all six per-component collections, nested footprint maps) hard-errors
   on (a) non-default mode/delete without a declaration span and (b) the
   contract-illegal `partial` + `delete: allow` combination even WITH a
   span (snapshot-intent: only `mode: complete` legalizes allow). Nested
   part-item collections need no walk (authored parts items are an E2 hard
   error already). The planner itself intentionally keeps tolerating
   partial+allow defensively (its test asserts allow alone never deletes
   under partial); the boundary check makes the combination unreachable
   from intents. New test
   `inconsistent_programmatic_scopes_are_rejected_not_dropped`.
3. **[FIXED — docs] Worklog `is_current` recovery advice claimed the
   impossible "or replace the whole set" (two findings).** Confirmed by
   code: `first_new_is_current = views.is_empty()` fires only on an EMPTY
   destination collection, so a single complete patch replacing every
   footprint creates all-new entries non-current and D2's coherence guard
   hard-errors (0 currents from a coherent input). The worklog sentence was
   corrected to the actually available routes: keep the current footprint,
   or empty the collection (complete + allow, zero entries) and author the
   new set in a follow-up patch; the elaborate.rs module docs now carry the
   same two-patch route. The E2 candidate extension (fire the rule when no
   SURVIVING original is current) is recorded in the deferred list.
4. **[RECORDED] Bare case-variant parameter mention is a rename.**
   Confirmed: an item authoring NO fields still writes its authored
   spelling, so a case-variant bare mention renames the destination
   parameter (idempotent, reviewable, fail-safe). DECISION: keep the
   behavior — silently adopting the destination spelling would drop
   authored bytes, the worse violation under this increment's no-silent-
   drop rule; the consequence (mirror generators must emit destination
   spellings) is now explicit in the module docs and the parameters bullet
   above, and belongs in the increment-F contract amendment.
5. **[RECORDED → F] Parameter case-insensitive keying deviates from the
   ratified proposal sentence ("exact parent-scoped natural keys").**
   The deviation stands on native evidence; the AD26 citation was
   RE-VERIFIED during this fix pass on this machine:
   `GetParameterByName` compares with `string.Equals(..,
   (StringComparison)5)` (= OrdinalIgnoreCase),
   `Altium.Sch.DataModel.Objects/SchDataContainer.cs` (decompiled tree at
   `~/projects/old-altium-cli/AD26-dotnet`; the in-repo `./AD26-dotnet`
   path does not exist on this checkout). Increment F must amend
   snapshot-intent.md's key contract with the parameter exception + this
   citation.
6. **[RECORDED → F] Alias reorder is membership-only against the
   "surviving keyed entries take source order" contract sentence.**
   A reorder-only complete alias intent elaborates to an empty patch.
   Kept as-is: AD26 stores aliases as a sorted SET, so alias order is not
   part of the native state space and cannot be honored; erroring on
   reorder-only listings would break snapshot-derived mirror intents for
   no data-integrity gain. Increment F must amend the contract sentence
   with the alias membership-only exception (recorded limitation above).
7. **[FIXED — docs] Count inaccuracies.** collection.rs has 9 tests (was
   recorded as 8 — corrected above); elaborate.rs had 12 tests/1923 lines
   before this fix pass (now 14 tests), matching the reviewer's counts;
   `component_lib_references` sits at snapshot.rs:112-120. No code impact;
   the implementer report's cargo/corpus lines were re-executed post-fix
   and are superseded by the Verification section above.

## Increment E2 — positional elaboration, parts, layout, swap fan-out, image sources (implemented, 2026-09-17)

Contract authority: memo §1/§2/§3/§5/§8c, `docs/proposals/snapshot-intent.md`
§145-155/§176-216 (positional alignment, parts projection, per-owner-part
scoping), `docs/proposals/typed-image-resources.md` (asset import contract),
and the B/C/E1 recorded obligations. New module
`crates/altium-sync/src/schlib/positional.rs`; `elaborate.rs` reworked
(the E1 staging errors are GONE — created components elaborate everything);
`schlib.rs` gains `SchLibIntentAssets`, `SchLibPartsShrink`,
`SchLibPatch::{from_intent_with_assets, parts_shrinks}`;
`image_asset.rs` gains the pure `requested_bundle_path`;
altium-format gains `SchLibSnapshot::image_assets()` (the snapshot analogue
of `SchLib::image_resources`, for asset pairing and no-op detection).

### Owner-group scoping (deliverable 1; resolves the B/C obligation)

- Positional pins/graphics align WITHIN one owner group's filtered
  projection. Scope → group mapping (memo §1/§2): `part N` → `Part(N)`;
  component-scope pins → `Part(1)` single-part / `SharedPins(0)` multi-part;
  component-scope graphics → `Part(1)` single-part, HARD ERROR on a
  multi-part component ("no shared graphics owner exists natively; author
  the graphic inside a part block").
- The filtered→full mapping is the group's member-index list over the full
  A2 projection (injective by construction); matching is by RAW owner id so
  a pre-growth projection (`OutOfRange(3)` under the old count) selects the
  same members `Part(3)` selects after growth (the C reclassification rule).
- Partial scope: authored item K addresses filtered index K; the tail is
  preserved; overflow items append with owner FORCED to the group's id.
  Appends land after every surviving full-projection slot, in owner-unit
  order (component scope first, then parts ascending) — recorded decision.
- Complete scope: final membership/order of THAT group only; members of all
  other groups keep their slots verbatim. The planner's positional
  complete-pairing rule (item N ↔ original N) applies per group.
- DESIGN DECISION (recorded): a MATCHED component whose intent changes the
  effective count across the single/multi boundary while managing
  component-scope pins hard-errors — the group mapping would silently
  retarget the authored items between `Part(1)` and `SharedPins`
  mid-patch. CREATED components map by the resulting count alone (no
  existing pins can be retargeted). Similarly, two scopes addressing one
  group (component scope + `part 1` on a single-part component) hard-error.
- CONSEQUENCE (recorded): owner-0 SHARED pins on a SINGLE-part component
  are unaddressable by any scope (component scope means Part(1) there, per
  the memo §1 mapping) — preserved-only until a dedicated address form
  exists. The corpus pass records such components as skipped, never
  silently.
- Blocking matrix: `PartIndependent`/`OutOfRange` owners and owner-0
  graphics can NEVER appear inside a targetable group's filtered projection
  (classification puts them in their own groups), so their preservation is
  structural — asserted in unit tests. The reachable
  intent-unrepresentable members are graphic KINDS (Symbol/Hyperlink/
  other): complete scope over a group containing one always blocks; partial
  blocks only when the authored prefix would ADDRESS one (tail preservation
  is fine); cross-kind positional replacement blocks naming both kinds.
  The matrix is tested against synthetic `SchGraphicView` values (public
  fields) because no public MUTATION path can create such members — the
  A2/B mutation boundary rejects owner-0/out-of-range/unrepresentable
  creation, so the integrated path cannot construct the fixture; recorded
  justification.

### Field semantics (deliverable 2)

- Every `PinIntent` axis elaborates through its policy: name, electrical,
  length (COORD policy), location via `resolve_pin_location` with
  `PinLocationContext::Definition` (independent x/y axis preservation
  included — tested), orientation, is_hidden, designator text (always
  authored; positional alignment never matches on it).
- `hidden_net_name`: the elaborated value must satisfy the format's C
  restriction (stored-or-empty). An unsatisfiable `Set` hard-errors at
  elaboration with the C rationale (binary pin body carries no hidden-net
  field; the pin-owned Parameter storage form is preserved-only).
- Swap fields per memo §5 rule 3: `swap_group` (SwapIdPin) and
  `pair_swap_group` (PairSwapID) are plain policy fields;
  `part_swap_group` authored ⇒ the combined-string PREFIX half is addressed
  (`Some(value)`, suffix `None` = preserved through the split, re-encoded
  per the native rule); unauthored ⇒ both halves `None` and the stored
  combined string stays byte-verbatim (wild encodings included).
- `inherit` on a NATIVE pin/graphic/part field is a hard error citing step
  7 (SchDoc scope): it resolves against an imported instance base, which
  definition-scope SchLib elaboration does not have. RECORDED EXCEPTION:
  layout metadata fields (anchor gap/side/offset, auto-size
  padding/min_width/min_height) keep the anchors module's documented
  `inherit ≡ reset ≡ versioned default` resolution — that contract is
  explicitly stated on `AutoSizeIntent.padding` and predates E2.
  `part_count` keeps its contextual policy (Inherit/Reset → the resulting
  authored/preserved count, per `PartCountPolicy`'s documented contract).
  RECORDED EXCEPTION (E2 review, 2026-09-17): the four COMPONENT SCALARS
  (`description`/`designator`/`component_kind`/`show_hidden_pins`) also
  predate E2 and keep their documented E1 behavior — `inherit` counts as
  authored and resolves through `FieldPolicy::definition`'s `_` arm to the
  declared definition default (inherit ≡ reset, overwriting the stored
  value, no error; stated on `SchLibPatch::descriptions`). The step-7
  "no instance base at definition scope" argument applies to them equally,
  so whether scalars should adopt the hard error for consistency is an
  OPEN INCREMENT-F QUESTION; until then the rule above is scoped to native
  pin/graphic/part fields, not universal.
- Graphics: per-kind field resolution against `GraphicKind::field_policies`
  (authored-or-creation → `definition()`; else destination value), with the
  compile-time inapplicable-property rule re-checked for hand-built intents.
  `font_id` unauthored ⇒ `None` (B optional semantics: stored id preserved
  UNVALIDATED on Replace; the native default font resolved on New).
  RECORDED DECISION: `Absent` font on a NEW Label/TextFrame resolves the
  ENGINE default (find-only native Times New Roman 10) rather than the
  intent registry's `FontReference(1)` creation default — the engine rule
  is the native-faithful one (memo §8a rule 3) and the two coincide on
  every gate fixture; authored Set/Reset still produce the registry value.

### Parts (deliverable 3)

- `resolve_parts_structure` (pure; also unit-tested directly): partial =
  resulting effective `max(destination, highest authored part number)`,
  explicit `part_count` must accommodate it (lowering under partial is an
  error, proposal §187); complete = authored numbers re-checked as the
  contiguous prefix `1..=n` (compile's `validate_parts` re-checked cheap),
  `part_count` must agree, `n ≥ 1`, shrink under `delete: allow` only.
- Growth lowers to `with_parts_appended` BEFORE the child batch (new-slot
  owners validate; projections index-stable). Shrink lowers to
  `with_parts_removed(removed, true)` — the intent's complete+allow IS the
  descendant-deletion authorization — and the typed report surfaces through
  `SchLibPatch::parts_shrinks()` (`SchLibPartsShrink { component,
  removed_parts, deleted_children, current_part_id_adjustment }`). The
  child batch then resolves against the POST-shrink snapshot (never fused
  with pre-deletion indices, per the C composition rule).
- RECORDED LIMITATION: the `Some` current_part_id adjustment case is not
  publicly constructible (no API raises `current_part_id` above 1), so the
  sync tests assert the `None` case plus the report shape; the decrement
  itself is format-tested (parts.rs). The corpus pass asserts whichever
  case occurs per file through patch inspection.
- `PartIntent::part_swap_group` fan-out (memo §5 rule 2): over the FINAL
  pin sequence, every non-owner-0 pin of that part takes the target prefix
  (suffix preserved); pins already carrying it stay byte-verbatim
  (untouched slots are never re-encoded); a destination `parts()` `Mixed`
  projection hard-blocks; a pin-level authored `part_swap_group`
  contradicting its part's fan-out target hard-errors (equal values
  compose). Fan-out works with or without a managed pins scope.

### Layout / auto-size (deliverable 4)

Per owner unit (component, then parts ascending), one `evaluate_layout` run
sees the owner's pin placements and graphic auto-sizes together; effective
geometry is authored-else-destination-else-registry-default (pin
length/location/orientation from the already-elaborated inits; box corners
from the elaborated shapes). Outputs override pin location/orientation and
box corners BEFORE no-op comparison, so re-elaboration is a fixed point
(tested). The component unit's evaluated geometry map feeds part units as
inherited geometry (part pins may anchor on component graphics); when the
component authors graphics without layout, their boxes are still published.
Anchoring/coordinate mutual-exclusion errors surface as span-carrying
`SchLibPatchError::Spec` diagnostics (via `resolve_pin_location` /
`evaluate_layout`). Imported-base INSTANCE semantics are out of definition
scope: `from_intent` has no imported-symbol input at all, and the
`inherit` hard error above closes the per-field trace — instance
inheritance (including `PinLocationContext::Instance` and cross-artifact
inherited geometry) lands in step 7 (SchDoc scope). A fix-pass regression:
an auto-size-only owner (graphics managed, NO pins scope) previously
panicked on an unwrap; now covered by the second stage of
`layout_anchor_and_auto_size_evaluate_to_a_fixed_point`.

### Image-asset sources (deliverable 5)

- `from_intent_with_assets(input, intent, &SchLibIntentAssets)` — the
  provider maps REQUESTED bundle paths (declaring-source dir ⊕ authored
  relative path, computed by the pure `requested_bundle_path` shared with
  `CapturedImageAsset::capture`) to captured bundles. Elaboration never
  SOURCES bundle bytes from the filesystem — the caller/example performs
  capture and the captured bytes are authoritative — but it DOES touch disk
  to re-verify each required capture (`verify_unchanged`: canonicalize +
  read + hash compare), so the bundle must still exist unchanged at plan
  time; a deleted/retargeted bundle blocks with replan context instead of
  planning a payload-equality decision on stale bytes. An authored
  `source:` without a provided capture blocks with the provider named.
  `from_intent` delegates with an empty provider.
- A pre-pass (`collect_authored_image_sources` + `collect_image_asset_ops`)
  resolves each authored source to its storage resource name — the
  authored `file_name` (non-empty, else error) taking precedence, else the
  positionally addressed destination image's stored name (never a basename
  or hash derivation, per the typed-image-resources contract) — then
  resolves each name to its concrete PAIRING TARGET (exact, then first
  case-insensitive against the input's stored assets — the native order —
  else a new-insert slot keyed by the ASCII-case-folded name) and dedupes
  per target: conflicting bodies for one target block with both contexts
  and both spellings (the target, not the name spelling, is the resource
  identity per memo §8c/D1 errata — keying on the exact name string let
  case-variant spellings of one stored slot emit two self-overwriting
  Replace ops, silently discarding one body and oscillating on replan;
  found by the E2 review, fixed 2026-09-17), while the same body authored
  under any spelling collapses to one op. It re-verifies each required
  capture (`verify_unchanged`), skips ops whose stored payload already
  equals the captured bundle (replan-empty), and queues the rest as ordered
  whole-resource ops on the SAME library edit that precedes component
  content: `Replace{index, stored name}` for a stored target or
  `Insert{end, first-authored spelling}` for a new one. The child batch
  then validates the record's `file_name`/`embed_image` against the
  intermediate context that already holds the asset (memo §8c rule 4; the
  D1 two-step folded into one patch).
- `source` Clear/Reset resolve through the graphic policies to `None` — no
  asset op, and never an asset deletion (asset removal stays an explicit
  library op). `embed_image:false` with an effective source re-checks
  through `validate_effective_asset_source` (span diagnostic).

### Created components (deliverable 6)

Created components elaborate pins/graphics/parts/layout/sources against the
Blank factory shape exactly like matched ones (destination effective 1); no
staging restriction remains. Component-scope pins of a created multi-part
component land shared (owner 0) per the resulting-count mapping and
re-elaborate as an aligned no-op once matched.

### Test evidence (fixture-free)

positional.rs (2): the synthetic-view unrepresentable matrix
(complete-always / partial-addressing / partial-tail, plus the structural
unaddressability of SharedPins/PartIndependent/OutOfRange and the raw-id
growth equivalence) and the parts resolution matrix (partial growth,
count accommodation/lowering, duplicate numbers, complete contiguity/
agreement/≥1/deny-shrink block, complete growth).

elaborate.rs (12 new/replacing the E1 staging test): multi-part isolation
incl. the shared group (the B/C obligation test); overflow append owner
forcing (Part(2) + SharedPins, with the recorded append order); complete
scope defining only its group (deny block + allow removal + other groups
verbatim); cross-kind block + same-kind recolor; owner-group mapping errors
(multi-part component graphics, boundary crossing, same-group conflict,
native-field inherit → step-7); pin axis preservation + unsatisfiable
hidden_net_name; part growth via intent with children in the new part +
part_count growth/lowering matrix; complete parts shrink with the report
asserted through `parts_shrinks()` and exact pristine restore; swap
fan-out (rewrite, owner-0 exclusion, unanimity no-op replan-empty, Clear,
Mixed block, pin-level contradiction); layout anchor + auto-size fixed
point + coordinate-conflict span diagnostic + the pins-scope-less
auto-size regression; image sources (missing-capture block, Insert on
first author, payload-equality replan-empty, whole-resource Replace under
the stored name, Clear never deleting, `captured_image_asset_names`
projection); created components with pins/graphics/parts (single- and
multi-part shapes, shared-owner mapping, replan-empty); the positional
complete-mirror no-op; plus the extended programmatic-scope test (part-item
scopes walked) and the E1 suites unchanged. Every new path asserts
apply → save/reopen typed equality → re-elaborate → empty.

### Corpus example (intent E2 pass)

`check_schlib_snapshots.rs` gains `intent_e2_pass` from a fresh open, per
file: (a) positional partial pin RENAME of component 0's first pin —
full-projection index 0 is necessarily filtered index 0 of its own group —
authored through the matching scope (part block for `Part(n)`, component
scope for a multi-part shared pin; a single-part shared pin records
`intent_pin_rename_skipped`, never silent), with save/reopen equality,
replan-empty, and an exact restore to the input snapshot; (b) part-block
graphic ADD on the first multi-part component whose Part(2) graphics group
is fully representable (bare mirror items address the existing members, the
probe rectangle appends), save/reopen equality + replan-empty, then a
complete-scope removal restoring the pre-add state exactly — Connectors
records `intent_part_graphic_skipped: 1` (no multi-part components), ICs
and STM32F4 run it; (c) part GROWTH on component 0 (new trailing slot with
one probe pin) + complete-parts REMOVAL, the shrink report (removed slot,
one deleted child, adjustment) asserted through `parts_shrinks()`, exact
restore when no adjustment was reported (the documented native decrement
otherwise). New JSON fields enforced in `main()`:
`intent_pin_rename_equal`, `intent_pin_rename_replan_empty`,
`intent_pin_restore_equal`, `intent_part_graphic_equal`,
`intent_part_graphic_replan_empty`, `intent_part_graphic_restore_equal`,
`intent_part_growth_equal`, `intent_part_growth_replan_empty`,
`intent_part_removal_equal` (plus the informational
`intent_pin_rename_skipped` / `intent_part_graphic_skipped` counts).
The example now carries `#![recursion_limit = "256"]` — the single
`json!` report object outgrew the default macro recursion depth.

### Verification (all default suites, fixture-free)

- `cargo test -p altium-format --lib` — 481 passed, 0 failed.
- `cargo test -p altium-sync` — 49 passed, 0 failed; `cargo build -p
  altium-sync` — zero warnings.
- `cargo test -p altium-spec-lang` — 362 passed, 0 failed.
- `cargo test -p altium-format-spec` — 168 passed, 0 failed.
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- Corpus gate: all checks — incl. the nine enforced intent-E2 fields —
  true for lucashudson-Connectors (56, part-graphic stage skipped=1),
  ICs (32), arthurbenemann-STM32F4 (135); exit 0.

### Known items deferred / flagged out of E2

- Owner-0 shared pins on SINGLE-part components are unaddressable by the
  memo §1 scope mapping (recorded consequence above); an explicit shared
  address form is an F/step-7 candidate.
- The `Some` current_part_id-adjustment case remains reachable only through
  wild files or a future explicit resolve op (C's open item, unchanged).
- Multi-pad definer intent axis, `is_current` authoring, alias order
  authoring, the `"|&|"` constant promotion, and the Additional-pin freeze
  are unchanged from E1/C.
- The unrepresentable-member matrix runs against synthetic views (recorded
  justification above); an integrated fixture would require a parser-level
  wild file, which the fixture-free suites deliberately exclude.

## Increment E2 — review findings resolution (2026-09-17)

All seven findings from the independent E2 review were verified against the
code; all seven were real (one major, six minor — two of the minors were the
same defect reported twice). Resolutions:

1. **[FIXED — major] Case-variant image-asset names bypassed the
   conflicting-bodies block (silent body loss + non-converging replan).**
   Confirmed: `collect_image_asset_ops` deduped authored sources by the
   EXACT resource-name string but paired each requirement to the stored
   asset store exact-then-case-insensitively, so two authored spellings
   ("LOGO.png"/"logo.png") with DIFFERENT captured bodies resolved to one
   stored slot as two Replace ops on the same index — the second silently
   overwrote the first (both ops apply in order on the format side, no
   error), and apply → re-elaborate oscillated instead of reaching the
   fixed point. Fixed: each requirement now resolves its concrete PAIRING
   TARGET (exact, then first case-insensitive against the stored assets —
   the same native order the lowering uses — else a new-insert slot keyed
   by the ASCII-case-folded name) BEFORE dedupe, and the conflicting-bodies
   check keys on that target: differing canonical_path/content_hash for one
   target hard-errors with both contexts and both spellings; equal bodies
   collapse to a single op (Replace under the stored spelling, or Insert
   under the first-authored spelling). New regression tests
   `case_variant_image_names_resolve_one_pairing_target` (stored-slot arm:
   block on conflict; exactly one Replace + replan-empty on agreement) and
   `case_variant_new_image_inserts_share_one_slot` (new-insert arm: block;
   single Insert named by the first-authored spelling + case-insensitive
   replan-empty). The image-asset bullet above and STATUS.md now state the
   per-target rule.
2. **[FIXED — docs] "No filesystem I/O in elaboration" contradicted
   `verify_unchanged` inside `collect_image_asset_ops`.** Confirmed: five
   statements (SchLibIntentAssets doc, elaborate.rs module doc, the
   missing-capture error text, STATUS.md, this worklog) claimed the
   absolute contract while elaboration re-verifies every required capture
   against disk (canonicalize + read + hash). Resolved by option (a) —
   keep the conservative fail-fast verification (it prevents a
   payload-equality no-op skip decided on stale bytes, which apply-time
   verification would never re-check) and rewrite all five statements plus
   the error message to the accurate contract: elaboration never SOURCES
   bundle bytes from the filesystem (captured bytes are authoritative) but
   re-verifies each capture's path resolution and hash, so bundles must
   still exist unchanged at plan time.
3. **[FIXED — tests] Untested contract-bearing image/mapping branches.**
   Added fixture-free tests for every branch the review listed: the
   fallback resource-name path (source authored without file_name resolves
   the addressed destination image's stored name —
   `image_source_without_file_name_resolves_the_addressed_stored_name`),
   the conflicting-bodies block
   (`image_source_conflicting_bodies_for_one_name_block`), case-insensitive
   stored-name pairing (covered by the case-variant regression tests, op
   named by the STORED spelling), the empty-authored-file_name error
   (`image_source_empty_file_name_blocks`), and the multi→single direction
   of the component-scope pins boundary-crossing error (complete parts
   shrink 2→1 with component-scope pins managed, added to
   `owner_group_mapping_errors_are_precise`, asserting "from 2 to 1").
4. **[FIXED] Hand-duplicated field lists had no compile-time coupling to
   the spec-lang intent structs (silent-drop hazard; reported twice).**
   Confirmed: `for_each_graphic_field!` hand-listed the 36 `GraphicFields`
   fields and `check_pin_inherit` hand-listed PinIntent's 12 FieldIntent
   axes — complete today, but a field added later would compile while its
   authored value was invisible to the gates. Fixed: both now exhaustively
   destructure the struct with NO `..` rest pattern from ONE field list
   (`for_each_graphic_field!` generates the bindings and the per-field
   applications from the same list; `check_pin_inherit` destructures
   `PinIntent` with `placement`/`designator` as the two documented
   non-FieldIntent exceptions), so adding a spec-lang field breaks the
   altium-sync build until the elaborator handles it. `DestGraphic`
   remains a hand mirror of per-kind destination values, but every
   authored field now necessarily passes the exhaustive gate first.
5. **[FIXED — docs] The B-obligation RESOLVED marker overstated one test's
   coverage.** Confirmed: `multi_part_positional_edits_touch_only_their_
   group` edits the Part(2) and SharedPins groups (part 1 asserted
   untouched by fingerprint); the part-1 group EDIT lives in
   `complete_scope_defines_only_its_group`. The marker now credits both
   tests; the obligation itself was and remains satisfied.
6. **[RECORDED → F] Component-scalar `inherit` ≡ reset is now a recorded
   exception to the E2 step-7 hard-error rule.** Confirmed: the four
   component scalars resolve `inherit` through `FieldPolicy::definition`'s
   `_` arm to the declared definition default (documented E1 behavior on
   `SchLibPatch::descriptions`, predates E2), while E2's rule hard-errors
   `inherit` on native pin/graphic/part fields with the same no-base
   argument. The Field-semantics exception list above now records the
   scalar behavior beside layout metadata and `part_count`, and whether
   scalars should adopt the hard error for consistency is queued as an
   increment-F question.

Post-fix verification (re-run after the last edit): `cargo test -p
altium-sync` — 54 passed, 0 failed (was 49; +5 new tests, one extended);
`cargo test -p altium-format --lib` — 481 passed, 0 failed; `cargo check
-p altium-cli` — clean; `cargo build -p altium-sync` — zero warnings;
`cargo fmt --check -p altium-format -p altium-sync` — clean; corpus gate
(Connectors/ICs/STM32F4) — exit 0, every enforced field true
(`intent_part_graphic_skipped: 1` on Connectors only, as before).
