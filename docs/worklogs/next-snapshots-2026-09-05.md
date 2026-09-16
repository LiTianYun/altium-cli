# SchLib complete snapshot investigation — 2026-09-05

This is bounded implementation evidence, not a claim that NEXT step 2 is complete.
Complete typed snapshots retain native records privately, including separate Data
and Additional collections, header fields, aliases and ordered image assets.
Capture/materialize equality does not use the lossy public `Component` conversion.

## ICs.SchLib pin-count cache mismatch

The real fixture `data/schlib/ICs.SchLib` contains 32 components. Comparing complete
private typed state before and after native save/reopen found exactly 32 changed
fields: each component's stored `all_pin_count` changed from zero to its number of
Data pins (3–32). No other typed field differed. The diagnosis wrote private Debug
representations to `/tmp/ics-before.snapshot` and `/tmp/ics-after.snapshot`; the
bounded field diff is `/tmp/ics-fast.diff`. Debug output was diagnostic only, not
a snapshot persistence format or a comparison that replaces typed equality.

Fixture repository revision: `9687d71cebbdd5bbf20771fe05a14d4a83c73c0a`.
Fixture SHA-256: `5505eb74b8e02f4f573c351a2e57a2b99590991db12164358443838587001ce5`.

The Rust `serialize_component_data` implementation recomputed nonpositive counts
before exporting them. Its justification conflated a data-object getter with a
separate engine-object getter.

Original source was freshly decompiled with the local ILSpy CLI from:

- Assembly: `/mnt/c/Program Files/Altium/AD26/System/Altium.Sch.DataModel.dll`
- SHA-256: `3a34fe9ea365fae99230e9c86d5c5b3f892db394145446f1e4b955f9cca879ed`
- `/tmp/altium-original-evidence/FileFormatV5.ExportComponent.cs`, lines 6–8,
  47: `Altium.Sch.DataModel.FileFormats.FileFormatV5.ExportComponent` receives
  `ISchDataObject`, casts to `ISchDataComponent`, and exports that object's
  `GetAllPinCount()` result.
- `/tmp/altium-original-evidence/SchDataComponent.GetAllPinCount.cs`, lines 1–4:
  `Altium.Sch.DataModel.Objects.SchDataComponent.GetAllPinCount` returns the stored
  `allPinCount` field directly.
- `/tmp/altium-original-evidence/SchComponent.GetAllPinCount.cs`, lines 1–5:
  `Altium.Sch.DataModel.EngineObjects.SchComponent.GetAllPinCount` first calls
  `UpdateAllPinCount`, then returns the data-object getter result.
- `/tmp/altium-original-evidence/SchComponent.UpdateAllPinCount.cs`, lines 1–7:
  the engine updater recalculates only when the stored value is nonpositive, using
  pins for all parts **in the current display mode**. The removed Rust calculation
  also differed here because it counted all Data pins.

These are managed-source dispatch findings, not an inference from method names or
an execution trace of Altium's GUI. They support exporting the captured stored
value; they do not justify introducing an implicit preparation/normalization pass.

The correction removes the redundant SchLib-specific recalculation and uses
`serialize_component_record`, which already writes nonzero stored values and
omits zero with the same decoded default. Snapshot equality, capture, and the
stored field remain unchanged. Positive cached values are preserved even if they
differ from the visible pin count. Future explicit structural pin edits must
define their cache-update policy separately.

Regression coverage checks stored zero and positive counts with multiple pins
and display modes. The zero case failed against the old writer (expected zero,
actual one) before the correction. A separate native save/reopen test establishes
a canonical parsed pin baseline and compares the complete typed state after a
description-only edit. It does not normalize fields during equality or claim that
every fully typed programmatic record already has verified native roundtrip
support.

Validation after the correction:

- `cargo test -p altium-format --lib schlib::snapshot`: 6 passed.
- `cargo run -p altium-sync --example check_schlib_snapshots -- data/schlib/ICs.SchLib`:
  all 32 components capture/materialize exactly; 32 description replacements equal
  their planned in-memory result; both native no-op and native patched reopen
  equal the complete expected typed snapshots.
- Independent source/code review passed for this narrow writer correction.

No fixture-feature or property-test suite was run for this investigation. The
full Step 2 scope/ownership/materialization gates remain separate work.

