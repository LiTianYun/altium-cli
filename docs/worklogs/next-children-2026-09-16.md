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
  mapping on a multi-part component.
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
