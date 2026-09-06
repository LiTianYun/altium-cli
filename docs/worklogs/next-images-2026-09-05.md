# SchLib image resource inventory, 2026-09-05

Dated, non-authoritative investigation for the NEXT step 1 image-resource gate.
The initial inventory below uses current Rust and fixture evidence. The later
**Original managed implementation follow-up** separately records fresh AD26
decompilation and a Windows decoder probe. `AD26-dotnet/` remains unavailable;
the installed original assemblies were found later. No native parser, public
resource type, or opaque retention field was added by this investigation.

## Corpus and method

Fixture repository: `data/schlib`, commit
`9687d71cebbdd5bbf20771fe05a14d4a83c73c0a`. The parent survey inspected `/Storage`
in 126 SchLib files and found 13 nonempty streams. This investigation extracted
all 58 resources from those 13 streams using `altium cfb cat FILE /Storage`.
It independently unpacked each little-endian block header, checked the low
24-bit payload length, decoded the `0xD0` envelope's length-prefixed Windows-1252
resource name, checked compressed length, and zlib-decompressed its content.
All decompressor inputs ended exactly at the zlib endpoint.

Temporary reproducibility artifacts (not tracked and not needed by production):

- `/tmp/altium-image-storage-survey.json`: parent stream survey.
- `/tmp/survey_altium_images.py`: envelope extraction and BMP header inventory.
- `/tmp/inventory_altium_trailers.py`: trailer/chunk/record framing checks.
- `/tmp/link_altium_images.py` and `/tmp/fix_altium_linkage.py`: component record
  linkage; the second normalizes case-insensitive parameter names.
- `/tmp/altium-image-inventory/inventory.json`: per-resource names, dimensions,
  framing fields, decoded-source paths and trailer inventory.
- `/tmp/altium-image-inventory/linkage.json`: component stream/record references.

These scripts are exploratory readers, not proposed native implementations.
Binary extracts remain under `/tmp/altium-image-inventory/` only.

| File under data/schlib | Resources | BMP only | PNG trailer | JPEG trailer | EMF trailer |
| --- | ---: | ---: | ---: | ---: | ---: |
| ICs.SchLib | 2 | 0 | 2 | 0 | 0 |
| Mohamadkhosravi-MonoLine.SchLib | 1 | 0 | 1 | 0 | 0 |
| Oktay97-OT_Library.SchLib | 4 | 1 | 3 | 0 | 0 |
| SIM808.SchLib | 1 | 1 | 0 | 0 | 0 |
| SMotlaq-Schem_lib.SchLib | 6 | 0 | 5 | 1 | 0 |
| aKaReZa75-Connector.SchLib | 10 | 1 | 0 | 0 | 9 |
| aKaReZa75-IC.SchLib | 5 | 1 | 0 | 0 | 4 |
| aKaReZa75-ModSen.SchLib | 6 | 0 | 0 | 0 | 6 |
| aKaReZa75-Other.SchLib | 19 | 10 | 0 | 0 | 9 |
| dungvh03-ICs.SchLib | 1 | 1 | 0 | 0 | 0 |
| ioelectro.SchLib | 1 | 0 | 1 | 0 | 0 |
| pyptec-Schlib_jp.SchLib | 1 | 0 | 1 | 0 | 0 |
| ryankurte-electronpowered.SchLib | 1 | 1 | 0 | 0 | 0 |
| **Total** | **58** | **16** | **13** | **1** | **28** |

## Resource framing findings

Every decompressed resource begins with a BMP file (`BM`). Every BMP uses a
40-byte BITMAPINFOHEADER, positive dimensions, one plane, BI_RGB compression,
zero reserved words and no color table. There are 43 24-bit and 15 32-bit BMPs.
The BMP-declared file size equals pixel offset plus padded scanline length times
height. The image-size field is zero in 48 cases and equals the computed size in
10. Resolution metadata includes `(0,0)`, `(3780,3780)`, `(4724,4724)` and
`(11811,11811)` pixels/metre. Preserve or explicitly interpret these fields;
replacing all metadata with a decoder's defaults is not a complete parse.

The fourth byte of every 32-bit pixel is uniformly zero in nine resources and
uniformly 255 in six. This does **not** establish an alpha interpretation. A
plain RGB decoder must not silently reinterpret or discard this channel without
an explicit supported-format policy.

