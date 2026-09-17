# Step-2 child-collection contract resolutions (A1 memo, persisted by A2)

This is a dated worklog copy of the step-2 increment-A1 contract resolutions
memo, persisted verbatim below the divider. Like everything under
`docs/worklogs/`, it is **non-authoritative evidence**, drafted 2026-09-16 from
AD26 decompiled sources (`/home/dev/projects/old-altium-cli/AD26-dotnet/`) and
a census over the 126 `data/schlib` corpus fixtures at baseline commit 436fa3c.
Where it conflicts with `docs/proposals/snapshot-intent.md` lines 204-210 (new
child owner-part assignment), THIS memo supersedes the proposal — native
evidence won that conflict (§2 below) — pending the increment-F rewrite of the
proposal and promotion of these rules into authoritative docs. Validate any
statement here against the current Rust code and the original AD26 sources
before relying on it.

---

# Step-2 A1 contract resolutions memo — 2026-09-16

Binding resolutions for plan increment A1 §1–9 (/tmp/step2/plan.md), drafted from the three
evidence dossiers (parts/display/swap; order/cache/refs; corpus census). Baseline commit
436fa3c.

Evidence classes used throughout:
- **[SRC]** original decompiled AD26 C# source, verified in this campaign
  (`/home/dev/projects/old-altium-cli/AD26-dotnet/`, paths relative to that tree,
  `Altium.Sch.DataModel/` unless noted). Includes [SRC-absence]: a grep-verified absence
  of any enforcement/code path.
- **[CORPUS]** observation over the 126 `data/schlib/*.SchLib` fixtures (74,572 components,
  2,749,732 text records, 1,376,142 binary pins; `corrupt/` and `encoding/` subdirs not
  scanned). Corpus evidence shows what *occurs*, never what AD26 *enforces* — per plan A2 §3
  it never selects a validation boundary.
- **[INF]** labeled inference; used only where flagged, never as the basis of a rule.

Boundary vocabulary (plan A2 §3 single rule, honored in every resolution below):
- **capture-forbidden** — no new validation may run at snapshot/parse time, because the AD26
  importer itself enforces nothing for this item; wild values must load and be preserved
  verbatim.
- **mutation** — checks run in altium-format when a batch replacement/creation API writes a
  value.
- **elaboration** — checks run in altium-sync when SchLibIntent is elaborated to a patch.

Where docs/proposals/snapshot-intent.md conflicts with native evidence, the conflict is
called out, native evidence wins for format behavior, and the doc change is queued for
increment F. Deliberate deviations from AD26 (fail-fast where AD26 silently tolerates or
coerces) are labeled as *our* policy, never attributed to AD26.

---

## §1 owner_part_id semantics — RESOLVED

**Rule (value domain and meaning).** `owner_part_id` is a signed short with exactly three
native meaning classes:

1. **−1 = "no part" sentinel.** Constructor/SetDefault value of every graphical data object
   [SRC: Objects/SchDataGraphicalObject.cs:34-39, 235-246] and the iterator ignore-filter
   value (`CIgnorePartId = -1`) [SRC: Iterator.Filters/SchDataIteratorFilterPartPrimitivesBase.cs:8].
   In files it belongs to the root text children: Designator and Parameter, which the part
   filter never filters by part or display mode at all (only hidden-flag) [SRC: same file
   :10-51]. [CORPUS]: −1 occurs 1,677,514× — 74,510 Designators + 1,603,004 Parameters;
   never on pins; never on drawing graphics.
2. **0 = common-to-all-parts — a pin-only wildcard.** The part filter passes a Pin for the
   current part when `OwnerPartId == partId || OwnerPartId == 0`; a generic graphic passes
   ONLY on exact equality — 0 is NOT a wildcard for non-pins [SRC: SchDataIteratorFilterPartPrimitivesBase.cs:10-51].
   Part-0 pins are excluded from part-swap fan-out [SRC: EngineObjects/SchPin.cs:349-353]
   and from implemented-part calculation [SRC: EngineObjects/SchComponent.cs:1742-1770].
   Compatibility containers (ParameterList, ImplementationList) hard-code 0 [SRC:
   Objects/SchDataParameterList.cs:9-14; SchDataImplementationList.cs:12]. [CORPUS]: pins
   with 0: 5,950 of 1,376,142 (18 files); non-pin 0 population is *exactly* the
   implementation chain (ImplementationList/Implementation/ImplementationMap/ParameterList/
   MapDefiner = 515,582); zero drawing graphics carry 0.
3. **1..native_count−1 = numbered parts.** −1/0 are the only "special" ids [SRC:
   Utils/MultiPartUtils.cs:5-12]; the editor setter enforces `1 <= id < native PartCount`
   [SRC: SchComponent.cs:1038-1044]; native slot 0 is the reserved/common slot, so
   effective = native − 1. `DeletePart` keeps parts contiguously 1-numbered and leaves 0/−1
   children untouched [SRC: SchComponent.cs:1593-1620].

**Wire encoding.** Text records: ShortInt, absent ≡ 0 (writer omits zeros, reader defaults
0), −1 always written explicitly [SRC: FileFormats/FileFormatV5.cs:5070-5102;
Serialization/SchDataSerializerParam.cs:807-817, 484-499; SchDataSerializer.cs:1096-1101].
Binary pins: i16 at offset 0x05, display mode u8 at 0x07 [SRC: FileFormatV5.cs:566-573;
matches crates/altium-format/src/sch_records.rs:259-299].

