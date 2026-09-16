# SchLib ownership investigation — 2026-09-05

Dated, non-authoritative implementation evidence for NEXT step 2. This establishes
native address interpretation, not completion of structural mutation or complete
validation of all schematic record kinds.

## Original source and reproducibility

Inspected installed AD26 managed assembly:

- `/mnt/c/Program Files/Altium/AD26/System/Altium.Sch.DataModel.dll`
- SHA-256: `3a34fe9ea365fae99230e9c86d5c5b3f892db394145446f1e4b955f9cca879ed`
- Decompiled with `/tmp/altium-ilspy-target/debug/ilspy`; evidence files below are
  under `/tmp/altium-original-evidence/`. These temporary paths are local evidence,
  not committed source snapshots. Reproduce using `ilspy decompile ASSEMBLY --type
  FULL_TYPE --method METHOD`; class files omit `--method`.

Primary classes: `Altium.Sch.DataModel.ImportExport.SchDataExporterLibraryV5`,
`Altium.Sch.DataModel.ImportExport.SchDataImporterLibraryV5`,
`Altium.Sch.DataModel.FileFormats.FileFormatV5`, and
`Altium.Sch.DataModel.Objects.SchDataContainer` / `SchDataObject`.

## Verified address spaces

Let `D[i]` be a Rust component's Data child vector and `A[j]` its Additional vector.
The component root is stored separately from `D`.

| Encoded owner selector | Native owner index | Referent |
| --- | --- | --- |
| false | 0 | Component root in Data |
| false | n > 0 | `D[n - 1]` |
| true | n >= 0 | `A[n]` (there is no Additional root record) |

Evidence is matching exporter/importer arithmetic, not an inferred universal +1:

- `SchDataExporterLibraryV5.cs:161–236`, `WriteBaseWarehouseData`: each component
  starts a Data stream; subtract the component's global BaseWarehouse position
  from child owner indices. Component root is first in that stream.
- `SchDataExporterLibraryV5.cs:498–559`, `WriteAdditionalWarehouseData`: subtract
  the component's BaseWarehouse origin when selector is false, or its first
  AdditionalWarehouse position when true. Additional stream contains objects and
  the final `RECORD=0`, without a component root/header.
- `SchDataImporterLibraryV5.cs:119–198`, `ReadBaseWarehouse`: append root/children,
  add component Base origin, and resolve against BaseWarehouse.
- `SchDataImporterLibraryV5.cs:284–377`, `ReadAdditionalWareHouse`: append object,
  select Base/Additional warehouse and matching origin using the selector, then
  resolve the owner.
- `ownership-SchDataObject-GetOwnerFromList.cs`: direct zero-based list lookup,
  returning null for negative/out-of-range indices.

**Important loader restriction:** Data import unconditionally resolves against
BaseWarehouse. It does not select Additional even when the flag is true. Therefore
Data-to-Additional links are not supported by this verified SchLib path and must
not be invented by a new graph API. Additional-to-Data links can target the complete
Data stream; Additional-to-Additional parents must already have been read. The
exporter writes parents before descendants. Self/cyclic/forward same-stream links
must fail structural validation instead of depending on Altium's invalid-owner
repair (the original importer sometimes repairs invalid links to a stream origin).

## Selector representation and defaults

`ownership-FileFormatV5-ExportDataObject.cs:7–10` exports `OwnerIndex`,
`IsNotAccesible`, **`OwnerIndexAdditionalList`**, then `IndexInSheet`.
`ownership-FileFormatV5-ImportDataObject.cs` imports the same fields.
`SchDataSerializerParam.cs:421–435` sets absent Boolean parameters to false;
initial values of by-reference locals in ImportDataObject are not wire defaults.
The existing Rust constant `OWNER_INDEX_ADDITIONAL_LIST` names the exact key.

Binary pins encode the same selector in PinConglomerate bit 7, using existing
`PIN_CONGLOMERATE_OWNER_INDEX_ADDITIONAL_LIST`; see
`ownership-FileFormatV5-{ExportPin,ImportPin}.cs`. Binary pins do not carry
`IndexInSheet` in that record payload. The selector is therefore not pin-only:
ordinary text records require the typed Boolean too.

The bounded implementation adds this Boolean with false default to
`SchPrimitiveBase`, preserves it in typed serialization, and explicitly initializes
new API bases to false. The dedicated root `SchComponent` does not gain an invented
Additional owner contract. A Component record encountered as a child cannot be
safely accepted as an ordinary child: the exporter treats component objects as
stream boundaries.

## Ordering and implementation compatibility containers

`ownership-SchDataContainer-Add.cs` appends to the owner's object list; AddEx calls
Add. `ownership-SchDataObject-UpdateOwner.cs` resolves an `ISchDataContainer` and
calls AddEx. `ownership-SchDataContainer-AddToListForSave.cs` stores the object's
original sibling `IndexOf` as `IndexInSheet`, then recursively emits children.
`ownership-SchDataContainer-AddToAdditionalListForSave.cs` emits an Additional
subtree depth-first; descendants select Additional, whereas an Additional subtree
root emitted from Data selects Data.

Do **not** turn IndexInSheet into an owner address. Do not silently overwrite it
with physical stream ordinal. Generic
`SchDataImporterBaseV5.ProcessImportedAdditionalWareHouse` has a MoveObjectToIndex
postpass, but the SchLib-specific importer does not call it:
`SchDataImporterLibraryV5.cs:109–112` calls only ReadAdditionalWareHouse. This
corrects the earlier tentative inference that the generic postpass applies to
SchLib. A future explicit sibling-order authoring operation must define its
IndexInSheet updates separately from owner-address remapping.