Only 16 resources end at the BMP-declared file size. The other 42 append a
one-byte class-name length, that many ASCII class-name bytes, and an original
image/metafile stream extending to the resource end:

- `0B TdxPNGImage` followed by a PNG signature: 13 resources.
- `0A TJPEGImage` followed by JPEG SOI/JFIF: one resource.
- `09 TMetafile` followed by an enhanced-metafile header: 28 resources.

Thus treating the payload as one BMP and accepting decoder trailing bytes would
drop meaningful source data in 42 of 58 resources. File extensions are not codec
authority: `/Storage` IDs ending `.png` and `.jpg` still start with BMP.

PNG trailers have CRC-valid framed chunks through IEND with no trailing bytes.
Observed chunks are IHDR, IDAT, IEND, pHYs, gAMA, cHRM, tIME and tEXt. Their typed
metadata cannot be replaced with an unknown-chunk byte bag. BMP preview and PNG
source dimensions are sometimes different: `ioelectro.SchLib` stores a
1024×472 BMP plus a 2500×1152 PNG; `pyptec-Schlib_jp.SchLib` stores 1024×683 plus
1536×1024. Two SMotlaq resources store 129×768 previews plus 130×775 PNGs. The
preview therefore cannot substitute for the source image.

The JPEG trailer has SOI and EOI framing; its complete marker/metadata semantics
were not decoded here. Every EMF has the expected signature and exact declared
byte/record counts, aligned record framing, and an EOF record. Observed numeric
record types: 1, 9, 10, 11, 14, 17, 21, 37, 48, 70, 75, 77 and 81. Each includes
a type-70 comment beginning `GDIC`; the first inspected comment contains a
large nested payload. Merely recognizing the outer EMF record type would not
parse that content. This inventory is **not full BMP/PNG/JPEG/EMF decoding**, and
in particular makes no claim to understand EMF comments or nested drawing data.

## Resource references and current API

The 13 libraries contain 72 image (`RECORD=30`) records in component `/Data`
streams. Parameter keys are case-insensitive (`FileName` and `FILENAME` both
occur). All 70 records with `EmbedImage=T` match a `/Storage` identifier by exact
decoded filename; some resources are shared by multiple records. Two records
omit EmbedImage and name external files absent from Storage: MonoLine's
`/Touch Spring/Data` record 8 and OT_Library's `/BATTERY_L91/Data` record 3.
One Storage resource in SMotlaq-Schem_lib has an identifier shaped as
`FileName=D:\PCB_SCH\_abedini\K7DAQ\HISPEED_IO_CONN\HDMI\HDMI.png|Orientation=90 Degrees`
and no exact reference among the surveyed component Data image records. Its
identifier syntax requires further investigation; do not split it heuristically
or treat this as permission to garbage-collect it. The scan did not prove
absence of references from other stream/record families.

Current implementation pointers:

- `crates/altium-format/src/embedded_object.rs:29`: envelope parsing and zlib.
- `crates/altium-format/src/schlib.rs:88`: private
  `SchLibEmbeddedImage { file_name, data: Vec<u8> }`.
- `crates/altium-format/src/schlib.rs:1204`: Storage parser currently transfers
  decompressed content directly into that byte vector, without typed image parse.
- `crates/altium-format/src/schlib.rs:1591`: Storage serializer writes it back.
- `crates/altium-format/src/sch_records.rs:1067`: typed record geometry,
  orientation, style, KeepAspect, EmbedImage and FileName.
- `crates/altium-format/src/api/schlib_types.rs:346`: public ImageGraphic has
  filename and flags, but no typed resource data or document resource operations.
- `crates/altium-format-spec/src/compiler.rs:4156`: legacy image_data decodes
  base64, but `executor.rs:1562` image construction does not consume those bytes.

## Smallest evidence-supported next implementation

First specify a document-owned resource API keyed by decoded resource name,
separate from graphic placement fields. Shared resources require lookup and
explicit insert/replace operations with duplicate-name/conflicting-content
errors. Embedded references must resolve; external references must remain
explicit external paths and must not trigger implicit filesystem loading.
Do not generate cross-artifact native identity or infer resource equality from
an extension or basename. Resource deletion requires independently established
reference coverage and authored scope.

