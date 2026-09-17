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