## Complete snapshot and first patch boundary

The snapshot captures every parsed native field using typed Clone/PartialEq,
not serialized CFB bytes or the lossy public Component DTO. Component snapshots
retain both Data and Additional record vectors. Programmatic tests cover Symbol,
Hyperlink, non-PCBLIB implementations, pin function vectors, fonts/styles,
nondefault component fields, aliases and ordered duplicate image resources.

Batch replacement resolves every original index, rejects duplicates/renames and
incompatible reference namespaces, then clones the library once and validates
the candidate before publishing it. A private shared reference context compares
existing native library ID/version, fonts, complete display settings, aliases and
ordered image names/body hashes. Independent recapture with equal values works;
pointer identity and generated tokens are not execution keys. Context hashing
does not decode image contents. This guard does not claim complete ownership or
reference-resolution validation.

The sync description-only entry point preserves absent fields, applies explicit
definition operations, retains complete component after-states and rejects stale
typed inputs before swapping a validated candidate into memory. It uses the same
crate-private collection assembly needed for later structural plans: immutable
input indices, ordered insertions/replacements, partial omission preservation,
complete final order, explicit deletion permission and overlap rejection.
The helper's length check alone is not an input identity check; the aggregate
executor checks complete typed before-state. Physical input hashes and serialized
plans remain later work.

All 223 components in three real libraries pass strict typed capture/materialize,
native no-op save/reopen, planned in-memory description replacement and native
patched save/reopen equality. Fixtures: lucashudson-Connectors (56), ICs (32),
arthurbenemann-STM32F4 (135). No normalization was added to snapshot equality.
Evidence: `/tmp/altium-snapshot-final-corpus.log`.

Final default tests: 403 core, 168 legacy spec and 12 sync tests pass
(`/tmp/altium-snapshot-final-tests.log`). The CLI builds
(`/tmp/altium-snapshot-final-cli-build.log`). The seven native snapshot tests
include the namespace guard; the twelve sync tests include six collection
assembly regressions, three image-capture checks and three scalar patch checks.

Independent reviews passed for typed retention, collection assembly, scalar
scope/before-state checking and the namespace guard. These are step-2 increments;
the numbered step is not complete.

## Ownership frontier

Original installed AD26 managed methods in `/tmp/altium-original-evidence/`
verify native PartCount includes reserved slot 0: GetState_PartCountNoPart0
returns GetPartCount()-1, SetState_PartCountNoPart0 stores value+1, and
SetState_CurrentPartID requires 1 <= value < GetPartCount(). The assembly and hash
are the same as the exporter evidence above. No new part-count enforcement or
source count conversion is claimed in this increment.

Current validation does not fully cover Additional ownership, all required
implementation parents, font/style/image references or part/display bounds.
None of 126 inspected top-level SchLib fixtures contained per-component Additional
streams; their logical address/index rules still need source-backed confirmation.
Structural editing must resolve the complete owner graph, build final ordered
collections from original addresses, remap references once, reject retained
orphans and validate save/reopen before completing step 2.

## Scalar and ownership increment

The next increment expands explicit component-property patches to description,
designator, component kind and hidden-pin visibility. Omitted fields retain their
complete native state. Kind projection follows the original AD26 compatibility
field precedence; updates emit the matching legacy/version2/version3 tuple.
Designator text edits preserve attributes. Creation captures a native ID in the
planned after-state, while creation/removal uses immutable record addresses to
rebuild Data and Additional owners. Header weight changes by the checked Data
record/alias count delta, preserving the input's accepted exporter convention.

The dual-stream graph implementation supersedes the unresolved address-formula
question above. Data uses a separate root at zero and child index plus one;
Additional has no root and uses zero-based addresses when its selector is set.
Both original and assembled graphs reject invalid addresses, cycles, forward
same-stream parents, retained orphans, nested Component boundaries and known
implementation-parent mismatches. IndexInSheet remains unchanged. See
[next-ownership-2026-09-05.md](next-ownership-2026-09-05.md) for original-source
provenance and the distinction between programmatic Additional roundtrips and
real corpus evidence.

This is still a bounded step-2 increment. Complete intent elaboration, all child
collections, part/display and font/image-reference validation, Additional pin
sidecars, saved-plan serialization and transaction execution remain outstanding.