A proposed semantic shape is an image resource containing a typed bitmap preview
and an optional typed original graphic. Candidate original variants are PNG,
JPEG and enhanced metafile, because all three are demonstrated by real data.
These are design candidates, not newly approved format-types. Each variant must
fully model its understood headers, pixels/drawing commands and metadata;
unknown fields, chunks, commands or trailers must hard-error. No encoded-source
byte passthrough variant, unknown-chunk map or temporary raw retention is valid.

The smallest parser slice supported by this corpus is the **16 BMP-only**
resources: implement checked 24/32-bit BI_RGB bitmap parsing and serialization,
including scanline layout, header metadata and an explicit fourth-channel policy.
Test it against extracted real examples and regenerate/reparse semantic pixels
and metadata. At this slice, every appended class/source object must produce a
contextual unsupported-resource error. That would establish bounded typed BMP
support; it would **not** complete the image gate or support all 58 resources.

Before declaring the complete gate closed, obtain original source evidence for
the appended-class contract and fully model the PNG/JPEG metadata and EMF/nested
comment content seen above. A generic raster codec alone cannot do that. Then
connect strict source image_data decoding to the same typed resource boundary
and public API, retaining the source authoring operation separately from target
resource scope. Until this exists, rejecting image_data is safer and more
accurate than accepting it into intent that cannot be materialized.


## Original managed implementation follow-up

The `ghidra` executable is installed, but `ghidra project info altium26`
reported that the project does not exist in its configured project directory
(`/home/dev/projects/parasolid-re/work/ghidra-projects`). The alternate
`/home/dev/ghidra-cli-projects` collection also has no altium26 project. No native
Ghidra program was imported or decompiled during this bounded investigation.

Original binaries were subsequently found at
`/mnt/c/Program Files/Altium/AD26/System/`. This includes native `AdvSch.dll`
and the managed assemblies needed for the immediate contracts. The `ilspy`
command was initially unavailable; the existing local ilspy-cli source was
built with its locked dependencies and an isolated `/tmp/altium-ilspy-target`
target directory. Individual original types/methods were then decompiled with
`/tmp/altium-ilspy-target/debug/ilspy`; no bulk assembly decompilation was needed.
Fresh output is under `/tmp/altium-original-evidence/`.

Original assembly SHA-256 values:

- `Altium.Sch.DataModel.dll`:
  `3a34fe9ea365fae99230e9c86d5c5b3f892db394145446f1e4b955f9cca879ed`.
- `Altium.Edp.Interfaces.dll`:
  `d78c97628715171adf6fe7974ec0bf2124d15712fd8775d82cbe9e91210dc6eb`.

### Serialization and loading contract

The original type
`Altium.Sch.DataModel.ImageCache.ImageCacheItem.GetDataForExport` establishes the
observed container framing directly. In `ImageCacheItem.cs:35`, a bitmap writes
only `image.SaveToStream`. A non-bitmap writes `image.SaveBmpToStream`, then a
single byte containing the UTF-8 byte length of `image.GraphicClass`, then those
UTF-8 bytes, then `image.SaveToStream`. The class tag is therefore UTF-8, even
though all observed names are ASCII. This is distinct from the enclosing
Storage resource identifier's Windows-1252 envelope encoding.

`EngineObjects.Utils.Images.ImageBase` declares these class/format associations:
`TBitmap`→BMP, `TJPEGImage`→JPEG, `TdxPNGImage`→PNG, `TMetafile`→EMF, and
`TGifImage`→GIF. Its dispatch also recognizes `TSVGImage`; for `TMetafile`, it
chooses enhanced versus Windows metafile based on the actual stream. The corpus
supports the narrower formats inventoried above; absent GIF/SVG fixtures are
not a reason to silently accept undecoded data.

`ImagePng` and `ImageJpg` override `IsBitmap` to false and supply the respective
class tags. `ImageBmp.SaveToStream` decodes through System.Drawing and saves in
the class's declared format. `ImageBase.SaveBmpToStream` (`ImageBase.cs:239`)
creates a 24-bit RGB bitmap, paints white, draws the image, and saves BMP.
`MaxSizeForBitmap` (`ImageBase.cs:294`) proportionally scales to at most 1024×768.
These functions substantiate the preview/source distinction and dimension
mismatches in the corpus; recreating only the preview is not full resource
semantics.