**Enforcement: none, anywhere on the file path.** Neither `SchDataImporterLibraryV5` nor
`FileFormatV5` bounds-checks OwnerPartId against PartCount [SRC-absence: both files
grepped; import stores raw, export writes raw]. The FileHeader `PartCount{n}` index is
write-only; the component record's PARTCOUNT is authoritative [SRC:
SchDataExporterLibraryV5.cs:142; SchDataImporterLibraryV5.cs:644-660].

**Out-of-range values exist in the wild and must load.** [CORPUS]: 150 records across 3
files / 11 components carry OWNERPARTID == native PARTCOUNT (one past the last legal part):
Oktay97-OT_Library (graphics + labels), aKaReZa75-Other (Designator + Parameter, raw-dump
verified), kmilo17pet-LT_PMIC_LT (Rectangle, raw-dump verified). [INF from the SRC filter]:
such records are simply never rendered for any selectable part; nothing native repairs or
rejects them.

**Binding mapping for A2's owner-path rule** (consumed by A2 §1 address vocabulary and
A2 §2 projections):
- Owner groups are keyed by the **raw** native value: `PartIndependent(−1)`,
  `SharedPins(0)` (pins only), `Part(n)` for `1..=effective`, and
  `OutOfRangeOwner(n ≥ native_count)`.
- Intent `part N` maps to owner_part_id `N` directly (1-based); the native/effective +1
  offset lives ONLY in the count field, never on child records.
- **Single-part case** (native 2, effective 1): component-level pins/graphics are the
  `Part(1)` group. [CORPUS]: every ordinary single-part component uses native PARTCOUNT=2
  (66,346 components); native 1/0/absent: zero occurrences. There is no native "component
  scope" container object — scope is purely the per-record field.
- **Multi-part case**: the shared owner for the component-level pins collection is
  `SharedPins(0)`. Graphics have NO shared form natively (0 is not a wildcard for
  graphics); a graphic with owner 0 or any out-of-range owner is preserved-only content.
- `parameters()` (root-owned Parameter records) and the designator are keyed/singleton
  collections; their owner_part_id (−1 dominant, wild tails at 1/≥2 exist [CORPUS]) is a
  preserved field, not a grouping axis.
- `OutOfRangeOwner` children and owner-0 graphics are **intent-unaddressable preserved
  content** (same treatment class as Symbol/Hyperlink in A2 §2): preserved verbatim through
  any patch; a managed complete/positional scope over a collection containing them blocks
  at **elaboration**; creating one is a hard error at **mutation**.

**Boundary.** Capture-forbidden (AD26 enforces nothing; the 150 wild records and the 5,950
part-0 pins must keep loading). Range/meaning checks on values *we write* run at mutation;
part-context mapping runs at elaboration.

**Consumers.** A2 (address vocabulary + projections), B/C (shared validation helpers), E
(part mapping + blocking rules).

---

## §2 owner_part_id for newly created children — RESOLVED (proposal conflict; native side adopted)

**Native evidence.** Editor placement into a SchLib assigns every registered graphical
object the component's **current part id (≥1) and current display mode — never 0** [SRC:
EngineObjects/SchLib.cs:227-236 `I_RegisterInDocument`]. Programmatic construction default
is −1 with display mode 0 [SRC: SchDataGraphicalObject.cs:34-39]. Part-0 pins are NOT a
creation default: the only producer is the V4-upgrade dedupe pass
`ReduceComponentCommonPins` [SRC: ImportExport/SchDataImporterExporterUtils.cs:507-541;
callers SchDataImporterLibraryV4.cs:119,395, SchSheetV4LoadEventHandler.cs:158]; the V5
path preserves but never creates them. For component parameters/designators, no dedicated
assignment path was found; the −1 ctor default surviving is the best-supported explanation
[INF, from SRC default + CORPUS distribution — flagged, but −1 is the only supported
value].

