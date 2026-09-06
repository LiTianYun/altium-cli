# Imported image assets

Revised direction following the user's 2026-09-05 instruction to defer image
codecs and treat image contents as opaque imported assets. Asset-reference intent,
ordered native resource APIs, and immutable input capture are implemented; the
integrated dump/planner/apply workflow remains outstanding.
This supersedes the shared codec/type proposal previously at this path.

## Boundary

Use the same explicit artifact workflow as routing: authored source references a
file, planning captures it, and dump exports it alongside the spec. Unlike copper,
image contents need no semantic geometry model. Do not add an image codec crate,
pixel model, JPEG coefficient model, or metafile command model for this workflow.

The user-authorized exception to the no-opaque-data rule is limited to image asset
payloads. Parse and validate Altium's outer Storage headers, counts, resource
identifiers, block framing, compression, and image-record references as before.
The entire decompressed image resource body is an indivisible encoded asset;
this includes its bitmap preview and any appended original/class framing. Do not
claim its internal headers or codec data have been semantically validated.
Unknown document streams, records, fields, and bytes outside this boundary still
hard-error. This exception does not permit opaque routing or other native data.

## Initial import/export contract

- Export one `.altium-image` asset per resource: exactly the decompressed native
  image resource body, without the outer compression envelope or resource name.
  This is an Altium image bundle, not a new general-purpose image file format.
- Keep the resource name and the explicit asset path in the spec. Keep placement,
  display flags, embedded/external behavior, and native reference/alias handling
  typed and separate. Never derive resource identity from a basename or hash.
- The new intent compiler accepts `image { source: "assets/logo.altium-image" }`
  alongside placement and `file_name` properties. Shared-resource elaboration
  remains to be implemented. Legacy CLI execution rejects this new property;
  it does not silently overload `image_data`.
- Initially import only these native bundles, obtained by dump from a document
  Altium has already embedded. Preserve the complete body byte-for-byte; never
  replace an original with its preview or normalize image encoding.
- Defer standalone PNG/JPEG/SVG import and preview generation. Reject those inputs
  explicitly until a conversion path exists. Rendering and codec validation are
  separate capabilities, not prerequisites for bundle import/export.
- Treat an asset change as whole-resource replacement. Preserve resource order
  and existing duplicate entries; block ambiguous edits. Resolve references and
  enforce collection scope before deletion. No implicit garbage collection or
  external-image file loading.

## Planning and transactions

Resolve explicit paths relative to the declaring source, consistent with other
artifact imports. The containing image declaration determines the path base;
imported string/let/spread values do not carry an alternate path base. Captured
compilation retains that declaration's canonical file path; AST-only compilation
requires an explicit origin before asset capture. Capture exact bytes and content hashes during planning; bind
them into the input manifest and include the payload in the self-contained saved
plan. Apply checks preconditions and executes captured bytes, never refreshed
asset contents. Byte hashes report changes without pretending to compare pixels.
Missing files, framing errors, unresolved references, and conflicting resource
replacements are contextual errors. Opaque contents can still be an invalid image;
this workflow promises preservation, not codec validity or visual equivalence.

Dump stages the spec and all named assets in one transaction, with deterministic
collision-safe paths and no silent overwrite. Native serialization rebuilds the
validated outer envelope around the unchanged asset body. Its compressed bytes
may differ; the decompressed body must not.

## Revised gates

Step 1 needs typed authored asset-reference intent and source-preservation tests,
not image decoding. Step 2 adds ordered resource snapshots and whole-asset patches.
Steps 3 and 4 capture assets in saved plans and enforce transactional execution;
the SchLib workflow gate includes paired spec/asset dump and import.

Test exact bundle equality across native save/reopen and export/import for BMP-only
and preview-plus-original corpus examples. Test shared references, ambiguous names,
missing assets, conflicting replacements, changed inputs, and scoped deletion at
their respective implementation steps. Existing outer-envelope validation remains
mandatory. Completing this design update does not complete a numbered plan step.

The [inventory](../worklogs/next-images-2026-09-05.md) and
[codec investigation](../worklogs/next-image-codecs-2026-09-05.md) remain historical
evidence. Their full-codec implementation direction is superseded here.