`ImageCache.GetRawImageData` (`ImageCache.cs:147`) loads the leading image,
re-encodes it to establish a boundary length, then reads the one-byte class-name
length, UTF-8 class name and source stream. It validates the class/stream before
using it. The original includes malformed-input bitmap fallbacks and ignored
errors. **Those are not acceptable parser policies for this repository**:
unknown or malformed class/source data must be a contextual error here.
The verified BMP-declared length in this corpus offers a directly checked
boundary; a proposed parser must not rely on nondeterministic codec re-encoding
to find trailing source objects.

For `TMetafile`, `GetRawImageData` appends an orientation byte to its internal
cache representation. `ImageEnhancedMetafile.SaveToStream`
(`ImageEnhancedMetafile.cs:201`) removes that byte while exporting and restores
it afterward. It is **not** an extra byte belonging to the native Storage EMF
payload. The class validates the EMF signature at offset 40 and delegates
rendering/decoding to System.Drawing.Metafile. This managed evidence does not
itself decode the GDIC/WMF nested data. That still requires typed format work;
recognizing or copying a GDI record is insufficient.

`SchDataExporterBaseV5.AddImageEmbeddedObjectToList` obtains the data through
`ImageCache.GetImageDataForExport`, associates it with the supplied resource
name, and adds that embedded object to the export list. This links the verified
class serializer to SchLib resource export rather than only an unrelated image
utility.

### Fourth-byte policy evidence

`ImageBmp` constructs `Picture` with `Image.FromStream`. Its `IsAlphaPicture`
(`ImageBmp.cs:186`) first checks decoded image flags for alpha; if that bit is
absent it returns false without treating the stored fourth byte as transparency.
It subsequently inspects decoded ARGB alpha only for images marked as alpha.
This alone does not prove every older BMP fourth-byte encoding.

A read-only Windows System.Drawing probe was run on all 15 extracted 32-bit
corpus BMP resources using Windows PowerShell, through the same public
`Image.FromStream` API. Every decoded image reported `Format32bppRgb`, no alpha
flag, and first-pixel alpha 255, including all nine resources whose stored
fourth bytes are uniformly zero. Evidence:
`/tmp/altium-bmp-gdi-probe.ps1` and
`/tmp/altium-original-evidence/bmp-gdi-probe.json`.
This used Windows PowerShell's .NET Framework/System.Drawing and GDI+, not a
running AD26 application or its exact .NET 8 runtime. It supports a typed RGB
interpretation with explicitly handled unused fourth-byte encoding for the
observed BI_RGB variant; blindly importing the fourth byte as alpha contradicts
both the observed decoder result and the original alpha-flag branch. Validation
and canonical serialization rules for unused bytes still need to be chosen
explicitly. No arbitrary alpha guess or opaque per-pixel payload is warranted.

### Original physical pen-width table

A fresh decompile of `Altium.Edp.Interfaces.dll`, type `Rt_Schematic.Consts`,
contains `LineWidthArrayC` (`Consts.cs:7273–7291` in this output):

| Original TSize | Internal coordinate width | Mil width using existing Coord scale |
| --- | ---: | ---: |
| eZeroSize | 0 | 0 |
| eSmall | 100000 | 10 |
| eMedium | 300000 | 30 |
| eLarge | 500000 | 50 |

This independently confirms the values cited by the current schematic renderer.
`BusLineWidthArrayC` is a separate table and must not substitute for graphical
pen widths. The table establishes exact supported physical widths; it does not
justify nearest-width rounding or saturation for arbitrary authored dimensions.
A source conversion should accept only exact matches and fail otherwise.

The fresh original managed evidence closes the previously missing class-framing
and pen-table evidence, but **does not complete typed image parsing, resource
API work, codec metadata handling, or independent native roundtrip validation**.

## Original embedded-envelope and Storage boundary follow-up

Additional fresh decompilation of the same hashed `Altium.Sch.DataModel.dll`
clarifies two different length fields. Temporary evidence files are
`SchDataEmbeddedObject.cs`, `SchDataSerializer.cs`,
`SchDataSerializerParam.cs`, `SchDataExporterBaseV5.cs` and
`SchDataImporterBaseV5.cs` under `/tmp/altium-original-evidence/`.