**Conflict with the proposal.** snapshot-intent.md:204-210 ("new component-level children
use owner part zero if the effective resulting part count exceeds one or explicit part
blocks exist, otherwise owner part one") is contradicted by native evidence for the
0-for-multipart half: AD26 has no creation path that assigns 0, and 0 is meaningless for
non-pin graphics (§1). **Native side wins.** Increment F must rewrite
snapshot-intent.md:204-210 to the rule below.

**Binding rule** (consumed by B for parameters/graphics, C for pins, E for part-context
elaboration):
- Children authored inside `part N` → owner_part_id = `N` (mutation-boundary check:
  `1 ≤ N ≤ effective`).
- Component-level pins/graphics in a **single-part** component → owner_part_id = 1
  (native-faithful: placement assigns the current part, which is 1).
- Component-level **pins** in a **multi-part** component → owner_part_id = 0. This is an
  explicit authored placement into the natively defined shared-pin slot ([SRC] wildcard
  semantics fully defined even though no editor creation path produces it — the caveat is
  recorded; the semantic is not guessed).
- Component-level **graphics** in a multi-part component → **hard error at elaboration**
  ("no shared graphics owner exists natively; author the graphic inside a part block").
  Creating a graphic with owner 0 or any out-of-range owner is a hard error at mutation.
- New root-owned **Parameter/Designator** records → owner_part_id = −1 (ctor default [SRC]
  + corpus-dominant [CORPUS]; [INF] caveat above recorded).
- New children get owner_part_display_mode = 0 (ctor default [SRC:
  SchDataGraphicalObject.cs:34-39]; batch APIs have no "current display mode" cursor; see
  §4).
- Part growth never reclassifies existing children (native `DeletePart`/`AddPart` precedent
  [SRC: SchComponent.cs:1575-1620]; proposal's "growth never moves owner-1 children to
  owner 0" survives — that half of the proposal is consistent with native evidence).

**Boundary.** Mutation (format-level creation validation of the assigned value) +
elaboration (part-context requirement / multi-part component-level graphics block).
Capture-forbidden stands: nothing here validates loaded files.

**Consumers.** B (parameter/graphic New inits), C (pin New inits), E (part-block
elaboration). F (doc rewrite of snapshot-intent.md:204-210).

---

## §3 current_part_id bounds and native part_count=1 — RESOLVED

**Native bounds.** Editor setter enforces `1 ≤ id < native PartCount`, silently no-ops
otherwise [SRC: SchComponent.cs:1038-1044]; `IsMultiPartComponent` = native > 2 [SRC:
:2076-2079]; navigation wraps within 1..native−1 [SRC: :825-845]. Import stores PARTCOUNT /
CURRENTPARTID / DISPLAYMODECOUNT raw through unchecked data setters [SRC:
FileFormatV5.cs:2933-3086; Objects/SchDataComponent.cs:632-635, 752-755, 677-680]. Absent
wire keys read as 0 [SRC: §1 wire cites].

**Native part_count<2 handling = documented upgrade, not error.** The ffv5 after-import
event floors native PartCount to 2 and DisplayModeCount to 1 [SRC: SchComponent.cs:895-903
`HandleAfterImportEvent`; :1777-1784 `SetState_PartCount_NoCheck`; :1786-1793]. `AddPart`
jumps native 1 → 3, i.e. treats native 1 as one effective part [SRC: :1575-1586].
**CurrentPartID is never touched on load — no clamp exists anywhere on the load path**
[SRC: same event handler; SRC-absence]. [CORPUS]: native PARTCOUNT=1 occurs **zero** times
(min observed is 2; histogram 2:66,346 … 51:4); CURRENTPARTID outside [1, PARTCOUNT):
**zero**; boundary CURRENTPARTID = PARTCOUNT−1 occurs. So the part_count=1 case is decided
by source evidence alone, exactly as plan A1 §3 anticipated.

**Binding rules:**
1. **Native part_count < 2 (including wire-absent ⇒ 0): documented upgrade to native 2 at
   load, surfaced as a reported normalization** — never a hard error (AD26 loads and
   upgrades such files [SRC]), never silent. This is a load-semantics normalization
   mirroring the native ffv5 pipeline (and the repo's "upgrade to latest format" policy),
   not a capture-time validation — the capture-forbidden rule is not violated.
   `effective_part_count()`'s contract is updated to "always ≥ 1" (fixes red-team [7];
   consumed by A2). Same treatment for display_mode_count < 1 → 1 (§4).
2. **current_part_id valid range is [1, native_count)**, enforced at **mutation** for any
   value our APIs write. Hard-erroring (instead of AD26's silent no-op) is a deliberate
   deviation, ours by fail-fast policy — never cite it as AD26 behavior.
3. **Out-of-range current_part_id in a loaded file is preserved verbatim** (AD26 leaves it
   alone [SRC]; capture-forbidden). A parts mutation on such a component must first resolve
   it explicitly (reported) or hard-error — never silently clamp.
4. **Shrink behavior (consumed by C):** follow the native `DeletePart` precedent —
   per removed slot, `CurrentPartID` decrements by 1 only when it was > 1, then PartCount
   decrements [SRC: SchComponent.cs:1608-1620]. A deterministic, **reported** field
   adjustment; a clamp-to-range has no native precedent and is forbidden (satisfies
   red-team [18]).
5. **Rust parse-default deviation, recorded:** AD26's absent-key in-memory value is 0 for
   PARTCOUNT/CURRENTPARTID (engine floor then fixes PartCount only); Rust parses PARTCOUNT
   default 1 (schlib.rs:304-306) and current_part_id default 1 (sch_records.rs:689).
   [CORPUS]: zero files exercise absent keys, and both pipelines re-omit the default on
   save, so serialized bytes agree. Keep the Rust defaults; rule 1 above subsumes the
   PARTCOUNT end-state. Documented deviation, revisit only if a wild absent-key file
   appears.

**Boundary.** Load normalization (rule 1, reported); mutation (rules 2–4).
**Consumers.** A2 (`effective_part_count` contract), C (parts grow/shrink +
current_part_id adjustment + reporting), E (report expansion).

---

## §4 display_mode_count / owner_part_display_mode / custom_display_mode_names — RESOLVED

**Native evidence.** DisplayModeCount is a Byte, imported raw, absent ⇒ 0, ffv5-floored to
1 [SRC: FileFormatV5.cs:2870, 2944-2946; SchComponent.cs:1786-1793]. Per-record
OwnerPartDisplayMode is a Byte with **no import/export validation** against
DisplayModeCount [SRC: FileFormatV5.cs:5074-5102 graphics; :445, 571-573 binary pins]. The
component's own DisplayMode imports raw [SRC: :2955-2957]; the only bounds live in checked
editor setters (`SetDisplayModeWithCheck` clamps to count−1 [SRC:
SchDataComponent.cs:672-675]; `SetState_DisplayModeCount_Check` refuses shrink below
current DisplayMode [SRC: SchComponent.cs:1020-1026]). Out-of-range values are retained
verbatim on load and save; `DeleteDisplayMode`/`PurgeUnusedDisplayModes` are explicit
editor commands, load-invoked only on the V4 path [SRC: SchComponent.cs:1623-1707;
SchDocumentV4LoadEventHandler.cs:30]. A record whose mode ≥ count is simply never displayed
[INF from the filter, flagged]. CustomDisplayModeName round-trips exactly indices
0..count−1 — names at indices ≥ count are dropped by AD26 on both load and save [SRC:
FileFormatV5.cs:2923-2928, 3080-3085; unbounded dict storage SchDataComponent.cs:692-704].
[CORPUS]: DISPLAYMODECOUNT ∈ {1:74,186, 2:360, 3:14, 4:6, 5:5, 8:1}, never 0/negative;
out-of-range or negative OWNERPARTDISPLAYMODE occurrences: **zero** corpus-wide (incl. the
dmc=8 component using all modes 0..7); no −1 display modes (the red-team [15] "known quirk
class" hypothesis has no support in these 126 files).

**Binding rules:**
1. display_mode_count < 1 at load → documented, reported normalization to 1 (same class as
   §3 rule 1; mirrors native ffv5).
2. **Capture-forbidden** for owner_part_display_mode vs count: preserved verbatim (AD26
   validates nothing on the file path).
3. **Mutation**: any owner_part_display_mode our APIs write must satisfy
   `0 ≤ mode < display_mode_count`; the component's own display_mode likewise (mirrors the
   checked setter's bound; hard error instead of silent-ignore is our deliberate
   deviation). New children default to mode 0 (§2).
4. **Mutation**: display_mode_count shrink must not orphan values — it is a hard error if
   any retained child's owner_part_display_mode, the component's display_mode, or a
   custom_display_mode_names index would fall outside the new count, unless the same batch
   explicitly removes/retargets them (reported). Never silently drop names — AD26's silent
   drop-on-save of names at indices ≥ count is native behavior we deliberately do NOT
   replicate (cardinal rule: no silent drops); replicating it may never be justified as
   "AD26 does it".
5. custom_display_mode_names writes: index < display_mode_count at mutation.

**Boundary.** Load normalization (rule 1); mutation (rules 3–5). No elaboration-only checks.
**Consumers.** A2 (validation helpers), B (graphics creation), C (pins; count changes if
ever exposed).

---

## §5 part swap groups — RESOLVED (projection + fan-out; combined-encoding parse mandatory)

**Native storage.** There is **no** part-level record or component field for swap groups
[SRC-absence: `SwapGroup`/`PartSwap` matches only in PCB assemblies; Altium.Sch.DataModel
has no schematic part-swap entity]. All swap state is per-pin: `swapIdPin`, `swapIdPair`
(PinMiscData sidecar as `PairSwapID`), and the combined `swapIdPartAndPartPin` [SRC:
Objects/SchDataPin.cs:19, 54-59, 297-311, 500-512; exporter SchDataExporterLibraryV5.cs:356-367,
393-409; importer SchDataImporterLibraryV5.cs:853, 931-977 (PinWideText overrides binary);
consts FileFormatConsts.cs:645-649].

**The "part swap group" is the prefix of the per-pin combined field.** Encoding
[SRC: EngineObjects/SchPin.cs:31, 152-175, 459-472]: `SwapIdPart` optionally followed by
`"|&|" + SwapIdPartPin`; no separator ⇒ the whole string is the part group; the separator
is appended only when the part-pin half is non-empty. Part-level semantics are derived by
editor fan-out: `SetState_SwapIdPart` rewrites the prefix of every pin of the same part and
display mode, **skipping owner-0 pins**, preserving each pin's own suffix [SRC:
SchPin.cs:331-360; UI path InteractiveProperties…/SchPinDataObject.cs:1015-1050].

**Binding rules:**
1. **Projection (A2 parts() view):** `part_swap_group` for part N is derived, never
   stored: parse each member pin's combined field per the native split rule; the group is
   the **unanimous** SwapIdPart prefix across the part's non-owner-0 pins (all display
   modes). Non-unanimous ⇒ the projection is a typed Mixed state; a part with no eligible
   pins projects "no group". (The dossier records that the SRC evidence directly supports
   this unanimity-or-block projection; mixed values are possible in files since nothing
   validates on load.)
2. **Elaboration (E):** authored `part_swap_group` Set(v) fans out: rewrite the prefix of
   every non-owner-0 pin of that part to `v`, preserving each pin's suffix and the native
   concat rule (separator emitted only when the suffix is non-empty [SRC]); Clear sets the
   prefix to empty the same way. A Set/Clear against a currently **Mixed** projection is a
   hard blocking error at elaboration (never silently overwrite a distinction the file
   holds). Deviation note: the native setter fans out per current display mode only; our
   batch fan-out covers all of the part's display modes — a deliberate batch-semantics
   choice made safe by rule 2's Mixed block (a per-mode distinction always projects Mixed
   and blocks first). Owner-0 pins are never touched by fan-out (native-faithful [SRC]).
