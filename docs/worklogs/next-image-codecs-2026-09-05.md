# Original graphic resource design review (2026-09-05)

Read-only repo investigation; scripts/downloaded package source/extracts remain in /tmp.
No native parser/domain type was changed. This is a concrete corpus-supported schema proposal, not complete production support or original Altium source verification.

## Verified corpus details

- All28 EMF comments: GDIC identifier0x43494447, public type0x80000001, version0x0300,reserved0,flags0. Embedded WMF is standard18-byte header, not EMF+.
- Each WMF has exactly6 records: SetMapMode(0x0103), SetWindowExt(0x020c), SetWindowOrg(0x020b), SetStretchBltMode(0x0107), DIBSTRETCHBLT(0x0b41), EOF. Header word count and every record size consume exactly the nested stream.
- Outer EMF contains Header, CommentWindowsMetafile, SetMapMode, SetViewportExtEx, SetWindowExtEx(twice), SetWindowOrgEx, SetStretchBltMode, StretchDibits(27) or StretchBlt(1), SelectObject(twice), SelectPalette, ExtSelectClipRgn, EOF. All28 whole-file additive little-endian DWORD checksums equal0.
- Nested and outer DIBs:27 uncompressed24-bit; Other-4 is1-bit indexed with2 RGBQUAD entries. Palette-aware framing consumes exact DIB size. Outer offsets are contiguous and consume exact record length.
- PNG13: depth8, RGB(3) or RGBA(10), noninterlaced, compression/filter methods0. Exact CRC validation all chunks, zlib EOF/no unused compressed bytes, exact scanline length all pass. All6945 scanlines use filter0. Typed observed metadata gAMA,cHRM,pHYs,tIME,tEXt. No ICC/EXIF/palette/APNG in these13.
- JPEG1: JFIF1.01, density72x72dpi, no thumbnail;1370x1379, baseline8-bit,3components, Y sampling2x2 and Cb/Cr1x1,2quant tables,4Huffman tables,1interleaved baseline scan, no restart markers or APP1/APP2/COM. Pure exploratory entropy decoder consumed exact entropy stream and decoded44,892 signed quantized8x8 coefficient blocks (29,928Y;7,482 each chroma),287,881nonzero coefficients. Coeff SHA256 ff18f10a60dfa388e2ddaa82b4b0141a3d0a40365bd54ddbf3be86907d1ae5bc. Not independently oracle-validated or encoded back yet.

## Proposed semantic schema

Document-owned ImageResource{name:ResourceName,preview:Bitmap,original:Option<OriginalGraphic>}; OriginalGraphic enum Png(PngImage),Jpeg(BaselineJpeg),EnhancedMetafile(EnhancedMetafile). No arbitrary encoded content member. Placement's embedded reference resolves ResourceName exactly; explicit external paths never implicitly read. Preserve resources independently of linkage until complete reference coverage is proven.

Bitmap holds typed InfoHeader metadata (dimensions,orientation,declared image-size policy,resolution,color usage/importance), decoded rows and typed encoding distinction Rgb24/Bgrx32/Indexed1. Bgrx32 fourth byte remains an explicit unused pixel lane under the Microsoft BI_RGB contract; don't silently map it to alpha or discard it. Indexed1 stores boolean pixel indices and typed two-entry RGBQUAD palette. Parsed declared sizes validated/derived on write, reserved fields validated, padding policy explicitly defined. This shared DIB model serves preview,WMF,EMF; source-specific coordinate types are pixel/logical-unit types, not Altium Coord.

PngImage holds pixel layout RGB8/RGBA8,dimensions,decoded samples and typed metadata: optional integer-scaled gamma; eight integer-scaled chromaticity coordinates; physical pixel density plus unit; UTC calendar timestamp; ordered Latin1 text keyword/value entries. Preserve actual gAMA/cHRM fields separately from derived rendering defaults. Strict chunk parser enforces allowed chunks, multiplicity/order,CRC,compressed EOF,dimensions and exact IEND endpoint. Lossless recompression can choose filters/IDAT boundaries; these are serialization choices, not opaque source retention. Reject every unsupported ancillary/critical chunk until typed support exists.

BaselineJpeg holds JFIF header/density/typed optional thumbnail,frame dimensions/precision,component IDs/sampling/quant-table reference,quantization matrices,Huffman canonical code definitions (length counts and DC-category/AC-run-size symbol enums),scan component/table selectors plus spectral/successive parameters,optional typed restart interval,component block dimensions and signed quantized8x8 coefficient arrays. The observed stream needs no opaque application metadata. Parse all entropy and re-encode coefficients without IDCT/requantization to avoid repeated JPEG loss. Reject unsupported APP/COM/progressive/arithmetic/extensions until fully typed. A decoded RGB image alone is insufficient to regenerate the original JPEG without loss.