`SchDataEmbeddedObject.ExportToFile` writes DynamicString(Name) and Binary(Data).
`SchDataSerializer.Export_DynamicString` delegates to WriteString: the encoded
name byte count is checked with `Convert.ToByte`, then that many bytes are
written. `WriteBinary` (`SchDataSerializer.cs:863`) zlib-compresses Data, obtains
`Convert.ToInt32(compressed.Length)`, and writes that integer followed by the
compressed content. `ReadBinary` (`SchDataSerializer.cs:1002`) reads the full
signed integer and that many bytes before decompression. Parametric binary-mode
overrides delegate to these methods. **The inner compressed-size word has no
24-bit mask or high-byte flag semantics in this original implementation.**
Negative lengths are invalid; representable inner lengths use the nonnegative
signed-32-bit domain. The enclosing record size places a separate constraint.
A native hardening fix must not turn inner lengths into flags or truncate them.

The outer parametric record header is separate. `FlushCurrent`
(`SchDataSerializerParam.cs:215`) writes text length with mode zero, or binary
payload length OR `0x01000000`. `Export_Instruction` selects binary mode for
`BINARY`. `GetNextLine` (`SchDataSerializerParam.cs:244`) normally splits mode
from the high byte and masks the low 24 bits, but has a D0-envelope exception:
it peeks the one-byte name length and full inner compressed length, adds
`name_length + 6`, and preserves the complete outer integer when that equals
this derived payload length. Thus some binary envelopes at or above 16 MiB have
an explicitly recognized representation. The ordinary 24-bit record limit is
**not** an original maximum for the inner size word. Supporting that larger
outer-record case would be a separately tested block-reader change; an envelope
hardening patch must not silently invent its representation.

`SchDataExporterBaseV5.WriteExtendedWarehouseData` (`:215`) starts `Storage`,
writes RECORD instruction zero, exact `HEADER=Icon storage`, `Weight=count`,
and for each resource writes BINARY instruction 208 followed by its envelope.
This proves the text-header and binary-entry modes as well as the header value.
The original importer does not itself check every sentinel; reproducing its
permissiveness is not required by this repository's fail-fast policy.

Original reference processing also resolves the earlier bounded linkage caveat.
`ProcessImportedExtendedWarehouse` (`SchDataImporterBaseV5.cs:112`) constructs
case-insensitive keys: plain filename for orientation zero, or
`FileName={filename}|Orientation={RotationBy90ToString(orientation)}` otherwise.
It has a plain-filename fallback and keeps the first stored value per resolved
orientation. The exporter (`SchDataExporterBaseV5.cs:113`) emits a plain-name
resource plus rotated-name resources when applicable. Consequently the SMotlaq
identifier with `Orientation=90 Degrees` is an established rotated resource
alias, not evidence of a malformed/orphan resource. The initial exact-filename
survey intentionally did not resolve this alias family. Future typed resource
identity needs filename **and** the explicitly parsed orientation relationship;
it must not split arbitrary names without the verified grammar.

The original loops also tolerate duplicate Storage names rather than declaring
them invalid. This evidence does not justify introducing unconditional duplicate
name rejection in a framing-only hardening change. A future typed resource API
must explicitly resolve ambiguity/collisions without silently discarding a
second resource; that policy remains distinct from the existing ordered native
Storage list and from source creation constraints.

## Implemented boundary hardening and real-file check

The envelope now validates the full signed inner length, requires zlib StreamEnd
and exact compressed input consumption, and rejects unmappable or overlong
Windows-1252 identifiers during save. Fixed-codec identifier decoding disables
BOM sniffing so valid Windows-1252 byte sequences are not reinterpreted as UTF.
The current shared block writer's outer-size limit is checked before output;
the native oversized-block exception described above remains unsupported.

Storage checks its exact header, optional RECORD=0 sentinel, declared count and
binary entry formats before decompression. Errors include stream and block
context, then resource identifier and payload offset once available. Duplicate
entries are preserved in order. These changes do not decode the image payload.

All 13 original image-bearing files in the table above validated both before and
after the changes. For a real-file negative case, a copy of `ICs.SchLib` had only
the first resource block's header changed from binary to text in `/Storage`;
the envelope/pixels and CFB stream length were unchanged. The previous CLI
accepted it. The rebuilt CLI exits 1 with:

```text
Error: parsing /Storage stream: Invalid parameter value for key 'Storage': /Storage block #1 resource entry must be binary
```

Temporary evidence: `/tmp/altium-image-validation-before.json`,
`/tmp/altium-image-validation-after.json`,
`/tmp/altium-image-wrong-block-format.SchLib`, and the corresponding
`/tmp/altium-image-wrong-block-before.json` / `after.json` reports.