3. **Encoding preservation (format layer):** the combined string on an untouched pin is
   preserved byte-verbatim (wild data includes `"|&|"` with both halves empty — 382,333
   pins [CORPUS] — which the native concat would not regenerate; only touched pins are
   re-encoded). Mutating either half re-encodes per the native rule. The Rust model must
   grow the prefix/suffix split (no `|&|` handling exists anywhere in the Rust tree today —
   sch_records.rs:230-232 stores the combined string raw); sidecar consistency
   (PinWideText `SwapId`/`SwapIDPart`, PinMiscData `PairSwapID`) rides the existing
   save-time sidecar serialization from typed pin state.
4. **Mojibake separators** (`?&?` variants, 75k+ pins [CORPUS]): NOT repaired — no AD26
   repair exists [SRC-absence]. Native parse semantics apply literally: no `|&|` found ⇒
   the entire string (mojibake included) is the prefix. Such pins project odd-but-defined
   prefixes and will typically block via the Mixed rule; any "fix-up" would be a guess and
   is forbidden.

**Boundary.** Capture-forbidden (nothing validates swap strings on load). Elaboration
(Mixed block, fan-out). Mutation (ordinary typed pin-field writes + re-encoding).
**Consumers.** A2 (parts() projection type incl. Mixed state), E (fan-out + block), C
(pin field plumbing / sidecar consistency). Resolves red-team [6]/[13]; the map-contracts §4
"swap groups" ambiguity is closed as: swap groups are scoped per-pin field bindings —
there is no swap-group collection, so aggregate replacement "covers" them via pin fields
plus this part-level projection/fan-out.

