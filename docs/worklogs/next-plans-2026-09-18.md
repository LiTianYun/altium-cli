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