The serialized implementation hierarchy uses compatibility containers:

- `ownership-SchDataComponent-AddToListForSave.cs`: temporarily moves component
  implementations into a synthetic ImplementationList, emits that list owned by
  the component, then restores runtime children.
- `ownership-SchDataImplementation-AddToListForSave.cs`: similarly emits parameters
  through a synthetic ParameterList owned by the implementation.
- `ownership-SchDataImplementationList.cs` and
  `ownership-SchDataParameterList-UpdateOwner.cs`: these wrappers' UpdateOwner is
  a no-op, not ordinary runtime attachment.
- `ownership-SchDataImplementation-UpdateOwner.cs`: when serialized owner is an
  ImplementationList, redirects to that list's owner before normal attachment.
- `ownership-SchDataImplementationMap.cs`: UpdateOwner requires an
  ISchDataImplementation and calls SetMap.
- `ownership-SchDataMapDefiner-UpdateOwner.cs`: requires an
  ISchDataImplementationMap and calls AddMapDefiner.

Consequently preserve and validate the **serialized** list/implementation/map/
map-definer chain. Do not flatten it to the runtime container graph. The generic
container implementation accepts ISchDataContainer; this investigation does not
establish an exhaustive semantic parent-type table for every schematic kind.

`ownership-SchDataConsts.cs:79–91` lists ten ordinary Additional-root object IDs:
harness connector, entry, connector type, signal harness, high-level-code symbol
and entry, open-bus pin group, blanket, object definition, reuse-block implementation
info. Descendants of those roots are also Additional, regardless of their own
object type. This is not permission to skip unknown records.

## Immutable-input structural algorithm

1. Capture every existing node as `(Data | Additional, input ordinal)` and preserve
   its fully typed payload. Root is a distinct owner, not a movable child.
2. Resolve all original native owner fields using the table above before applying
   any insert/delete/reorder. Reject unsupported Data selector=true, invalid
   indices, cycles, forward same-stream owners, and invalid known parent kinds.
3. Resolve requested selectors against that immutable input. New nodes use explicit
   batch-local identities and explicit owners. Reject duplicate/conflicting edits.
   Do not silently cascade deletion or reparent descendants whose owners disappear.
4. Construct separate final Data and Additional sequences. Preserve stream
   membership unless a separately proven operation authorizes moving a subtree.
   Parents must precede children within each stream; all Data precedes Additional.
5. Build old/new-node-to-final-address maps once. Rewrite only owner selector and
   index from the final owner's address: Root -> false/0, Data[i] -> false/i+1,
   Additional[j] -> true/j. Preserve IndexInSheet unless explicitly edited.
6. Recompute ordinal-addressed pin sidecars from final **Data pin encounter order**.
   Current load/save sidecars operate on Data pins only. Do not claim supported
   Additional pin sidecars without further evidence.
7. Validate the entire candidate before returning it; serialize/reopen tests must
   verify complete typed equality, both owner selectors, metadata, and sidecars.

Minimum tests: Additional index0 distinct from Data root0; Data insertion shifting
an Additional-to-Data parent; Additional insertion shifting descendant owners;
multiple inserts/deletes/reorders resolved against immutable inputs; rejected
owner deletion, cycles, self/forward links, Data-to-Additional links and child
Component; complete native save/reopen with both streams. Existing tests for field
parse/serialize cover missing/F/T and snapshot clone retention covers an Additional
child selecting Additional0.

## Part/display and remaining evidence limits

Previously extracted `GetState_PartCountNoPart0.cs` returns native PartCount minus
one. `SetState_PartCountNoPart0.cs` writes value plus one;
`SetState_CurrentPartID.cs` requires `1 <= currentPartID < native PartCount`.
Native part0 is the reserved/common slot, not another active numbered part. These
methods are in the same hashed assembly. Do not permit active part equal to count.

A bounded scan of 126 top-level local `data/schlib/*.SchLib` files found no component
Additional streams. Connectors and STM32F4 samples corroborated Data root owner0
and common display0, but are **not** Additional corpus validation. The address
formula rests on matching original import/export code. No Altium GUI roundtrip or
Additional native fixture oracle was performed. Exhaustive record-kind parenting,
Additional pin sidecars, and explicit sibling-order editing remain separate gates.

The preexisting validator checks mainly Data owner range/backward order and some
implementation parent kinds; it does not establish complete Additional graph,
selector, part/display, or child-Component validity. The new owned graph helper
must close its structural subset explicitly rather than presenting the old
validator as proof.

## Bounded implementation checks

- `cargo test -p altium-format text_owner_address_space_defaults_and_roundtrips --lib`:
  passed; `/tmp/altium-owner-selector-tests.log`.
- `cargo test -p altium-format complete_snapshot_retains_all_typed_state_and_stream_partition --lib`:
  passed; `/tmp/altium-owner-snapshot-tests.log`.

- `cargo test -p altium-format sch_records::tests --lib`: 59/59 passed;
  `/tmp/altium-owner-record-tests.log`.

Independent selector-field review passed: original absent-Boolean semantics,
complete typed field retention, constructor defaults and strict roundtrip test.
The separately implemented dual-stream helper also received source/code review;
its structural tests are recorded by its owning increment.

No fixture-feature or property suites were run for this increment.
