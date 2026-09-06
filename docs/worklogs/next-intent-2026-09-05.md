# SchLib authored-intent corpus check — 2026-09-05

Corpus: `akiselev/altium-cli-test-schlib` at `9687d71cebbdd5bbf20771fe05a14d4a83c73c0a`.
Selected the smallest, quarter, median, three-quarter, and largest entries from
126 `.SchLib` files sorted by size and path. This spans 122KB–43.5MB binary inputs.

Each original was dumped with the existing CLI to a temporary `.schlib-spec`,
then processed with the diagnostic example:

```sh
altium dump INPUT.SchLib --output /tmp/INPUT.schlib-spec
cargo run -p altium-spec-lang --example check_schlib_intent -- /tmp/INPUT.schlib-spec
```

The example captures source/imports once, compiles the captured graph twice,
requires equal typed intent, and prints counts. All five inputs succeeded.

| Binary source | Bytes | Components | Pins | Graphics |
| --- | ---: | ---: | ---: | ---: |
| `data/schlib/lucashudson-Connectors.SchLib` | 121856 | 56 | 330 | 125 |
| `data/schlib/Synthiam.SchLib` | 566784 | 172 | 2222 | 743 |
| `data/schlib/arthurbenemann-STM32F4.SchLib` | 2500096 | 135 | 19003 | 351 |
| `data/schlib/kmilo17pet-Microchip_Linear.SchLib` | 7440384 | 855 | 7863 | 14072 |
| `data/schlib/dungvh03-Resistors_SMD.SchLib` | 43483648 | 6042 | 12110 | 12173 |

Totals: 7260 components, 41528 pins, 27464 graphics.

This establishes deterministic captured front-end compilation for these dumps.
It does not establish complete binary-to-intent projection, preservation of every
native field, materialize/project equality, saved-plan execution, or lossless
roundtrips. The [earlier binary smoke check](next-corpus-2026-09-05.md) found
unresolved semantic CFB differences; those remain open.
