# NEXT implementation corpus smoke check — 2026-09-05

This bounded run validates the CLI after front-end extraction commit `6a2ba94`.
It does not prove corpus-wide support or snapshot plan/apply correctness.
Selected the three smallest files by size/path per extension; additionally the
three smallest binary CFB files for SchDoc/PcbDoc after ASCII-only initial selections.
All runs used `target/debug/altium` from `cargo build -p altium-cli --bin altium`.

Commands per selected file: `altium validate FILE`; on success except IntLib,
`altium save-as FILE TEMP` then `altium cfb diff --semantic FILE TEMP`.
No fixture-feature or property tests were run.

## Corpus revisions

- `schlib`: `9687d71cebbdd5bbf20771fe05a14d4a83c73c0a` (akiselev/altium-cli-test-schlib).
- `pcblib`: `c5dfdc75715a38c03fc0c129c65b951ba6ec77ab` (akiselev/altium-cli-test-pcblib).
- `intlib`: `c5e0b35fb87d1f4975cb77083e9911440ef46fad` (akiselev/altium-cli-test-intlib).
- `schdoc`: `6d0ff22117b826b45ec8ed3fe44716a456d7f954` (akiselev/altium-cli-test-schdoc).
- `pcbdoc`: `6bc5aa7cdca25875d8b29f18ccd85f8c395be076` (akiselev/altium-cli-test-pcbdoc).

## Results

Exit codes: 0 success/equal; 1 parser error or semantic differences, as labeled.

| File | Validate | Save | Semantic diff |
| --- | --- | --- | --- |
| `data/schlib/lucashudson-Connectors.SchLib` | 0 | 0 | 1 |
| `data/schlib/Diodes.SchLib` | 0 | 0 | 1 |
| `data/schlib/aiskylab-Optoelectronics.SchLib` | 0 | 0 | 1 |
| `data/pcblib/Senior-Design-Parts_Library.PcbLib` | 0 | 0 | 1 |
| `data/pcblib/TranDangKhoa-LEDs.PcbLib` | 0 | 0 | 1 |
| `data/pcblib/TranDangKhoa-Diodes.PcbLib` | 0 | 0 | 1 |
| `data/intlib/Vishay_Optocoupler.IntLib` | 1 | — | — |
| `data/intlib/Amphenol_RF_MCX.IntLib` | 1 | — | — |
| `data/intlib/Holtek_VoltageReg.IntLib` | 1 | — | — |
| `data/schdoc/talhadduman_2EDGB-5.08-02P__Altium__2EDGB-5.08-02P.schdoc` | 1 | — | — |
| `data/schdoc/PythonOrC_HAB__CosmicRayHAB__AP2138N-3.3.schdoc` | 1 | — | — |
| `data/schdoc/Bjersgen_Simplefoc__STM32H7_FOC_circle_library__NX3225GD-8MHZ-STD-CRA-3_2022-02-28.schdoc` | 1 | — | — |
| `data/pcbdoc/at32-foc-bottom.PcbDoc` | 1 | — | — |
| `data/pcbdoc/esp32-drone.PcbDoc` | 1 | — | — |
| `data/pcbdoc/mppt-controller.PcbDoc` | 1 | — | — |
| `data/schdoc/a3ng7n_Altium-Schematic-Parser__tests_altium_crap__FPGAProject_Top.SchDoc` | 0 | 0 | 1 |
| `data/schdoc/a3ng7n_Altium-Schematic-Parser__tests_altium_crap__FPGA_Project2.SchDoc` | 0 | 0 | 1 |
| `data/schdoc/a3ng7n_Altium-Schematic-Parser__tests_altium_crap__TRange.SchDoc` | 0 | 0 | 1 |
| `data/pcbdoc/fingerprint-lock-v1.PcbDoc` | 0 | 0 | 1 |
| `data/pcbdoc/heron-4layer.PcbDoc` | 0 | 0 | 1 |
| `data/pcbdoc/kitsprout-template.PcbDoc` | 0 | 0 | 1 |

12/21 files validate and save. All 12 report semantic differences; none is
certified lossless by this run. The other 9 reject unsupported content.

Observed examples requiring classification before acceptance:

- SchLib Boolean `T` → `TRUE`; component description encoding/value rendering differences.
- PcbLib native record size upgrades; `/Library/EmbeddedFonts` absent after save.
- IntLib PCB 5.0 header unsupported and unrecognized `LAYERSET*ACTIVELAYER` parameters.
- ASCII SchDoc/PcbDoc rejected by binary parser.

These are observations, not a blanket upgrade allowlist. Investigate each changed
typed value or missing stream before claiming preservation. Raw local command
output and temporary saved documents are under `/tmp/altium-next-corpus/`; they
are diagnostic scratch artifacts, not durable acceptance evidence. The table and
pinned revisions make this smoke selection reproducible.