---

## §6 IndexInSheet — RESOLVED

**Native evidence.** New-object value is −1 [SRC: Objects/SchDataObject.cs:63, 356];
on-wire absent ⇒ 0 [SRC: FileFormatV5.cs:5049-5051; SchDataSerializerParam.cs:512-528 —
matches Rust default 0, sch_records.rs:130-131]. Save **unconditionally overwrites** the
field from the current runtime 0-based sibling index for every child living in its owner's
objectList [SRC: Objects/SchDataContainer.cs:241, 270-273, 388-394; export writes exactly
that value, FileFormatV5.cs:5021; LibraryV5 path confirmed SchDataExporterLibraryV5.cs:68 →
SchDataLibrary.cs:106-117 → SchDataComponent.cs:861-885]. Objects not in an objectList
serialize −1: component roots, Designator/Comment field objects, the synthetic
ImplementationList wrapper [SRC: same cites; CORPUS corroboration: 122× `INDEXINSHEET=-1`
in ICs.SchLib]. The SchLib importer never applies IndexInSheet on load — the
`MoveObjectToIndex` postpass is shadowed out of the LibraryV5 flow [SRC:
SchDataImporterLibraryV5.cs:50-63, 110-113 vs SchDataImporterBaseV5.cs:72, 178-189].
Sibling ordinals count pins and Additional-destined subtree roots even though binary pins
carry no IndexInSheet in their payload [SRC + prior worklog]. Emission *order* is a
separate mechanism (SchDataObjectComparator; binary codes > 225 sort last) and is not
IndexInSheet [SRC: Objects.Comparators/SchDataObjectComparator.cs:8-17].

**Binding rules:**
1. **Creation (B, C):** a new in-objectList child is written with index_in_sheet = its
   0-based position in the owner's **final** runtime child sequence (pins and
   Additional-destined subtree roots count as siblings) — exactly what AD26's
   AddToListForSave would produce. New Designator/Comment field objects and any
   non-objectList object get −1.
2. **Reorder (B, C):** for every owner whose child sequence a batch structurally touched
   (insert/remove/reorder), **renumber all** of that owner's in-objectList children to
   final 0-based sibling positions (AD26-faithful output for those owners [SRC:
   unconditional renumber]). For **untouched owners, preserve stored values verbatim** —
   this half deviates from AD26's renumber-everything-on-save and is our deliberate policy
   ([INF], adopted): it keeps no-op snapshot equality and foreign-file byte preservation,
   consistent with the ownership worklog's "preserve unless explicitly edited". Never cite
   the preserve half as AD26 behavior.
3. index_in_sheet is never an owner address, never identity, and never validated for
   consistency against sibling order at capture (**capture-forbidden**; AD26's importer
   ignores it entirely [SRC]).

**Boundary.** Mutation (assignment policy inside the batch assembly engine; no new
validation anywhere). **Consumers.** B (parameters/graphics), C (pins/parts). Resolves
red-team [8] and the map-contracts §4 "IndexInSheet on creation" ambiguity.

---

## §7 all_pin_count — RESOLVED

**Native evidence.** Export writes the stored field verbatim [SRC: FileFormatV5.cs:2906;
SchDataComponent.cs:363-365]; import stores it back [SRC: :3075-3076]. The complete writer
set: import; engine Init 0 for fresh engine components; `UpdateAllPinCount` fills **only
when stored ≤ 0**; scrap-compile reset (SchDoc path) [SRC: SchComponent.cs:414-420,
2659-2665, 4888-4891; ComponentsAllPinCountsUpdater.cs]. **No pin add/remove path
invalidates it — a stale positive count survives pin edits** [SRC-absence: full-tree grep].
The save-time fill only happens when engine objects are attached (editor open), riding the
font-marking pass [SRC: FileFormatV5.cs:5236-5243 → SchDataLibrary.cs:119-129 →
SchDataPin.cs:587-589, 704-708 → SchComponent.cs:953-991]; a pure data-model save exports
the stored value verbatim — matching the observed ICs.SchLib behavior.