EnhancedMetafile holds typed header(bounds/frame/device pixel/mm/micrometer dimensions,version,handle limits,optional UTF16 description),ordered typed drawing/state records above and typed EOF/palette. CommentWindowsMetafile holds version and a nested WindowsMetafile(header,ordered typed six-record sequence). Each raster operation retains source/destination signed geometry,typed raster operation/stretch mode/color use,optional transform/background and fully typed DIB. Stock-object references are an enum distinct from allocated object handles. Clip-region is typed absent/reset or region rectangles; corpus uses absent reset. Checksum/counts/offsets are checked derived serializer values. Preserve BOTH nested WMF and outer EMF commands; matching rendering is not permission to delete one. Reject unknown comments/records, unaccounted offset ranges and unsupported bitmap modes. Full EMF+ is not needed for these28 but must hard-error if encountered.

## Reuse audit

Existing Cargo.lock has png0.17.16 and zune-jpeg0.4.21. png Info exposes pixel dimensions,gama_chunk,chrm_chunk,Latin1 text; decoder's parse_chunk has no tIME dispatch and falls through for unknown chunks. Use its pixel decode/encode with a strict typed framing/metadata layer, not Info as a complete image resource. Source local ~/.cargo/registry/.../png-0.17.16/src/{common.rs,decoder/stream.rs}.

zune-jpeg public decode/decode_into APIs yield samples and metadata APIs offer ICC/EXIF bytes; coefficients internal, not public. Useful raster rendering oracle, not sufficient original JPEG resource persistence. libjpeg-turbo/mozjpeg coefficient APIs are candidates (jpeg_read_coefficients/jpeg_write_coefficients) but need audited Rust wrapper, typed marker parser and strict boundary consumption. Custom narrowly bounded baseline Huffman coefficient codec is demonstrated feasible by /tmp probe, but needs independent differential and roundtrip tests before adoption.

Downloaded emf-core0.1.1 and wmf-core0.1.2 source to /tmp/altium-codec-review. EMR_COMMENT stores private_data:Vec<u8> and consume_remaining_bytes; WMF DeviceIndependentBitmap stores BitmapBuffer.a_data and comments describe silently skipped undefined space. They provide useful enum/structure references; not a compliant direct resource IR/parser boundary. Their converters deliberately skip unsupported drawing commands. Do not adopt converters as semantic validation. No existing workspace EMF/WMF module found.

## Remaining blockers and implementation order

1. Native RE follow-up: intent_contract found original AD26 install and freshly decompiled ImageBase.cs in /tmp/altium-original-evidence. It reports class mappings including PNG/JPEG/EMF and a white24-bit preview bounded1024x768. ImageCacheItem.GetDataForExport now also verifies BMP-plus-class-plus-original framing; the Microsoft BI_RGB contract identifies the fourth BMP lane as unused rather than alpha; public Windows/PNG/JPEG specs resolve contained codec structure, not this wrapper contract.
2. Agree new domain types via parent/user process, then implement shared strict BI_RGB24/32 and indexed1 bitmap/DIB types. BMP-only16 first is bounded support, not gate completion.
3. Typed PNG metadata wrapper plus reusable lossless pixel codec covers13 originals.
4. Typed WMF+EMF observed-record subset, checksum/offset/state validation covers28 originals without generic vector rendering.
5. Baseline JPEG coefficient codec + lossless coefficient re-encode covers1; require independent decoder pixel oracle and exact coefficient/metadata roundtrip.
6. Resource API strict insert/replace/reference validation, authored image_data parse,semantic serialization checks. Do not enable gate until all required formats and actual corpus reopen checks pass. Unknown formats/comments never become opaque fallback variants.

## Primary references

https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-emf/0dbabbd2-c61d-4534-b526-b9431c494da8 (nested WMF comment fields)
https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-gdicomment (additive checksum across whole EMF)
https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-wmf/e666a66f-b29d-4adb-82da-e00eaf032ea6 (DIBSTRETCHBLT typed fields)
https://www.w3.org/TR/png-3/ (PNG chunk/pixel/metadata specification)
https://www.w3.org/Graphics/JPEG/itu-t81.pdf (T.81 primary specification hosted by W3C)
https://www.w3.org/Graphics/JPEG/jfif.pdf (JFIF primary specification hosted by W3C)
https://docs.rs/emf-core/0.1.1/emf_core/parser/struct.EMR_COMMENT.html (candidate API, unsuitable opaque comment)
https://docs.rs/wmf-core/0.1.2/wmf_core/parser/struct.DeviceIndependentBitmap.html (candidate API)