Targeted envelope tests pass 18/18; Storage tests pass 4/4. Both implementations
passed independent review. After repairing seven unrelated missing-file
dependencies in CFB wrapper test setup, the full default core-library suite
passes 385 tests, the legacy spec suite passes 168, and CLI compilation succeeds.
Fixture-feature and property-test suites were not enabled. This confirms the
bounded framing behavior, not complete native image semantics or roundtrips.

### Oversized D0 representability equations

Let `N` be the encoded identifier's one-byte length, `C` the full nonnegative
signed-32-bit compressed length, and `E = C + N + 6` the envelope payload length
(the six bytes are tag, identifier length and compressed-length word). Original
`SchDataSerializerParam.FlushCurrent:231` writes outer header
`H = E | 0x01000000`. `GetNextLine:253–273` derives mode as `H >> 24`, peeks the
D0 metadata only for mode 1, and retains the full `H` exactly when `H == E`;
otherwise the original masks to `H & 0x00FFFFFF`.

The intersection of this original writer and reader is:

| Envelope length E | Header H | Original successful size branch |
| --- | --- | --- |
| below `0x01000000` (16 MiB) | `E + 0x01000000` | Mask to low 24 bits |
| `0x01000000` through `0x01FFFFFF` | `E` | D0 metadata equality retains full header |
| `0x02000000` (32 MiB) | `0x03000000` | Mode 3, no binary/D0 branch; not supported |

Thus the largest supported **complete envelope** is `0x01FFFFFF` bytes. Its
compressed-content limit is that value minus `N + 6`. The original writer does
not guard the 32 MiB transition; copying that omission would create output its
own corresponding binary reader cannot interpret. This is separate from the
inner signed-32-bit length domain and the generic 24-bit outer block writer.
Only the D0-aware writer should use the extended range.

The generic block reader must treat the D0 prefix as an **extension candidate**,
not proof that the standard block owns all bytes inspected after its header.
Under binary mode 1, select the full H length only when a safely bounded peek
finds the D0 tag, complete identifier/length prefix, nonnegative signed C,
checked E, and exact `E == H`. Once selected, require H payload bytes to remain;
a truncated proven oversized frame is an error. Otherwise retain ordinary
low-24-bit framing. A candidate's incomplete prefix, negative C or mismatched E
must not itself invalidate an otherwise valid standard block sequence.

A concrete counterexample is an empty binary block (`H=0x01000000`) followed by
a 208-byte text block. The next block's header begins with byte D0, although it
is not part of the empty binary payload. Requiring D0 envelope fields merely
because the next byte is D0 rejects this valid sequence. Another case is ordinary
binary data beginning D0 that is not an embedded-resource envelope. Source-
correct bounded candidate recognition preserves these standard sequences.

This is layered framing, **not** the image decoder's malformed-source fallback:
no bytes are discarded or replaced. A caller expecting an embedded object still
passes the complete selected binary payload to `parse_embedded_object`, which
strictly validates tag, identifier/length fields, full signed compressed length,
exact envelope exhaustion, and complete single zlib-stream consumption. Invalid
resource semantics remain contextual errors at that boundary. Unknown outer
flag bytes remain hard errors. The same size decision governs eager and lazy
readers; an actual framing error terminates lazy iteration at the unproven
boundary. Regression coverage must include empty-binary-plus-208-byte-text,
valid oversized D0 with a following block, and malformed resource semantics
checked by the embedded-object layer.

### Implemented extension validation

The extension reader and embedded-specific writer are now implemented. This
supersedes the earlier bounded writer-cap limitation. Writer length arithmetic
rejects envelopes above `0x01FFFFFF` rather than masking them. Generic text and
binary writers remain unchanged.

The empty-binary-plus-208-byte-text case failed against the initial overly strict
reader, then passed after correction. Final targeted tests pass 17/17 for framing
and 20/20 for envelopes. The latter includes a deterministic incompressible source
whose compressed envelope exceeds 16 MiB, followed by another entry; it requires
exact decoded contents and correct following-entry boundaries. It is a synthetic
framing test, not a real native image or an image-codec oracle.

Both implementations passed independent review. Final default core-library tests
pass 392/392, legacy spec tests pass 168/168, and the CLI builds. All 13 original
image-bearing SchLibs still validate; the deliberately text-tagged resource in
the copied ICs library remains rejected. Validation results are recorded in
`/tmp/altium-large-image-validation.json`. Typed image resources remain open.
