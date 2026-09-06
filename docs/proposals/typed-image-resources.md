# Typed image resources

Proposed shared boundary for the NEXT image-resource gate. Not implemented.

The real SchLib corpus stores a bitmap preview and, in 42 of 58 resources, an
additional PNG, JPEG, or enhanced-metafile source. The preview cannot replace
that source. See the [inventory](../worklogs/next-images-2026-09-05.md).

## Ownership and dependency boundary

- `altium-format-types::image` owns shared semantic resource and codec types.
  It remains free of parsing and I/O.
- A document-independent `altium-image-codec` crate owns strict image decoding
  and serialization. Both the language and document crates can use it without
  introducing a language-to-document dependency.
- `altium-format` privately owns Storage framing and Altium's appended-class
  dispatch. It exposes typed document resource operations.
- `altium-spec-lang` retains authored field operations and compiles image input
  through the shared codec. Encoded native payloads are not semantic IR fields.
- `altium-sync` will own source scope, immutable resource snapshots, patches,
  and planning of resource changes alongside graphic changes.

## Proposed shared types

The initial shared vocabulary is:

- `ImageResourceName`: exact decoded resource name, with no basename matching.
- `PixelDimensions`, `RasterResolution`, and `BitmapRowOrder`.
- `BitmapImageSizeDeclaration`: explicit zero versus computed size declaration.
- Typed bitmap pixels: indexed pixels and palette, 24-bit BGR colors, or 32-bit
  BGRX pixels with a separate unused-byte field. Existing `Color` represents RGB;
  the unused byte must not become alpha. Pixel counts and palette indices are
  checked. The nested metafile corpus requires indexed 1-bit bitmap support too.
- `BitmapPreview`: dimensions, row order, resolution, size declaration, and
  fully decoded typed pixels/metadata.
- `OriginalGraphic`: fully typed PNG, JPEG, or enhanced-metafile variants.
- `ImageResourceData`: bitmap preview plus optional original graphic.
- `ImageResource`: resource name and typed data.
- `ImageReference`: embedded resource name or explicit external image path.

Codec-specific headers, chunks, commands, and metadata types will follow the
verified source/specification contracts. There is no encoded-source fallback,
unknown-chunk map, raw command payload, or temporarily retained trailer. Unsupported
content produces a contextual error until its typed support is implemented.
The original Altium dispatch also recognizes GIF and SVG; these are not covered
by the current corpus-derived codec proposal and must error until supported.
The detailed initial schemas and reuse constraints are recorded in the
[codec investigation](../worklogs/next-image-codecs-2026-09-05.md).

## Document and source behavior

The public SchLib API will support resource enumeration, exact-name lookup, and
explicit insert/replace. Duplicate names and conflicting content error. No implicit
upsert, basename matching, external file loading, or resource garbage collection.
Embedded references must resolve; external references stay explicit. Deletion
requires authored scope and verified reference coverage.

Image placement refers to the resource separately from its geometry and display
flags. Source image input must decode strictly into the shared model; its authored
Set/Clear/Absent operation remains separate from the resolved target value.
Original AD26 `ImageBase.SaveBmpToStream` now confirms a white-backed 24-bit
preview bounded to 1024 by 768 pixels. Source authoring must either supply an
explicit preview or use a specified deterministic rendering policy; decoding the
original and discarding it is not an option. Font and graphic-geometry validation
are independent elaboration responsibilities.

## Acceptance

Decode all observed resource structures completely, serialize from typed values,
reparse, and compare pixels, commands, metadata, resource names, and references.
Unknown content must fail. Exercise shared resources and conflicting operations,
and use real corpus examples for every observed source variant. A BMP-only slice
does not close this gate.
