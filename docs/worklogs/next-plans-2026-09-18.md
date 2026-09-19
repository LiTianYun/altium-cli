# NEXT.md step 3 — saved plans and both directional planners (worklog)

Non-authoritative worklog. Design authority: the step-3 design memo
(`/tmp/step3/design.md`, 2026-09-18) implementing NEXT.md's "Saved-plan
contract"; shipped collection semantics per
`docs/spec-lang/reference/schlib-collections.md`.

## Ratified contract decisions (orchestrator, 2026-09-18)

- **Q1 — privacy-rule reading (RATIFIED):** `altium-format` exposes a
  deliberate, versioned public serialization contract
  (`ComponentStatePayload`, gated by `SCHLIB_STATE_FORMAT_VERSION`) for
  complete component state. This is NOT an export of internal Rust types:
  payload fields stay private, serde impls stay inside `altium-format`, and
  downstream crates hold the payload as opaque-to-them data that only
  `altium-format` can validate or materialize. Rationale: memo §2 — the
  alternative (patch-op-only plans with rederivation at apply) violates
  "allocate creation IDs at planning" (fresh minting inside
  `with_library_edit`) and "the plan embeds typed before/after state"; the
  only payload-bearing alternative would copy the component model into
  `altium-sync`, strictly worse for the privacy rule. Internal refactors
  either preserve payload byte semantics or bump the version (old plans then
  fail with a regenerate message, which the contract blesses).
- **Q2 — hash algorithm (RATIFIED): BLAKE3**, recorded per manifest entry as
  `algorithm: "blake3"` so any future switch is explicit in the data.
  Rationale: memo §S5 — NEXT.md's "such as SHA-256" names an example, not a
  mandate; BLAKE3 is 256-bit collision-resistant, already a direct
  dependency of `altium-format` and `altium-sync`, and already the hash of
  record for captured image assets (`CapturedImageAsset::content_hash`), so
  one algorithm covers every manifest entry and the image payloads verbatim.

## Increment A — plan container, manifest, hashing, state payload (complete, 2026-09-18)

Baseline 1666028 (step 2 complete). All work fixture-free by default; corpus
evidence via `check_schlib_snapshots` only.

### altium-format

- `crates/altium-format/src/schlib/snapshot/state.rs` (new):
  `ComponentStatePayload` (private fields: `state_format_version`,
  `component: SchLibComponent`, `index: SchLibComponentIndex`),
  `SCHLIB_STATE_FORMAT_VERSION = 1`,
  `SchLibComponentSnapshot::encode_state()`. Version checked during
  deserialization (field-level `deserialize_with`) with an explicit
  regenerate message; `deny_unknown_fields` at every level. Public payload
  accessors: `state_format_version()`, `lib_reference()`, `aliases()`.
- `crates/altium-format/src/schlib/snapshot/library.rs`: new edit forms
  `SchLibComponentEdit::ReplaceWithState(usize, ComponentStatePayload)` and
  `SchNewComponent::FromState(ComponentStatePayload)` resolved inside
  `with_library_edit` slot resolution — install with NO minting,
  rename/alias-change rejections identical to `ReplaceWith`, a dedicated
  state-install validation pass (`validate_state_install`, added by the
  increment-A review), then the shared final-namespace/ownership/reference
  validation and `ReferenceContext` recompute all edit forms use.
  `validate_state_install` re-establishes the checks other edit forms get
  at authoring time, which a decoded payload would otherwise bypass:
  index/root description agreement, root `AliasList` = comma-join of index
  aliases, `FromState` alias-name legality (non-empty, comma-free), every
  record font reference resolved against the final (post-append) font
  table, and every embedded-image reference resolved against the final or
  input asset store. Wild content a LOADED library can carry (dangling
  font ids, dangling image references, index/root desync) keeps
  round-tripping: `ReplaceWithState` inherits exactly the input slot's own
  wild references/pairs, so `decode(encode(c))` stays a typed no-op over
  arbitrary wild libraries while a tampered payload introducing a FRESH
  dangling reference is rejected. NOTE: `ReplaceWith`'s captured
  `ReferenceContext` drift rejection has no state-form equivalent (the
  context is process-local and unserializable) — a reference that resolves
  to a DIFFERENT font/asset in another library is not detectable at
  install; in the saved-plan pipeline the manifest whole-file hash plus
  before/after typed equality at apply (increment B, binding) subsume that
  check. Font-reference enumeration is `SchRecord::collect_font_references`
  (`sch_records.rs`), an exhaustive match so a new font-bearing record
  variant cannot silently escape validation.