**Binding rule:** structural pin edits in altium-format **must not touch** stored
`all_pin_count` — preserve verbatim, positive or zero (this IS the AD26 pure-data-path
behavior our writer already implements). Silent recomputation is forbidden. If a refresh is
ever wanted it must be a separate, explicit, **reported** operation replicating the exact
native formula — first-level pins across ALL parts, filtered to
`owner_part_display_mode == component display_mode`, deduplicated by pin designator,
last-wins [SRC: SchComponent.cs:1929-1954] — and it is NOT in step-2 scope.

**Boundary.** None — no new validation at any boundary; the rule is a hands-off invariant
of the mutation engine. **Consumers.** C (pin structural edits + targeted save/reopen
tests).

---

## §8 font / style / image references — SPLIT RESOLUTION

### 8a. font_id — RESOLVED

**Native evidence: AD26 validates nothing and coerces silently.** Import maps ids through a
1000-entry translator reset to identity; ids beyond the table but ≤ 1000 pass through
unmapped; ids < 1 or > 1000 are silently coerced to 1; missing-font lookups fall back
(size 10 / null name) [SRC: SchDataSerializer.cs:756-775; FileFormatV5.cs:5273-5302;
FontManager/SchDataFontManager.cs:532-538, 633-641, 190-271; pin sidecar same path,
SchDataImporterLibraryV5.cs:782-810]. Export silently rewrites any unmarked/dangling id to
1 [SRC: SchDataFontManager.cs:541-568; SchDataSerializer.cs:415-435]. New text objects take
the global find-or-create id for Times New Roman 10 [SRC: SchDataFontManager.cs:617-621;
SchDataLabel.cs:30; SchDataPin.cs:684, 689]. [CORPUS — two observations, method-distinct,
reported without averaging]: the typed census over 1,717,426 enumerated font references
found **0 dangling**; a separate strings-based approximate scan flagged 6 files with
`|FONTID=` beyond FONTIDCOUNT (strings can misread binary blocks; the >1000 hits are likely
corruption). The discrepancy is a method artifact and does not affect the rule below,
which does not depend on corpus cleanliness.

**Binding rules:**
1. **Capture-forbidden:** font ids load and are preserved verbatim; no table-membership
   check at snapshot time (AD26 enforces none; "AD26 validates it" can never justify
   capture-time rejection).
2. **Mutation:** every font_id our APIs write must reference an existing font-table entry
   `[1, table_len]` — hard error otherwise. Never replicate AD26's silent coerce-to-1
   (deliberate deviation, ours).
3. **Creation default:** a new text-bearing child with no authored font resolves via
   find-or-create against the destination file's font table for the native default entry
   (Times New Roman, size 10, rotation 0) — mirroring the native GetFontID find-or-create
   projection [SRC]. Font-table appends are an ordinary format-level mutation (fonts are
   not part of ReferenceContext).
