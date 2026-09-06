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