- Serde derives added to serve the contract only: the internal record model
  (`sch_records.rs`: all 49 `pub(crate)` record types incl. `SchRecord`,
  `SchPin` with its sidecar-backed fields, `SchComponent`,
  `SchLibComponent`; `deny_unknown_fields` everywhere),
  `SchLibComponentIndex`, `SchAngle`; `altium-format-types` gained
  `cfg_attr(feature = "serde", ...)` derives on the sch/common/coord/color
  types (feature now enabled by `altium-format` and `altium-sync`).
- Round-trip law tests (`state.rs`): a programmatic "collection zoo"
  (multi-part slots, shared owner-0 pin with sidecar-backed functions and
  package length, Additional-stream records with in-stream owner addresses,
  PCBLIB footprint chain with a pin-pad map next to a non-PCBLIB SIM
  sibling, embedded image graphic paired to a store asset, aliases, wild
  out-of-range `owner_part_id`, stale `AllPinCount`), survives native
  save/reopen; per component `decode(encode(c))` installed via
  `ReplaceWithState` is a typed no-op; `FromState` reinstalls a removed
  component exactly; version/unknown-field rejection at payload top,
  nested record, and index levels; rename/alias-smuggle/broken-ownership/
  empty-name rejections through the identical validation path.

### altium-sync

- `crates/altium-sync/src/plan.rs` (new): `SavedPlan`
  (`PLAN_SCHEMA_VERSION = 1` + `state_format_version` + domain + direction +
  `PlanScope` summary + manifest + `PlanPatches` + diagnostics, exactly memo
  §S4; field order = serialization order), `ManifestEntry` with the six
  roles and `PlanPrecondition::{Hash(PlanHash), Absent}` (absence recorded
  and verified via `symlink_metadata`, memo §S6), the single shared
  `canonicalize_plan_path` (full canonicalize for existing paths — a
  dangling symlink occupies its path and fails resolution; canonicalized
  parent + file name for absent ones), and `check_manifest_collisions`
  implementing every §S11 rejection at construction and re-run on decode.
  Compile lowering `SavedPlan::compile_from_schlib_patch` (component
  Create/Replace/Delete with embedded payloads; `parts_shrink` carried;
  library ops in D1 application order: font appends, image ops, alias
  removes, alias adds; image payloads as base64 + capture-time BLAKE3 hash +
  canonical source path). Deterministic compact JSON emission
  (`to_json_bytes`), strict decode (`from_json_bytes`: exact versions with
  regenerate messages, unknown fields rejected, §S11 re-checked, every
  embedded image payload hash-verified — mismatch is a hard error).
  `mask_unique_ids` is the ID-masking display/test normalizer (never an
  execution input). §S7 helper `load_schlib_hashed` (read once, hash and
  parse the same buffer via `SchLib::from_bytes`).
- `crates/altium-sync/src/schlib.rs`: crate-visible lowering accessors
  `input_snapshot()`, `result_snapshot()`, `created_component_positions()`.
- **Increment-B seam (documented in the module docs):** `PlanArtifact`
  carries artifact BYTES (or expected absence), not just paths — the caller
  reads each input exactly once and hashes and parses the same buffer. The
  in-memory path passes programmatic bytes; increment B's planner binds
  real files through `load_schlib_hashed` (documents), `CapturedSource`
  text (specs/imports), and `CapturedImageAsset` bytes (bundles) with no
  rework of the plan types.
- Tests: lowering fidelity over one intent patch producing
  create + replace (with parts shrink) + delete + alias op, decoded plan ==
  constructed plan with typed payload equality; byte-determinism of two
  emissions; determinism modulo allocated IDs across two plan constructions
  (unmasked bytes differ, masked JSON equal); version/unknown-field
  rejection at plan and payload layers; all §S11 collision classes with
  context-bearing errors (duplicate destinations, cross-artifact alias,
  duplicate same-role entries, plan-path collision, occupied
  expected-absent output, non-Destination absence, decode-time re-check,
  unix symlink alias + dangling-symlink cases); image payload
  embed/verify/tamper (body swap and bad base64 both fail decode);
  `load_schlib_hashed` parse-the-hashed-buffer equality.
- `crates/altium-sync/examples/check_schlib_snapshots.rs`: new saved-plan
  pass per corpus library — §S7 load, per-component payload round-trip
  no-op via one `ReplaceWithState` batch, plan strict-decode equality, and
  byte-determinism across two independent plannings (no creations ⇒
  byte-identical).

### Deviations / notes within the memo's latitude

- `check_manifest_collisions` also rejects two entries of the SAME role
  resolving to one canonical path (memo §S11 lists different-role aliases
  and duplicate Destinations explicitly; same-role duplicates of input
  roles are rejected as accidental aliases too — strictly safer, and a
  legitimate shared import cannot occur twice in one manifest).
- `ExpectedAbsent` is restricted to `Destination` entries (memo §S6 speaks
  only of destinations; every other role is a read input that must exist).