4. **Elaboration (E):** intent-level font policies resolve against the destination font
   table using the A2 helpers (satisfies the language doc's "font defaults require
   validation against the destination font table").

**Boundary.** Mutation + elaboration; capture-forbidden. **Consumers.** A2 (helpers), B
(text-bearing children), E (font policy resolution).

### 8b. style_id — UNSUPPORTED (authoring); preservation RESOLVED

**Evidence.** The managed V5 format neither reads, writes, nor validates StyleID —
`StyleID`/`StyleIDCount` are absent from FileFormatConsts.cs and all of
Altium.Sch.DataModel; the string survives only as legacy interface constants [SRC-absence:
Altium.Edp.Interfaces/Rt_Schematic/Consts.cs:708, 743; FileFormatV5.cs:3089-3130 font-only
table]. The Delphi/native side, where a display-style table implementation must live, was
**not examined** (no ghidra evidence this campaign). [CORPUS]: STYLEID in 3 of 126 files,
4,204 nonzero refs, 0 dangling, none in the three gate fixtures.

**Binding rules:** loaded style_id values and style tables are preserved verbatim
(capture-forbidden trivially — there is no evidence of ANY native enforcement to mirror).
**Authoring or modifying style_id references or style tables through any step-2 API is a
hard error with context** ("style_id semantics unproven — native (Delphi) style-table
implementation not yet reverse engineered"). No validation rule may claim AD26 authority.

**Upgrade path:** ghidra-cli decompilation (project altium26) of the Delphi binary that
consumes STYLEID/STYLEIDCOUNT on the SchLib load/save path — specifically its import
lookup, bounds handling, and dangling-id behavior — or discovery of a managed reader.
Until then the hard error stands.

**Boundary.** Mutation (the hard error fires there; no intent surface exists, so E never
reaches it). **Consumers.** B (graphics engine must refuse style_id edits), F (docs note).

### 8c. image file_name ↔ embedded storage — RESOLVED

**Native evidence: fully silent tolerance in both directions.** `ImportImage` reads
FileName/EmbedImage unchecked [SRC: FileFormatV5.cs:1760-1804]; the SchLib matching pass
pairs EmbedImage=true images to Storage entries by exact key then case-insensitive
filename; an unmatched storage blob is silently skipped and an unmatched image simply never
receives data — no error either way [SRC: SchDataImporterLibraryV5.cs:389-453]. [CORPUS]:
70/70 embedded images exact-match a storage resource; 2 external-path images; 1 wild
orphan storage entry (SMotlaq-Schem_lib.SchLib, raw-byte verified param-fragment name)
that no image references — it must keep loading.

**Binding rules:**
1. **Capture-forbidden:** dangling image→storage references and orphan storage blobs load
   and are preserved verbatim (the SMotlaq file is the regression case).
2. **Mutation:** creating or retargeting an embedded image record must reference a storage
   asset that exists in the resulting library state — either preexisting or inserted by the
   same batch's library-level image-asset op (increment D's namespace-transition ops, which
   recompute ReferenceContext). Hard error otherwise.
3. **Mutation (D):** removing a storage asset still referenced by any image record is a
   hard error; removing image records may leave orphan assets only when the batch does not
   also claim complete asset scope — asset lifecycle is owned exclusively by D's explicit
   asset ops, never inferred from graphics edits.
4. **Elaboration (E):** image-graphic intents with asset sources elaborate through D's ops;
   an intent referencing an asset the plan does not provide or find blocks at elaboration.

**Boundary.** Mutation + elaboration; capture-forbidden. **Consumers.** A2 (helpers), B
(image record edits), D (asset ops + context recompute), E (asset sources).

---

## §9 corpus census — RESOLVED (recorded observations; corpus never selects boundaries)

All [CORPUS]; 126/126 files parse with the strict parser, 0 failures; 74,572 components;
0 Additional-stream records corpus-wide. Full tables preserved at scratchpad
`census-release2.txt` (outside the repo). Headline distributions, binding as *test
expectations only*:

- **PARTCOUNT** (native): min 2, max 51; 2:66,346, 3:5,008, 4:1,211, 5:1,170, … 51:4.
  Native 1/0/absent: zero. → The §3 part_count=1 rule rests on [SRC] alone, as required.
- **CURRENTPARTID**: 73,757 of 74,572 are 1; zero violations of [1, PARTCOUNT); boundary
  value PARTCOUNT−1 occurs (e.g. (51,50)).
- **OWNERPARTID**: pins never −1/negative; pin 0: 5,950 in 18 files (occurs even at
  PARTCOUNT=2); non-pin 0 = exactly the implementation chain (515,582); −1 = exactly
  Designator+Parameter root text (1,677,514, with tiny wild tails at ≥1); graphics always
  ≥1; **150 records / 3 files / 11 components carry OWNERPARTID == native PARTCOUNT**
  (raw-dump verified in aKaReZa75 and kmilo17pet) — the concrete capture-must-accept class
  for §1.
- **OWNERPARTDISPLAYMODE**: zero out-of-range or negative values corpus-wide;
  DISPLAYMODECOUNT never 0; the single dmc=8 component uses all modes 0..7; no −1 quirk
  values found.
- **References**: font_id 0 dangling over 1,717,426 typed refs (corpus absence is not
  proof about the wild — the renderer's default-font fallback exists for a reason);
  style_id 0 dangling (3 files with tables; no nonzero ref without a table); images 70/70
  embedded matched exactly, 1 orphan storage entry (SMotlaq).
- Not scanned: `data/schlib/corrupt/` (4) and `data/schlib/encoding/` (2) — this census
  says nothing about strict-load failure modes.

**Binding consumption:** (a) every increment's capture path must keep accepting all 126
files verbatim — explicitly including the 150 out-of-range-owner records, the 5,950 part-0
pins, the mojibake swap separators, and the SMotlaq orphan storage entry; (b) the corpus
run in the standing gate uses the three real fixture names
(lucashudson-Connectors.SchLib, ICs.SchLib, arthurbenemann-STM32F4.SchLib); (c) no census
figure may be cited as an enforcement boundary — boundaries come from §1–§8's [SRC]
evidence and our declared policies only.

---

## Consumption / boundary summary

| § | Status | Consumed by | New validation boundary |
|---|--------|-------------|--------------------------|
| 1 | RESOLVED | A2 (addresses, projections), B/C (helpers), E (blocks) | capture-forbidden; mutation (written values); elaboration (unaddressable-content blocks) |
| 2 | RESOLVED (proposal 204-210 overridden by native; F rewrites doc) | B, C, E | mutation (creation values); elaboration (part context) |
| 3 | RESOLVED | A2 (effective count contract), C, E (reports) | load normalization (reported); mutation |
| 4 | RESOLVED | A2, B, C | load normalization (reported); mutation |
| 5 | RESOLVED | A2 (projection), C (pin plumbing), E (fan-out) | capture-forbidden; elaboration (Mixed block); mutation (re-encode) |
| 6 | RESOLVED | B, C | mutation only (assignment policy; no validation) |
| 7 | RESOLVED | C | none (hands-off invariant) |
| 8a | RESOLVED | A2, B, E | capture-forbidden; mutation + elaboration |
| 8b | UNSUPPORTED (authoring) / RESOLVED (preserve) | B (hard error), F (docs) | mutation hard error until Delphi evidence |
| 8c | RESOLVED | A2, B, D, E | capture-forbidden; mutation + elaboration |
| 9 | RESOLVED (observations only) | all (test expectations), standing gate | none — corpus never selects boundaries |

Doc changes queued for increment F: rewrite snapshot-intent.md:204-210 (new-child owner
assignment, §2); note the §3/§4 load normalizations and the §6 renumber-touched/preserve-
untouched policy wherever the addressing section lands; record §8b's style_id hard error
and its upgrade path.

---

# Errata (appended 2026-09-16, increment-B review fix pass — NOT part of the verbatim memo above)

Corrections to the memo body discovered while implementing/reviewing increment B.
Each is queued for the increment-F promotion pass; until then the memo text above
stands as-written EXCEPT where contradicted below.

1. **§6 — the synthetic ImplementationList wrapper serializes IndexInSheet 0, not −1.**
   The memo's "Objects not in an objectList serialize −1" list wrongly includes the
   ImplementationList wrapper. Its constructor calls `SetIndexInSheetForSave(0)`
   (Objects/SchDataImplementationList.cs:9-14), and raw bytes of all three gate
   fixtures show the RECORD=44 param block as literally `|RECORD=44` with NO other
   keys (INDEXINSHEET absent ⇒ wire 0) — e.g. ICs.SchLib
   `…|UNIQUEID=WTPYYOAU\0 … |RECORD=44\0 … |RECORD=45|OWNERINDEX=13|INDEXINSHEET=-1|…`.
   (Implementation records themselves DO serialize −1, as the bytes show.) Harmless
   for B — the chain is never renumbered and round-trips byte-identically — but the
   memo sentence must be corrected in F.
2. **§6 — field-object classification scoping.** The "Designator/Comment field
   objects" phrasing under-describes the native mechanism: `SchDataParameter.
   UpdateOwner` (Objects/SchDataParameter.cs:128-196) dispatches on NAME ALONE
   (DefaultNet/HiddenNetName/PinSelectionMemory/Comment/Description/PinUniqueId)
   with owner-interface guards INSIDE the branches, and a special-named parameter
   whose owner lacks the interface (e.g. root "Description" — `SchDataComponent`
   is not an `ISchDataDescriptionOwner`, and `GetFieldObjects` yields only
   designator+comment) attaches nowhere and is silently DROPPED by AD26 on the
   next save. Rule 1's "New Designator/Comment field objects … get −1" remains
   correct; F must add: (a) the name-dispatch/interface-guard mechanics, (b) that
   AD26 drops unattached specials on save and our engine instead preserves AND
   numbers them as ordinary siblings (deliberate no-silent-drop deviation; wild
   corpus precedent: Custom.SchLib root Description INDEXINSHEET=3,
   kmilo17pet-PIC16_Part1 INDEXINSHEET=4), and (c) that INDEXINSHEET=-1 in the
   corpus is carried ONLY by Designators and root Comments — ordinary root
   parameters carry sibling ordinals (the §1 census figures are OWNERPARTID-only
   evidence).
3. **§8a rule 3 — "fonts are not part of ReferenceContext" is contradicted by the
   implementation.** `ReferenceContext` carries the captured font table
   (crates/altium-format/src/schlib/snapshot.rs:25-32) and `replace_components`
   enforces context equality (snapshot.rs:179), so a font-table append is NOT an
   ordinary format-level mutation today. Increment B therefore implements the
   default-font resolution find-only and hard-errors when an append would be
   required, naming the increment-D dependency. Increment D's namespace-transition
   design must explicitly own one of: a declared font-table append that recomputes
   the context, or moving fonts out of the context — after which the find-or-create
   rule becomes implementable as the memo states. F updates the memo parenthetical
   to match whichever D adopts.

   **RESOLVED by increment D1 (2026-09-16):** D adopted the FIRST option —
   fonts remain inside `ReferenceContext`, and font-table appends are a
   declared library-level namespace transition (`SchLibEdit::font_appends`
   via `SchLibSnapshot::with_library_edit`,
   `crates/altium-format/src/schlib/snapshot/library.rs`), which recomputes
   the resulting snapshot's context from the final library state. The
   `with_children` default-font resolution stays find-only against the
   captured table; its hard error now names `with_library_edit` as the
   declared path (apply the library edit first, then take component
   snapshots from the result). Increment F must rewrite the §8a rule 3
   parenthetical to: "font-table appends are a library-level namespace
   transition declared via `SchLibEdit::font_appends`; fonts are part of
   `ReferenceContext`".

4. **§8c rule 2 — "inserted by the same batch's library-level image-asset op" does not
   match the adopted composition.** (Appended by the increment-D1 review fix pass,
   2026-09-16.) Under D1's composition, embedded image records are created through
   `with_children` against a snapshot's CAPTURED context, so a record referencing an
   asset inserted "by the same batch" is not expressible; the supported path is the
   TWO-STEP flow: a library edit declaring the asset insert (which recomputes the
   result's `ReferenceContext`), then component edits taken from the RESULT snapshot
   (tested end-to-end: `image_asset_insert_unlocks_two_step_record_creation`,
   crates/altium-format/src/schlib/snapshot/library.rs). Increment F must rewrite the
   §8c rule 2 clause to: "either preexisting or inserted by a PRIOR library-level
   image-asset op whose result snapshot the component edit is taken from (increment D's
   namespace-transition ops recompute ReferenceContext; same-edit composition applies
   only to NEW components created within the library edit itself)" — alongside the §8a
   parenthetical rewrite from item 3. Additionally, D1's review fix pass strengthened
   the §8c rule 3 implementation to per-pairing-target semantics (the record's concrete
   input pairing target — first exact, then first case-insensitive match — must survive
   name-intact; a duplicate-name or case-variant sibling is no substitute), which is
   STRICTER than plain name resolvability and exactly matches rule 3's "an asset still
   referenced by any image record" reading; no memo change needed for rule 3.
