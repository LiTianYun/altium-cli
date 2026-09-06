# NEXT image asset boundary and step-1 closure

The user replaced the image codec prerequisite with explicit opaque asset imports.
This is a narrow exception for the complete decompressed image resource body,
including preview and appended original. Outer native framing remains strict.
AGENTS.md and CLAUDE.md now record the exception so it survives agent handoffs.

## Implemented

- Image-only `source` intent preserves all five field operations and canonical
  containing-declaration provenance. Imported let/object values do not change
  the path base. AST-only compilation requires an explicit origin before capture.
  No assets are read during language compilation. Clearing/resetting a source
  does not authorize deleting a shared resource.
- Borrowed ordered native image views and explicit indexed insert/replace/remove
  preserve complete bodies, duplicate names, and order. Serialization constraints
  are checked before mutation. Aggregate reference validation is still outstanding.
- Sync capture records exact bytes, BLAKE3 hash, requested path and canonical path.
  Verification detects changed content/deletion and symlink retargeting without
  refreshing the captured bytes. Saved-plan serialization/execution is later work.
- Step-1 review closed inconsistent standalone namespace validation, empty evaluated
  pin/pad identifiers, duplicate annotation metadata overwrites, and empty named
  swap-group declarations. Empty native swap-field values remain legal.
- The real image corpus exposed a self-alias save defect in dungvh03-ICs.SchLib.
  Its FileHeader has LIBREF688=AT25XV041B and COMP688ALIAS0=AT25XV041B;
  /AT25XV041B contains Data and Redirection, whose SECTIONNAME is AT25XV041B.
  Save now reuses the component storage only for an exact self-alias, while still
  serializing Redirection. Other alias storage collisions still error.

## Validation and limits

The explicit `altium-sync` example `check_image_assets` exports native bodies into
temporary `.altium-image` files, captures them, replaces indexed resources, then
saves/reopens the native document and compares ordered names and exact bodies.
It does not implement the user-facing paired dump/import transaction, saved plans,
source elaboration, codec validity, or whole-document semantic convergence.

The first corpus run passed eleven original libraries and then correctly rejected
an intentionally malformed /Storage resource block accidentally included in the
prior validation input list. The next run passed pyptec-Schlib_jp and exposed the
self-alias failure in dungvh03-ICs; that failure was investigated rather than omitted.
After the self-alias fix, all 13 originals pass: 58 resources and 56,777,028 bytes
of image bodies. Combined results: `/tmp/altium-image-assets-final-corpus.json`.
The three incremental corpus logs are `/tmp/altium-image-assets-corpus.log`,
`/tmp/altium-image-assets-corpus-remaining.log`, and
`/tmp/altium-image-assets-corpus-self-alias.log`.

Final default suites pass: 362 language tests
(`/tmp/altium-final-language-tests.log`), 396 core, 168 legacy spec and 3 sync
tests (`/tmp/altium-image-final-default-tests.log`). No fixture-feature or proptest
suites were enabled. The CLI builds at the final code state
(`/tmp/altium-image-final-cli-build.log`). Real corpus validation used the explicit
example command.

Independent reviews covered language image operations/provenance, native API
mutation atomicity and framing, immutable sync capture, and the self-alias fix.
The numbered step-1 review also checked dependency separation, captured imports,
source preservation, typed fields, strict collection scope and retained layout
semantics. Review findings were fixed and independently re-reviewed; final
numbered step-1 verdict PASS. Native materialization and source-scope enforcement
remain step 2.

## Next implementation boundary

Existing api::Component is insufficient for complete replacement snapshots:
process_records drops Symbol and converts Hyperlink to Label; update_component
rebuilds children while preserving only selected component fields. Step 2 must
use format-owned encapsulated complete typed snapshots, preserving ordered Data
and Additional records, header/global metadata, implementation subtrees, aliases,
and image resources. Sync owns managed projections, original-snapshot addresses,
and scope checks. Format-owned candidate edits must preserve untouched typed data,
rebuild ownership, and validate before replacing the destination aggregate.
No native CFB bytes or generic unknown-value maps may substitute for typed state.