- `PlanLibraryOp` excludes component create/remove: those lower to
  `PlanComponentPatch::Create`/`Delete` (memo §S4 lists component patches
  separately from the namespace ops).
- Lowering emits component patches as creations (final order), then
  replacements (patch order), then deletions (input order); a created
  component's children arrive as a content Replace following its Create,
  mirroring the D1 two-step composition.
- `PlanError::UnsupportedSchemaVersion`/`UnsupportedStateFormatVersion`
  exist as typed variants; the schema-version check itself fires inside
  deserialization (field-level) so skew is reported before any payload
  parse, with the same regenerate wording.

### Verification (2026-09-18, all verbatim; re-run after the review fixes below)

- `cargo test -p altium-format --lib` — 490 passed (487 increment-A + 3 from
  the review fixes).
- `cargo test -p altium-sync` — 61 passed (54 step-2 + 7 new).
- `cargo test -p altium-spec-lang` — 362 passed (unchanged).
- `cargo test -p altium-format-spec` — 168 passed (unchanged).
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync -p altium-format-types`
  — clean.
- `cargo check -p altium-sync` — zero warnings.
- Corpus gate: `cargo run -p altium-sync --example check_schlib_snapshots --
  data/schlib/lucashudson-Connectors.SchLib data/schlib/ICs.SchLib
  data/schlib/arthurbenemann-STM32F4.SchLib` — exit 0, all checks true for
  56/32/135 components incl. the new `plan_payload_noop_equal`,
  `plan_roundtrip_equal`, `plan_deterministic` keys.

### Increment-A independent review — findings resolution (2026-09-18)

Nine findings (1 major, 8 minor) from the independent completeness review,
each verified against the code before acting:

1. **[fixed — major] State-install forms skipped reference validation.**
   Confirmed: `ReplaceWithState`/`FromState` installed decoded payloads
   verbatim, so a tampered payload with a FRESH dangling `font_id` (or an
   embedded-image reference resolving against no store) installed, saved,
   and reopened silently. Fixed with `validate_state_install` (see the
   library.rs bullet above). The reviewer's literal fix (unconditional
   `check_font_id_exists` over state slots) was NOT adopted: it would
   reject wild dangling references loaded libraries legitimately carry,
   breaking the increment-A round-trip-law gate ("wild preserved content")
   and the capture-forbidden boundary (`validation.rs` module contract).
   Instead the per-slot wild-inheritance carve-out accepts exactly the
   input slot's own wild references and rejects everything fresh. Covered
   by `state_installs_reject_fresh_dangling_references` and
   `wild_dangling_references_round_trip_but_do_not_relocate`; the corpus
   gate re-proves the no-op law over all 223 real components. The
   remaining gap the reviewer's probe cannot close either — a reference
   resolving to a DIFFERENT font/asset in another library — is
   undetectable without the process-local context and is subsumed by
   increment B's manifest-hash + before/after typed-equality apply checks
   (now a recorded increment-B obligation, below).
2. **[fixed — minor] Index-field desync/legality gaps.** Confirmed
   (description desync and comma-alias smuggle probes reproduced in
   review). Same fix: `validate_state_install` cross-checks description
   and `AliasList`, and runs `SchAliasOp`-equivalent alias legality on
   `FromState` payloads, with the wild pairs inherited per-slot.
3. **[fixed — minor] Zoo omitted the parameters collection class.**
   Confirmed: the blank AD26 component's `Comment`/`Designator` field
   objects are not parameters-collection members. The zoo now creates a
   root-owned hidden `ZOO_PARAM` parameter via `SchParameterEdit::New`, so
   the fixture-free law test covers all eight collection classes.
4. **[fixed — minor] Worklog overstated `ReplaceWith` equivalence.** The
   library.rs bullet above now names the omitted `ReferenceContext` drift
   check explicitly (as does the `state.rs` module doc), and the
   increment-B obligations below carry the compensating controls.
5. **[fixed — minor] `BoundingBox` gained an invariant-bypassing
   `Deserialize`.** Confirmed unused by any payload (workspace grep: no
   user outside `altium-format-types`). Serde derives removed; a comment
   on the type records why a future need must go through a validating
   `Deserialize`.
6. **[fixed — minor] Non-finite `SchAngle` encoded to undecodable JSON
   `null`.** Confirmed (`str::parse::<f64>` accepts NaN/inf; serde_json
   emits null for non-finite). `SchAngle::from_param_value` now rejects
   non-finite values at the parse boundary with the offending key, per
   fail-fast; unit-tested.
7. **[fixed — minor] Stringly-typed `PlanScopeEntry`.** `subject` and
   `collection` are now the typed `PlanScopeSubject`
   (`Library | Component { lib_reference }`) and `PlanCollectionKind`
   (the eight-member `CollectionKind` mirror), matching the file's
   existing serializable-mirror pattern — done before increment B, so no
   schema-version bump.
8. **[recorded — minor] Q1/Q2 ratification provenance.** Not resolvable
   from the repository: the worklog attributes the ratification to the
   orchestrator, and this fixer cannot verify the rule's owner (the user)
   answered Q1/Q2. Carried below as an explicit open question for the
   orchestrator/user; the implementation follows the ratified reading
   either way.
9. **[done — minor] Verification claims re-observed.** All gates re-run
   verbatim after the last edit of this fix pass; results above.

### Open questions (carried, not blocking A)

- Memo Q3–Q6 remain queued for the user (CLI namespace/exit codes, early
  read-only plan CLI, in-place declaration surface, step-2 closure constant
  promotions).
- **Q1/Q2 provenance (review finding 8):** the ratification section above
  records orchestrator decisions; the memo requires Q1 (privacy-rule
  reading, a cardinal-rule interpretation) to be confirmed with the USER
  before increment A lands. The orchestrator should confirm the user
  answered Q1/Q2 — or put them to the user now — before increment B builds
  further on the v1 payload/plan schemas.
- Increment B must decide the `address.rs`/`positional.rs` unification
  direction (memo §4B) and reconcile alias-carrying after-states with the
  two-step apply order when a plan both replaces content and edits aliases
  of the same component (the install forms keep `ReplaceWith`'s
  alias-change rejection, so apply reconstructs alias changes as
  `SchAliasOp`s before installing after-states).

### Increment-B obligations recorded by the review

- The apply pipeline MUST NOT expose `ReplaceWithState`/`FromState`
  outside the manifest-hash-verified + before/after typed-equality flow of
  memo §2: those plan-level controls are the substitute for `ReplaceWith`'s
  process-local `ReferenceContext` drift check, which decoded payloads
  cannot carry (a payload reference that RESOLVES in the target but to a
  different font/asset is undetectable at install). Step 9's
  reference-pairing guard remains the closure point for out-of-plan API
  misuse of image references; `validate_state_install` closes the
  fresh-dangling-reference vector.
- `FromState` is strict (no wild inheritance). If increment B lowers a
  CLONE of a wild component (dangling font/image reference or index/root
  desync) into a `Create` patch, applying that `FromState` will fail
  fast; the planner must either reject such clones at planning with the
  same message or thread the source slot's wild allowance — decide
  in-increment, never by weakening install validation silently.

## Increment B — checked apply, planner entry, exact report, address unification (complete, 2026-09-18)

Design authority: memo §2 (apply-side verification order), §3 (exact-report
model + exit codes), §4B; the increment-A "Increment-B obligations" above
(all discharged below); F-finding-4
(`docs/worklogs/next-children-2026-09-16.md`).

### Planner entry (`crates/altium-sync/src/planner.rs`, new)

`plan_compile_schlib(spec_path, target_path, output, plan_path) ->
SchLibCompilePlanning { plan, changes, exit_code }` with
`SchLibPlanOutput::{InPlace, Alternate(PathBuf)}` (§S9/§S10 — the declared
mode is recorded, nothing inferred). Reads each input ONCE via the §S7
seam: `capture_imports` retained source text (hashed and compiled from the
same buffers via `compile_captured_schlib`), `load_schlib_hashed` for the
target, `CapturedImageAsset` for every authored bundle (deduplicated by the
pure requested-path arithmetic BEFORE any read). Elaboration through
`from_intent_with_assets`; blocking diagnostics are `Err` (exit 1). The
§S8 recheck runs immediately before emission:
`CapturedSourceGraph::verify_unchanged()` (exact content comparison — this
SUBSUMES the memo's "extended to also compare the now-computed hashes";
no weaker hash-only check was added), per-asset `verify_unchanged()`, and a
re-read + re-hash of the target; drift is
`PlanError::ChangedDuringPlanning`. Exit codes per memo §3:
`PLAN_EXIT_NO_CHANGES = 0`, `PLAN_EXIT_BLOCKED = 1`,
`PLAN_EXIT_CHANGES = 2`, plus `plan_exit_code(&Result<..>)`. The recorded
`PlanScope` summary carries the library components scope plus every
DECLARED per-component collection scope (part-block pin/graphic scopes fold
into the component's kinds; exact duplicates recorded once).

### Checked apply (`crates/altium-sync/src/apply.rs`, new)

`apply_saved_plan(&SavedPlan) -> Result<AppliedPlan, PlanError>` — takes
ONLY the plan (frozen §S9 paths; no path parameters). Verification order
exactly per memo §2: version/direction gate → manifest re-resolution
(recomputed canonicalization; a changed resolution is the explicit
`PathResolutionChanged` error, so a retargeted symlink fails even on
byte-identical content) + §S11 re-check + EVERY hash/absence precondition
(sources/imports/bundles read and hashed, never re-parsed — §S13) → decode
embedded image payloads (hash-guarded) → open the target FROM THE HASHED
BYTES (`SchLib::from_bytes`) → snapshot → resolve every patch address
against that immutable snapshot (through `crate::address`'s
`ComponentIndex::resolve` — the F-finding-4 consumption) → per-patch
decoded-before typed equality (`StaleComponentState`, naming the
component) → execute → verify produced components equal decoded
after-states + membership + library invariants → save to the frozen
destination → reopen → typed-equality check.

Execution reconstructs the D1 two-step from the plan, each phase ONE
`SchLibEdit`:

1. Library edit: fonts, image ops, membership (kept input slots ascending,
   deletions omitted under `allow_component_removal`, creations appended as
   `New(FromState(seed))`), alias ops. The creation seed is the paired
   replacement's `before` payload — the planner's INTERMEDIATE state — so
   the original two-step is replayed exactly; a creation with no content
   replacement seeds its final `after` directly. **Alias-edit
   reconstruction (the increment-A note, discharged):** `AliasAdd` ops
   targeting a created component are subsumed by the `FromState` install
   (the payload's index carries the alias) and are dropped after verifying
   the alias is present in the payload (absent ⇒ `InvalidPlan`); alias ops
   on kept components stay in the edit, so phase 2's `ReplaceWithState`
   alias-equality check (`intermediate slot aliases == after aliases`)
   holds exactly as at planning. `AliasRemove` ops always stay (they may
   release a removed component's aliases).
2. Content: `ReplaceWithState(final_index, after)` per replacement against
   the intermediate. Replacements paired with creations verify/install
   uniformly (their before equals what phase 1 produced by construction —
   any tamper breaks the equality).

**Lowering-time dry run (and the FromState wild-clone obligation,
discharged as option 1 — reject at planning):** the execution core is the
shared `pub(crate) fn execute_compile_patches(plan, input)` in `apply.rs`;
`compile_from_schlib_patch` runs it against the patch's input snapshot
immediately after constructing the plan and requires the produced snapshot
to equal the reviewed result (typed equality). Every emitted plan is
therefore proven executable at planning: a `Create` lowered from a CLONE
of a wild component fails the dry run's strict `FromState` install with
the install's own message (never emitted, never a weakened validation —
a structural guarantee; the direct-test gap is recorded in the findings
resolution below), and any reconstruction gap (membership, alias
subsumption, two-step order) surfaces as `UnrepresentablePatch` at
planning instead of an apply failure. Planning cost roughly doubles (one
in-memory execution per lowering); accepted for the guarantee.

Since the plan records deletions (input slots) and creations (final order)
but NO keep permutation, only the canonical final sequence — kept slots in
input order, creations appended — is representable:
`SavedPlan::compile_from_schlib_patch` now hard-errors
(`UnrepresentablePatch`) on any patch whose slot vector deviates (a keyed
complete components scope CAN legally reorder; such patches must be
restructured or applied in memory — recorded as a v1 schema limitation,
enforced at PLANNING, never silently reordered at apply). Apply validates
every replacement address against the reconstruction it derives from the
plan (mismatch ⇒ `InvalidPlan`).

Step-4 boundary (documented in the module docs): NEXT.md "Checked apply
and recovery" steps 1–3 run in memory here; the write is a plain
`SchLib::save` + reopen typed-equality check ONLY — staging beside the
destination, the durable journal, commit-by-rename, and recovery are
step 4 and are absent.

### Exact report (`crates/altium-sync/src/report.rs`, new)

`report(&SavedPlan) -> Result<PlanReport, PlanError>` — a pure function of
the plan (memo §3), never stored, never an execution input. Child detail is
computed from the before/after payloads through THE SAME typed views the
planner used, via the new format-side
`ComponentStatePayload::component_view()` (below): keyed collections pair
by key + occurrence ordinal under the planner's own equivalence —
parameters by the native ordinal case-insensitive fold
(`parameter_key_fold`, the equivalence elaboration pairs with, so a
case-only respelling reports as ONE Replace whose `name` field diff shows
the respelling, never delete + create; review finding B-1), aliases by
exact value, footprints by exact model name, parts by number —,
positional by index (pins, graphics, and
pin-pad maps under their paired footprint owner), `parts_shrink` deleted
children surface as per-record delete entries plus the `current_part_id`
adjustment field; deletes ALWAYS expand (component scalar before-values
plus every child as its own delete entry; never counts-only); library ops
(fonts, image assets by hash/length — bodies never render —, alias
add/remove) each appear as entries; `allocated_ids` lists every non-empty
`unique_id` of each created after-state (the same field-name convention
`mask_unique_ids` normalizes). NO heuristics — exact indexed diff only;
both old/new indices retained wherever the plan embeds the state to
recover them (library-level alias ops carry none — see deviations,
finding B-7). Entry elision is VERIFIED, never assumed (finding B-9):
an alias-add subsumed by a created component's Create expansion is
checked against the embedded after-state, and a creation's paired
content replacement is checked payload-identical before being elided —
violations are `PlanError::InvalidPlan`, mirroring apply's cross-checks.
Field values are typed serde serializations of the projection views (the
views gained `Serialize` derives), superseding the Debug-string
`scalar_changes()` surface for reporting (that API is retained).
`render_plan_report` is the plain-text renderer for tests/example use; no
CLI wiring (step 5). Known
non-embedded values: an `ImageAssetRemove`/`ImageAssetReplace` prior body
is not in the plan (the store is whole-file-hash protected), so those
entries carry the exact store slot, not a before body.

### Format-side additions (`altium-format`)

- `ComponentStatePayload::component_view() -> SchLibComponentSnapshot`
  (`schlib/snapshot/state.rs`): report/inspection projection bound to an
  EMPTY captured reference namespace — the same child/scalar typed views,
  captured-namespace projections empty, and installation impossible (any
  install path rejects the context drift; tested). Part of the ratified
  versioned payload contract.
- `Serialize` derives (serialize-only, no `Deserialize`) on the public view
  types (`SchOwnerGroup`, `SchPartSwapGroup`, `SchPartView`, `SchPinView`,
  `SchParameterView`, `SchGraphicView`, `SchGraphicDetail`,
  `SchFootprintMapsView`, `SchFootprintMapView`, `SchPinPadMapView`) to
  serve the report's typed field rendering.

### Address unification (F-finding-4 discharge)

`crate::address` is now CONSUMED: apply resolves every decoded component
patch address through `ComponentIndex::resolve` (deletes against the input
snapshot, replacements against the intermediate). Direction decision for
the owner-group mappings: NEITHER delegates — delegation in either
direction would change shipped step-2 semantics (positional's raw-owner-id
matching must keep selecting pre-growth `OutOfRange(n)` members;
address resolution must keep rejecting parts beyond effective). Instead
the anticipated visibility restructuring landed (`positional` is
`pub(crate)`; `group_member`/`component_scope_pins_target`/
`component_scope_graphics_target` and address.rs's
`pin_owner_group`/`graphic_owner_group` are `pub(crate)`), and the
cross-module equivalence test
`address::tests::owner_group_mappings_agree_with_positional_raw_id_matching`
pins the two implementations over the owner × effective-count × group
matrix (component scope and every part owner; effective 1..=4; groups
incl. `PartIndependent`, `SharedPins`, in/over-range `Part(n)`, and
`OutOfRange` ids): membership under positional's raw-id predicate equals
exact-`SchOwnerGroup` equality PLUS exactly the `OutOfRange(target)`
pre-growth allowance — asserted as the ONLY divergence — with the
component-scope target computations checked equal (incl. created
components and the multi-part graphics rejection on both sides), and
beyond-effective parts shown unreachable through address resolution. The
address.rs module docs record the reconciliation.

### Tests (fixture-free) and corpus evidence

- `apply.rs`: end-to-end no-op / create / update / delete plan→apply with
  save/reopen equality and replan-empty; in-place apply; alias+content on
  one component (the two-step reconstruction); stale-input matrix — spec
  source edited, transitive import edited, target byte-changed,
  expected-absent output occupied, image bundle changed, unix symlink
  retarget (byte-identical twin ⇒ `PathResolutionChanged`), plan-file
  payload tamper caught by before-equality (valid-but-different state) or
  by strict decode (contract-breaking tamper); wrong-direction rejection;
  reordering-patch lowering rejection.
- `planner.rs`: real spec files (root + import) planned against a real
  library in both output modes, manifest role/precondition assertions,
  recorded scope summary, apply + replan-with-exit-0, and the 0/2/1
  exit-code mapping incl. `plan_exit_code` on `Err`.
- `report.rs`: delete expansion (component + children, old indices, before
  values), field-level before/after pairs (description,
  `effective_part_count` from the parts shrink), created-component
  expansion with recovered final index, `allocated_ids` from created
  after-states, summary/entry-count agreement, renderer content, report
  purity (derive twice ⇒ identical), and the case-respelled-parameter
  pairing regression
  (`case_respelled_parameter_reports_as_replace_not_delete_create`).
- `altium-format` `state.rs`: `component_view` projection equality against
  live snapshots over the collection zoo + install-rejection.
- `check_schlib_snapshots.rs` full-circle stage per corpus file:
  programmatic intent → plan against a COPY of the real library →
  serialize plan to disk → re-read + strict decode → `apply_saved_plan` →
  save/reopen equality + replan-empty + exact report entry counts; then
  one byte appended to the TARGET file asserting the manifest content-hash
  rejection — an input-drift check, not a plan-file tamper (plan-payload
  tampering is covered by the fixture-free apply unit tests), so the key
  is named accordingly (review finding B-6). New enforced JSON fields:
  `plan_full_circle_applied`, `plan_full_circle_replan_empty`,
  `plan_report_entries_exact`, `plan_target_drift_rejected`.

### Deviations / notes within the memo's latitude

- Memo §2's "install after-states + library ops as one `SchLibEdit`
  batch" is implemented as the D1 two-step (one library-edit `SchLibEdit`,
  then one content `SchLibEdit` of `ReplaceWithState`s): a single batch is
  IMPOSSIBLE whenever a plan both replaces a component's content and edits
  its aliases, because `ReplaceWithState` requires the payload's alias set
  to equal the pre-alias-op slot's (the increment-A reconstruction note is
  the more specific, later authority and prescribes exactly this order).
- The canonical-sequence restriction (`UnrepresentablePatch`) narrows what
  can be LOWERED (not what `apply_in_memory` executes); it is the honest
  fail-fast consequence of the increment-A schema recording no keep
  permutation, surfaced at planning.
- `AppliedPlan.destination` is the recomputed canonical path (identical to
  the recorded one by the resolution check).
- The report's `ReportCollection` display vocabulary extends the plan's
  eight collection kinds with `Fonts`/`ImageAssets` so library-namespace
  ops have labeled entries; this is report-local display data, not plan
  schema.
- Report-model micro-deviations from memo §3, deliberate compile-slice
  simplifications (review finding B-3): (a) the `PlanReport` header carries
  domain/direction/schema versions but not the memo's "plan hashes
  summary" — artifact hash values render as the precondition kind
  (`"hash"`/`"absent"`) only; (b) summary rows are keyed
  `(collection, op)`, not the memo's `(artifact, collection_kind, op)`;
  (c) `ChangeAddress` has no `artifact` member. All three collapse
  benignly for compile plans, which write exactly one artifact. Increment
  C's dump-direction reports (per-source-file entries) MUST revisit these
  and add the per-artifact dimension.
- Library-level alias entries (`AliasAdd`/`AliasRemove` on kept
  components) carry `old_index`/`new_index` = `None` (review finding B-7):
  the v1 plan schema embeds no component state for alias-only ops, so the
  slot's alias position is unrecoverable from the plan. For a component
  that is BOTH alias-edited and content-replaced, the Replace expansion's
  keyed alias diff is empty by construction — the embedded before payload
  is the post-alias-op INTERMEDIATE state, so before == after aliases —
  and the alias change is visible exactly once, through the index-less
  library-op entry. Every executable change still appears in expanded
  output; only the index annotation is missing on these entries.

### Increment-B independent review — findings resolution (2026-09-18)

Eleven findings from the independent completeness review (three of them the
same corpus-observation issue), each verified against the code before
acting:

1. **[fixed — major, B-1] Report paired parameters by exact name; the
   planner pairs case-insensitively.** Confirmed: elaboration pairs
   parameters through the native ordinal case-insensitive
   `parameter_key_fold` (elaborate.rs, per `GetParameterByName`
   OrdinalIgnoreCase) and rewrites the stored name to the authored
   spelling, so a case-only respelling plans as ONE Replace — but
   `report()` paired by exact name and presented it as delete + create.
   Fixed: `parameter_key_fold` is now `pub(crate)` (re-exported from
   `schlib`), the report's Parameters collection pairs by
   `(fold(name), occurrence)` with exact names kept inside the field
   values so the respelling surfaces as a `name` before/after diff.
   Aliases and footprints keep exact pairing (matching the planner).
   Regression test
   `case_respelled_parameter_reports_as_replace_not_delete_create`.
2. **[fixed — minor, B-9] `report()` silently elided ops on unverified
   subsumption assumptions.** Confirmed: the alias-add-on-created skip did
   not verify the created after-state carries the alias (apply cross-checks
   this; report did not), and the created-component Replace skip did not
   verify the paired payloads are identical. Both now fail fast with
   `PlanError::InvalidPlan` (mirroring apply's checks) before eliding —
   a hand-crafted plan violating either assumption can no longer make an
   executable op disappear from the report.
3. **[fixed — minor, B-10] Dead conditional in the image-asset field
   builder.** Confirmed: both branches of the `side` closure returned
   `(None, Some(value))`. Collapsed to a single construction with the
   explanatory comment (the plan does not embed a replaced asset's prior
   body; the store is whole-file-hash protected).
4. **[fixed — minor, B-6] `plan_tamper_rejected` tested target-file
   drift, not plan tampering.** Confirmed: the corpus stage appends one
   byte to the TARGET copy and asserts the manifest content-hash
   rejection. Renamed to `plan_target_drift_rejected` (example field,
   enforcement list, docs); plan-payload tampering remains covered by the
   fixture-free apply unit test
   `stale_target_expected_absent_and_payload_tamper_are_rejected`.
5. **[recorded — minor, B-3] Unrecorded report-model micro-deviations
   from memo §3.** Confirmed (no artifact member on `ChangeAddress`,
   `(collection, op)` summary keys, no plan-hashes header summary); now
   recorded in the deviations list above as deliberate compile-slice
   simplifications with an explicit increment-C revisit obligation.
6. **[recorded — minor, B-7] Index-less library-level alias entries.**
   Confirmed; recorded in the deviations list above (including the
   intermediate-payload reason the component-level alias diff is empty
   for replace+alias plans).
7. **[recorded — minor, B-2] Wild-clone `FromState` planning rejection is
   structural, not directly tested.** Confirmed: no test lowers a clone of
   a genuinely wild component. The rejection is guaranteed structurally by
   the lowering-time dry run (`compile_from_schlib_patch` executes
   `execute_compile_patches` against the input snapshot; a wild clone's
   after-payload fails the strict `FromState` install inside the dry run,
   and the error propagates as the planning error), but no direct test
   exercises it because wild states are NOT constructible through the
   public `altium-sync` API (D1 validation forbids building them; see the
   elaborate.rs test note on wild loaded state) and fixture-gated suites
   are out of bounds for this pass. Recorded as a known test gap: the
   first fixture-run pass that identifies a genuinely wild corpus
   component should add the direct rejection test.
8. **[resolved — major, B-corpus (three findings)] The final corpus gate
   was recorded as passing without being observed at the final code
   state.** Confirmed: the implementing session's last corpus re-run (after
   adding the lowering-time dry run) completed only
   lucashudson-Connectors + ICs before the session ended, yet this
   worklog and STATUS.md recorded an unqualified 56/32/135 pass —
   violating the "report verification honestly" rule. The claim itself
   was true for that state: the independent review re-ran the verbatim
   gate to completion there (exit 0, all keys true for 56/32/135). This
   fix pass re-ran the gate after its own edits but was itself forced to
   report before the STM32F4 line completed — the verification section
   below records the partial observation honestly and the gate must be
   re-run to completion before merge. The provenance is recorded here so
   the verification section's claims stay auditable.
9. **[no change — minor, B-11] Implementer report said "+ 2 tests" in
   `state.rs`; the diff adds one test function covering both behaviors.**
   The repo documents (this worklog, STATUS.md) already state it
   correctly ("490 + component_view"); the miscount existed only in the
   session report.

### Verification (2026-09-18, all verbatim, re-run after the review-fix edits)

- `cargo test -p altium-format --lib` — 491 passed (490 + component_view).
- `cargo test -p altium-sync` — 76 passed (61 increment-A + 15: 10 apply,
  2 planner, 2 report, 1 address equivalence).
- `cargo test -p altium-spec-lang` — 362 passed (unchanged).
- `cargo test -p altium-format-spec` — 168 passed (unchanged).
- `cargo check -p altium-cli` — clean.
- `cargo fmt --check -p altium-format -p altium-sync` — clean.
- `cargo check -p altium-sync` — zero warnings.
- Corpus gate: `cargo run -p altium-sync --example check_schlib_snapshots
  -- data/schlib/lucashudson-Connectors.SchLib data/schlib/ICs.SchLib
  data/schlib/arthurbenemann-STM32F4.SchLib` — PARTIALLY observed at this
  final state: lucashudson-Connectors (56) and ICs (32) completed with
  every check true incl. `plan_full_circle_applied`,
  `plan_full_circle_replan_empty`, `plan_report_entries_exact`,
  `plan_target_drift_rejected`; the arthurbenemann-STM32F4 (135) line was
  still executing when this fix session was forced to report and has NOT
  been observed at this state. (The independent review observed the full
  56/32/135 pass at exit 0 on the pre-fix increment-B state; this pass's
  edits touch only report pairing/fail-fast checks, the corpus key
  rename, and one test.) The verbatim gate MUST be re-run to completion
  and this line updated before merge.

### Handoffs

- Step 4 wraps `apply_saved_plan`'s save with staging/journal/commit (the
  boundary is documented in `apply.rs`); step 5 wires
  `plan_compile_schlib`'s 0/2/1 exit codes and `render_plan_report`.
- Increment C builds the dump-direction planner on the same container and
  the now-consumed address vocabulary.
