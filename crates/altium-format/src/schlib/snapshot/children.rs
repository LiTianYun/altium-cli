//! Batch structural editing of the `parameters()`, `graphics()`, and `pins()`
//! child collections (step-2 increments B and C; contract authority
//! `docs/worklogs/next-child-contracts-2026-09-16.md`, cited as "memo §N").
//!
//! # Batch model
//!
//! [`SchLibComponentSnapshot::with_children`] takes, per edited collection, the
//! FINAL ordered sequence of items. Every `Keep`/`Replace` index is a position
//! in the corresponding A2 projection of the IMMUTABLE input snapshot
//! (`parameters()[i]` / `graphics()[i]` / `pins()[i]`); multiple edits never
//! shift each other's targets. Referencing the same original slot twice is a
//! conflict. Omission of an original member from the final sequence is a
//! deletion and requires the explicit per-call `allow_deletion` authorization.
//!
//! # Stream-order rule
//!
//! Within each native stream (Data, and the Additional stream), records that
//! are not members of an edited collection keep their original relative order.
//! The edited collection's final sequence occupies its members' original
//! stream positions in encounter order (Data slots first, then Additional
//! slots); when the final sequence is longer than the original membership, the
//! extras are placed contiguously right after the last original member (in
//! that member's stream), or at the end of the Data stream for a previously
//! empty collection (parameters extras before graphics extras before pins
//! extras when several collections append to an empty tail). The resulting A2
//! projection of the edited collection equals the requested final sequence
//! exactly.
//!
//! Originals never migrate between streams (verified loader restriction), and
//! creating NEW records in the Additional stream is unsupported: a final
//! arrangement that would require either is a hard error.
//!
//! # Pins (increment C)
//!
//! `pins()` members follow the same final-sequence, stream-order,
//! deletion-authorization, and atomicity semantics. Two pin-specific rules:
//!
//! - **Additional-stream pins freeze the collection.** The nine pin sidecar
//!   streams are serialized from — and merged back by — final DATA pin
//!   encounter order only; Additional pin sidecars are unproven (ownership
//!   worklog). While ANY Additional pin member exists, the final sequence is
//!   directly enforced to be positional: every member must sit at its
//!   original ordinal (Additional members strictly `Keep`, Data members
//!   `Keep` or field `Replace`) and no member may be added or deleted, so
//!   pin insert/delete/reorder — a Data-only permutation included, even
//!   though it would be sidecar-safe under the current save path — is a hard
//!   error naming the frozen member. Keep-untouched arrangements (and
//!   batches with `pins: None`) flow through unchanged.
//! - **`all_pin_count` is hands-off (memo §7).** Structural pin edits never
//!   touch the stored component `AllPinCount` cache — positive or zero, stale
//!   or not, it is preserved verbatim; no recompute path exists anywhere in
//!   this engine.
//!
//! Save-time sidecar consistency needs no new recompute layer:
//! `serialize_pin_sidecars` already enumerates the FINAL Data pin encounter
//! order at save time, so insert/remove/reorder keeps every ordinal-addressed
//! sidecar entry attached to its pin (verified by targeted save/reopen tests).
//!
//! # IndexInSheet (memo §6)
//!
//! Owners whose child sequence the batch structurally touched (membership or
//! relative order of their children changed) get ALL their in-objectList
//! children renumbered to final 0-based sibling positions — pins count as
//! siblings but carry no stored field. Untouched owners are preserved
//! verbatim (our deliberate deviation from AD26's renumber-everything-on-save).
//! Objects that natively never live in an owner's objectList occupy no sibling
//! slot and are never renumbered: Designator records and root-owned Parameter
//! records named "Comment" (component field objects), pin-owned special
//! parameters (DefaultNet/HiddenNetName/PinSelectionMemory/PinUniqueId), the
//! implementation compatibility CONTAINERS (ImplementationList, Implementation,
//! ImplementationMap, ParameterList — implementations are removed from the
//! objectList before AD26's save-time numbering, and the wrapper ctors pin
//! their own values), and ParameterList-owned parameters (the native
//! IndexOf-mismatch serializes −1; wild fixture bytes confirm). MapDefiners DO
//! occupy a slot in their map's objectList and carry 0-based ordinals
//! natively. Newly created field objects are written with −1 (memo §6 rule 1);
//! other new children receive their final sibling position through the
//! renumbering above.
//!
//! Documented deviation from AD26's import dispatch: `SchDataParameter.
//! UpdateOwner` selects its special-name branches by NAME ALONE (DefaultNet /
//! HiddenNetName / PinSelectionMemory / Comment / Description / PinUniqueId),
//! and when the resolved owner lacks the matching interface the parameter is
//! attached NOWHERE — AD26 then silently DROPS it on the next save (its save
//! walk enumerates only objectList members plus field objects). We must never
//! replicate that silent drop (cardinal rule), so this engine classifies by
//! name AND owner kind: only combinations AD26 actually consumes as field
//! objects/pin fields (root "Comment"; pin-owned specials) are exempt from
//! numbering, and every preserved record that AD26 would have dropped (e.g. a
//! root-owned Parameter named "Description" — `SchDataComponent` implements
//! `ISchDataCommentOwner` but NOT `ISchDataDescriptionOwner`, and its
//! `GetFieldObjects` yields only designator+comment) is numbered as an
//! ordinary sibling when its owner is touched, matching the wild-file
//! precedent (editor-attached root Description parameters carry ordinals,
//! e.g. Custom.SchLib INDEXINSHEET=3, kmilo17pet-PIC16_Part1 INDEXINSHEET=4).
//!
//! # New/Replace field surfaces
//!
//! Inits cover exactly the intent-addressable fields the A2 views expose:
//! `SchParameterInit` (name/text/is_hidden), `SchGraphicInit` (owner part,
//! display mode, and the 13 representable kinds' view fields), and
//! `SchPinInit` (the `SchPinView` surface: designator, name, electrical type,
//! length, location, orientation, is_hidden, hidden_net_name, owner part,
//! display mode, and the swap-group fields). Replace preserves every
//! non-addressed field of the original record verbatim (unique_id, style_id,
//! sidecar-backed pin fields — defined/selected functions, package length,
//! propagation delay, fractional coordinates, custom text positioning,
//! symbol-line width, description/default value —, unrepresentable extras); a
//! Replace target's kind must equal the replacement's kind — no cross-kind
//! mapping exists, and intent-unrepresentable members (Symbol/Hyperlink/other)
//! can only be kept or deleted. New records get typed wire defaults, a fresh
//! UniqueId (where the kind carries one — NOT pins: the native SchLib binary
//! pin body has no UniqueId field, so an assigned id could never survive a
//! native save/reopen and none is generated), `owner_part_id` per memo §1/§2
//! (parameters → −1; graphics → the part they are created for, 1..=effective;
//! pins → 0 [the natively defined shared-pin wildcard] or 1..=effective, −1
//! forbidden), `owner_part_display_mode` per memo §4 (default 0, bounded by
//! `display_mode_count`), and style_id 0 (authoring a style id is the memo
//! §8b hard error; no init exposes one).
//!
//! Recorded creation-default deviation from the native editor default
//! (`SchDataPin.SetDefault`, Objects/SchDataPin.cs): a New pin keeps the
//! typed WIRE defaults for the fields `SchPinInit` does not address —
//! `show_name`/`show_designator` false (PIN_CONGLOMERATE wire zero) and the
//! wire-zero formal type — where AD26's editor creation default sets both
//! show flags TRUE and `formalType = eStdLogic_ForcingUnknown` (it also
//! seeds the combined swap string as the bare `"|&|"` separator, which our
//! empty-halves encoding writes as `""`). Neither `SchPinInit` nor
//! `SchPinView` exposes the show flags or the formal type, so callers cannot
//! author them; whether New pins should adopt the native `SetDefault` values
//! (or the init/view surface should grow the fields) is a queued user
//! decision recorded in the increment-C worklog. The current values
//! round-trip exactly (asserted in tests) — this is a creation-semantics
//! deviation, not a data-integrity one.
//!
//! Pin swap encoding (memo §5 rule 3): the stored combined
//! `SwapIdPartAndPartPin` string stays byte-verbatim on every untouched pin
//! (wild forms such as a bare `"|&|"` included). `SchPinInit` addresses the
//! two halves separately (`part_swap_group` prefix, `part_pin_swap_suffix`);
//! when BOTH are `None` on a Replace the combined string is preserved
//! byte-verbatim, and when either half is addressed the original is split and
//! re-encoded through the native rule (separator emitted only for a non-empty
//! suffix) with the unaddressed half preserved through the split. A New pin
//! resolves both halves (empty defaults) and encodes natively. The native
//! concat rule cannot regenerate the wild bare-separator form; re-encoding a
//! pin that carried one normalizes it, which is exactly the memo's contract
//! for touched halves.
//!
//! Authoring a non-empty pin `hidden_net_name` different from the stored value
//! is a hard error (`check_pin_hidden_net_name_write`): the binary pin body
//! carries no such field and the native pin-owned-Parameter storage form is
//! preserved-only, so a written value would be silently lost on save.
//!
//! # Footprints (increment D2)
//!
//! `footprints()` membership is the A2 `footprint_maps()` projection: PCBLIB
//! implementations only (case-insensitive model type). Each member is the
//! Implementation record PLUS its whole transitively-owned subtree
//! (ImplementationMap, MapDefiners, ParameterList, implementation-owned
//! parameters, and any wild descendants), moved, replaced, or deleted as a
//! unit. The serialized compatibility-container chain (ImplementationList →
//! Implementation → ImplementationMap → MapDefiner, ParameterList) is
//! preserved as serialized, never flattened (ownership worklog 2026-09-05).
//!
//! - **Non-PCBLIB implementations (SIM, PCB3D, SI, …) are OUTSIDE the
//!   collection**: their subtrees are preserved verbatim in their original
//!   relative stream slots under every footprint edit, including complete
//!   replacement with deletion authorized. Footprint reordering permutes only
//!   the footprint slots (a member's final sequence entry is emitted at the
//!   original member's Implementation position); the shared ImplementationList
//!   and interleaved non-PCBLIB siblings keep their positions.
//! - **Creation** materializes the full native chain: the Implementation
//!   (model_type `PCBLIB`, a fresh UniqueId, the native single default
//!   datafile link `{location: "", entity: model_name, kind: "PCBLib"}` —
//!   AD26 synthesizes exactly that for a link-less PCBLIB implementation,
//!   EngineObjects/SchImplementation.cs `HandleAfterImportEvent`, and every
//!   corpus footprint carries it), one ImplementationMap, one MapDefiner per
//!   pin-pad map, and one ParameterList (AD26 emits map and parameter list
//!   UNCONDITIONALLY per implementation — SchDataImplementation.cs ctor +
//!   `AddToListForSave`; gate fixtures: RECORD 45/46/48 counts are 1:1:1).
//!   New implementations attach to the component's existing shared
//!   ImplementationList (AD26 emits exactly one per component,
//!   SchDataComponent.AddToListForSave; every gate-fixture component carries
//!   RECORD=44 even with zero models); a list is created only when the
//!   component has none at all — owner root, owner_part_id 0 and
//!   index_in_sheet 0 per the ctor (Objects/SchDataImplementationList.cs:9-14)
//!   and the fixture bytes (bare `|RECORD=44`). The whole chain hard-codes
//!   owner_part_id 0 (memo §1); Implementation records serialize
//!   INDEXINSHEET=-1 (fixture bytes; the native IndexOf-mismatch mechanism),
//!   ImplementationMap/ParameterList 0 (ctors), and MapDefiners carry their
//!   0-based sibling ordinal within the map (wild bytes: Custom.SchLib
//!   `RECORD=47 … INDEXINSHEET=1`), assigned through the shared memo-§6
//!   touched-owner renumbering.
//! - **Replace** writes exactly the addressed axes (model_name, description,
//!   is_current, pin_pad_maps); everything else — unique_id, datafile links,
//!   GUIDs, model_location, lock/database flags, stored model_type casing,
//!   the ParameterList subtree — is preserved verbatim. `pin_pad_maps: None`
//!   is unaddressed (definers preserved verbatim); `Some(maps)` rebuilds the
//!   MapDefiner records under the implementation's single map (creating the
//!   map only when none exists and `maps` is non-empty; several maps make the
//!   rebuild unaddressable and hard-error). Duplicate pin designators
//!   (case-insensitive) are rejected: AD26's `AddMapDefiner` silently DROPS
//!   case-insensitive duplicates on load (SchDataImplementationMap.cs), so
//!   writing them would be silent data loss. A `pin_pad_maps` rebuild
//!   recreates the definers with native ctor base state (native definers
//!   only ever carry ctor base state; AD26 creates them fresh through
//!   `AddMapDefiner`): a wild non-default base-field value on a replaced
//!   definer (e.g. `graphically_locked`) is NOT carried over — leave
//!   `pin_pad_maps` unaddressed (`None`) to preserve such definers verbatim.
//! - **is_current** (single-current invariant): AD26's SchLib importer itself
//!   enforces exactly one current implementation per model type at load
//!   (`ValidateImplementationsCurrentState`, called from
//!   SchDataImporterLibraryV5.cs:239,385) by silently repairing the state,
//!   and the wild class is real (two of the three gate fixtures —
//!   lucashudson-Connectors, 58 footprints, and arthurbenemann-STM32F4, 381 —
//!   store every footprint with no ISCURRENT key at all). We never repair
//!   silently:
//!   loaded wild states capture verbatim, and the mutation boundary blocks
//!   only NEW divergence — a batch may neither break a coherent input's
//!   single-current state (by flipping, deleting, or out-creating the
//!   current footprint) nor author an ACTIVE current claim (a Replace
//!   changing the stored value, or a New with is_current true) into an
//!   incoherent final state. A pre-existing wild state passes through
//!   verbatim when the batch writes no current value (Keeps, value-equal
//!   Replaces, and non-current News): AD26's load repair of it is unchanged
//!   by such an edit. Wild states are resolvable in the same batch by
//!   authoring is_current explicitly into a coherent final assignment.
//! - **Deletion** removes the member's whole owned subtree — never leaves
//!   orphans — and requires `allow_deletion`. The shared ImplementationList is
//!   NOT deleted when the last implementation goes: it is outside-collection
//!   container state, and AD26 emits one per component unconditionally.
//! - **Additional-stream chain records freeze the collection** (same
//!   conservative shape as pins): the native exporter cannot emit chain
//!   records into Additional (implementation kinds are not in the
//!   Additional-root object set, ownership worklog; zero corpus occurrences),
//!   so while any chain record lives there the only accepted footprints edit
//!   is the all-positional-Keep no-op.
//! - A footprint subtree containing a Pin record is unaddressable wild state
//!   (hard error — pin sidecar ordering couples to the frozen pins
//!   contract), and a subtree record that is simultaneously a member of
//!   another collection edited in the same batch is a conflict (hard error).
//!
//! The optional init fields (`owner_part_display_mode`, and `font_id` on
//! Label/TextFrame) mean "unaddressed" when `None`: on `New` they resolve the
//! creation default (mode 0; the native default font), on `Replace` they
//! PRESERVE the original record's stored value verbatim (capture-forbidden
//! wild values included). An absent optional field never resets a stored
//! value; `from_view` populates them explicitly.
//!
//! Font ids are validated against the captured font table; a text-bearing
//! graphic created with no authored font resolves the native default (Times
//! New Roman 10) against EXISTING table entries only — appending a missing
//! default entry is a library-level namespace change that must be DECLARED
//! through `SchLibSnapshot::with_library_edit` font appends (which recompute
//! the reference context), so it is a hard error here: apply the library edit
//! first and re-take the component snapshot from the edited library. NOTE:
//! `SchParameterInit` mirrors `SchParameterView` and therefore has no font
//! field, so in a library whose captured table lacks the native default
//! entry, parameter creation requires that library edit first (the hard error
//! names it). Embedded image records must reference an existing captured
//! storage asset (memo §8c rule 2); external-path images are exempt.

use std::collections::{HashMap, HashSet};

use super::views::is_graphic_like;
use super::{SchGraphicDetail, SchGraphicView, SchLibComponentSnapshot, SchOwnerGroup, SchPinView};
use crate::param_collection::ParameterCollection;
use crate::param_value::SchAngle;
use crate::sch_records::{
    ModelDatafileLink, SchArc, SchBezier, SchEllipse, SchEllipticalArc, SchImage,
    SchImplementation, SchImplementationList, SchImplementationMap, SchLabel, SchLibComponent,
    SchLine, SchMapDefiner, SchParameter, SchParameterList, SchPie, SchPin, SchPolygon,
    SchPolyline, SchRecord, SchRectangle, SchRoundRectangle, SchTextFrame,
    encode_swap_id_part_and_part_pin, parse_text_pin, split_swap_id_part_and_part_pin,
};
use crate::schlib::ownership::{
    RecordAddress, RecordItem, RecordOwner, RecordStream, assemble_records, owner_of,
};
use crate::schlib::validation::{
    WrittenOwnerPartClass, check_embedded_image_reference, check_font_id_exists,
    check_pin_hidden_net_name_write, check_style_id_write, check_written_display_mode,
    check_written_owner_part_id, find_or_create_default_font,
};
use crate::util::generate_unique_id;
use crate::{AltiumFormatError, Result, ResultExt};
use altium_format_types::{
    Color, Coord, CoordPoint, HorizontalAlign, LineShape, LineStyle, PenWidth, PinElectricalType,
    RotationBy90, TextJustification,
};

/// One batch of structural edits over the parameters, graphics, pins, and
/// footprints collections, applied through a single assembly pass. `None`
/// leaves a collection completely untouched; `Some(vec![])` deletes every
/// member (authorization required).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchChildBatch {
    /// Final ordered `parameters()` sequence, or `None` to leave it untouched.
    pub parameters: Option<Vec<SchParameterEdit>>,
    /// Final ordered `graphics()` sequence, or `None` to leave it untouched.
    pub graphics: Option<Vec<SchGraphicEdit>>,
    /// Final ordered `pins()` sequence, or `None` to leave it untouched.
    pub pins: Option<Vec<SchPinEdit>>,
    /// Final ordered `footprint_maps().footprints` sequence, or `None` to
    /// leave it untouched. Non-PCBLIB implementations are outside the
    /// collection and preserved verbatim under every footprint edit.
    pub footprints: Option<Vec<SchFootprintEdit>>,
    /// Explicit per-call deletion authorization: an original collection member
    /// omitted from a final sequence is rejected unless this is set.
    pub allow_deletion: bool,
}

/// One item of the final `parameters()` sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum SchParameterEdit {
    /// The original member at `parameters()[i]`, preserved verbatim.
    Keep(usize),
    /// The original member at `parameters()[i]` with the intent-addressable
    /// fields replaced; every other field is preserved verbatim.
    Replace(usize, SchParameterInit),
    /// A newly created root-owned parameter.
    New(SchParameterInit),
}

/// One item of the final `graphics()` sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum SchGraphicEdit {
    /// The original member at `graphics()[i]`, preserved verbatim. This is the
    /// only way an intent-unrepresentable member (Symbol/Hyperlink/other) can
    /// appear in the final sequence.
    Keep(usize),
    /// The original member at `graphics()[i]` with the intent-addressable
    /// fields replaced; the target's kind must equal the replacement's kind.
    Replace(usize, SchGraphicInit),
    /// A newly created root-owned graphic.
    New(SchGraphicInit),
}

/// Intent-addressable parameter fields (the `SchParameterView` surface).
#[derive(Debug, Clone, PartialEq)]
pub struct SchParameterInit {
    pub name: String,
    pub text: String,
    pub is_hidden: bool,
}

impl SchParameterInit {
    /// Copies the intent-addressable fields out of a projected view.
    pub fn from_view(view: &super::SchParameterView<'_>) -> Self {
        Self {
            name: view.name.to_owned(),
            text: view.text.to_owned(),
            is_hidden: view.is_hidden,
        }
    }
}

/// One item of the final `pins()` sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum SchPinEdit {
    /// The original member at `pins()[i]`, preserved verbatim (sidecar-backed
    /// fields and the raw combined swap string included). The only edit
    /// allowed for a pin living in the Additional stream, and only at its
    /// original position.
    Keep(usize),
    /// The original member at `pins()[i]` with the intent-addressable fields
    /// replaced; every other field — including the sidecar-backed typed
    /// fields — is preserved verbatim.
    Replace(usize, SchPinInit),
    /// A newly created root-owned pin.
    New(SchPinInit),
}

/// Intent-addressable pin fields (the `SchPinView` surface).
///
/// The two combined-swap halves are optional with "unaddressed" semantics per
/// memo §5 rule 3: both `None` on a `Replace` preserves the stored combined
/// string byte-verbatim (wild encodings included); addressing either half
/// splits the original and re-encodes through the native rule, preserving the
/// unaddressed half through the split. On `New` both default to empty.
/// `owner_part_display_mode: None` means unaddressed exactly as in
/// [`SchGraphicInit`]: creation default 0 on `New`, stored value preserved on
/// `Replace`.
#[derive(Debug, Clone, PartialEq)]
pub struct SchPinInit {
    /// 0 (the natively defined shared-pin wildcard, memo §1) or a numbered
    /// part `1..=effective_part_count`. −1 is forbidden on pins.
    pub owner_part_id: i32,
    pub owner_part_display_mode: Option<i32>,
    pub designator: String,
    pub name: String,
    pub electrical: PinElectricalType,
    pub length: Coord,
    pub location: CoordPoint,
    pub orientation: RotationBy90,
    pub is_hidden: bool,
    /// Only the stored value or the empty default may be written
    /// (`check_pin_hidden_net_name_write`; no verified SchLib serialization
    /// path exists for authored hidden net names).
    pub hidden_net_name: String,
    /// `SwapIdPin` (the pin swap group).
    pub swap_group: String,
    /// `SwapIdPart` — the prefix half of the combined swap string; `None` =
    /// unaddressed.
    pub part_swap_group: Option<String>,
    /// `SwapIdPartPin` — the suffix half of the combined swap string; `None` =
    /// unaddressed. The native encoding emits the separator only for a
    /// non-empty suffix, so an authored empty suffix and an authored absent
    /// suffix serialize identically.
    pub part_pin_swap_suffix: Option<String>,
    /// `PairSwapID` (PinMiscData sidecar).
    pub pair_swap_group: String,
}

impl SchPinInit {
    /// Copies the intent-addressable fields out of a projected view. Errors
    /// for views whose owner group has no pin init representation
    /// (`PartIndependent` and `OutOfRange` owners are preserved-only content).
    ///
    /// Both combined-swap halves are populated explicitly (a view suffix of
    /// `None` maps to an authored empty suffix — the two serialize identically
    /// under the native concat rule), so a `Replace` built `from_view`
    /// re-encodes the combined string: byte-identical for every natively
    /// encodable string, and normalizing for the wild bare-separator forms. A
    /// caller that must preserve a wild combined string byte-verbatim leaves
    /// both halves `None` instead.
    pub fn from_view(view: &SchPinView<'_>) -> Result<Self> {
        let owner_part_id = match view.owner_group {
            SchOwnerGroup::SharedPins => 0,
            SchOwnerGroup::Part(part) => part,
            other => {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "OwnerPartId".to_owned(),
                    detail: format!(
                        "pin owner group {other:?} has no init representation: only shared \
                         (0) and numbered parts are authorable on pins (memo §1/§2)"
                    ),
                });
            }
        };
        Ok(Self {
            owner_part_id,
            owner_part_display_mode: Some(view.owner_part_display_mode),
            designator: view.designator.to_owned(),
            name: view.name.to_owned(),
            electrical: view.electrical,
            length: view.length,
            location: view.location,
            orientation: view.orientation,
            is_hidden: view.is_hidden,
            hidden_net_name: view.hidden_net_name.to_owned(),
            swap_group: view.swap_group.to_owned(),
            part_swap_group: Some(view.part_swap_group.to_owned()),
            part_pin_swap_suffix: Some(view.part_pin_swap_suffix.unwrap_or("").to_owned()),
            pair_swap_group: view.pair_swap_group.to_owned(),
        })
    }
}

/// One item of the final `footprint_maps().footprints` sequence. Each member
/// is the PCBLIB Implementation record plus its whole owned subtree, handled
/// as a unit (module docs, "Footprints").
#[derive(Debug, Clone, PartialEq)]
pub enum SchFootprintEdit {
    /// The original member at `footprint_maps().footprints[i]`, its whole
    /// subtree preserved verbatim.
    Keep(usize),
    /// The original member at `footprint_maps().footprints[i]` with the
    /// intent-addressable axes replaced; every other field and the rest of
    /// the subtree are preserved verbatim (definers rebuilt only when
    /// `pin_pad_maps` is addressed).
    Replace(usize, SchFootprintInit),
    /// A newly created footprint: the full native chain is materialized under
    /// the component's shared ImplementationList (created only when the
    /// component has none).
    New(SchFootprintInit),
}

/// Intent-addressable footprint fields (the `SchFootprintMapView` surface).
///
/// The implementation chain hard-codes `owner_part_id` 0 natively (memo §1),
/// so no owner axis exists. `pin_pad_maps: None` means unaddressed: on `New`
/// it resolves to no mappings, on `Replace` it preserves the original
/// MapDefiner records verbatim; `Some(maps)` rebuilds the definers.
#[derive(Debug, Clone, PartialEq)]
pub struct SchFootprintInit {
    /// PCB footprint model name. Must be non-empty on `New` (the native
    /// default datafile link derives its entity name from it).
    pub model_name: String,
    pub description: String,
    /// Whether this footprint is the component's current PCBLIB model. A
    /// batch that changes the collection must land with exactly one current
    /// footprint when the final set is non-empty (module docs).
    pub is_current: bool,
    /// Pin designator → ordered pad designators, one MapDefiner each.
    /// Duplicate pin designators (case-insensitive) are rejected: AD26's
    /// loader silently drops them.
    pub pin_pad_maps: Option<Vec<SchPinPadMapInit>>,
}

/// One authored MapDefiner: a pin designator mapped to its ordered pad
/// designators (`DesIntf` → `DesImp{i}`).
#[derive(Debug, Clone, PartialEq)]
pub struct SchPinPadMapInit {
    pub pin: String,
    pub pads: Vec<String>,
}

impl SchFootprintInit {
    /// Copies the intent-addressable axes out of a projected view, populating
    /// `pin_pad_maps` explicitly (a `Replace` built `from_view` therefore
    /// rebuilds the definer records; leave `pin_pad_maps` as `None` to
    /// preserve them verbatim instead).
    pub fn from_view(view: &super::SchFootprintMapView<'_>) -> Self {
        Self {
            model_name: view.model_name.to_owned(),
            description: view.description.to_owned(),
            is_current: view.is_current,
            pin_pad_maps: Some(
                view.pin_pad_maps
                    .iter()
                    .map(|map| SchPinPadMapInit {
                        pin: map.pin.to_owned(),
                        pads: map.pads.to_vec(),
                    })
                    .collect(),
            ),
        }
    }
}

/// Intent-addressable graphic fields (the `SchGraphicView` surface): the owner
/// part slot, the display mode, and the kind-specific shape fields.
#[derive(Debug, Clone, PartialEq)]
pub struct SchGraphicInit {
    /// The effective part this graphic belongs to (`1..=effective_part_count`,
    /// memo §2). Owner 0 and out-of-range owners are never created.
    pub owner_part_id: i32,
    /// `None` means unaddressed: creation default 0 on `New` (memo §2/§4),
    /// the original record's stored mode preserved verbatim on `Replace`. A
    /// specified mode must satisfy `0 ≤ mode < display_mode_count`.
    pub owner_part_display_mode: Option<i32>,
    pub shape: SchGraphicShape,
}

impl SchGraphicInit {
    /// Copies the intent-addressable fields out of a projected view. Errors
    /// for intent-unrepresentable kinds (Symbol/Hyperlink/other) and for
    /// views whose owner group is not a numbered part (`SharedPins`,
    /// `PartIndependent`, and `OutOfRange` owners are preserved-only content
    /// that no init may express).
    pub fn from_view(view: &SchGraphicView<'_>) -> Result<Self> {
        let SchOwnerGroup::Part(part) = view.owner_group else {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "OwnerPartId".to_owned(),
                detail: format!(
                    "graphic owner group {:?} has no init representation: only numbered \
                     parts are authorable (memo §1/§2)",
                    view.owner_group
                ),
            });
        };
        let shape = SchGraphicShape::from_detail(&view.detail)?;
        Ok(Self {
            owner_part_id: part,
            owner_part_display_mode: Some(view.owner_part_display_mode),
            shape,
        })
    }
}

/// The 13 intent-representable graphic kinds with exactly the fields their A2
/// views expose. `font_id: None` on `Label`/`TextFrame` means unaddressed: on
/// `New` it resolves the native default font (Times New Roman 10) against
/// existing captured table entries; on `Replace` it preserves the original
/// record's stored font verbatim.
#[derive(Debug, Clone, PartialEq)]
pub enum SchGraphicShape {
    Line {
        from: CoordPoint,
        to: CoordPoint,
        line_width: PenWidth,
        line_style: LineStyle,
        color: Color,
    },
    Rectangle {
        from: CoordPoint,
        to: CoordPoint,
        line_width: PenWidth,
        line_style: LineStyle,
        color: Color,
        area_color: Color,
        is_solid: bool,
        transparent: bool,
    },
    RoundRectangle {
        from: CoordPoint,
        to: CoordPoint,
        corner_x_radius: Coord,
        corner_y_radius: Coord,
        line_width: PenWidth,
        color: Color,
        area_color: Color,
        is_solid: bool,
    },
    Arc {
        center: CoordPoint,
        radius: Coord,
        line_width: PenWidth,
        start_angle: f64,
        end_angle: Option<f64>,
        color: Color,
    },
    EllipticalArc {
        center: CoordPoint,
        radius: Coord,
        secondary_radius: Coord,
        line_width: PenWidth,
        start_angle: f64,
        end_angle: Option<f64>,
        color: Color,
    },
    Ellipse {
        center: CoordPoint,
        radius: Coord,
        secondary_radius: Coord,
        line_width: PenWidth,
        color: Color,
        area_color: Color,
        is_solid: bool,
        transparent: bool,
    },
    Pie {
        center: CoordPoint,
        radius: Coord,
        line_width: PenWidth,
        start_angle: f64,
        end_angle: Option<f64>,
        color: Color,
        area_color: Color,
        is_solid: bool,
    },
    Polyline {
        vertices: Vec<CoordPoint>,
        line_width: PenWidth,
        line_style: LineStyle,
        start_line_shape: LineShape,
        end_line_shape: LineShape,
        line_shape_size: PenWidth,
        color: Color,
    },
    Polygon {
        vertices: Vec<CoordPoint>,
        line_width: PenWidth,
        color: Color,
        area_color: Color,
        is_solid: bool,
        transparent: bool,
    },
    Bezier {
        vertices: Vec<CoordPoint>,
        line_width: PenWidth,
        color: Color,
    },
    Image {
        from: CoordPoint,
        to: CoordPoint,
        orientation: RotationBy90,
        line_width: PenWidth,
        color: Color,
        is_solid: bool,
        keep_aspect: bool,
        embed_image: bool,
        file_name: String,
    },
    Label {
        at: CoordPoint,
        orientation: RotationBy90,
        justification: TextJustification,
        color: Color,
        font_id: Option<i32>,
        text: String,
        is_mirrored: bool,
        url: String,
    },
    TextFrame {
        from: CoordPoint,
        to: CoordPoint,
        line_width: PenWidth,
        color: Color,
        area_color: Color,
        text_color: Color,
        font_id: Option<i32>,
        is_solid: bool,
        show_border: bool,
        alignment: HorizontalAlign,
        word_wrap: bool,
        clip_to_rect: bool,
        text: String,
        text_margin: Coord,
        transparent: bool,
    },
}

impl SchGraphicShape {
    /// Stable diagnostic label matching `SchGraphicDetail::kind_name`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Line { .. } => "Line",
            Self::Rectangle { .. } => "Rectangle",
            Self::RoundRectangle { .. } => "RoundRectangle",
            Self::Arc { .. } => "Arc",
            Self::EllipticalArc { .. } => "EllipticalArc",
            Self::Ellipse { .. } => "Ellipse",
            Self::Pie { .. } => "Pie",
            Self::Polyline { .. } => "Polyline",
            Self::Polygon { .. } => "Polygon",
            Self::Bezier { .. } => "Bezier",
            Self::Image { .. } => "Image",
            Self::Label { .. } => "Label",
            Self::TextFrame { .. } => "TextFrame",
        }
    }

    /// Owned copy of a projected representable detail; errors for
    /// intent-unrepresentable kinds.
    pub fn from_detail(detail: &SchGraphicDetail<'_>) -> Result<Self> {
        Ok(match *detail {
            SchGraphicDetail::Line {
                from,
                to,
                line_width,
                line_style,
                color,
            } => Self::Line {
                from,
                to,
                line_width,
                line_style,
                color,
            },
            SchGraphicDetail::Rectangle {
                from,
                to,
                line_width,
                line_style,
                color,
                area_color,
                is_solid,
                transparent,
            } => Self::Rectangle {
                from,
                to,
                line_width,
                line_style,
                color,
                area_color,
                is_solid,
                transparent,
            },
            SchGraphicDetail::RoundRectangle {
                from,
                to,
                corner_x_radius,
                corner_y_radius,
                line_width,
                color,
                area_color,
                is_solid,
            } => Self::RoundRectangle {
                from,
                to,
                corner_x_radius,
                corner_y_radius,
                line_width,
                color,
                area_color,
                is_solid,
            },
            SchGraphicDetail::Arc {
                center,
                radius,
                line_width,
                start_angle,
                end_angle,
                color,
            } => Self::Arc {
                center,
                radius,
                line_width,
                start_angle,
                end_angle,
                color,
            },
            SchGraphicDetail::EllipticalArc {
                center,
                radius,
                secondary_radius,
                line_width,
                start_angle,
                end_angle,
                color,
            } => Self::EllipticalArc {
                center,
                radius,
                secondary_radius,
                line_width,
                start_angle,
                end_angle,
                color,
            },
            SchGraphicDetail::Ellipse {
                center,
                radius,
                secondary_radius,
                line_width,
                color,
                area_color,
                is_solid,
                transparent,
            } => Self::Ellipse {
                center,
                radius,
                secondary_radius,
                line_width,
                color,
                area_color,
                is_solid,
                transparent,
            },
            SchGraphicDetail::Pie {
                center,
                radius,
                line_width,
                start_angle,
                end_angle,
                color,
                area_color,
                is_solid,
            } => Self::Pie {
                center,
                radius,
                line_width,
                start_angle,
                end_angle,
                color,
                area_color,
                is_solid,
            },
            SchGraphicDetail::Polyline {
                vertices,
                line_width,
                line_style,
                start_line_shape,
                end_line_shape,
                line_shape_size,
                color,
            } => Self::Polyline {
                vertices: vertices.to_vec(),
                line_width,
                line_style,
                start_line_shape,
                end_line_shape,
                line_shape_size,
                color,
            },
            SchGraphicDetail::Polygon {
                vertices,
                line_width,
                color,
                area_color,
                is_solid,
                transparent,
            } => Self::Polygon {
                vertices: vertices.to_vec(),
                line_width,
                color,
                area_color,
                is_solid,
                transparent,
            },
            SchGraphicDetail::Bezier {
                vertices,
                line_width,
                color,
            } => Self::Bezier {
                vertices: vertices.to_vec(),
                line_width,
                color,
            },
            SchGraphicDetail::Image {
                from,
                to,
                orientation,
                line_width,
                color,
                is_solid,
                keep_aspect,
                embed_image,
                file_name,
            } => Self::Image {
                from,
                to,
                orientation,
                line_width,
                color,
                is_solid,
                keep_aspect,
                embed_image,
                file_name: file_name.to_owned(),
            },
            SchGraphicDetail::Label {
                at,
                orientation,
                justification,
                color,
                font_id,
                text,
                is_mirrored,
                url,
            } => Self::Label {
                at,
                orientation,
                justification,
                color,
                font_id: Some(font_id),
                text: text.to_owned(),
                is_mirrored,
                url: url.to_owned(),
            },
            SchGraphicDetail::TextFrame {
                from,
                to,
                line_width,
                color,
                area_color,
                text_color,
                font_id,
                is_solid,
                show_border,
                alignment,
                word_wrap,
                clip_to_rect,
                text,
                text_margin,
                transparent,
            } => Self::TextFrame {
                from,
                to,
                line_width,
                color,
                area_color,
                text_color,
                font_id: Some(font_id),
                is_solid,
                show_border,
                alignment,
                word_wrap,
                clip_to_rect,
                text: text.to_owned(),
                text_margin,
                transparent,
            },
            SchGraphicDetail::Symbol
            | SchGraphicDetail::Hyperlink
            | SchGraphicDetail::Other { .. } => {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "graphic kind".to_owned(),
                    detail: format!(
                        "graphic kind {} is intent-unrepresentable: it can only be kept in \
                         place or deleted with authorization",
                        detail.kind_name()
                    ),
                });
            }
        })
    }
}

/// The four structurally editable collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Collection {
    Parameters,
    Graphics,
    Pins,
    Footprints,
}

impl Collection {
    fn name(self) -> &'static str {
        match self {
            Self::Parameters => "parameters",
            Self::Graphics => "graphics",
            Self::Pins => "pins",
            Self::Footprints => "footprints",
        }
    }
}

/// One resolved final item: either an original member (optionally with its
/// intent-addressable fields replaced) or a newly created record.
enum FinalRecord {
    Existing {
        address: RecordAddress,
        replacement: Option<SchRecord>,
    },
    Created(SchRecord),
}

/// Identity of a record across the batch: original input address or the
/// batch-local id of a created record. Shared with the parts engine
/// (`snapshot::parts`), which renumbers touched owners after part-scoped
/// deletions through the same policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Identity {
    Original(RecordStream2, usize),
    New(usize),
}

/// `RecordStream` mirror with derives needed for identity ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum RecordStream2 {
    Data,
    Additional,
}

pub(super) fn identity_of(address: RecordAddress) -> Identity {
    Identity::Original(
        match address.stream {
            RecordStream::Data => RecordStream2::Data,
            RecordStream::Additional => RecordStream2::Additional,
        },
        address.index,
    )
}

/// A record's owner, keyed by identity so input and final graphs compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum OwnerId {
    Root,
    Record(Identity),
}

/// Whether an init is building a new record or replacing an original's
/// addressed fields: `None` optional fields resolve creation defaults on
/// `Create` and preserve the original's stored values on `Replace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditMode {
    Create,
    Replace,
}

impl SchLibComponentSnapshot {
    /// Applies one batch of structural edits to the parameters, graphics,
    /// pins, and footprints collections; see the module documentation for the complete contract
    /// (final-sequence semantics, the stream-order rule, deletion
    /// authorization, creation defaults, and the IndexInSheet policy). The
    /// input snapshot is never modified; errors leave no partial state.
    pub fn with_children(&self, batch: SchChildBatch) -> Result<Self> {
        self.with_children_inner(batch).with_context(|| {
            format!(
                "applying structural child batch to component '{}'",
                self.lib_reference()
            )
        })
    }

    fn with_children_inner(&self, batch: SchChildBatch) -> Result<Self> {
        let component = &self.component;
        let param_members = self.parameter_member_addresses()?;
        let graphic_members = graphic_member_addresses(component);
        let pin_members = pin_member_addresses(component);

        let param_finals = match batch.parameters {
            Some(edits) => {
                Some(self.resolve_parameter_edits(edits, &param_members, batch.allow_deletion)?)
            }
            None => None,
        };
        let graphic_finals = match batch.graphics {
            Some(edits) => {
                Some(self.resolve_graphic_edits(edits, &graphic_members, batch.allow_deletion)?)
            }
            None => None,
        };
        let pin_finals = match batch.pins {
            Some(edits) => {
                Some(self.resolve_pin_edits(edits, &pin_members, batch.allow_deletion)?)
            }
            None => None,
        };
        let footprint_plan = match batch.footprints {
            Some(edits) => self.resolve_footprint_edits(
                edits,
                batch.allow_deletion,
                &graphic_members,
                graphic_finals.is_some(),
            )?,
            None => None,
        };

        // Working copy with Replace field-mutations applied in place: indices
        // and the ownership graph are unchanged, so every later resolution
        // still refers to the immutable input addressing.
        let mut working = component.clone();
        for finals in [&param_finals, &graphic_finals, &pin_finals]
            .into_iter()
            .flatten()
        {
            for item in finals {
                if let FinalRecord::Existing {
                    address,
                    replacement: Some(record),
                } = item
                {
                    *record_slot_mut(&mut working, *address) = record.clone();
                }
            }
        }
        if let Some(plan) = &footprint_plan {
            for (address, record) in &plan.replacements {
                *record_slot_mut(&mut working, *address) = record.clone();
            }
        }

        let plan = plan_streams(
            component,
            &param_members,
            &graphic_members,
            &pin_members,
            param_finals,
            graphic_finals,
            pin_finals,
            footprint_plan,
        )?;
        let (data_items, data_ids) = to_items(&plan.data)?;
        let (additional_items, additional_ids) = {
            let (items, ids) = to_items(&plan.additional)?;
            for (item, id) in items.iter().zip(&ids) {
                if matches!(item, RecordItem::New { .. }) {
                    return Err(AltiumFormatError::InvalidParamValue {
                        key: "record stream".to_owned(),
                        detail: format!(
                            "final arrangement places created record {id:?} in the Additional \
                             stream; creating new parameters/graphics/pins/footprints in the \
                             Additional stream is unsupported (no verified native creation \
                             path exists)"
                        ),
                    });
                }
            }
            (items, ids)
        };

        let assembled = assemble_records(&working, &data_items, &additional_items)
            .context("assembling structural child batch")?;

        let mut candidate = self.clone();
        candidate.component.records = assembled.data;
        candidate.component.additional_records = assembled.additional;
        renumber_touched_owners(
            component,
            &mut candidate.component,
            &data_ids,
            &additional_ids,
        )
        .context("renumbering IndexInSheet for structurally touched owners")?;
        Ok(candidate)
    }

    /// Addresses of `parameters()` members (root-owned Parameter records) in
    /// encounter order, mirroring the projection's membership exactly.
    fn parameter_member_addresses(&self) -> Result<Vec<RecordAddress>> {
        let component = &self.component;
        let mut members = Vec::new();
        for (address, record) in all_records(component) {
            if !matches!(record, SchRecord::Parameter(_)) {
                continue;
            }
            if owner_of(component, address, record)?.is_none() {
                members.push(address);
            }
        }
        Ok(members)
    }

    fn resolve_parameter_edits(
        &self,
        edits: Vec<SchParameterEdit>,
        members: &[RecordAddress],
        allow_deletion: bool,
    ) -> Result<Vec<FinalRecord>> {
        let mut referenced = HashSet::new();
        let mut finals = Vec::with_capacity(edits.len());
        for (position, edit) in edits.into_iter().enumerate() {
            let item = match edit {
                SchParameterEdit::Keep(index) => FinalRecord::Existing {
                    address: resolve_member(
                        Collection::Parameters,
                        members,
                        index,
                        position,
                        &mut referenced,
                    )?,
                    replacement: None,
                },
                SchParameterEdit::Replace(index, init) => {
                    let address = resolve_member(
                        Collection::Parameters,
                        members,
                        index,
                        position,
                        &mut referenced,
                    )?;
                    let SchRecord::Parameter(original) = record_slot(&self.component, address)
                    else {
                        unreachable!("parameter members are Parameter records")
                    };
                    let mut replaced = original.clone();
                    replaced.name = init.name;
                    replaced.text = init.text;
                    replaced.is_hidden = init.is_hidden;
                    // No init exposes style ids; wire the §8b guard so any
                    // future field addition trips it.
                    check_style_id_write(Some(original.base.style_id), replaced.base.style_id)?;
                    FinalRecord::Existing {
                        address,
                        replacement: Some(SchRecord::Parameter(replaced)),
                    }
                }
                SchParameterEdit::New(init) => {
                    FinalRecord::Created(self.build_parameter(init, position)?)
                }
            };
            finals.push(item);
        }
        check_deletions(
            Collection::Parameters,
            members.len(),
            &referenced,
            allow_deletion,
        )?;
        Ok(finals)
    }

    fn resolve_graphic_edits(
        &self,
        edits: Vec<SchGraphicEdit>,
        members: &[RecordAddress],
        allow_deletion: bool,
    ) -> Result<Vec<FinalRecord>> {
        let mut referenced = HashSet::new();
        let mut finals = Vec::with_capacity(edits.len());
        for (position, edit) in edits.into_iter().enumerate() {
            let item = match edit {
                SchGraphicEdit::Keep(index) => FinalRecord::Existing {
                    address: resolve_member(
                        Collection::Graphics,
                        members,
                        index,
                        position,
                        &mut referenced,
                    )?,
                    replacement: None,
                },
                SchGraphicEdit::Replace(index, init) => {
                    let address = resolve_member(
                        Collection::Graphics,
                        members,
                        index,
                        position,
                        &mut referenced,
                    )?;
                    let record = self
                        .replace_graphic(record_slot(&self.component, address), init)
                        .with_context(|| {
                            format!("replacing graphics()[{index}] (final position {position})")
                        })?;
                    FinalRecord::Existing {
                        address,
                        replacement: Some(record),
                    }
                }
                SchGraphicEdit::New(init) => {
                    FinalRecord::Created(self.build_graphic(init).with_context(|| {
                        format!("creating graphic at final position {position}")
                    })?)
                }
            };
            finals.push(item);
        }
        check_deletions(
            Collection::Graphics,
            members.len(),
            &referenced,
            allow_deletion,
        )?;
        Ok(finals)
    }

    fn resolve_pin_edits(
        &self,
        edits: Vec<SchPinEdit>,
        members: &[RecordAddress],
        allow_deletion: bool,
    ) -> Result<Vec<FinalRecord>> {
        let mut referenced = HashSet::new();
        let mut finals = Vec::with_capacity(edits.len());
        for (position, edit) in edits.into_iter().enumerate() {
            let item = match edit {
                SchPinEdit::Keep(index) => FinalRecord::Existing {
                    address: resolve_member(
                        Collection::Pins,
                        members,
                        index,
                        position,
                        &mut referenced,
                    )?,
                    replacement: None,
                },
                SchPinEdit::Replace(index, init) => {
                    let address = resolve_member(
                        Collection::Pins,
                        members,
                        index,
                        position,
                        &mut referenced,
                    )?;
                    let SchRecord::Pin(original) = record_slot(&self.component, address) else {
                        unreachable!("pin members are Pin records")
                    };
                    let mut replaced = original.clone();
                    self.apply_pin_init(&mut replaced, init, EditMode::Replace)
                        .with_context(|| {
                            format!("replacing pins()[{index}] (final position {position})")
                        })?;
                    FinalRecord::Existing {
                        address,
                        replacement: Some(SchRecord::Pin(replaced)),
                    }
                }
                SchPinEdit::New(init) => FinalRecord::Created(
                    self.build_pin(init)
                        .with_context(|| format!("creating pin at final position {position}"))?,
                ),
            };
            finals.push(item);
        }
        // Additional-stream pins freeze the collection to a positional
        // Keep/Replace arrangement: the nine sidecar streams are serialized
        // from final DATA pin encounter order only, and Additional pin
        // sidecars are unproven (ownership worklog), so while any Additional
        // pin member exists every member must stay at its original final
        // position (Additional members strictly Keep, Data members Keep or
        // field Replace) and no member may be added or deleted. A Data-only
        // permutation would be sidecar-safe under the current save path, but
        // the published conservative contract rejects it until Additional
        // pin sidecars are proven (module docs).
        let has_additional_pins = members
            .iter()
            .any(|address| address.stream == RecordStream::Additional);
        if has_additional_pins {
            for (ordinal, address) in members.iter().enumerate() {
                let (positional, replaced) = match finals.get(ordinal) {
                    Some(FinalRecord::Existing {
                        address: kept,
                        replacement,
                    }) if kept == address => (true, replacement.is_some()),
                    _ => (false, false),
                };
                if address.stream == RecordStream::Additional {
                    if !positional || replaced {
                        return Err(AltiumFormatError::InvalidParamValue {
                            key: "pins".to_owned(),
                            detail: format!(
                                "pins()[{ordinal}] lives in the Additional stream; structural \
                                 pin edits touching Additional-stream pins are unsupported \
                                 (Additional pin sidecars are unproven) — such pins can only \
                                 be kept untouched at their original positions"
                            ),
                        });
                    }
                } else if !positional {
                    return Err(AltiumFormatError::InvalidParamValue {
                        key: "pins".to_owned(),
                        detail: format!(
                            "pins()[{ordinal}] must stay at final position {ordinal}: pin \
                             members living in the Additional stream freeze the collection \
                             (Additional pin sidecars are unproven), so pin \
                             insert/delete/reorder is unsupported — only positional \
                             Keep/Replace of Data pins is possible"
                        ),
                    });
                }
            }
            if finals.len() > members.len() {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "pins".to_owned(),
                    detail: format!(
                        "final pin sequence adds {} member(s), but appending pins is \
                         unsupported while Additional-stream pin members exist (Additional \
                         pin sidecars are unproven; the collection is frozen to positional \
                         Keep/Replace of Data pins)",
                        finals.len() - members.len()
                    ),
                });
            }
        }
        check_deletions(Collection::Pins, members.len(), &referenced, allow_deletion)?;
        Ok(finals)
    }

    /// Resolves a footprints edit into a [`FootprintPlan`] (module docs,
    /// "Footprints"). Returns `None` for the frozen all-positional-Keep no-op
    /// accepted while Additional-stream chain records exist.
    fn resolve_footprint_edits(
        &self,
        edits: Vec<SchFootprintEdit>,
        allow_deletion: bool,
        graphic_members: &[RecordAddress],
        graphics_edited: bool,
    ) -> Result<Option<FootprintPlan>> {
        let component = &self.component;

        // Membership mirrors footprint_maps(): PCBLIB implementations in
        // encounter order.
        let mut member_addresses = Vec::new();
        for (address, record) in all_records(component) {
            if let SchRecord::Implementation(implementation) = record {
                if implementation
                    .model_type
                    .eq_ignore_ascii_case(MODEL_TYPE_PCBLIB)
                {
                    member_addresses.push(address);
                }
            }
        }

        // Whole-subtree scope: a member's Implementation record plus every
        // transitive descendant. Owners precede children in encounter order
        // (validated graph), so one forward pass closes the set.
        let mut scope_member: HashMap<RecordAddress, usize> = member_addresses
            .iter()
            .enumerate()
            .map(|(ordinal, address)| (*address, ordinal))
            .collect();
        for (address, record) in all_records(component) {
            if scope_member.contains_key(&address) {
                continue;
            }
            if let Some(owner) = owner_of(component, address, record)? {
                if let Some(&ordinal) = scope_member.get(&owner) {
                    scope_member.insert(address, ordinal);
                }
            }
        }

        // Additional-stream chain records freeze the collection to the
        // all-positional-Keep no-op (module docs: the native exporter cannot
        // emit chain records into Additional; zero corpus occurrences).
        let chain_in_additional = component.additional_records.iter().any(|record| {
            matches!(
                record,
                SchRecord::ImplementationList(_)
                    | SchRecord::Implementation(_)
                    | SchRecord::ImplementationMap(_)
                    | SchRecord::MapDefiner(_)
                    | SchRecord::ParameterList(_)
            )
        }) || scope_member
            .keys()
            .any(|address| address.stream == RecordStream::Additional);
        if chain_in_additional {
            let positional_keep = edits.len() == member_addresses.len()
                && edits.iter().enumerate().all(
                    |(position, edit)| matches!(edit, SchFootprintEdit::Keep(i) if *i == position),
                );
            if !positional_keep {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "footprints".to_owned(),
                    detail: "implementation-chain records live in the Additional stream; \
                             structural footprint edits are unsupported there (the native \
                             exporter never emits chain records into Additional and no \
                             sidecar/order evidence exists) — the collection is frozen, so \
                             only the all-positional-Keep arrangement is accepted"
                        .to_owned(),
                });
            }
            return Ok(None);
        }

        // Per-member subtrees in encounter order; the Implementation record is
        // always first (owners precede children).
        let mut subtrees: Vec<Vec<RecordAddress>> = vec![Vec::new(); member_addresses.len()];
        let mut last_scope_address = None;
        for (address, record) in all_records(component) {
            let Some(&ordinal) = scope_member.get(&address) else {
                continue;
            };
            subtrees[ordinal].push(address);
            last_scope_address = Some(address);
            // Conflict checks: a pin inside a footprint subtree is
            // unaddressable wild state; a graphics member inside the scope
            // conflicts with a graphics edit in the same batch.
            if matches!(record, SchRecord::Pin(_)) {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "footprints".to_owned(),
                    detail: format!(
                        "footprint_maps().footprints[{ordinal}] owns a Pin record \
                         (/{:?} record[{}]); footprint edits over subtrees containing pins \
                         are unsupported (pin sidecar ordering is outside the footprint \
                         collection's contract)",
                        address.stream, address.index
                    ),
                });
            }
            if graphics_edited
                && address != member_addresses[ordinal]
                && graphic_members.contains(&address)
            {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "footprints".to_owned(),
                    detail: format!(
                        "/{:?} record[{}] is both a graphics() member and part of \
                         footprint_maps().footprints[{ordinal}]'s subtree; editing both \
                         collections in one batch conflicts over it",
                        address.stream, address.index
                    ),
                });
            }
        }

        // Shared-ImplementationList resolution for New footprints: reuse the
        // list owning the existing implementations (AD26 emits exactly one per
        // component), else the first bare list, else create one.
        let creates = edits
            .iter()
            .any(|edit| matches!(edit, SchFootprintEdit::New(_)));
        let mut next_local_id = 0usize;
        let mut created_list_entry = None;
        let list_owner = if creates {
            let mut implementation_list_owners = Vec::new();
            let mut first_bare_list = None;
            for (address, record) in all_records(component) {
                match record {
                    SchRecord::Implementation(_) => {
                        let owner = owner_of(component, address, record)?.ok_or_else(|| {
                            AltiumFormatError::InvalidParamValue {
                                key: "footprints".to_owned(),
                                detail: format!(
                                    "/{:?} record[{}] is an Implementation with no owner",
                                    address.stream, address.index
                                ),
                            }
                        })?;
                        if !implementation_list_owners.contains(&owner) {
                            implementation_list_owners.push(owner);
                        }
                    }
                    SchRecord::ImplementationList(_) if first_bare_list.is_none() => {
                        first_bare_list = Some(address);
                    }
                    _ => {}
                }
            }
            if implementation_list_owners.len() > 1 {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "footprints".to_owned(),
                    detail: format!(
                        "existing implementations are owned by {} distinct \
                         ImplementationList records; the native model has exactly one \
                         shared list per component, so this wild state must be resolved \
                         before footprints can be created",
                        implementation_list_owners.len()
                    ),
                });
            }
            match implementation_list_owners
                .first()
                .copied()
                .or(first_bare_list)
            {
                Some(address) => PlannedOwner::Original(address),
                None => {
                    // AD26 creates the synthetic list unconditionally per
                    // component (SchDataComponent.AddToListForSave); its ctor
                    // hard-codes owner_part_id 0 and IndexInSheet 0
                    // (Objects/SchDataImplementationList.cs:9-14; fixture
                    // bytes: bare `|RECORD=44`).
                    let record = SchRecord::ImplementationList(
                        SchImplementationList::from_params(&mut ParameterCollection::new())
                            .context("creating ImplementationList from typed wire defaults")?,
                    );
                    let local_id = next_local_id;
                    next_local_id += 1;
                    created_list_entry = Some(PlannedEntry::Created {
                        local_id,
                        record,
                        owner: PlannedOwner::Root,
                    });
                    PlannedOwner::Created(local_id)
                }
            }
        } else {
            PlannedOwner::Root // unused
        };

        // Resolve the final sequence.
        let mut referenced = HashSet::new();
        let mut final_entries = Vec::with_capacity(edits.len());
        let mut final_currents = Vec::with_capacity(edits.len());
        let mut active_current_writes = false;
        let mut replacements = Vec::new();
        for (position, edit) in edits.into_iter().enumerate() {
            match edit {
                SchFootprintEdit::Keep(index) => {
                    let address = resolve_member(
                        Collection::Footprints,
                        &member_addresses,
                        index,
                        position,
                        &mut referenced,
                    )?;
                    let ordinal = scope_member[&address];
                    final_currents.push(footprint_is_current(component, address));
                    final_entries.push(
                        subtrees[ordinal]
                            .iter()
                            .map(|address| PlannedEntry::Original(*address))
                            .collect::<Vec<_>>(),
                    );
                }
                SchFootprintEdit::Replace(index, init) => {
                    let address = resolve_member(
                        Collection::Footprints,
                        &member_addresses,
                        index,
                        position,
                        &mut referenced,
                    )?;
                    let ordinal = scope_member[&address];
                    active_current_writes |=
                        init.is_current != footprint_is_current(component, address);
                    final_currents.push(init.is_current);
                    let entries = self
                        .replace_footprint(
                            address,
                            &subtrees[ordinal],
                            init,
                            &mut replacements,
                            &mut next_local_id,
                        )
                        .with_context(|| {
                            format!(
                                "replacing footprint_maps().footprints[{index}] (final \
                                 position {position})"
                            )
                        })?;
                    final_entries.push(entries);
                }
                SchFootprintEdit::New(init) => {
                    active_current_writes |= init.is_current;
                    final_currents.push(init.is_current);
                    let entries = self
                        .build_footprint(init, list_owner, &mut next_local_id)
                        .with_context(|| {
                            format!("creating footprint at final position {position}")
                        })?;
                    final_entries.push(entries);
                }
            }
        }
        check_deletions(
            Collection::Footprints,
            member_addresses.len(),
            &referenced,
            allow_deletion,
        )?;

        // Single-current invariant over the final PCBLIB set (module docs,
        // "is_current"): a batch may never BREAK a coherent input, nor author
        // an active current claim (a changed Replace value, or a New with
        // is_current true) into an incoherent final state. Pre-existing wild
        // states pass through verbatim when no current value is touched —
        // AD26's own load repair of them is unchanged by such an edit.
        let input_currents = member_addresses
            .iter()
            .filter(|address| footprint_is_current(component, **address))
            .count();
        let input_coherent = member_addresses.is_empty() || input_currents == 1;
        let final_current_count = final_currents.iter().filter(|current| **current).count();
        let final_incoherent = !final_currents.is_empty() && final_current_count != 1;
        if final_incoherent && (input_coherent || active_current_writes) {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "footprints".to_owned(),
                detail: format!(
                    "the final footprint set has {final_current_count} current footprints \
                     across {} members; the native model keeps exactly one current \
                     implementation per model type (AD26 \
                     ValidateImplementationsCurrentState) — author is_current explicitly \
                     so exactly one footprint is current (a pre-existing wild current \
                     state passes through only when the batch writes no current value)",
                    final_currents.len()
                ),
            });
        }

        // Slot the finals: entries for final position m sit at original member
        // m's Implementation address; extras follow the last scope record (or
        // the Data end when no members existed), preceded by a created list.
        let mut finals: Vec<Option<Vec<PlannedEntry>>> = final_entries
            .drain(..member_addresses.len().min(final_entries.len()))
            .map(Some)
            .collect();
        finals.resize_with(member_addresses.len(), || None);
        let mut extras: Vec<PlannedEntry> = created_list_entry.into_iter().collect();
        extras.extend(final_entries.into_iter().flatten());

        Ok(Some(FootprintPlan {
            scope: scope_member.keys().copied().collect(),
            slots: member_addresses
                .iter()
                .enumerate()
                .map(|(ordinal, address)| (*address, ordinal))
                .collect(),
            finals,
            extras,
            last_scope_address,
            replacements,
            next_local_id,
        }))
    }

    /// Field-level footprint Replace: writes exactly the addressed axes into a
    /// clone of the original Implementation record (recorded for the working
    /// copy) and rebuilds the MapDefiner records when `pin_pad_maps` is
    /// addressed; every other subtree record is preserved verbatim.
    fn replace_footprint(
        &self,
        implementation: RecordAddress,
        subtree: &[RecordAddress],
        init: SchFootprintInit,
        replacements: &mut Vec<(RecordAddress, SchRecord)>,
        next_local_id: &mut usize,
    ) -> Result<Vec<PlannedEntry>> {
        let component = &self.component;
        let SchRecord::Implementation(original) = record_slot(component, implementation) else {
            unreachable!("footprint members are Implementation records")
        };
        let mut replaced = original.clone();
        replaced.model_name = init.model_name;
        replaced.description = init.description;
        replaced.is_current = init.is_current;
        // The stored model_type (casing included), unique_id, datafile links,
        // GUIDs, locks, and location are preserved verbatim.
        check_style_id_write(Some(original.base.style_id), replaced.base.style_id)?;
        replacements.push((implementation, SchRecord::Implementation(replaced)));

        let Some(maps) = init.pin_pad_maps else {
            // Unaddressed: the whole subtree (definers included) is preserved.
            return Ok(subtree.iter().map(|a| PlannedEntry::Original(*a)).collect());
        };
        check_pin_pad_maps(&maps)?;

        // The rebuild target is the implementation's single ImplementationMap.
        let mut implementation_maps: Vec<RecordAddress> = Vec::new();
        for address in subtree {
            if matches!(
                record_slot(component, *address),
                SchRecord::ImplementationMap(_)
            ) && owner_address_of(component, *address)? == Some(implementation)
            {
                implementation_maps.push(*address);
            }
        }
        if implementation_maps.len() > 1 {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "footprints".to_owned(),
                detail: format!(
                    "the implementation owns {} ImplementationMap records; the native model \
                     has exactly one map per implementation, so pin_pad_maps cannot be \
                     rebuilt over this wild state (leave pin_pad_maps unaddressed to \
                     preserve it verbatim)",
                    implementation_maps.len()
                ),
            });
        }
        let target_map = implementation_maps.first().copied();

        // Original definers of the target map are replaced; a definer owning
        // descendants is unaddressable wild state (deleting it would orphan
        // or silently delete records the axes never addressed).
        let mut doomed_definers = HashSet::new();
        if let Some(map_address) = target_map {
            for address in subtree {
                if matches!(record_slot(component, *address), SchRecord::MapDefiner(_))
                    && owner_address_of(component, *address)?.as_ref() == Some(&map_address)
                {
                    doomed_definers.insert(*address);
                }
            }
            for address in subtree {
                if let Some(owner) = owner_address_of(component, *address)? {
                    if doomed_definers.contains(&owner) {
                        return Err(AltiumFormatError::InvalidParamValue {
                            key: "footprints".to_owned(),
                            detail: format!(
                                "MapDefiner /{:?} record[{}] owns descendants; rebuilding \
                                 pin_pad_maps would delete records the addressed axes never \
                                 covered (leave pin_pad_maps unaddressed to preserve them)",
                                owner.stream, owner.index
                            ),
                        });
                    }
                }
            }
        }

        let mut entries = Vec::new();
        for address in subtree {
            if doomed_definers.contains(address) {
                continue;
            }
            entries.push(PlannedEntry::Original(*address));
            if *address == implementation && target_map.is_none() && !maps.is_empty() {
                // No map exists: materialize one right after the
                // implementation (native subtree order), then the definers.
                let map_id = *next_local_id;
                *next_local_id += 1;
                entries.push(PlannedEntry::Created {
                    local_id: map_id,
                    record: build_implementation_map()?,
                    owner: PlannedOwner::Original(implementation),
                });
                for map in &maps {
                    let local_id = *next_local_id;
                    *next_local_id += 1;
                    entries.push(PlannedEntry::Created {
                        local_id,
                        record: build_map_definer(map)?,
                        owner: PlannedOwner::Created(map_id),
                    });
                }
            }
            if Some(*address) == target_map {
                for map in &maps {
                    let local_id = *next_local_id;
                    *next_local_id += 1;
                    entries.push(PlannedEntry::Created {
                        local_id,
                        record: build_map_definer(map)?,
                        owner: PlannedOwner::Original(*address),
                    });
                }
            }
        }
        Ok(entries)
    }

    /// Materializes the full native chain for a new footprint: Implementation,
    /// ImplementationMap, one MapDefiner per pin-pad map, and ParameterList
    /// (module docs, "Footprints" — creation evidence).
    fn build_footprint(
        &self,
        init: SchFootprintInit,
        list_owner: PlannedOwner,
        next_local_id: &mut usize,
    ) -> Result<Vec<PlannedEntry>> {
        if init.model_name.is_empty() {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "ModelName".to_owned(),
                detail: "a new footprint requires a non-empty model name (the native \
                         default datafile link derives its entity name from it)"
                    .to_owned(),
            });
        }
        let maps = init.pin_pad_maps.as_deref().unwrap_or(&[]);
        check_pin_pad_maps(maps)?;

        let mut implementation = SchImplementation::from_params(&mut ParameterCollection::new())
            .context("creating Implementation from typed wire defaults")?;
        implementation.model_name = init.model_name.clone();
        implementation.model_type = MODEL_TYPE_PCBLIB.to_owned();
        implementation.description = init.description;
        implementation.is_current = init.is_current;
        implementation.unique_id = generate_unique_id();
        // The native single default datafile link for a PCBLIB implementation
        // (EngineObjects/SchImplementation.cs HandleAfterImportEvent; every
        // corpus footprint carries exactly this shape).
        implementation.datafile_links = vec![ModelDatafileLink {
            location: String::new(),
            entity_name: init.model_name,
            file_kind: MODEL_DATAFILE_KIND_PCBLIB.to_owned(),
        }];
        // Chain sentinels: owner_part_id 0 (memo §1) and the Implementation's
        // INDEXINSHEET=-1 (fixture bytes; the native IndexOf-mismatch).
        implementation.base.owner_part_id = 0;
        implementation.base.index_in_sheet = -1;
        check_style_id_write(None, implementation.base.style_id)?;

        let implementation_id = *next_local_id;
        *next_local_id += 1;
        let mut entries = vec![PlannedEntry::Created {
            local_id: implementation_id,
            record: SchRecord::Implementation(implementation),
            owner: list_owner,
        }];
        let map_id = *next_local_id;
        *next_local_id += 1;
        entries.push(PlannedEntry::Created {
            local_id: map_id,
            record: build_implementation_map()?,
            owner: PlannedOwner::Created(implementation_id),
        });
        for map in maps {
            let local_id = *next_local_id;
            *next_local_id += 1;
            entries.push(PlannedEntry::Created {
                local_id,
                record: build_map_definer(map)?,
                owner: PlannedOwner::Created(map_id),
            });
        }
        let parameter_list_id = *next_local_id;
        *next_local_id += 1;
        let parameter_list = SchParameterList::from_params(&mut ParameterCollection::new())
            .context("creating ParameterList from typed wire defaults")?;
        entries.push(PlannedEntry::Created {
            local_id: parameter_list_id,
            record: SchRecord::ParameterList(parameter_list),
            owner: PlannedOwner::Created(implementation_id),
        });
        Ok(entries)
    }

    /// Builds a new root-owned pin from typed wire defaults plus the init's
    /// addressed fields. No UniqueId is generated: the native SchLib binary
    /// pin body carries none, so an assigned id could never survive a native
    /// save/reopen (documented deviation from the graphics creation rule).
    fn build_pin(&self, init: SchPinInit) -> Result<SchRecord> {
        let mut pin = parse_text_pin(&mut ParameterCollection::new())
            .context("creating pin from typed wire defaults")?;
        // SchLib pins serialize as binary records, whose bodies always carry
        // the four IEEE symbol fields; the presence flags are SchDoc text-pin
        // omission artifacts. Mark them present so the created state is the
        // binary writer's save/reopen fixed point.
        pin.symbol_inner_edge_present = true;
        pin.symbol_outer_edge_present = true;
        pin.symbol_inside_present = true;
        pin.symbol_outside_present = true;
        self.apply_pin_init(&mut pin, init, EditMode::Create)?;
        Ok(SchRecord::Pin(pin))
    }

    /// Writes exactly the view-addressed fields of `init` into `pin` (memo
    /// §1/§4/§5/§7 rules): owner part and display mode validated at the
    /// mutation boundary, combined swap halves preserved or re-encoded per
    /// memo §5 rule 3, hidden_net_name guarded, and every other typed field —
    /// sidecar-backed fields included — left untouched. Never touches the
    /// component's stored `all_pin_count` (memo §7).
    fn apply_pin_init(&self, pin: &mut SchPin, init: SchPinInit, mode: EditMode) -> Result<()> {
        let root = &self.component.component;
        check_written_owner_part_id(
            WrittenOwnerPartClass::Pin,
            init.owner_part_id,
            root.part_count,
        )?;
        pin.owner_part_id = init.owner_part_id;
        match (init.owner_part_display_mode, mode) {
            (None, EditMode::Replace) => {}
            (authored, _) => {
                let value = authored.unwrap_or(0);
                check_written_display_mode("OwnerPartDisplayMode", value, root.display_mode_count)?;
                pin.owner_part_display_mode =
                    u8::try_from(value).map_err(|_| AltiumFormatError::InvalidParamValue {
                        key: "OwnerPartDisplayMode".to_owned(),
                        detail: format!(
                            "display mode {value} exceeds the native byte range of the \
                             binary pin field"
                        ),
                    })?;
            }
        }
        let original_hidden_net = match mode {
            EditMode::Replace => Some(pin.hidden_net_name.clone()),
            EditMode::Create => None,
        };
        check_pin_hidden_net_name_write(original_hidden_net.as_deref(), &init.hidden_net_name)?;
        pin.hidden_net_name = init.hidden_net_name;
        // Coordinate range note: the binary pin body stores length/location as
        // i16 counts of DXP base units with the remainder in the PinFrac
        // sidecar; |i32::MAX| / C_BASE_UNIT = 21,474 < i16::MAX, so every
        // representable Coord fits and no range check is needed.
        pin.designator = init.designator;
        pin.name = init.name;
        pin.electrical = init.electrical;
        pin.pin_length = init.length;
        pin.location = init.location;
        pin.orientation = init.orientation;
        pin.is_hidden = init.is_hidden;
        pin.swap_id_pin = init.swap_group;
        pin.swap_id_pair = init.pair_swap_group;
        match (init.part_swap_group, init.part_pin_swap_suffix) {
            // Both halves unaddressed: the stored combined string stays
            // byte-verbatim (Replace) or keeps the empty wire default (Create).
            (None, None) => {}
            (prefix, suffix) => {
                let combined = {
                    let (original_prefix, original_suffix) =
                        split_swap_id_part_and_part_pin(&pin.swap_id_part);
                    let prefix = prefix.unwrap_or_else(|| original_prefix.to_owned());
                    let suffix = suffix.unwrap_or_else(|| original_suffix.unwrap_or("").to_owned());
                    encode_swap_id_part_and_part_pin(&prefix, &suffix)
                };
                pin.swap_id_part = combined;
            }
        }
        Ok(())
    }

    /// Builds a new root-owned parameter with typed wire defaults, a fresh
    /// UniqueId, owner_part_id −1 (memo §2), display mode 0, and the default
    /// font resolved against existing captured table entries.
    fn build_parameter(&self, init: SchParameterInit, position: usize) -> Result<SchRecord> {
        let mut record = SchParameter::from_params(&mut ParameterCollection::new())
            .with_context(|| format!("creating parameter at final position {position}"))?;
        record.name = init.name;
        record.text = init.text;
        record.is_hidden = init.is_hidden;
        record.unique_id = generate_unique_id();
        record.base.owner_part_id = -1;
        check_written_owner_part_id(
            WrittenOwnerPartClass::RootText,
            record.base.owner_part_id,
            self.component.component.part_count,
        )
        .with_context(|| format!("creating parameter at final position {position}"))?;
        record.font_id = self
            .default_font_id(
                "SchParameterInit has no font field, so append the default font through a \
                 library edit (SchLibSnapshot::with_library_edit font_appends) and re-take \
                 the component snapshot from the edited library first",
            )
            .with_context(|| format!("creating parameter at final position {position}"))?;
        check_style_id_write(None, record.base.style_id)?;
        // A root parameter named "Comment" is the native comment FIELD OBJECT:
        // it never lives in the owner's objectList, so it takes −1 instead of
        // a sibling ordinal (memo §6 rule 1; SchDataParameter.UpdateOwner).
        if record.name.eq_ignore_ascii_case("Comment") {
            record.base.index_in_sheet = -1;
        }
        Ok(SchRecord::Parameter(record))
    }

    /// Resolves the font for a text-bearing graphic: `Some(id)` must reference
    /// an existing captured table entry; `None` preserves `current` (the
    /// original record's stored font) on Replace and resolves the native
    /// default font on Create.
    fn edited_font_id(&self, authored: Option<i32>, current: i32, mode: EditMode) -> Result<i32> {
        match (authored, mode) {
            (Some(id), _) => {
                check_font_id_exists(&self.reference_context.fonts, id)?;
                Ok(id)
            }
            (None, EditMode::Replace) => Ok(current),
            (None, EditMode::Create) => {
                self.default_font_id("author an explicit font_id from the existing table instead")
            }
        }
    }

    /// Resolves the native default font (Times New Roman 10) against EXISTING
    /// captured table entries only — a required append is a library-level
    /// namespace change that must be DECLARED through
    /// `SchLibSnapshot::with_library_edit` font appends (which recompute the
    /// reference context); it hard-errors here with caller-specific advice
    /// (parameters have no authorable font field, so their advice cannot
    /// suggest one).
    fn default_font_id(&self, advice: &str) -> Result<i32> {
        let fonts = &self.reference_context.fonts;
        let mut probe = fonts.clone();
        let id = find_or_create_default_font(&mut probe)
            .context("resolving the default font for a new text-bearing child")?;
        if probe.len() != fonts.len() {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "FontId".to_owned(),
                detail: format!(
                    "the captured font table ({} entries) has no default-font entry \
                     (Times New Roman 10); appending one is a library-level namespace \
                     change that must be declared through SchLibSnapshot::with_library_edit \
                     — {advice}",
                    fonts.len()
                ),
            });
        }
        Ok(id)
    }

    /// Validates the graphic init's owner/display/reference fields against
    /// this snapshot (memo §2/§4/§8a/§8c). A `None` display mode on Replace is
    /// not validated: it preserves the original's stored value verbatim
    /// (capture-forbidden wild values included).
    fn check_graphic_init(&self, init: &SchGraphicInit, mode: EditMode) -> Result<()> {
        let root = &self.component.component;
        check_written_owner_part_id(
            WrittenOwnerPartClass::Graphic,
            init.owner_part_id,
            root.part_count,
        )?;
        match (init.owner_part_display_mode, mode) {
            (None, EditMode::Replace) => {}
            (authored, _) => check_written_display_mode(
                "OwnerPartDisplayMode",
                authored.unwrap_or(0),
                root.display_mode_count,
            )?,
        }
        if let SchGraphicShape::Image {
            file_name,
            embed_image,
            ..
        } = &init.shape
        {
            check_embedded_image_reference(
                self.reference_context
                    .image_assets
                    .iter()
                    .map(|(name, _)| name.as_str()),
                file_name,
                *embed_image,
            )?;
        }
        Ok(())
    }

    /// Builds a new root-owned graphic record from an init: typed wire
    /// defaults, a fresh UniqueId where the kind carries one, and the
    /// validated owner/display assignment.
    fn build_graphic(&self, init: SchGraphicInit) -> Result<SchRecord> {
        self.check_graphic_init(&init, EditMode::Create)?;
        let mut collection = ParameterCollection::new();
        if matches!(
            &init.shape,
            SchGraphicShape::Polyline { .. }
                | SchGraphicShape::Polygon { .. }
                | SchGraphicShape::Bezier { .. }
        ) {
            // The indexed-coordinate kinds require the vertex-count key even
            // on creation; apply_shape below writes the real vertex list.
            collection.insert(
                altium_format_types::constants::visual::LOCATION_COUNT,
                "0".to_owned(),
            );
        }
        let mut record = match &init.shape {
            SchGraphicShape::Line { .. } => SchRecord::Line(SchLine::from_params(&mut collection)?),
            SchGraphicShape::Rectangle { .. } => {
                SchRecord::Rectangle(SchRectangle::from_params(&mut collection)?)
            }
            SchGraphicShape::RoundRectangle { .. } => {
                SchRecord::RoundRectangle(SchRoundRectangle::from_params(&mut collection)?)
            }
            SchGraphicShape::Arc { .. } => SchRecord::Arc(SchArc::from_params(&mut collection)?),
            SchGraphicShape::EllipticalArc { .. } => {
                SchRecord::EllipticalArc(SchEllipticalArc::from_params(&mut collection)?)
            }
            SchGraphicShape::Ellipse { .. } => {
                SchRecord::Ellipse(SchEllipse::from_params(&mut collection)?)
            }
            SchGraphicShape::Pie { .. } => SchRecord::Pie(SchPie::from_params(&mut collection)?),
            SchGraphicShape::Polyline { .. } => {
                SchRecord::Polyline(SchPolyline::from_params(&mut collection)?)
            }
            SchGraphicShape::Polygon { .. } => {
                SchRecord::Polygon(SchPolygon::from_params(&mut collection)?)
            }
            SchGraphicShape::Bezier { .. } => {
                SchRecord::Bezier(SchBezier::from_params(&mut collection)?)
            }
            SchGraphicShape::Image { .. } => {
                SchRecord::Image(SchImage::from_params(&mut collection)?)
            }
            SchGraphicShape::Label { .. } => {
                SchRecord::Label(SchLabel::from_params(&mut collection)?)
            }
            SchGraphicShape::TextFrame { .. } => {
                SchRecord::TextFrame(SchTextFrame::from_params(&mut collection)?)
            }
        };
        self.apply_shape(&mut record, &init.shape, EditMode::Create)?;
        set_fresh_unique_id(&mut record);
        let base = record
            .base_mut()
            .expect("all representable graphic kinds carry a primitive base");
        base.owner_part_id = init.owner_part_id;
        base.owner_part_display_mode = init.owner_part_display_mode.unwrap_or(0);
        check_style_id_write(None, base.style_id)?;
        Ok(record)
    }

    /// Clones an original graphic member and replaces exactly the
    /// intent-addressable fields; the target's kind must equal the
    /// replacement's kind (no cross-kind mapping exists).
    fn replace_graphic(&self, original: &SchRecord, init: SchGraphicInit) -> Result<SchRecord> {
        let original_kind = graphic_kind_name(original);
        if original_kind != init.shape.kind_name() {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "graphic kind".to_owned(),
                detail: format!(
                    "Replace target kind {original_kind} does not match replacement kind {}; \
                     cross-kind positional replacement has no mapping (delete with \
                     authorization and create instead)",
                    init.shape.kind_name()
                ),
            });
        }
        self.check_graphic_init(&init, EditMode::Replace)?;
        let mut record = original.clone();
        self.apply_shape(&mut record, &init.shape, EditMode::Replace)?;
        let original_style = original
            .base()
            .expect("representable graphic kinds carry a primitive base")
            .style_id;
        let base = record
            .base_mut()
            .expect("representable graphic kinds carry a primitive base");
        base.owner_part_id = init.owner_part_id;
        if let Some(mode) = init.owner_part_display_mode {
            base.owner_part_display_mode = mode;
        }
        check_style_id_write(Some(original_style), base.style_id)?;
        Ok(record)
    }

    /// Writes exactly the view-addressed fields of `shape` into `record`
    /// (which must be of the matching kind). Every other typed field is left
    /// untouched; a `None` font on Replace preserves the record's stored font.
    fn apply_shape(
        &self,
        record: &mut SchRecord,
        shape: &SchGraphicShape,
        mode: EditMode,
    ) -> Result<()> {
        match (record, shape) {
            (
                SchRecord::Line(record),
                SchGraphicShape::Line {
                    from,
                    to,
                    line_width,
                    line_style,
                    color,
                },
            ) => {
                record.location = *from;
                record.corner = *to;
                record.line_width = *line_width;
                record.line_style = *line_style;
                record.color = *color;
            }
            (
                SchRecord::Rectangle(record),
                SchGraphicShape::Rectangle {
                    from,
                    to,
                    line_width,
                    line_style,
                    color,
                    area_color,
                    is_solid,
                    transparent,
                },
            ) => {
                record.location = *from;
                record.corner = *to;
                record.line_width = *line_width;
                record.line_style = *line_style;
                record.color = *color;
                record.area_color = *area_color;
                record.is_solid = *is_solid;
                record.transparent = *transparent;
            }
            (
                SchRecord::RoundRectangle(record),
                SchGraphicShape::RoundRectangle {
                    from,
                    to,
                    corner_x_radius,
                    corner_y_radius,
                    line_width,
                    color,
                    area_color,
                    is_solid,
                },
            ) => {
                record.location = *from;
                record.corner = *to;
                record.corner_x_radius = *corner_x_radius;
                record.corner_y_radius = *corner_y_radius;
                record.line_width = *line_width;
                record.color = *color;
                record.area_color = *area_color;
                record.is_solid = *is_solid;
            }
            (
                SchRecord::Arc(record),
                SchGraphicShape::Arc {
                    center,
                    radius,
                    line_width,
                    start_angle,
                    end_angle,
                    color,
                },
            ) => {
                record.location = *center;
                record.radius = *radius;
                record.line_width = *line_width;
                record.start_angle = SchAngle(*start_angle);
                record.end_angle = end_angle.map(SchAngle);
                record.color = *color;
            }
            (
                SchRecord::EllipticalArc(record),
                SchGraphicShape::EllipticalArc {
                    center,
                    radius,
                    secondary_radius,
                    line_width,
                    start_angle,
                    end_angle,
                    color,
                },
            ) => {
                record.location = *center;
                record.radius = *radius;
                record.secondary_radius = *secondary_radius;
                record.line_width = *line_width;
                record.start_angle = SchAngle(*start_angle);
                record.end_angle = end_angle.map(SchAngle);
                record.color = *color;
            }
            (
                SchRecord::Ellipse(record),
                SchGraphicShape::Ellipse {
                    center,
                    radius,
                    secondary_radius,
                    line_width,
                    color,
                    area_color,
                    is_solid,
                    transparent,
                },
            ) => {
                record.location = *center;
                record.radius = *radius;
                record.secondary_radius = *secondary_radius;
                record.line_width = *line_width;
                record.color = *color;
                record.area_color = *area_color;
                record.is_solid = *is_solid;
                record.transparent = *transparent;
            }
            (
                SchRecord::Pie(record),
                SchGraphicShape::Pie {
                    center,
                    radius,
                    line_width,
                    start_angle,
                    end_angle,
                    color,
                    area_color,
                    is_solid,
                },
            ) => {
                record.location = *center;
                record.radius = *radius;
                record.line_width = *line_width;
                record.start_angle = SchAngle(*start_angle);
                record.end_angle = end_angle.map(SchAngle);
                record.color = *color;
                record.area_color = *area_color;
                record.is_solid = *is_solid;
            }
            (
                SchRecord::Polyline(record),
                SchGraphicShape::Polyline {
                    vertices,
                    line_width,
                    line_style,
                    start_line_shape,
                    end_line_shape,
                    line_shape_size,
                    color,
                },
            ) => {
                record.vertices = vertices.clone();
                record.line_width = *line_width;
                record.line_style = *line_style;
                record.start_line_shape = *start_line_shape;
                record.end_line_shape = *end_line_shape;
                record.line_shape_size = *line_shape_size;
                record.color = *color;
            }
            (
                SchRecord::Polygon(record),
                SchGraphicShape::Polygon {
                    vertices,
                    line_width,
                    color,
                    area_color,
                    is_solid,
                    transparent,
                },
            ) => {
                record.vertices = vertices.clone();
                record.line_width = *line_width;
                record.color = *color;
                record.area_color = *area_color;
                record.is_solid = *is_solid;
                record.transparent = *transparent;
            }
            (
                SchRecord::Bezier(record),
                SchGraphicShape::Bezier {
                    vertices,
                    line_width,
                    color,
                },
            ) => {
                record.vertices = vertices.clone();
                record.line_width = *line_width;
                record.color = *color;
            }
            (
                SchRecord::Image(record),
                SchGraphicShape::Image {
                    from,
                    to,
                    orientation,
                    line_width,
                    color,
                    is_solid,
                    keep_aspect,
                    embed_image,
                    file_name,
                },
            ) => {
                record.location = *from;
                record.corner = *to;
                record.orientation = *orientation;
                record.line_width = *line_width;
                record.color = *color;
                record.is_solid = *is_solid;
                record.keep_aspect = *keep_aspect;
                record.embed_image = *embed_image;
                record.file_name = file_name.clone();
            }
            (
                SchRecord::Label(record),
                SchGraphicShape::Label {
                    at,
                    orientation,
                    justification,
                    color,
                    font_id,
                    text,
                    is_mirrored,
                    url,
                },
            ) => {
                record.location = *at;
                record.orientation = *orientation;
                record.justification = *justification;
                record.color = *color;
                record.font_id = self.edited_font_id(*font_id, record.font_id, mode)?;
                record.text = text.clone();
                record.is_mirrored = *is_mirrored;
                record.url = url.clone();
            }
            (
                SchRecord::TextFrame(record),
                SchGraphicShape::TextFrame {
                    from,
                    to,
                    line_width,
                    color,
                    area_color,
                    text_color,
                    font_id,
                    is_solid,
                    show_border,
                    alignment,
                    word_wrap,
                    clip_to_rect,
                    text,
                    text_margin,
                    transparent,
                },
            ) => {
                record.location = *from;
                record.corner = *to;
                record.line_width = *line_width;
                record.color = *color;
                record.area_color = *area_color;
                record.text_color = *text_color;
                record.font_id = self.edited_font_id(*font_id, record.font_id, mode)?;
                record.is_solid = *is_solid;
                record.show_border = *show_border;
                record.alignment = *alignment;
                record.word_wrap = *word_wrap;
                record.clip_to_rect = *clip_to_rect;
                record.text = text.clone();
                record.text_margin = *text_margin;
                record.transparent = *transparent;
            }
            (record, shape) => {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "graphic kind".to_owned(),
                    detail: format!(
                        "internal kind mismatch applying {} shape to {} record",
                        shape.kind_name(),
                        graphic_kind_name(record)
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Native PCBLIB model-type string (`ModelTypePCBLIB = "PCBLIB"`,
/// EngineObjects/SchImplementation.cs:31). Local to altium-format for now —
/// FLAGGED for promotion into `altium-format-types::constants` after user
/// discussion (same class as the `"|&|"` swap separator).
const MODEL_TYPE_PCBLIB: &str = "PCBLIB";
/// Native default datafile-link kind for a PCBLIB implementation (exact
/// casing; EngineObjects/SchImplementation.cs `HandleAfterImportEvent` and the
/// corpus wire bytes `MODELDATAFILEKIND0=PCBLib`). Same promotion flag.
const MODEL_DATAFILE_KIND_PCBLIB: &str = "PCBLib";

/// Stored `is_current` of a footprint member's Implementation record.
fn footprint_is_current(component: &SchLibComponent, address: RecordAddress) -> bool {
    match record_slot(component, address) {
        SchRecord::Implementation(implementation) => implementation.is_current,
        _ => unreachable!("footprint members are Implementation records"),
    }
}

/// Owner address of the record at `address`.
fn owner_address_of(
    component: &SchLibComponent,
    address: RecordAddress,
) -> Result<Option<RecordAddress>> {
    owner_of(component, address, record_slot(component, address))
}

/// Rejects duplicate pin designators (case-insensitive): AD26's
/// `AddMapDefiner` silently DROPS a definer whose designator interface is
/// already mapped (SchDataImplementationMap.cs, OrdinalIgnoreCase), so
/// serializing duplicates would be silent data loss on the next AD26 load.
fn check_pin_pad_maps(maps: &[SchPinPadMapInit]) -> Result<()> {
    let mut seen = HashSet::new();
    for map in maps {
        if !seen.insert(map.pin.to_ascii_lowercase()) {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "DesIntf".to_owned(),
                detail: format!(
                    "pin_pad_maps maps pin designator '{}' more than once \
                     (case-insensitive); AD26 silently drops duplicate map definers on \
                     load, so writing them would lose data",
                    map.pin
                ),
            });
        }
    }
    Ok(())
}

/// New ImplementationMap with the native ctor state: owner_part_id 0 and
/// IndexInSheet 0 (SchDataImplementationMap.cs ctor; fixture bytes: bare
/// `|RECORD=46|OWNERINDEX=n`), no UniqueId.
fn build_implementation_map() -> Result<SchRecord> {
    let map = SchImplementationMap::from_params(&mut ParameterCollection::new())
        .context("creating ImplementationMap from typed wire defaults")?;
    Ok(SchRecord::ImplementationMap(map))
}

/// New MapDefiner for one authored pin-pad map. owner_part_id stays 0 (the
/// chain hard-codes it, memo §1); IndexInSheet is assigned by the memo-§6
/// touched-owner renumbering — definers ARE objectList children of their map
/// and carry 0-based sibling ordinals natively (wild bytes: Custom.SchLib
/// `RECORD=47 … INDEXINSHEET=1`).
fn build_map_definer(map: &SchPinPadMapInit) -> Result<SchRecord> {
    let mut definer = SchMapDefiner::from_params(&mut ParameterCollection::new())
        .context("creating MapDefiner from typed wire defaults")?;
    definer.des_intf = map.pin.clone();
    definer.des_imps = map.pads.clone();
    Ok(SchRecord::MapDefiner(definer))
}

fn graphic_kind_name(record: &SchRecord) -> &'static str {
    match record {
        SchRecord::Line(_) => "Line",
        SchRecord::Rectangle(_) => "Rectangle",
        SchRecord::RoundRectangle(_) => "RoundRectangle",
        SchRecord::Arc(_) => "Arc",
        SchRecord::EllipticalArc(_) => "EllipticalArc",
        SchRecord::Ellipse(_) => "Ellipse",
        SchRecord::Pie(_) => "Pie",
        SchRecord::Polyline(_) => "Polyline",
        SchRecord::Polygon(_) => "Polygon",
        SchRecord::Bezier(_) => "Bezier",
        SchRecord::Image(_) => "Image",
        SchRecord::Label(_) => "Label",
        SchRecord::TextFrame(_) => "TextFrame",
        SchRecord::Symbol(_) => "Symbol",
        SchRecord::Hyperlink(_) => "Hyperlink",
        _ => "unrepresentable",
    }
}

fn set_fresh_unique_id(record: &mut SchRecord) {
    // Pie is the only representable kind without a UniqueId field.
    match record {
        SchRecord::Line(r) => r.unique_id = generate_unique_id(),
        SchRecord::Rectangle(r) => r.unique_id = generate_unique_id(),
        SchRecord::RoundRectangle(r) => r.unique_id = generate_unique_id(),
        SchRecord::Arc(r) => r.unique_id = generate_unique_id(),
        SchRecord::EllipticalArc(r) => r.unique_id = generate_unique_id(),
        SchRecord::Ellipse(r) => r.unique_id = generate_unique_id(),
        SchRecord::Polyline(r) => r.unique_id = generate_unique_id(),
        SchRecord::Polygon(r) => r.unique_id = generate_unique_id(),
        SchRecord::Bezier(r) => r.unique_id = generate_unique_id(),
        SchRecord::Image(r) => r.unique_id = generate_unique_id(),
        SchRecord::Label(r) => r.unique_id = generate_unique_id(),
        SchRecord::TextFrame(r) => r.unique_id = generate_unique_id(),
        _ => {}
    }
}

fn all_records(component: &SchLibComponent) -> impl Iterator<Item = (RecordAddress, &SchRecord)> {
    let data = component.records.iter().enumerate().map(|(index, record)| {
        (
            RecordAddress {
                stream: RecordStream::Data,
                index,
            },
            record,
        )
    });
    let additional = component
        .additional_records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            (
                RecordAddress {
                    stream: RecordStream::Additional,
                    index,
                },
                record,
            )
        });
    data.chain(additional)
}

fn record_slot(component: &SchLibComponent, address: RecordAddress) -> &SchRecord {
    match address.stream {
        RecordStream::Data => &component.records[address.index],
        RecordStream::Additional => &component.additional_records[address.index],
    }
}

fn record_slot_mut(component: &mut SchLibComponent, address: RecordAddress) -> &mut SchRecord {
    match address.stream {
        RecordStream::Data => &mut component.records[address.index],
        RecordStream::Additional => &mut component.additional_records[address.index],
    }
}

/// Addresses of `graphics()` members in encounter order, mirroring the
/// projection's membership exactly (including intent-unrepresentable kinds).
fn graphic_member_addresses(component: &SchLibComponent) -> Vec<RecordAddress> {
    all_records(component)
        .filter(|(_, record)| is_graphic_like(record))
        .map(|(address, _)| address)
        .collect()
}

/// Addresses of `pins()` members in encounter order, mirroring the projection's
/// membership exactly.
fn pin_member_addresses(component: &SchLibComponent) -> Vec<RecordAddress> {
    all_records(component)
        .filter(|(_, record)| matches!(record, SchRecord::Pin(_)))
        .map(|(address, _)| address)
        .collect()
}

fn resolve_member(
    collection: Collection,
    members: &[RecordAddress],
    index: usize,
    position: usize,
    referenced: &mut HashSet<usize>,
) -> Result<RecordAddress> {
    let address =
        members
            .get(index)
            .copied()
            .ok_or_else(|| AltiumFormatError::InvalidParamValue {
                key: "collection index".to_owned(),
                detail: format!(
                    "{}()[{index}] (referenced at final position {position}) exceeds the input \
                 collection size {}",
                    collection.name(),
                    members.len()
                ),
            })?;
    if !referenced.insert(index) {
        return Err(AltiumFormatError::InvalidParamValue {
            key: "collection index".to_owned(),
            detail: format!(
                "{}()[{index}] is referenced more than once (again at final position \
                 {position}); duplicate references to one original slot are conflicting edits",
                collection.name()
            ),
        });
    }
    Ok(address)
}

fn check_deletions(
    collection: Collection,
    member_count: usize,
    referenced: &HashSet<usize>,
    allow_deletion: bool,
) -> Result<()> {
    let omitted: Vec<usize> = (0..member_count)
        .filter(|index| !referenced.contains(index))
        .collect();
    if omitted.is_empty() || allow_deletion {
        return Ok(());
    }
    Err(AltiumFormatError::InvalidParamValue {
        key: "collection deletion".to_owned(),
        detail: format!(
            "final {} sequence omits original member indices {omitted:?}; deletion requires \
             the explicit allow_deletion authorization",
            collection.name()
        ),
    })
}

/// Owner of a created record inside one batch plan, resolved to a final
/// stream position by `to_items`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlannedOwner {
    Root,
    Original(RecordAddress),
    /// The created record with this batch-local id.
    Created(usize),
}

/// Planned final content of one stream.
enum PlannedEntry {
    /// An original input record (possibly field-replaced in the working copy).
    Original(RecordAddress),
    /// A record created by this batch, with a batch-local id so other created
    /// records can name it as their owner.
    Created {
        local_id: usize,
        record: SchRecord,
        owner: PlannedOwner,
    },
}

struct PlannedStreams {
    data: Vec<PlannedEntry>,
    additional: Vec<PlannedEntry>,
}

/// The resolved footprint half of a batch (module docs, "Footprints"): every
/// member subtree record is consumed by the plan; the final entry list for
/// member ordinal `m` is emitted at the original member `m`'s Implementation
/// position, and extras (new footprints, preceded by a created
/// ImplementationList when the component had none) follow the last
/// member-subtree record — or the end of the Data stream when no members
/// existed.
struct FootprintPlan {
    /// Every record address consumed by the collection (all member subtrees).
    scope: HashSet<RecordAddress>,
    /// Member ordinal by the member's Implementation record address.
    slots: HashMap<RecordAddress, usize>,
    /// Final entry list per original member ordinal (`None` once taken or for
    /// a deleted member).
    finals: Vec<Option<Vec<PlannedEntry>>>,
    /// New footprint subtrees (and a created ImplementationList, first).
    extras: Vec<PlannedEntry>,
    /// The last member-subtree record in encounter order; extras attach after
    /// it. `None` when the input had no members (extras go to the Data end).
    last_scope_address: Option<RecordAddress>,
    /// Field-level Replace results applied to the working copy.
    replacements: Vec<(RecordAddress, SchRecord)>,
    /// Next unused batch-local created-record id.
    next_local_id: usize,
}

/// Applies the stream-order rule: non-members keep their original relative
/// order; each edited collection's final sequence occupies its members'
/// original stream positions in encounter order, extras following the last
/// original member (or the end of the Data stream for an empty membership,
/// parameters before graphics before pins before footprints).
#[allow(clippy::too_many_arguments)]
fn plan_streams(
    component: &SchLibComponent,
    param_members: &[RecordAddress],
    graphic_members: &[RecordAddress],
    pin_members: &[RecordAddress],
    param_finals: Option<Vec<FinalRecord>>,
    graphic_finals: Option<Vec<FinalRecord>>,
    pin_finals: Option<Vec<FinalRecord>>,
    footprint_plan: Option<FootprintPlan>,
) -> Result<PlannedStreams> {
    let member_ordinals = |members: &[RecordAddress]| -> HashMap<RecordAddress, usize> {
        members
            .iter()
            .enumerate()
            .map(|(ordinal, address)| (*address, ordinal))
            .collect()
    };
    let param_ordinals = member_ordinals(param_members);
    let graphic_ordinals = member_ordinals(graphic_members);
    let pin_ordinals = member_ordinals(pin_members);

    struct EditedCollection {
        members: Vec<RecordAddress>,
        finals: Vec<Option<FinalRecord>>,
        extras: Vec<FinalRecord>,
    }
    let split = |members: &[RecordAddress], finals: Option<Vec<FinalRecord>>| {
        finals.map(|mut finals| {
            let extras = if finals.len() > members.len() {
                finals.split_off(members.len())
            } else {
                Vec::new()
            };
            let mut slotted: Vec<Option<FinalRecord>> = finals.into_iter().map(Some).collect();
            slotted.resize_with(members.len(), || None);
            EditedCollection {
                members: members.to_vec(),
                finals: slotted,
                extras,
            }
        })
    };
    let mut params = split(param_members, param_finals);
    let mut graphics = split(graphic_members, graphic_finals);
    let mut pins = split(pin_members, pin_finals);
    let mut footprints = footprint_plan;
    let mut next_local_id = footprints
        .as_ref()
        .map(|plan| plan.next_local_id)
        .unwrap_or(0);

    let mut data = Vec::new();
    let mut additional = Vec::new();
    for (address, _) in all_records(component) {
        let output = match address.stream {
            RecordStream::Data => &mut data,
            RecordStream::Additional => &mut additional,
        };
        // Footprint-scope records are consumed as whole member subtrees; the
        // final assigned to a slot is emitted at the member's Implementation
        // position and extras follow the last scope record.
        if let Some(plan) = footprints.as_mut() {
            if plan.scope.contains(&address) {
                if let Some(&ordinal) = plan.slots.get(&address) {
                    if let Some(entries) = plan.finals[ordinal].take() {
                        output.extend(entries);
                    }
                }
                if plan.last_scope_address == Some(address) {
                    output.append(&mut plan.extras);
                }
                continue;
            }
        }
        let slot = if let Some(ordinal) = param_ordinals.get(&address) {
            params.as_mut().map(|collection| (collection, *ordinal))
        } else if let Some(ordinal) = graphic_ordinals.get(&address) {
            graphics.as_mut().map(|collection| (collection, *ordinal))
        } else if let Some(ordinal) = pin_ordinals.get(&address) {
            pins.as_mut().map(|collection| (collection, *ordinal))
        } else {
            None
        };
        let Some((collection, ordinal)) = slot else {
            // Not a member of an edited collection: preserved in place.
            output.push(PlannedEntry::Original(address));
            continue;
        };
        if let Some(item) = collection.finals[ordinal].take() {
            output.push(planned(item, &mut next_local_id));
        }
        // Extras follow the last original member of their collection.
        if ordinal + 1 == collection.members.len() {
            for item in std::mem::take(&mut collection.extras) {
                output.push(planned(item, &mut next_local_id));
            }
        }
    }
    // Previously empty collections append their extras at the end of the Data
    // stream, parameters before graphics before pins before footprints.
    for collection in [&mut params, &mut graphics, &mut pins]
        .into_iter()
        .flatten()
    {
        if collection.members.is_empty() {
            for item in std::mem::take(&mut collection.extras) {
                data.push(planned(item, &mut next_local_id));
            }
        }
    }
    if let Some(plan) = footprints.as_mut() {
        if plan.last_scope_address.is_none() {
            data.append(&mut plan.extras);
        }
    }
    Ok(PlannedStreams { data, additional })
}

fn planned(item: FinalRecord, next_local_id: &mut usize) -> PlannedEntry {
    match item {
        FinalRecord::Existing { address, .. } => PlannedEntry::Original(address),
        FinalRecord::Created(record) => {
            let local_id = *next_local_id;
            *next_local_id += 1;
            PlannedEntry::Created {
                local_id,
                record,
                owner: PlannedOwner::Root,
            }
        }
    }
}

/// Converts one planned stream into assembly items plus the parallel identity
/// list used by the IndexInSheet renumbering. Batch-local created-record ids
/// resolve to final stream positions (creation is Data-only, so the id space
/// never spans streams).
fn to_items(entries: &[PlannedEntry]) -> Result<(Vec<RecordItem>, Vec<Identity>)> {
    let mut positions_by_local_id = HashMap::new();
    for (position, entry) in entries.iter().enumerate() {
        if let PlannedEntry::Created { local_id, .. } = entry {
            if positions_by_local_id.insert(*local_id, position).is_some() {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "record stream".to_owned(),
                    detail: format!("duplicate batch-local created-record id {local_id}"),
                });
            }
        }
    }
    let mut items = Vec::with_capacity(entries.len());
    let mut ids = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        match entry {
            PlannedEntry::Original(address) => {
                items.push(RecordItem::Original(*address));
                ids.push(identity_of(*address));
            }
            PlannedEntry::Created {
                record,
                owner,
                local_id: _,
            } => {
                let owner = match owner {
                    PlannedOwner::Root => RecordOwner::Root,
                    PlannedOwner::Original(address) => RecordOwner::Original(*address),
                    PlannedOwner::Created(local_id) => {
                        let owner_position =
                            positions_by_local_id.get(local_id).ok_or_else(|| {
                                AltiumFormatError::InvalidParamValue {
                                    key: "record stream".to_owned(),
                                    detail: format!(
                                        "created record at position {position} names absent \
                                     batch-local owner id {local_id}"
                                    ),
                                }
                            })?;
                        RecordOwner::New {
                            stream: RecordStream::Data,
                            id: *owner_position,
                        }
                    }
                };
                items.push(RecordItem::New {
                    id: position,
                    record: record.clone(),
                    owner,
                });
                ids.push(Identity::New(position));
            }
        }
    }
    Ok((items, ids))
}

/// Memo §6 rule 2: renumber ALL in-objectList children of owners whose child
/// sequence the batch structurally touched to final 0-based sibling positions;
/// untouched owners keep their stored values verbatim. Shared with the parts
/// engine (`snapshot::parts`), whose part-scoped deletions apply the same
/// policy.
pub(super) fn renumber_touched_owners(
    input: &SchLibComponent,
    final_component: &mut SchLibComponent,
    data_ids: &[Identity],
    additional_ids: &[Identity],
) -> Result<()> {
    let identity_at = |address: RecordAddress| -> Identity {
        match address.stream {
            RecordStream::Data => data_ids[address.index],
            RecordStream::Additional => additional_ids[address.index],
        }
    };

    // Ordered child identities per owner, input side.
    let mut input_children: HashMap<OwnerId, Vec<Identity>> = HashMap::new();
    for (address, record) in all_records(input) {
        let owner = match owner_of(input, address, record)? {
            None => OwnerId::Root,
            Some(owner_address) => OwnerId::Record(identity_of(owner_address)),
        };
        input_children
            .entry(owner)
            .or_default()
            .push(identity_of(address));
    }

    // Ordered child identities per owner, final side, plus each record's
    // resolved owner for the numbering walk.
    let mut final_children: HashMap<OwnerId, Vec<Identity>> = HashMap::new();
    let mut owners = Vec::new();
    for (address, record) in all_records(final_component) {
        let owner_address = owner_of(final_component, address, record)?;
        let owner = match owner_address {
            None => OwnerId::Root,
            Some(owner_address) => OwnerId::Record(identity_at(owner_address)),
        };
        final_children
            .entry(owner)
            .or_default()
            .push(identity_at(address));
        owners.push((owner, owner_address));
    }

    let empty = Vec::new();
    let touched: HashSet<OwnerId> = final_children
        .iter()
        .filter(|(owner, children)| input_children.get(owner).unwrap_or(&empty) != *children)
        .map(|(owner, _)| *owner)
        .collect();

    // Numbering walk over the final encounter order.
    let mut ordinals: HashMap<OwnerId, i32> = HashMap::new();
    let mut assignments: Vec<(RecordAddress, i32)> = Vec::new();
    for ((address, record), (owner, owner_address)) in all_records(final_component).zip(&owners) {
        if !touched.contains(owner) {
            continue;
        }
        let owner_record =
            owner_address.map(|owner_address| record_slot(final_component, owner_address));
        if !occupies_object_list_slot(record, owner_record) {
            continue;
        }
        let ordinal = ordinals.entry(*owner).or_insert(0);
        let position = *ordinal;
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| AltiumFormatError::InvalidParamValue {
                key: "IndexInSheet".to_owned(),
                detail: "sibling ordinal exceeds native signed range".to_owned(),
            })?;
        if !matches!(record, SchRecord::Pin(_)) {
            // Pins count as siblings but carry no stored IndexInSheet field.
            assignments.push((address, position));
        }
    }
    for (address, position) in assignments {
        record_slot_mut(final_component, address)
            .base_mut()
            .expect("non-pin objectList children carry a primitive base")
            .index_in_sheet = position;
    }
    Ok(())
}

/// Whether a child occupies a slot in its owner's native objectList (and so
/// participates in save-time IndexInSheet numbering). Field objects, the
/// implementation compatibility CONTAINERS, ParameterList-owned parameters,
/// and pin-owned special parameters never do — AD26 either never attaches
/// them to an objectList or removes them before numbering (memo §6 incl. the
/// errata; SchDataParameter/SchDataDesignator UpdateOwner;
/// SchDataComponent/SchDataImplementation.AddToListForSave — the Implementation
/// and the ParameterList-owned parameters serialize INDEXINSHEET=-1 through the
/// native IndexOf-mismatch, and the ImplementationList/ImplementationMap/
/// ParameterList ctors pin theirs at 0; all confirmed in fixture bytes).
/// MapDefiners DO occupy a slot: they are objectList children of their map and
/// carry 0-based sibling ordinals natively (wild bytes: Custom.SchLib
/// `RECORD=47 … INDEXINSHEET=1`). The name checks are deliberately
/// owner-conditional, NARROWER than AD26's name-first import dispatch: a
/// special-named parameter whose owner lacks the matching interface is
/// attached nowhere by AD26 and silently dropped on its next save; we
/// preserve such records and number them as ordinary siblings instead (see
/// the module-level IndexInSheet deviation note).
fn occupies_object_list_slot(record: &SchRecord, owner: Option<&SchRecord>) -> bool {
    match record {
        SchRecord::Designator(_) => false,
        SchRecord::ImplementationList(_)
        | SchRecord::Implementation(_)
        | SchRecord::ImplementationMap(_)
        | SchRecord::ParameterList(_) => false,
        SchRecord::MapDefiner(_) => true,
        SchRecord::Parameter(parameter) => {
            if owner.is_none() && parameter.name.eq_ignore_ascii_case("Comment") {
                return false;
            }
            if matches!(owner, Some(SchRecord::ParameterList(_))) {
                return false;
            }
            if matches!(owner, Some(SchRecord::Pin(_)))
                && [
                    "DefaultNet",
                    "HiddenNetName",
                    "PinSelectionMemory",
                    "PinUniqueId",
                ]
                .iter()
                .any(|name| parameter.name.eq_ignore_ascii_case(name))
            {
                return false;
            }
            true
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schlib::{SchLib, SchLibEmbeddedImage, compute_weight};

    fn param(name: &str, text: &str, index_in_sheet: i32) -> SchRecord {
        let mut record = SchParameter::from_params(&mut ParameterCollection::new()).unwrap();
        record.base.owner_part_id = -1;
        record.base.index_in_sheet = index_in_sheet;
        record.name = name.to_owned();
        record.text = text.to_owned();
        record.unique_id = format!("UID_{name}");
        SchRecord::Parameter(record)
    }

    fn line(owner_index: i32, index_in_sheet: i32) -> SchRecord {
        let mut record = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
        record.base.owner_index = owner_index;
        record.base.owner_part_id = 1;
        record.base.index_in_sheet = index_in_sheet;
        SchRecord::Line(record)
    }

    fn rectangle(index_in_sheet: i32) -> SchRecord {
        let mut record = SchRectangle::from_params(&mut ParameterCollection::new()).unwrap();
        record.base.owner_part_id = 1;
        record.base.index_in_sheet = index_in_sheet;
        record.unique_id = "RECT0001".to_owned();
        SchRecord::Rectangle(record)
    }

    fn pin(owner_part_id: i32) -> SchRecord {
        let mut record =
            crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
        record.owner_part_id = owner_part_id;
        record.designator = "1".to_owned();
        SchRecord::Pin(record)
    }

    fn rect_shape(color: Color) -> SchGraphicShape {
        SchGraphicShape::Rectangle {
            from: CoordPoint::zero(),
            to: CoordPoint {
                x: Coord::from_mils(100).unwrap(),
                y: Coord::from_mils(50).unwrap(),
            },
            line_width: PenWidth::Small,
            line_style: LineStyle::Solid,
            color,
            area_color: Color::new(0x00FF_FFFF),
            is_solid: true,
            transparent: false,
        }
    }

    fn rect_init(part: i32) -> SchGraphicInit {
        SchGraphicInit {
            owner_part_id: part,
            owner_part_display_mode: None,
            shape: rect_shape(Color::new(0x0000_00FF)),
        }
    }

    fn finish(mut library: SchLib) -> SchLib {
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        library
    }

    /// Blank + root Value parameter + rectangle + parent/child lines + pin +
    /// Symbol (intent-unrepresentable).
    fn library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        let records = &mut library.components[0].records;
        records.push(param("Value", "10k", 99));
        records.push(rectangle(40));
        let parent_native = records.len() as i32 + 1;
        records.push(line(0, 41));
        records.push(line(parent_native, 42));
        records.push(pin(1));
        let mut symbol =
            crate::sch_records::SchSymbol::from_params(&mut ParameterCollection::new()).unwrap();
        symbol.base.owner_part_id = 1;
        symbol.base.index_in_sheet = 43;
        records.push(SchRecord::Symbol(symbol));
        // Establish the native writer's canonical baseline (text pins carry
        // presence flags that binary pins normalize) before comparisons.
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        SchLib::open(file.path()).unwrap()
    }

    fn parameter_names(snapshot: &SchLibComponentSnapshot) -> Vec<String> {
        snapshot
            .parameters()
            .unwrap()
            .iter()
            .map(|view| view.name.to_owned())
            .collect()
    }

    fn graphic_kinds(snapshot: &SchLibComponentSnapshot) -> Vec<&'static str> {
        snapshot
            .graphics()
            .iter()
            .map(|view| view.detail.kind_name())
            .collect()
    }

    #[test]
    fn batch_insert_reorder_replace_projects_exactly_and_preserves_untouched_state() {
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        assert_eq!(parameter_names(&component), ["Comment", "Value"]);
        assert_eq!(
            graphic_kinds(&component),
            ["Rectangle", "Line", "Line", "Symbol"]
        );

        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(1),
                    SchParameterEdit::Replace(
                        0,
                        SchParameterInit {
                            name: "Comment".to_owned(),
                            text: "changed".to_owned(),
                            is_hidden: false,
                        },
                    ),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Tolerance".to_owned(),
                        text: "1%".to_owned(),
                        is_hidden: true,
                    }),
                ]),
                graphics: Some(vec![
                    SchGraphicEdit::New(rect_init(1)),
                    SchGraphicEdit::Keep(1),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Keep(3),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();

        // Projection equality: the edited collections' A2 views equal the
        // requested final sequences exactly.
        let parameters = edited.parameters().unwrap();
        assert_eq!(
            parameters
                .iter()
                .map(|view| (view.name, view.text, view.is_hidden))
                .collect::<Vec<_>>(),
            [
                ("Value", "10k", false),
                ("Comment", "changed", false),
                ("Tolerance", "1%", true),
            ]
        );
        assert_eq!(
            parameters[2].owner_group,
            SchOwnerGroup::PartIndependent,
            "new parameters take owner_part_id -1"
        );
        assert_eq!(
            graphic_kinds(&edited),
            ["Rectangle", "Line", "Line", "Rectangle", "Symbol"]
        );
        let graphics = edited.graphics();
        assert_eq!(graphics[0].owner_group, SchOwnerGroup::Part(1));
        assert_eq!(graphics[0].owner_part_display_mode, 0);
        let new_unique = graphics[0].unique_id.unwrap();
        assert_eq!(new_unique.len(), 8);
        assert_ne!(new_unique, "RECT0001");
        assert!(matches!(
            graphics[0].detail,
            SchGraphicDetail::Rectangle { is_solid: true, .. }
        ));

        // Replace preserved every non-addressed field of the original record.
        // Final Data layout: [Designator, Value, Comment*, Tolerance, NewRect,
        // LineA, LineB, Pin, Rect, Symbol].
        let SchRecord::Parameter(replaced) = &edited.component.records[2] else {
            panic!("replaced Comment slot");
        };
        let SchRecord::Parameter(original) = &component.component.records[1] else {
            unreachable!()
        };
        assert_eq!(replaced.text, "changed");
        assert!(!replaced.is_hidden);
        assert_eq!(replaced.unique_id, original.unique_id);
        assert_eq!(replaced.base.style_id, original.base.style_id);
        assert_eq!(
            replaced.base.index_in_sheet, -1,
            "root Comment stays a field object"
        );

        // Untouched records: pin and symbol typed state preserved verbatim
        // (the pin has no stored IndexInSheet; the symbol is renumbered as an
        // in-objectList child of the touched root).
        assert_eq!(
            edited.component.records[7], component.component.records[6],
            "pin preserved verbatim"
        );

        // Save/reopen typed equality through the snapshot path.
        let replaced_snapshot = snapshot.replace_component(0, edited.clone()).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced_snapshot
            .materialize()
            .unwrap()
            .save(file.path())
            .unwrap();
        let reopened = SchLib::open(file.path()).unwrap().snapshot().unwrap();
        assert_eq!(reopened, replaced_snapshot);
        assert_eq!(
            graphic_kinds(&reopened.component(0).unwrap()),
            ["Rectangle", "Line", "Line", "Rectangle", "Symbol"]
        );

        // Weight: +2 records, input convention preserved.
        assert_eq!(
            i64::from(replaced_snapshot.materialize().unwrap().header.weight),
            i64::from(library.header.weight) + 2
        );

        // No-op batch: identical final sequences leave everything (including
        // IndexInSheet) untouched.
        let noop = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::Keep(0), SchParameterEdit::Keep(1)]),
                graphics: Some((0..4).map(SchGraphicEdit::Keep).collect()),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(noop, component);
    }

    #[test]
    fn index_in_sheet_renumbers_touched_owner_and_preserves_untouched_owners() {
        let library = library();
        let component = library.snapshot().unwrap().component(0).unwrap();
        // Reorder graphics: [Rectangle, ParentLine, ChildLine, Symbol] →
        // [ParentLine, ChildLine, Rectangle, Symbol]. Root's in-objectList
        // sequence changes; the parent line's child sequence does not.
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(1),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Keep(3),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let index_in_sheet: Vec<Option<i32>> = edited
            .component
            .records
            .iter()
            .map(|record| record.base().map(|base| base.index_in_sheet))
            .collect();
        // Data: Designator(-1 field object), Comment(-1 field object),
        // Value(1st objectList child → 0), ParentLine(1), ChildLine(owned by
        // the untouched parent → preserved 42), Rectangle(2), Pin(counts as
        // sibling 3, no stored field), Symbol(4).
        assert_eq!(
            index_in_sheet,
            [
                Some(-1),
                Some(-1),
                Some(0),
                Some(1),
                Some(42),
                Some(2),
                None,
                Some(4)
            ]
        );

        // A pure field replace touches no child sequence: nothing renumbers.
        let replaced = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::Replace(
                        1,
                        SchParameterInit {
                            name: "Value".to_owned(),
                            text: "22k".to_owned(),
                            is_hidden: false,
                        },
                    ),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Parameter(value) = &replaced.component.records[2] else {
            panic!("value slot");
        };
        assert_eq!(value.text, "22k");
        assert_eq!(
            value.base.index_in_sheet, 99,
            "untouched owner sequences preserve stored IndexInSheet verbatim"
        );
    }

    #[test]
    fn deletion_authorization_duplicates_ranges_and_owned_dependents() {
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();

        // Omission without authorization.
        let error = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::Keep(0)]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("allow_deletion"), "{error}");
        assert!(error.contains("[1]"), "{error}");

        // Authorized deletion works and the weight delta follows cardinality.
        let deleted = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::Keep(0)]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: true,
            })
            .unwrap();
        assert_eq!(parameter_names(&deleted), ["Comment"]);
        let replaced = snapshot.replace_component(0, deleted).unwrap();
        assert_eq!(
            i64::from(replaced.materialize().unwrap().header.weight),
            i64::from(library.header.weight) - 1
        );

        // Duplicate references to one original slot conflict.
        let error = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::Keep(0), SchParameterEdit::Keep(0)]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: true,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("more than once"), "{error}");

        // Out-of-range index.
        let error = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::Keep(7)]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: true,
            })
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("exceeds the input collection size"),
            "{error}"
        );

        // Deleting a graphic that still owns a retained dependent is rejected
        // by the assembly (retained orphan), even when deletion is authorized.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::Keep(3),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: true,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("orphan"), "{error}");
        assert_eq!(snapshot.component(0).unwrap(), component);

        // Insert + delete + reorder in ONE batch, every reference resolved
        // against the immutable input projection: delete Comment (index 0),
        // put a new parameter first, keep Value (still index 1) after it.
        let combined = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::New(SchParameterInit {
                        name: "First".to_owned(),
                        text: "f".to_owned(),
                        is_hidden: false,
                    }),
                    SchParameterEdit::Keep(1),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: true,
            })
            .unwrap();
        assert_eq!(parameter_names(&combined), ["First", "Value"]);
    }

    #[test]
    fn replace_rejects_cross_kind_and_unrepresentable_targets() {
        let component = library().snapshot().unwrap().component(0).unwrap();
        // Line replaced with a Rectangle shape: no cross-kind mapping.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Replace(1, rect_init(1)),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::Keep(3),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("target kind Line") && error.contains("Rectangle"),
            "{error}"
        );

        // Symbol targets are intent-unrepresentable.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Keep(1),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::Replace(3, rect_init(1)),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("target kind Symbol"), "{error}");
        let views = component.graphics();
        assert!(SchGraphicInit::from_view(&views[3]).is_err());
        assert!(SchGraphicShape::from_detail(&views[3].detail).is_err());

        // Replacing a representable graphic from its own view round-trips the
        // addressed fields and preserves everything else.
        let mut init = SchGraphicInit::from_view(&views[0]).unwrap();
        let SchGraphicShape::Rectangle { color, .. } = &mut init.shape else {
            unreachable!()
        };
        *color = Color::new(0x0055_AA55);
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Replace(0, init),
                    SchGraphicEdit::Keep(1),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::Keep(3),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Rectangle(replaced) = &edited.component.records[3] else {
            panic!("rectangle slot");
        };
        assert_eq!(replaced.color, Color::new(0x0055_AA55));
        assert_eq!(replaced.unique_id, "RECT0001");
    }

    #[test]
    fn creation_validates_owner_display_font_and_image_references() {
        let mut library = library();
        library.embedded_images = vec![SchLibEmbeddedImage {
            file_name: "logo.png".to_owned(),
            data: vec![1, 2, 3],
        }];
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        let keeps: Vec<SchGraphicEdit> = (0..4).map(SchGraphicEdit::Keep).collect();
        let batch = |init: SchGraphicInit| SchChildBatch {
            parameters: None,
            graphics: Some(
                keeps
                    .iter()
                    .cloned()
                    .chain([SchGraphicEdit::New(init)])
                    .collect(),
            ),
            pins: None,
            footprints: None,
            allow_deletion: false,
        };

        // Owner part: 0 and out-of-range hard-error (effective part count 1).
        for part in [0, 2, -1] {
            let error = component
                .with_children(batch(rect_init(part)))
                .unwrap_err()
                .to_string();
            assert!(error.contains("not writable"), "part {part}: {error}");
        }

        // Display mode bound (display_mode_count is 1).
        let mut init = rect_init(1);
        init.owner_part_display_mode = Some(1);
        let error = component
            .with_children(batch(init))
            .unwrap_err()
            .to_string();
        assert!(error.contains("OwnerPartDisplayMode"), "{error}");
        let mut init = rect_init(1);
        init.owner_part_display_mode = Some(0);
        component.with_children(batch(init)).unwrap();

        // Fonts: explicit ids must exist; the default resolves against the
        // captured table only.
        let label = |font_id: Option<i32>| SchGraphicInit {
            owner_part_id: 1,
            owner_part_display_mode: None,
            shape: SchGraphicShape::Label {
                at: CoordPoint::zero(),
                orientation: RotationBy90::Rotate0,
                justification: TextJustification::BottomLeft,
                color: Color::BLACK,
                font_id,
                text: "hello".to_owned(),
                is_mirrored: false,
                url: String::new(),
            },
        };
        let error = component
            .with_children(batch(label(Some(9))))
            .unwrap_err()
            .to_string();
        assert!(error.contains("font id 9"), "{error}");
        let created = component.with_children(batch(label(None))).unwrap();
        let SchRecord::Label(record) = created.component.records.last().unwrap() else {
            panic!("appended label");
        };
        assert_eq!(record.font_id, 1, "blank table's Times New Roman 10");

        // A table without the native default cannot resolve an unspecified
        // font without a declared library-level namespace transition first.
        let mut foreign = library.clone();
        foreign.header.fonts[0].name = "Arial".to_owned();
        let foreign = finish(foreign);
        let error = foreign
            .snapshot()
            .unwrap()
            .component(0)
            .unwrap()
            .with_children(batch(label(None)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("with_library_edit"), "{error}");
        foreign
            .snapshot()
            .unwrap()
            .component(0)
            .unwrap()
            .with_children(batch(label(Some(1))))
            .unwrap();

        // Embedded images must reference captured storage assets; external
        // paths are exempt (memo §8c).
        let image = |file_name: &str, embed_image: bool| SchGraphicInit {
            owner_part_id: 1,
            owner_part_display_mode: None,
            shape: SchGraphicShape::Image {
                from: CoordPoint::zero(),
                to: CoordPoint::zero(),
                orientation: RotationBy90::Rotate0,
                line_width: PenWidth::Small,
                color: Color::BLACK,
                is_solid: false,
                keep_aspect: true,
                embed_image,
                file_name: file_name.to_owned(),
            },
        };
        let error = component
            .with_children(batch(image("missing.png", true)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing.png"), "{error}");
        component
            .with_children(batch(image("LOGO.PNG", true)))
            .unwrap();
        component
            .with_children(batch(image("C:/anywhere/else.png", false)))
            .unwrap();

        // A new root parameter named Comment is a field object: created with
        // IndexInSheet -1 and skipped by the sibling numbering.
        let created = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::Keep(1),
                    SchParameterEdit::New(SchParameterInit {
                        name: "comment".to_owned(),
                        text: "second".to_owned(),
                        is_hidden: false,
                    }),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Parameter(second_comment) = &created.component.records[3] else {
            panic!("inserted comment slot");
        };
        assert_eq!(second_comment.base.index_in_sheet, -1);
        assert_eq!(second_comment.base.owner_part_id, -1);
        assert_eq!(second_comment.unique_id.len(), 8);
    }

    #[test]
    fn dual_stream_shifts_remap_additional_to_data_owners_and_survive_save_reopen() {
        // Ownership worklog minimum list: a Data insertion shifts an
        // Additional-to-Data parent's owner index; an Additional subtree with
        // descendants survives with owners remapped.
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            aggregate.records.push(param("Value", "10k", 0));
            // Additional-to-Data: X is owned by the Value parameter (Data
            // native address 3); its descendant Y selects Additional.
            let mut x = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            x.base.owner_index = 3;
            x.base.owner_part_id = 1;
            let mut y = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            y.base.owner_index = 0;
            y.base.owner_index_additional_list = true;
            y.base.owner_part_id = 1;
            aggregate.additional_records = vec![SchRecord::Line(x), SchRecord::Line(y)];
        }
        let library = finish(library);
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        assert_eq!(parameter_names(&component), ["Comment", "Value"]);
        // The Additional lines are graphics()[0..2].
        assert_eq!(graphic_kinds(&component), ["Line", "Line"]);

        // Insert a parameter: the final sequence puts the new one in the
        // Comment slot and shifts Value one slot later (extra after the last
        // member), moving its native Data address from 3 to 4.
        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::New(SchParameterInit {
                        name: "STEP2B".to_owned(),
                        text: "inserted".to_owned(),
                        is_hidden: false,
                    }),
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::Keep(1),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(parameter_names(&edited), ["STEP2B", "Comment", "Value"]);
        let SchRecord::Line(x) = &edited.component.additional_records[0] else {
            panic!("additional X");
        };
        assert_eq!(
            x.base.owner_index, 4,
            "Additional-to-Data owner follows the shifted Data parent"
        );
        assert!(!x.base.owner_index_additional_list);
        let SchRecord::Line(y) = &edited.component.additional_records[1] else {
            panic!("additional Y");
        };
        assert_eq!(y.base.owner_index, 0);
        assert!(y.base.owner_index_additional_list);

        // Both streams survive a native save/reopen with typed equality.
        let replaced = snapshot.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap().snapshot().unwrap();
        assert_eq!(reopened, replaced);
        assert_eq!(
            parameter_names(&reopened.component(0).unwrap()),
            ["STEP2B", "Comment", "Value"]
        );
    }

    #[test]
    fn new_records_cannot_enter_additional_and_originals_cannot_migrate_streams() {
        // A root-owned parameter living in the Additional stream makes the
        // collection span both streams.
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.components[0]
            .additional_records
            .push(param("AddSide", "x", 7));
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        assert_eq!(parameter_names(&component), ["Comment", "AddSide"]);

        // The extra would follow the last member, which lives in Additional:
        // creating records there is unsupported.
        let error = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::Keep(1),
                    SchParameterEdit::New(SchParameterInit {
                        name: "New".to_owned(),
                        text: String::new(),
                        is_hidden: false,
                    }),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional stream"), "{error}");
        assert!(error.contains("unsupported"), "{error}");

        // Reordering across the streams would migrate originals: rejected by
        // the assembly.
        let error = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::Keep(1), SchParameterEdit::Keep(0)]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("between Data and Additional"), "{error}");
    }

    #[test]
    fn stream_order_rule_places_extras_after_last_member_or_at_data_end() {
        // Graphics extras land immediately after the last original graphic,
        // before trailing non-members; a previously empty parameters
        // collection appends at the end of the Data stream.
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let records = &mut library.components[0].records;
            // Remove the blank Comment so parameters() starts empty.
            records.retain(|record| !matches!(record, SchRecord::Parameter(_)));
            records.push(rectangle(0));
            records.push(pin(1));
        }
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        assert!(parameter_names(&component).is_empty());
        assert_eq!(graphic_kinds(&component), ["Rectangle"]);

        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![SchParameterEdit::New(SchParameterInit {
                    name: "P1".to_owned(),
                    text: "1".to_owned(),
                    is_hidden: false,
                })]),
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::New(rect_init(1)),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let kinds: Vec<&str> = edited
            .component
            .records
            .iter()
            .map(|record| match record {
                SchRecord::Designator(_) => "Designator",
                SchRecord::Parameter(_) => "Parameter",
                SchRecord::Rectangle(_) => "Rectangle",
                SchRecord::Pin(_) => "Pin",
                _ => "other",
            })
            .collect();
        // The graphics extra follows the last original graphic (before the
        // pin); the parameters extra goes to the end of the Data stream.
        assert_eq!(
            kinds,
            ["Designator", "Rectangle", "Rectangle", "Pin", "Parameter"]
        );
        assert_eq!(parameter_names(&edited), ["P1"]);
        assert_eq!(graphic_kinds(&edited), ["Rectangle", "Rectangle"]);
    }

    #[test]
    fn blank_library_root_text_uses_field_object_sentinels_and_reopens_exactly() {
        // Increment-B reconciliation (deliverable 5): AD26-written root
        // Designators and root "Comment" parameters — the component FIELD
        // OBJECTS — carry OWNERPARTID=-1 and INDEXINSHEET=-1 in the gate
        // fixtures (ordinary root parameters carry 0-based sibling ordinals
        // instead); the blank library's designator/comment now match.
        let library = SchLib::new_blank_ad26().unwrap();
        let SchRecord::Designator(designator) = &library.components[0].records[0] else {
            panic!("blank designator");
        };
        assert_eq!(designator.base.owner_part_id, -1);
        assert_eq!(designator.base.index_in_sheet, -1);
        let SchRecord::Parameter(comment) = &library.components[0].records[1] else {
            panic!("blank comment");
        };
        assert_eq!(comment.base.owner_part_id, -1);
        assert_eq!(comment.base.index_in_sheet, -1);

        // The serialized parameter text carries both keys explicitly.
        let data = crate::schlib::serialize_component_data(&library.components[0]).unwrap();
        let blocks = crate::block_stream::parse_blocks(&data).unwrap();
        for block in &blocks[1..=2] {
            let text = std::str::from_utf8(&block.data)
                .expect("blank-library parameter blocks are ASCII")
                .to_ascii_uppercase();
            assert!(text.contains("|OWNERPARTID=-1"), "{text}");
            assert!(text.contains("|INDEXINSHEET=-1"), "{text}");
        }

        // Save/reopen fixed point and the parameters() owner group.
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        assert_eq!(reopened, library);
        let component = reopened.snapshot().unwrap().component(0).unwrap();
        let parameters = component.parameters().unwrap();
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].name, "Comment");
        assert_eq!(parameters[0].owner_group, SchOwnerGroup::PartIndependent);

        // Designator creation through the scalar path also writes the field
        // object sentinels and passes the wired owner-part check.
        let cleared = component.with_designator(None).unwrap();
        let created = cleared.with_designator(Some("Q?".to_owned())).unwrap();
        let Some(SchRecord::Designator(created_designator)) = created.component.records.last()
        else {
            panic!("created designator");
        };
        assert_eq!(created_designator.base.owner_part_id, -1);
        assert_eq!(created_designator.base.index_in_sheet, -1);
    }

    #[test]
    fn second_batch_resolves_against_new_snapshot_and_removes_inserted_children() {
        // Two sequential batches: insert then remove, each resolved against
        // its own immutable input, restoring the original projections.
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let inserted = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::Keep(1),
                    SchParameterEdit::New(SchParameterInit {
                        name: "STEP2B".to_owned(),
                        text: "tmp".to_owned(),
                        is_hidden: false,
                    }),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let step = parameter_names(&inserted)
            .iter()
            .position(|name| name == "STEP2B")
            .unwrap();
        let removed = inserted
            .with_children(SchChildBatch {
                parameters: Some(
                    (0..3)
                        .filter(|index| *index != step)
                        .map(SchParameterEdit::Keep)
                        .collect(),
                ),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: true,
            })
            .unwrap();
        assert_eq!(parameter_names(&removed), ["Comment", "Value"]);
        // The round trip restores the original projections; IndexInSheet of
        // the root's children was renumbered by the touched-owner rule, so
        // typed equality holds after normalizing through a no-op renumber.
        assert_eq!(
            removed.parameters().unwrap(),
            component.parameters().unwrap()
        );
        assert_eq!(removed.graphics(), component.graphics());
    }

    fn label_shape(font_id: Option<i32>, text: &str) -> SchGraphicShape {
        SchGraphicShape::Label {
            at: CoordPoint::zero(),
            orientation: RotationBy90::Rotate0,
            justification: TextJustification::BottomLeft,
            color: Color::BLACK,
            font_id,
            text: text.to_owned(),
            is_mirrored: false,
            url: String::new(),
        }
    }

    /// Blank library + a mode-1 rectangle, a font-2 mode-1 label, and a
    /// polyline (display_mode_count 2, two fonts).
    fn multi_mode_library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let component = &mut library.components[0];
            component.component.display_mode_count = 2;
            component
                .component
                .custom_display_mode_names
                .push(String::new());
            let mut rect = SchRectangle::from_params(&mut ParameterCollection::new()).unwrap();
            rect.base.owner_part_id = 1;
            rect.base.owner_part_display_mode = 1;
            rect.unique_id = "RECT0001".to_owned();
            component.records.push(SchRecord::Rectangle(rect));
            let mut label = SchLabel::from_params(&mut ParameterCollection::new()).unwrap();
            label.base.owner_part_id = 1;
            label.base.owner_part_display_mode = 1;
            label.font_id = 2;
            label.text = "hello".to_owned();
            label.unique_id = "LABL0001".to_owned();
            component.records.push(SchRecord::Label(label));
            let mut polyline_params = ParameterCollection::new();
            polyline_params.insert(
                altium_format_types::constants::visual::LOCATION_COUNT,
                "0".to_owned(),
            );
            let mut polyline = SchPolyline::from_params(&mut polyline_params).unwrap();
            polyline.base.owner_part_id = 1;
            polyline.vertices = vec![
                CoordPoint::zero(),
                CoordPoint {
                    x: Coord::from_mils(100).unwrap(),
                    y: Coord::from_mils(0).unwrap(),
                },
            ];
            polyline.unique_id = "PLIN0001".to_owned();
            component.records.push(SchRecord::Polyline(polyline));
        }
        let mut second = library.header.fonts[0].clone();
        second.id = 2;
        second.name = "Arial".to_owned();
        second.size = 12;
        library.header.fonts.push(second);
        finish(library)
    }

    #[test]
    fn replace_with_unaddressed_optionals_preserves_display_mode_and_font() {
        let library = multi_mode_library();
        let component = library.snapshot().unwrap().component(0).unwrap();
        assert_eq!(
            graphic_kinds(&component),
            ["Rectangle", "Label", "Polyline"]
        );

        // Hand-built inits with None optionals: Replace PRESERVES the stored
        // display mode (1) and font (2) instead of resetting them to the
        // creation defaults.
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Replace(
                        0,
                        SchGraphicInit {
                            owner_part_id: 1,
                            owner_part_display_mode: None,
                            shape: rect_shape(Color::new(0x0000_1122)),
                        },
                    ),
                    SchGraphicEdit::Replace(
                        1,
                        SchGraphicInit {
                            owner_part_id: 1,
                            owner_part_display_mode: None,
                            shape: label_shape(None, "edited"),
                        },
                    ),
                    SchGraphicEdit::Keep(2),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        // Data layout: [Designator, Comment, Rectangle, Label, Polyline].
        let SchRecord::Rectangle(rect) = &edited.component.records[2] else {
            panic!("rectangle slot");
        };
        assert_eq!(rect.base.owner_part_display_mode, 1);
        assert_eq!(rect.color, Color::new(0x0000_1122));
        assert_eq!(rect.unique_id, "RECT0001");
        let SchRecord::Label(label) = &edited.component.records[3] else {
            panic!("label slot");
        };
        assert_eq!(label.base.owner_part_display_mode, 1);
        assert_eq!(label.font_id, 2, "None font preserved on Replace");
        assert_eq!(label.text, "edited");

        // Some(...) still overrides both, validated against table and count.
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Replace(
                        1,
                        SchGraphicInit {
                            owner_part_id: 1,
                            owner_part_display_mode: Some(0),
                            shape: label_shape(Some(1), "explicit"),
                        },
                    ),
                    SchGraphicEdit::Keep(2),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Label(label) = &edited.component.records[3] else {
            panic!("label slot");
        };
        assert_eq!(label.base.owner_part_display_mode, 0);
        assert_eq!(label.font_id, 1);
        assert_eq!(label.text, "explicit");

        // New with None optionals still resolves the creation defaults.
        let created = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Keep(1),
                    SchGraphicEdit::Keep(2),
                    SchGraphicEdit::New(SchGraphicInit {
                        owner_part_id: 1,
                        owner_part_display_mode: None,
                        shape: label_shape(None, "fresh"),
                    }),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Label(created_label) = created
            .component
            .records
            .iter()
            .filter(|record| matches!(record, SchRecord::Label(_)))
            .nth(1)
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(created_label.base.owner_part_display_mode, 0);
        assert_eq!(created_label.font_id, 1, "creation default font");
    }

    #[test]
    fn creation_covers_all_thirteen_representable_kinds() {
        // Every representable kind must be creatable through New (the
        // indexed-coordinate kinds require the vertex-count key at
        // construction — a defect found in review), project back exactly, and
        // survive a native save/reopen.
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let mils = |x: i32, y: i32| CoordPoint {
            x: Coord::from_mils(x).unwrap(),
            y: Coord::from_mils(y).unwrap(),
        };
        let shapes = vec![
            SchGraphicShape::Line {
                from: CoordPoint::zero(),
                to: mils(10, 10),
                line_width: PenWidth::Small,
                line_style: LineStyle::Solid,
                color: Color::BLACK,
            },
            rect_shape(Color::BLACK),
            SchGraphicShape::RoundRectangle {
                from: CoordPoint::zero(),
                to: mils(20, 10),
                corner_x_radius: Coord::from_mils(2).unwrap(),
                corner_y_radius: Coord::from_mils(2).unwrap(),
                line_width: PenWidth::Small,
                color: Color::BLACK,
                area_color: Color::new(0x00FF_FFFF),
                is_solid: false,
            },
            SchGraphicShape::Arc {
                center: CoordPoint::zero(),
                radius: Coord::from_mils(10).unwrap(),
                line_width: PenWidth::Small,
                start_angle: 0.0,
                end_angle: Some(90.0),
                color: Color::BLACK,
            },
            SchGraphicShape::EllipticalArc {
                center: CoordPoint::zero(),
                radius: Coord::from_mils(10).unwrap(),
                secondary_radius: Coord::from_mils(5).unwrap(),
                line_width: PenWidth::Small,
                start_angle: 0.0,
                end_angle: Some(180.0),
                color: Color::BLACK,
            },
            SchGraphicShape::Ellipse {
                center: CoordPoint::zero(),
                radius: Coord::from_mils(10).unwrap(),
                secondary_radius: Coord::from_mils(5).unwrap(),
                line_width: PenWidth::Small,
                color: Color::BLACK,
                area_color: Color::new(0x00FF_FFFF),
                is_solid: true,
                transparent: false,
            },
            SchGraphicShape::Pie {
                center: CoordPoint::zero(),
                radius: Coord::from_mils(10).unwrap(),
                line_width: PenWidth::Small,
                start_angle: 0.0,
                end_angle: Some(45.0),
                color: Color::BLACK,
                area_color: Color::new(0x00FF_FFFF),
                is_solid: true,
            },
            SchGraphicShape::Polyline {
                vertices: vec![CoordPoint::zero(), mils(10, 0)],
                line_width: PenWidth::Small,
                line_style: LineStyle::Solid,
                start_line_shape: LineShape::None,
                end_line_shape: LineShape::Arrow,
                line_shape_size: PenWidth::Small,
                color: Color::BLACK,
            },
            SchGraphicShape::Polygon {
                vertices: vec![CoordPoint::zero(), mils(10, 0), mils(5, 8)],
                line_width: PenWidth::Small,
                color: Color::BLACK,
                area_color: Color::new(0x00FF_FFFF),
                is_solid: true,
                transparent: false,
            },
            SchGraphicShape::Bezier {
                vertices: vec![CoordPoint::zero(), mils(3, 5), mils(7, 5), mils(10, 0)],
                line_width: PenWidth::Small,
                color: Color::BLACK,
            },
            SchGraphicShape::Image {
                from: CoordPoint::zero(),
                to: mils(10, 10),
                orientation: RotationBy90::Rotate0,
                line_width: PenWidth::Small,
                color: Color::BLACK,
                is_solid: false,
                keep_aspect: true,
                embed_image: false,
                file_name: "C:/external/logo.png".to_owned(),
            },
            label_shape(Some(1), "created label"),
            SchGraphicShape::TextFrame {
                from: CoordPoint::zero(),
                to: mils(40, 20),
                line_width: PenWidth::Small,
                color: Color::BLACK,
                area_color: Color::new(0x00FF_FFFF),
                text_color: Color::BLACK,
                font_id: Some(1),
                is_solid: false,
                show_border: true,
                alignment: HorizontalAlign::Left,
                word_wrap: true,
                clip_to_rect: false,
                text: "created frame".to_owned(),
                text_margin: Coord::from_mils(2).unwrap(),
                transparent: false,
            },
        ];
        let inits: Vec<SchGraphicInit> = shapes
            .into_iter()
            .map(|shape| SchGraphicInit {
                owner_part_id: 1,
                owner_part_display_mode: Some(0),
                shape,
            })
            .collect();
        let original_len = component.graphics().len();
        let edits: Vec<SchGraphicEdit> = (0..original_len)
            .map(SchGraphicEdit::Keep)
            .chain(inits.iter().cloned().map(SchGraphicEdit::New))
            .collect();
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(edits),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let views = edited.graphics();
        assert_eq!(views.len(), original_len + inits.len());
        for (offset, submitted) in inits.iter().enumerate() {
            let view = &views[original_len + offset];
            let created = SchGraphicInit::from_view(view).unwrap_or_else(|error| {
                panic!("created {} view: {error}", submitted.shape.kind_name())
            });
            assert_eq!(
                created,
                *submitted,
                "created {} projects back exactly",
                submitted.shape.kind_name()
            );
        }

        // Native save/reopen with all thirteen created kinds.
        let replaced = snapshot.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap().snapshot().unwrap();
        assert_eq!(reopened, replaced);
    }

    #[test]
    fn replace_round_trips_polyline_and_label_views() {
        // from_view-driven Replace round trips for a vertex-list kind and a
        // text kind: every addressed field lands, everything else survives.
        let library = multi_mode_library();
        let component = library.snapshot().unwrap().component(0).unwrap();
        let views = component.graphics();

        let mut label_init = SchGraphicInit::from_view(&views[1]).unwrap();
        assert_eq!(label_init.owner_part_display_mode, Some(1));
        let SchGraphicShape::Label { text, .. } = &mut label_init.shape else {
            unreachable!()
        };
        *text = "rewired".to_owned();

        let mut polyline_init = SchGraphicInit::from_view(&views[2]).unwrap();
        let SchGraphicShape::Polyline {
            vertices,
            line_shape_size,
            ..
        } = &mut polyline_init.shape
        else {
            unreachable!()
        };
        vertices.push(CoordPoint {
            x: Coord::from_mils(100).unwrap(),
            y: Coord::from_mils(50).unwrap(),
        });
        *line_shape_size = PenWidth::Medium;

        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(vec![
                    SchGraphicEdit::Keep(0),
                    SchGraphicEdit::Replace(1, label_init.clone()),
                    SchGraphicEdit::Replace(2, polyline_init.clone()),
                ]),
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let edited_views = edited.graphics();
        assert_eq!(
            SchGraphicInit::from_view(&edited_views[1]).unwrap(),
            label_init,
            "label view equals the submitted init field-for-field"
        );
        assert_eq!(
            SchGraphicInit::from_view(&edited_views[2]).unwrap(),
            polyline_init,
            "polyline view equals the submitted init field-for-field"
        );
        let SchRecord::Polyline(polyline) = &edited.component.records[4] else {
            panic!("polyline slot");
        };
        assert_eq!(polyline.vertices.len(), 3);
        assert_eq!(polyline.unique_id, "PLIN0001");
    }

    // ── Increment C: pins ────────────────────────────────────────────────────

    fn pin_init(part: i32, designator: &str) -> SchPinInit {
        SchPinInit {
            owner_part_id: part,
            owner_part_display_mode: Some(0),
            designator: designator.to_owned(),
            name: format!("N{designator}"),
            electrical: PinElectricalType::Passive,
            length: Coord::from_mils(300).unwrap(),
            location: CoordPoint::zero(),
            orientation: RotationBy90::Rotate0,
            is_hidden: false,
            hidden_net_name: String::new(),
            swap_group: String::new(),
            part_swap_group: Some(String::new()),
            part_pin_swap_suffix: Some(String::new()),
            pair_swap_group: String::new(),
        }
    }

    fn pin_designators(snapshot: &SchLibComponentSnapshot) -> Vec<String> {
        snapshot
            .pins()
            .iter()
            .map(|view| view.designator.to_owned())
            .collect()
    }

    /// A pin exercising the coordinate-fraction, overflow-description,
    /// pair-swap, text-positioning, and wide-text sidecar streams.
    fn frac_text_pin(designator: &str) -> SchRecord {
        let mut record =
            crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
        record.owner_part_id = 1;
        record.designator = designator.to_owned();
        record.name = format!("µNAME{designator}");
        record.description = "D".repeat(300);
        record.location = CoordPoint {
            x: Coord::from_internal(1_005_000),
            y: Coord::from_internal(-2_000_250),
        };
        record.pin_length = Coord::from_internal(3_000_777);
        record.swap_id_part = "G1|&|s1".to_owned();
        record.swap_id_pair = "PAIR-A".to_owned();
        record.name_text_data = Some(crate::sch_records::PinTextPositioning {
            position_mode_custom: true,
            rotation_anchor_component: false,
            rotation_relative: RotationBy90::Rotate90,
            font_mode_custom: true,
            custom_position_margin: Some(Coord::from_mils(2).unwrap()),
            custom_font_id: Some(1),
            custom_color: Some(Color::new(0x0000_7F00)),
        });
        SchRecord::Pin(record)
    }

    /// A pin exercising the symbol-line-width, package-length,
    /// propagation-delay, and function-data sidecar streams.
    fn function_pin(designator: &str) -> SchRecord {
        let mut record =
            crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
        record.owner_part_id = 1;
        record.designator = designator.to_owned();
        record.name = format!("N{designator}");
        record.pin_symbol_line_width = Some(1);
        record.pin_package_length = "10mil".to_owned();
        record.propagation_delay = "1.5ns".to_owned();
        record.defined_functions = vec!["CLOCK".to_owned(), "GPIO".to_owned()];
        record.selected_functions = vec!["GPIO".to_owned()];
        SchRecord::Pin(record)
    }

    /// Blank + three Data pins (two sidecar-rich, one plain) + a root
    /// parameter, canonicalized through a native save/reopen.
    fn sidecar_library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let records = &mut library.components[0].records;
            records.push(param("Value", "10k", 0));
            records.push(frac_text_pin("1"));
            records.push(pin(1)); // plain pin, designator "1" → rename below
            let SchRecord::Pin(plain) = records.last_mut().unwrap() else {
                unreachable!()
            };
            plain.designator = "2".to_owned();
            records.push(function_pin("3"));
        }
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        SchLib::open(file.path()).unwrap()
    }

    #[test]
    fn pin_batch_edits_keep_all_nine_sidecar_streams_consistent_across_save_reopen() {
        let library = sidecar_library();
        // The fixture engages every one of the nine pin sidecar streams, so
        // the save/reopen equalities below are evidence about all of them.
        let pins: Vec<&crate::sch_records::SchPin> = library.components[0]
            .records
            .iter()
            .filter_map(|record| match record {
                SchRecord::Pin(pin) => Some(pin),
                _ => None,
            })
            .collect();
        let streams: Vec<&str> = crate::schlib::serialize_pin_sidecars(&pins)
            .unwrap()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            streams,
            [
                "PinFrac",
                "PinDesc",
                "PinMiscData",
                "PinTextData",
                "PinWideText",
                "PinSymbolLineWidth",
                "PinPackageLength",
                "PinPropagationDelay",
                "PinFunctionData",
            ]
        );

        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        assert_eq!(pin_designators(&component), ["1", "2", "3"]);

        // Stage 1: insert at the front, reorder the tail, and replace the
        // function pin's addressed fields — one batch against the immutable
        // input projection.
        let mut replaced_init = SchPinInit::from_view(&component.pins()[2]).unwrap();
        replaced_init.designator = "3R".to_owned();
        replaced_init.electrical = PinElectricalType::Output;
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![
                    SchPinEdit::New(pin_init(1, "P")),
                    SchPinEdit::Keep(0),
                    SchPinEdit::Replace(2, replaced_init.clone()),
                    SchPinEdit::Keep(1),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();

        // Projection equality: pins() equals the requested final sequence.
        assert_eq!(pin_designators(&edited), ["P", "1", "3R", "2"]);
        let views = edited.pins();
        assert_eq!(SchPinInit::from_view(&views[0]).unwrap(), pin_init(1, "P"));
        assert_eq!(SchPinInit::from_view(&views[2]).unwrap(), replaced_init);

        // The untouched sidecar-rich pin is preserved verbatim; the replaced
        // pin keeps every sidecar-backed field.
        let SchRecord::Pin(untouched) = &edited.component.records[4] else {
            panic!("untouched pin slot");
        };
        assert_eq!(
            SchRecord::Pin(untouched.clone()),
            library.components[0].records[3],
            "untouched pin preserved verbatim (frac coords, long description, \
             pair swap, text positioning, wide-text name)"
        );
        let SchRecord::Pin(replaced) = &edited.component.records[5] else {
            panic!("replaced pin slot");
        };
        assert_eq!(replaced.designator, "3R");
        assert_eq!(replaced.electrical, PinElectricalType::Output);
        assert_eq!(replaced.pin_symbol_line_width, Some(1));
        assert_eq!(replaced.pin_package_length, "10mil");
        assert_eq!(replaced.propagation_delay, "1.5ns");
        assert_eq!(replaced.defined_functions, ["CLOCK", "GPIO"]);
        assert_eq!(replaced.selected_functions, ["GPIO"]);
        let SchRecord::Pin(created) = &edited.component.records[3] else {
            panic!("created pin slot");
        };
        assert_eq!(
            created.unique_id, "",
            "SchLib binary pins carry no UniqueId; none is generated"
        );

        // Native save/reopen: the ordinal-addressed sidecars follow the
        // shifted, reordered, and replaced pins exactly.
        let inserted = snapshot.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        inserted.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), inserted);
        assert_eq!(
            i64::from(reopened.header.weight),
            i64::from(library.header.weight) + 1
        );
        let SchRecord::Pin(reopened_rich) = &reopened.components[0].records[4] else {
            panic!("reopened rich pin");
        };
        assert_eq!(reopened_rich.description.len(), 300);
        assert_eq!(reopened_rich.location.x, Coord::from_internal(1_005_000));
        assert_eq!(reopened_rich.swap_id_pair, "PAIR-A");
        let SchRecord::Pin(reopened_fn) = &reopened.components[0].records[5] else {
            panic!("reopened function pin");
        };
        assert_eq!(reopened_fn.defined_functions, ["CLOCK", "GPIO"]);

        // Stage 2: removal requires authorization, then removes the probe and
        // the plain pin; the remaining sidecar-rich pins keep their fields
        // through another save/reopen.
        let reopened_snapshot = reopened.snapshot().unwrap();
        let component = reopened_snapshot.component(0).unwrap();
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Keep(1), SchPinEdit::Keep(2)]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("allow_deletion"), "{error}");
        assert!(error.contains("[0, 3]"), "{error}");
        let removed = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Keep(1), SchPinEdit::Keep(2)]),
                footprints: None,
                allow_deletion: true,
            })
            .unwrap();
        assert_eq!(pin_designators(&removed), ["1", "3R"]);
        let shrunk = reopened_snapshot.replace_component(0, removed).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        shrunk.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), shrunk);
        let SchRecord::Pin(kept_fn) = &reopened.components[0].records[4] else {
            panic!("kept function pin");
        };
        assert_eq!(kept_fn.designator, "3R");
        assert_eq!(kept_fn.pin_package_length, "10mil");
    }

    #[test]
    fn all_pin_count_stays_verbatim_through_structural_pin_edits() {
        // Memo §7: positive-stale and zero stored caches survive pin
        // insert/remove batches and native save/reopen untouched; no
        // recompute path exists.
        for stale in [17, 0] {
            let mut library = sidecar_library();
            library.components[0].component.all_pin_count = stale;
            let library = finish(library);
            let snapshot = library.snapshot().unwrap();
            let component = snapshot.component(0).unwrap();
            assert_eq!(component.stored_all_pin_count(), stale);
            let edited = component
                .with_children(SchChildBatch {
                    parameters: None,
                    graphics: None,
                    pins: Some(vec![
                        SchPinEdit::New(pin_init(1, "P")),
                        SchPinEdit::Keep(0),
                        SchPinEdit::Keep(2),
                    ]),
                    footprints: None,
                    allow_deletion: true,
                })
                .unwrap();
            assert_eq!(edited.stored_all_pin_count(), stale);
            let replaced = snapshot.replace_component(0, edited).unwrap();
            let file = tempfile::NamedTempFile::new().unwrap();
            replaced.materialize().unwrap().save(file.path()).unwrap();
            let reopened = SchLib::open(file.path()).unwrap();
            assert_eq!(reopened.components[0].component.all_pin_count, stale);
            assert_eq!(reopened.snapshot().unwrap(), replaced);
        }
    }

    #[test]
    fn multi_collection_batch_edits_all_four_collections_against_immutable_input() {
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        assert_eq!(pin_designators(&component), ["1"]);
        assert!(component.footprint_maps().unwrap().footprints.is_empty());

        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(1),
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Tolerance".to_owned(),
                        text: "1%".to_owned(),
                        is_hidden: false,
                    }),
                ]),
                graphics: Some(
                    (0..4)
                        .map(SchGraphicEdit::Keep)
                        .chain([SchGraphicEdit::New(rect_init(1))])
                        .collect(),
                ),
                pins: Some(vec![SchPinEdit::New(pin_init(1, "P")), SchPinEdit::Keep(0)]),
                footprints: Some(vec![SchFootprintEdit::New(SchFootprintInit {
                    model_name: "R0805".to_owned(),
                    description: "resistor body".to_owned(),
                    is_current: true,
                    pin_pad_maps: Some(vec![SchPinPadMapInit {
                        pin: "1".to_owned(),
                        pads: vec!["A1".to_owned()],
                    }]),
                })]),
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(parameter_names(&edited), ["Value", "Comment", "Tolerance"]);
        assert_eq!(
            graphic_kinds(&edited),
            ["Rectangle", "Line", "Line", "Symbol", "Rectangle"]
        );
        assert_eq!(pin_designators(&edited), ["P", "1"]);
        let maps = edited.footprint_maps().unwrap();
        assert_eq!(maps.preserved_non_pcblib_implementations, 0);
        assert_eq!(maps.footprints.len(), 1);
        assert_eq!(maps.footprints[0].model_name, "R0805");
        assert_eq!(maps.footprints[0].pin_pad_maps.len(), 1);

        let replaced = snapshot.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        assert_eq!(
            SchLib::open(file.path()).unwrap().snapshot().unwrap(),
            replaced
        );
    }

    #[test]
    fn pin_inserts_shift_data_indices_and_remap_additional_to_data_owners() {
        // Ownership worklog minimum list, pin flavor: a Data pin insertion
        // shifts an Additional-to-Data parent's owner index; the descendant's
        // Additional-to-Additional link survives.
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            aggregate.records.push(pin(1)); // Data native address 3
            let mut x = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            x.base.owner_index = 3;
            x.base.owner_part_id = 1;
            let mut y = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            y.base.owner_index = 0;
            y.base.owner_index_additional_list = true;
            y.base.owner_part_id = 1;
            aggregate.additional_records = vec![SchRecord::Line(x), SchRecord::Line(y)];
        }
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let library = SchLib::open(file.path()).unwrap();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();

        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::New(pin_init(1, "P")), SchPinEdit::Keep(0)]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(pin_designators(&edited), ["P", "1"]);
        let SchRecord::Line(x) = &edited.component.additional_records[0] else {
            panic!("additional X");
        };
        assert_eq!(
            x.base.owner_index, 4,
            "Additional-to-Data owner follows the pin shifted by the insert"
        );
        let SchRecord::Line(y) = &edited.component.additional_records[1] else {
            panic!("additional Y");
        };
        assert_eq!(y.base.owner_index, 0);
        assert!(y.base.owner_index_additional_list);

        let replaced = snapshot.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        assert_eq!(
            SchLib::open(file.path()).unwrap().snapshot().unwrap(),
            replaced
        );
    }

    #[test]
    fn additional_stream_pins_can_only_be_kept_untouched() {
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.components[0].records.push(pin(1));
        library.components[0].additional_records.push(pin(1));
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let library = SchLib::open(file.path()).unwrap();
        let component = library.snapshot().unwrap().component(0).unwrap();
        assert_eq!(pin_designators(&component), ["1", "1"]);

        // Replace of the Additional pin.
        let mut init = SchPinInit::from_view(&component.pins()[1]).unwrap();
        init.designator = "9".to_owned();
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Keep(0), SchPinEdit::Replace(1, init)]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional stream"), "{error}");
        assert!(error.contains("unproven"), "{error}");

        // Reorder that moves it.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Keep(1), SchPinEdit::Keep(0)]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional stream"), "{error}");

        // Deletion, even authorized.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Keep(0)]),
                footprints: None,
                allow_deletion: true,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional stream"), "{error}");

        // Insertion whose extra would land in the Additional stream.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![
                    SchPinEdit::Keep(0),
                    SchPinEdit::Keep(1),
                    SchPinEdit::New(pin_init(1, "P")),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional"), "{error}");
        assert!(error.contains("unsupported"), "{error}");

        // Keep-untouched flows through: a field Replace of the DATA pin with
        // the Additional pin kept in place is fine, and the Additional pin's
        // record is preserved verbatim.
        let mut data_init = SchPinInit::from_view(&component.pins()[0]).unwrap();
        data_init.designator = "2".to_owned();
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Replace(0, data_init), SchPinEdit::Keep(1)]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(pin_designators(&edited), ["2", "1"]);
        assert_eq!(
            edited.component.additional_records,
            component.component.additional_records
        );

        // No-op Keep both is identity.
        let noop = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Keep(0), SchPinEdit::Keep(1)]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(noop, component);
    }

    #[test]
    fn data_pin_structural_edits_are_frozen_while_additional_pins_exist() {
        // 2 Data pins + 1 Additional pin: the conservative freeze rejects
        // even Data-only permutations and balanced substitutions (they would
        // be sidecar-safe — sidecars regenerate from final Data encounter
        // order — but the published contract is positional Keep/Replace only
        // until Additional pin sidecars are proven).
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            for designator in ["A", "B"] {
                let SchRecord::Pin(mut record) = pin(1) else {
                    unreachable!()
                };
                record.designator = designator.to_owned();
                aggregate.records.push(SchRecord::Pin(record));
            }
            aggregate.additional_records.push(pin(1));
        }
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let library = SchLib::open(file.path()).unwrap();
        let component = library.snapshot().unwrap().component(0).unwrap();
        assert_eq!(pin_designators(&component), ["A", "B", "1"]);

        let batch = |edits| SchChildBatch {
            parameters: None,
            graphics: None,
            pins: Some(edits),
            footprints: None,
            allow_deletion: true,
        };

        // Pure Data permutation that leaves the Additional ordinal in place.
        let error = component
            .with_children(batch(vec![
                SchPinEdit::Keep(1),
                SchPinEdit::Keep(0),
                SchPinEdit::Keep(2),
            ]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("pins()[0]"), "{error}");
        assert!(error.contains("Additional stream"), "{error}");
        assert!(error.contains("unsupported"), "{error}");

        // Balanced (count-preserving) delete+insert of a Data pin, even
        // authorized, is frozen too.
        let error = component
            .with_children(batch(vec![
                SchPinEdit::Keep(0),
                SchPinEdit::New(pin_init(1, "P")),
                SchPinEdit::Keep(2),
            ]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("pins()[1]"), "{error}");
        assert!(error.contains("Additional stream"), "{error}");

        // Positional field Replace of BOTH Data pins stays possible, with
        // the Additional pin preserved byte-verbatim.
        let mut first = SchPinInit::from_view(&component.pins()[0]).unwrap();
        first.designator = "A2".to_owned();
        let mut second = SchPinInit::from_view(&component.pins()[1]).unwrap();
        second.designator = "B2".to_owned();
        let edited = component
            .with_children(batch(vec![
                SchPinEdit::Replace(0, first),
                SchPinEdit::Replace(1, second),
                SchPinEdit::Keep(2),
            ]))
            .unwrap();
        assert_eq!(pin_designators(&edited), ["A2", "B2", "1"]);
        assert_eq!(
            edited.component.additional_records,
            component.component.additional_records
        );
    }

    #[test]
    fn pin_swap_halves_preserve_or_reencode_per_the_native_rule() {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let records = &mut library.components[0].records;
            for combined in ["|&|", "OLD|&|suf", "PRE"] {
                let SchRecord::Pin(mut record) = pin(1) else {
                    unreachable!()
                };
                record.swap_id_part = combined.to_owned();
                records.push(SchRecord::Pin(record));
            }
        }
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let library = SchLib::open(file.path()).unwrap();
        let component = library.snapshot().unwrap().component(0).unwrap();

        let unaddressed = |index: usize, designator: &str| {
            let mut init = pin_init(1, designator);
            init.part_swap_group = None;
            init.part_pin_swap_suffix = None;
            SchPinEdit::Replace(index, init)
        };
        // Both halves unaddressed on the wild bare-separator pin: preserved
        // byte-verbatim even though other fields are replaced.
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![
                    unaddressed(0, "A"),
                    SchPinEdit::Keep(1),
                    SchPinEdit::Keep(2),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Pin(wild) = &edited.component.records[2] else {
            panic!("wild pin slot");
        };
        assert_eq!(wild.designator, "A");
        assert_eq!(
            wild.swap_id_part, "|&|",
            "unaddressed halves stay byte-verbatim"
        );

        // Addressing one half re-encodes through the split, preserving the
        // other half.
        let mut prefix_only = pin_init(1, "B");
        prefix_only.part_swap_group = Some("G".to_owned());
        prefix_only.part_pin_swap_suffix = None;
        let mut suffix_only = pin_init(1, "C");
        suffix_only.part_swap_group = None;
        suffix_only.part_pin_swap_suffix = Some("s2".to_owned());
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![
                    SchPinEdit::Keep(0),
                    SchPinEdit::Replace(1, prefix_only),
                    SchPinEdit::Replace(2, suffix_only),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Pin(prefixed) = &edited.component.records[3] else {
            panic!("prefixed pin slot");
        };
        assert_eq!(prefixed.swap_id_part, "G|&|suf");
        let SchRecord::Pin(suffixed) = &edited.component.records[4] else {
            panic!("suffixed pin slot");
        };
        assert_eq!(suffixed.swap_id_part, "PRE|&|s2");

        // Addressing both halves of the wild form normalizes it (the native
        // concat cannot regenerate a bare separator), and the projection
        // round-trips the halves.
        let mut both = pin_init(1, "D");
        both.part_swap_group = Some(String::new());
        both.part_pin_swap_suffix = Some(String::new());
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![
                    SchPinEdit::Replace(0, both),
                    SchPinEdit::Keep(1),
                    SchPinEdit::Keep(2),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let SchRecord::Pin(normalized) = &edited.component.records[2] else {
            panic!("normalized pin slot");
        };
        assert_eq!(normalized.swap_id_part, "");
        let views = edited.pins();
        assert_eq!(views[0].part_swap_group, "");
        assert_eq!(views[0].part_pin_swap_suffix, None);
    }

    #[test]
    fn pin_creation_validates_owner_display_and_hidden_net_writes() {
        let library = library();
        let component = library.snapshot().unwrap().component(0).unwrap();
        let batch = |init: SchPinInit| SchChildBatch {
            parameters: None,
            graphics: None,
            pins: Some(vec![SchPinEdit::Keep(0), SchPinEdit::New(init)]),
            footprints: None,
            allow_deletion: false,
        };

        // Owner part: −1 and out-of-range hard-error; 0 (the native shared
        // wildcard) and 1 are writable (effective part count 1).
        for part in [-1, 2] {
            let error = component
                .with_children(batch(pin_init(part, "P")))
                .unwrap_err()
                .to_string();
            assert!(error.contains("not writable"), "part {part}: {error}");
        }
        for part in [0, 1] {
            let created = component.with_children(batch(pin_init(part, "P"))).unwrap();
            let expected = if part == 0 {
                SchOwnerGroup::SharedPins
            } else {
                SchOwnerGroup::Part(1)
            };
            assert_eq!(created.pins()[1].owner_group, expected);
            // Recorded creation-default deviation (module docs): typed wire
            // defaults, NOT the native SchDataPin.SetDefault values (which
            // set both show flags true and a non-zero formal type).
            let SchRecord::Pin(created_pin) = created
                .component
                .records
                .iter()
                .find(|record| {
                    matches!(record, SchRecord::Pin(created_pin) if created_pin.designator == "P")
                })
                .expect("created pin record")
            else {
                unreachable!()
            };
            assert!(!created_pin.show_name);
            assert!(!created_pin.show_designator);
        }

        // Display mode bound (display_mode_count is 1); None resolves 0.
        let mut init = pin_init(1, "P");
        init.owner_part_display_mode = Some(1);
        let error = component
            .with_children(batch(init))
            .unwrap_err()
            .to_string();
        assert!(error.contains("OwnerPartDisplayMode"), "{error}");
        let mut init = pin_init(1, "P");
        init.owner_part_display_mode = None;
        let created = component.with_children(batch(init)).unwrap();
        assert_eq!(created.pins()[1].owner_part_display_mode, 0);

        // hidden_net_name has no verified SchLib serialization path.
        let mut init = pin_init(1, "P");
        init.hidden_net_name = "VCC".to_owned();
        let error = component
            .with_children(batch(init))
            .unwrap_err()
            .to_string();
        assert!(error.contains("hidden_net_name"), "{error}");
        assert!(error.contains("unsupported"), "{error}");

        // A Replace preserving the (empty) stored value passes; on a pin
        // whose display mode is unaddressed the stored mode survives.
        let from_view = SchPinInit::from_view(&component.pins()[0]).unwrap();
        let replaced = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: Some(vec![SchPinEdit::Replace(0, from_view.clone())]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(
            SchPinInit::from_view(&replaced.pins()[0]).unwrap(),
            from_view,
            "from_view Replace round-trips the addressable pin surface"
        );
    }

    #[test]
    fn part_growth_composes_with_child_batches_creating_children_in_the_new_part() {
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        assert_eq!(component.effective_part_count().unwrap(), 1);

        // Growth changes only the two count fields: every child projection of
        // the grown snapshot is identical to the input's, so a following
        // batch resolves against the same collections the caller saw
        // (documented composition rule; growth never reclassifies, memo §2).
        let grown = component.with_parts_appended(1).unwrap();
        assert_eq!(grown.effective_part_count().unwrap(), 2);
        assert_eq!(grown.component.records, component.component.records);
        assert_eq!(grown.graphics(), component.graphics());
        assert_eq!(grown.pins(), component.pins());
        assert_eq!(
            grown.component.component.display_mode_count,
            component.component.component.display_mode_count
        );
        assert_eq!(
            grown.component.component.current_part_id,
            component.component.component.current_part_id
        );

        let edited = grown
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some(
                    (0..4)
                        .map(SchGraphicEdit::Keep)
                        .chain([SchGraphicEdit::New(rect_init(2))])
                        .collect(),
                ),
                pins: Some(vec![
                    SchPinEdit::Keep(0),
                    SchPinEdit::New(pin_init(2, "P2")),
                    SchPinEdit::New(pin_init(0, "PS")),
                ]),
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(edited.graphics()[4].owner_group, SchOwnerGroup::Part(2));
        let pins = edited.pins();
        assert_eq!(pins[1].owner_group, SchOwnerGroup::Part(2));
        assert_eq!(
            pins[2].owner_group,
            SchOwnerGroup::SharedPins,
            "multi-part components accept the shared-pin wildcard"
        );
        // Existing children were never reclassified by the growth.
        assert_eq!(pins[0].owner_group, SchOwnerGroup::Part(1));
        assert_eq!(edited.graphics()[0].owner_group, SchOwnerGroup::Part(1));

        let replaced = snapshot.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), replaced);
        assert_eq!(reopened.components[0].component.part_count, 3);
        assert_eq!(reopened.header.components[0].part_count, 3);
    }

    // ── Footprints (increment D2) ─────────────────────────────────────────

    /// Pushes one full implementation chain (Implementation → Map →
    /// Definers, ParameterList [→ one parameter]) onto a Data record vector,
    /// with the native chain sentinels (owner_part_id 0, Implementation
    /// IndexInSheet −1, containers 0, definers at their 0-based ordinal).
    fn push_chain(
        records: &mut Vec<SchRecord>,
        list_native: i32,
        model_type: &str,
        model_name: &str,
        is_current: bool,
        definers: &[(&str, &[&str])],
        with_parameter: bool,
    ) {
        let implementation_native = records.len() as i32 + 1;
        let mut implementation =
            SchImplementation::from_params(&mut ParameterCollection::new()).unwrap();
        implementation.base.owner_index = list_native;
        implementation.base.index_in_sheet = -1;
        implementation.model_type = model_type.to_owned();
        implementation.model_name = model_name.to_owned();
        implementation.description = format!("{model_name} description");
        implementation.is_current = is_current;
        implementation.unique_id = format!("UI{model_name:.6}");
        implementation.model_vault_guid = "VAULT-GUID".to_owned();
        implementation.datafile_links = vec![ModelDatafileLink {
            location: String::new(),
            entity_name: model_name.to_owned(),
            file_kind: "PCBLib".to_owned(),
        }];
        records.push(SchRecord::Implementation(implementation));

        let map_native = records.len() as i32 + 1;
        let mut map = SchImplementationMap::from_params(&mut ParameterCollection::new()).unwrap();
        map.base.owner_index = implementation_native;
        records.push(SchRecord::ImplementationMap(map));
        for (ordinal, (pin, pads)) in definers.iter().enumerate() {
            let mut definer = SchMapDefiner::from_params(&mut ParameterCollection::new()).unwrap();
            definer.base.owner_index = map_native;
            definer.base.index_in_sheet = ordinal as i32;
            definer.des_intf = (*pin).to_owned();
            definer.des_imps = pads.iter().map(|pad| (*pad).to_owned()).collect();
            records.push(SchRecord::MapDefiner(definer));
        }

        let parameter_list_native = records.len() as i32 + 1;
        let mut parameter_list =
            SchParameterList::from_params(&mut ParameterCollection::new()).unwrap();
        parameter_list.base.owner_index = implementation_native;
        records.push(SchRecord::ParameterList(parameter_list));
        if with_parameter {
            let mut parameter = SchParameter::from_params(&mut ParameterCollection::new()).unwrap();
            parameter.base.owner_index = parameter_list_native;
            parameter.base.owner_part_id = -1;
            parameter.base.index_in_sheet = -1;
            parameter.name = "ModelParam".to_owned();
            parameter.text = format!("{model_name} value");
            parameter.unique_id = format!("UP{model_name:.6}");
            records.push(SchRecord::Parameter(parameter));
        }
    }

    /// Blank + shared ImplementationList + footprint A (one definer, one
    /// implementation parameter) + a SIM chain (one definer, one parameter)
    /// INTERLEAVED between the footprint slots + footprint B (bare map +
    /// list), save/reopened to the native writer's canonical baseline.
    fn footprint_library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let records = &mut library.components[0].records;
            let list_native = records.len() as i32 + 1;
            records.push(SchRecord::ImplementationList(
                SchImplementationList::from_params(&mut ParameterCollection::new()).unwrap(),
            ));
            push_chain(
                records,
                list_native,
                "PCBLIB",
                "FP_A",
                true,
                &[("1", &["A1", "B1"])],
                true,
            );
            push_chain(
                records,
                list_native,
                "SIM",
                "RESISTOR",
                true,
                &[("1", &["1"])],
                true,
            );
            // Mixed-case model type: still a footprint member; casing must be
            // preserved through Keep and Replace.
            push_chain(records, list_native, "PcbLib", "FP_B", false, &[], false);
        }
        let library = finish(library);
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        SchLib::open(file.path()).unwrap()
    }

    /// (model_name, description, is_current, pin→pads) tuples of the
    /// footprint projection, plus the preserved non-PCBLIB count.
    #[allow(clippy::type_complexity)]
    fn footprint_projection(
        snapshot: &SchLibComponentSnapshot,
    ) -> (
        Vec<(String, String, bool, Vec<(String, Vec<String>)>)>,
        usize,
    ) {
        let maps = snapshot.footprint_maps().unwrap();
        (
            maps.footprints
                .iter()
                .map(|footprint| {
                    (
                        footprint.model_name.to_owned(),
                        footprint.description.to_owned(),
                        footprint.is_current,
                        footprint
                            .pin_pad_maps
                            .iter()
                            .map(|map| (map.pin.to_owned(), map.pads.to_vec()))
                            .collect(),
                    )
                })
                .collect(),
            maps.preserved_non_pcblib_implementations,
        )
    }

    /// Typed SIM-chain records (Implementation model_type SIM plus its whole
    /// subtree) in encounter order, for verbatim-preservation assertions.
    /// `owner_index` is normalized to 0 in the returned clones: it is stream
    /// address plumbing that assembly legitimately remaps when earlier records
    /// shift; the chain's PARENT KINDS stay validated by `assemble_records`,
    /// and every other typed field must compare verbatim.
    fn sim_chain_records(snapshot: &SchLibComponentSnapshot) -> Vec<SchRecord> {
        let component = &snapshot.component;
        let mut sim_addresses = HashSet::new();
        let mut chain = Vec::new();
        for (index, record) in component.records.iter().enumerate() {
            let address = RecordAddress {
                stream: RecordStream::Data,
                index,
            };
            let in_chain = match record {
                SchRecord::Implementation(implementation) => {
                    implementation.model_type.eq_ignore_ascii_case("SIM")
                }
                _ => owner_of(component, address, record)
                    .unwrap()
                    .is_some_and(|owner| sim_addresses.contains(&owner)),
            };
            if in_chain {
                sim_addresses.insert(address);
                let mut normalized = record.clone();
                normalized
                    .base_mut()
                    .expect("chain records carry a primitive base")
                    .owner_index = 0;
                chain.push(normalized);
            }
        }
        chain
    }

    fn save_reopen(snapshot: &SchLibSnapshot) -> SchLibSnapshot {
        let file = tempfile::NamedTempFile::new().unwrap();
        snapshot.materialize().unwrap().save(file.path()).unwrap();
        SchLib::open(file.path()).unwrap().snapshot().unwrap()
    }

    use super::super::SchLibSnapshot;

    #[test]
    fn footprint_creation_materializes_full_chain_and_list_rules() {
        // A blank component has NO chain at all: creation materializes the
        // shared ImplementationList too.
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let record_count = component.component.records.len();
        let created = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::New(SchFootprintInit {
                    model_name: "SOT-23-5".to_owned(),
                    description: "5 leads".to_owned(),
                    is_current: true,
                    pin_pad_maps: Some(vec![
                        SchPinPadMapInit {
                            pin: "1".to_owned(),
                            pads: vec!["A1".to_owned(), "B1".to_owned()],
                        },
                        SchPinPadMapInit {
                            pin: "2".to_owned(),
                            pads: vec!["C2".to_owned()],
                        },
                    ]),
                })]),
                allow_deletion: false,
            })
            .unwrap();

        // Chain appended at the Data end: List, Implementation, Map, two
        // Definers, ParameterList — with the native sentinels.
        let records = &created.component.records;
        assert_eq!(records.len(), record_count + 6);
        let SchRecord::ImplementationList(list) = &records[record_count] else {
            panic!("list slot");
        };
        assert_eq!(list.base.owner_index, 0);
        assert_eq!(list.base.owner_part_id, 0);
        assert_eq!(list.base.index_in_sheet, 0);
        let SchRecord::Implementation(implementation) = &records[record_count + 1] else {
            panic!("implementation slot");
        };
        assert_eq!(
            implementation.base.owner_index,
            record_count as i32 + 1,
            "implementation owned by the created list"
        );
        assert_eq!(implementation.base.owner_part_id, 0);
        assert_eq!(implementation.base.index_in_sheet, -1);
        assert_eq!(implementation.model_type, "PCBLIB");
        assert_eq!(implementation.model_name, "SOT-23-5");
        assert_eq!(implementation.description, "5 leads");
        assert!(implementation.is_current);
        assert_eq!(implementation.unique_id.len(), 8);
        assert_eq!(
            implementation.datafile_links,
            vec![ModelDatafileLink {
                location: String::new(),
                entity_name: "SOT-23-5".to_owned(),
                file_kind: "PCBLib".to_owned(),
            }],
            "the native single default datafile link"
        );
        let SchRecord::ImplementationMap(map) = &records[record_count + 2] else {
            panic!("map slot");
        };
        assert_eq!(map.base.owner_index, record_count as i32 + 2);
        assert_eq!(map.base.owner_part_id, 0);
        assert_eq!(map.base.index_in_sheet, 0);
        for (offset, ordinal) in [(3usize, 0i32), (4, 1)] {
            let SchRecord::MapDefiner(definer) = &records[record_count + offset] else {
                panic!("definer slot {offset}");
            };
            assert_eq!(definer.base.owner_index, record_count as i32 + 3);
            assert_eq!(definer.base.owner_part_id, 0);
            assert_eq!(
                definer.base.index_in_sheet, ordinal,
                "definers carry 0-based map-sibling ordinals"
            );
        }
        let SchRecord::ParameterList(parameter_list) = &records[record_count + 5] else {
            panic!("parameter list slot");
        };
        assert_eq!(parameter_list.base.owner_index, record_count as i32 + 2);
        assert_eq!(parameter_list.base.owner_part_id, 0);
        assert_eq!(parameter_list.base.index_in_sheet, 0);

        // Projection equality and save/reopen typed equality.
        let (footprints, preserved) = footprint_projection(&created);
        assert_eq!(
            footprints,
            vec![(
                "SOT-23-5".to_owned(),
                "5 leads".to_owned(),
                true,
                vec![
                    ("1".to_owned(), vec!["A1".to_owned(), "B1".to_owned()]),
                    ("2".to_owned(), vec!["C2".to_owned()]),
                ],
            )]
        );
        assert_eq!(preserved, 0);
        let replaced = snapshot.replace_component(0, created.clone()).unwrap();
        assert_eq!(save_reopen(&replaced), replaced);

        // A bare existing list (the ICs shape: RECORD=44, zero models) is
        // REUSED — no second list appears.
        let mut bare = library.clone();
        bare.components[0]
            .records
            .push(SchRecord::ImplementationList(
                SchImplementationList::from_params(&mut ParameterCollection::new()).unwrap(),
            ));
        let bare = finish(bare);
        let list_native = bare.components[0].records.len() as i32;
        let component = bare.snapshot().unwrap().component(0).unwrap();
        let created = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::New(SchFootprintInit {
                    model_name: "R0805".to_owned(),
                    description: String::new(),
                    is_current: true,
                    pin_pad_maps: None,
                })]),
                allow_deletion: false,
            })
            .unwrap();
        let lists = created
            .component
            .records
            .iter()
            .filter(|record| matches!(record, SchRecord::ImplementationList(_)))
            .count();
        assert_eq!(lists, 1, "the existing bare list is reused");
        let SchRecord::Implementation(implementation) =
            &created.component.records[list_native as usize]
        else {
            panic!("implementation follows the reused list at the Data end");
        };
        assert_eq!(implementation.base.owner_index, list_native);

        // Creation validation: empty model name, duplicate pin designators
        // (case-insensitive), and an incoherent current assignment all fail.
        let component = library.snapshot().unwrap().component(0).unwrap();
        let creation = |init: SchFootprintInit| SchChildBatch {
            parameters: None,
            graphics: None,
            pins: None,
            footprints: Some(vec![SchFootprintEdit::New(init)]),
            allow_deletion: false,
        };
        let error = component
            .with_children(creation(SchFootprintInit {
                model_name: String::new(),
                description: String::new(),
                is_current: true,
                pin_pad_maps: None,
            }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("non-empty model name"), "{error}");
        let error = component
            .with_children(creation(SchFootprintInit {
                model_name: "X".to_owned(),
                description: String::new(),
                is_current: true,
                pin_pad_maps: Some(vec![
                    SchPinPadMapInit {
                        pin: "a1".to_owned(),
                        pads: vec![],
                    },
                    SchPinPadMapInit {
                        pin: "A1".to_owned(),
                        pads: vec![],
                    },
                ]),
            }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("more than once"), "{error}");
        let error = component
            .with_children(creation(SchFootprintInit {
                model_name: "X".to_owned(),
                description: String::new(),
                is_current: false,
                pin_pad_maps: None,
            }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("0 current footprints"), "{error}");
    }

    #[test]
    fn footprint_creation_alongside_sim_reuses_shared_list_and_leaves_sim_untouched() {
        let library = footprint_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let sim_before = sim_chain_records(&component);
        assert_eq!(sim_before.len(), 5, "impl + map + definer + list + param");
        let (footprints_before, preserved_before) = footprint_projection(&component);
        assert_eq!(footprints_before.len(), 2);
        assert_eq!(preserved_before, 1);
        let list_index = component
            .component
            .records
            .iter()
            .position(|record| matches!(record, SchRecord::ImplementationList(_)))
            .unwrap();

        let mut edits: Vec<SchFootprintEdit> =
            vec![SchFootprintEdit::Keep(0), SchFootprintEdit::Keep(1)];
        edits.push(SchFootprintEdit::New(SchFootprintInit {
            model_name: "FP_C".to_owned(),
            description: "third".to_owned(),
            is_current: false,
            pin_pad_maps: Some(vec![SchPinPadMapInit {
                pin: "2".to_owned(),
                pads: vec!["P2".to_owned()],
            }]),
        }));
        let created = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(edits),
                allow_deletion: false,
            })
            .unwrap();

        // SIM chain verbatim, shared list reused, projection extended.
        assert_eq!(sim_chain_records(&created), sim_before);
        let lists = created
            .component
            .records
            .iter()
            .filter(|record| matches!(record, SchRecord::ImplementationList(_)))
            .count();
        assert_eq!(lists, 1);
        let new_implementation = created
            .component
            .records
            .iter()
            .find_map(|record| match record {
                SchRecord::Implementation(implementation)
                    if implementation.model_name == "FP_C" =>
                {
                    Some(implementation)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            new_implementation.base.owner_index,
            list_index as i32 + 1,
            "new implementation attaches to the existing shared list"
        );
        let (footprints, preserved) = footprint_projection(&created);
        assert_eq!(preserved, 1);
        assert_eq!(footprints.len(), 3);
        assert_eq!(footprints[..2], footprints_before[..]);
        assert_eq!(
            footprints[2],
            (
                "FP_C".to_owned(),
                "third".to_owned(),
                false,
                vec![("2".to_owned(), vec!["P2".to_owned()])],
            )
        );
        let replaced = snapshot.replace_component(0, created).unwrap();
        assert_eq!(save_reopen(&replaced), replaced);
    }

    #[test]
    fn footprint_replace_rebuilds_definers_and_preserves_unaddressed_state() {
        let library = footprint_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let (footprints_before, _) = footprint_projection(&component);
        let SchRecord::Implementation(original_a) = component
            .component
            .records
            .iter()
            .find(|record| matches!(record, SchRecord::Implementation(i) if i.model_name == "FP_A"))
            .unwrap()
        else {
            unreachable!()
        };
        let original_a = original_a.clone();

        // Rebuild the definers and change the addressed axes; footprint B is
        // replaced too (from_view keeps its axes but rebuilds its empty map).
        let views = component.footprint_maps().unwrap();
        let mut init_b = SchFootprintInit::from_view(&views.footprints[1]);
        assert_eq!(init_b.pin_pad_maps.as_deref(), Some(&[][..]));
        init_b.is_current = false;
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Replace(
                        0,
                        SchFootprintInit {
                            model_name: "FP_A2".to_owned(),
                            description: "rebuilt".to_owned(),
                            is_current: true,
                            pin_pad_maps: Some(vec![
                                SchPinPadMapInit {
                                    pin: "3".to_owned(),
                                    pads: vec!["C3".to_owned()],
                                },
                                SchPinPadMapInit {
                                    pin: "4".to_owned(),
                                    pads: vec!["D4".to_owned(), "E4".to_owned()],
                                },
                            ]),
                        },
                    ),
                    SchFootprintEdit::Replace(1, init_b),
                ]),
                allow_deletion: false,
            })
            .unwrap();

        let (footprints, preserved) = footprint_projection(&edited);
        assert_eq!(preserved, 1);
        assert_eq!(
            footprints[0],
            (
                "FP_A2".to_owned(),
                "rebuilt".to_owned(),
                true,
                vec![
                    ("3".to_owned(), vec!["C3".to_owned()]),
                    ("4".to_owned(), vec!["D4".to_owned(), "E4".to_owned()]),
                ],
            )
        );
        assert_eq!(footprints[1].0, "FP_B");

        // Unaddressed axes preserved verbatim on the replaced implementation.
        let SchRecord::Implementation(replaced_a) = edited
            .component
            .records
            .iter()
            .find(
                |record| matches!(record, SchRecord::Implementation(i) if i.model_name == "FP_A2"),
            )
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(replaced_a.unique_id, original_a.unique_id);
        assert_eq!(replaced_a.datafile_links, original_a.datafile_links);
        assert_eq!(replaced_a.model_vault_guid, original_a.model_vault_guid);
        assert_eq!(replaced_a.model_type, original_a.model_type);
        let SchRecord::Implementation(replaced_b) = edited
            .component
            .records
            .iter()
            .find(|record| matches!(record, SchRecord::Implementation(i) if i.model_name == "FP_B"))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(replaced_b.model_type, "PcbLib", "stored casing preserved");
        // Rebuilt definers carry fresh 0-based map ordinals.
        let ordinals: Vec<i32> = edited
            .component
            .records
            .iter()
            .filter_map(|record| match record {
                SchRecord::MapDefiner(definer)
                    if definer.des_intf == "3" || definer.des_intf == "4" =>
                {
                    Some(definer.base.index_in_sheet)
                }
                _ => None,
            })
            .collect();
        assert_eq!(ordinals, [0, 1]);
        // The implementation parameter survives untouched (ParameterList
        // subtree is outside the addressed axes).
        assert!(
            edited.component.records.iter().any(|record| {
                matches!(record, SchRecord::Parameter(p) if p.text == "FP_A value")
            })
        );
        // SIM chain untouched by the double Replace.
        assert_eq!(sim_chain_records(&edited), sim_chain_records(&component));
        let replaced = snapshot.replace_component(0, edited).unwrap();
        assert_eq!(save_reopen(&replaced), replaced);

        // Unaddressed pin_pad_maps (None) preserves the definers verbatim —
        // the edit is a pure field replace and the subtree stays identical.
        let renamed = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Replace(
                        0,
                        SchFootprintInit {
                            model_name: "FP_A".to_owned(),
                            description: "described".to_owned(),
                            is_current: true,
                            pin_pad_maps: None,
                        },
                    ),
                    SchFootprintEdit::Keep(1),
                ]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&renamed);
        assert_eq!(footprints[0].1, "described");
        assert_eq!(
            footprints[0].3, footprints_before[0].3,
            "definers preserved verbatim when pin_pad_maps is unaddressed"
        );

        // Replacing the sole current footprint to non-current without moving
        // the flag is an incoherent final assignment.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Replace(
                        0,
                        SchFootprintInit {
                            model_name: "FP_A".to_owned(),
                            description: String::new(),
                            is_current: false,
                            pin_pad_maps: None,
                        },
                    ),
                    SchFootprintEdit::Keep(1),
                ]),
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("0 current footprints"), "{error}");
    }

    #[test]
    fn complete_footprint_replacement_never_touches_the_sim_chain() {
        // The red-team [12] case: complete footprint replacement with
        // deletion authorized must preserve the SIM chain and the shared
        // ImplementationList verbatim.
        let library = footprint_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let sim_before = sim_chain_records(&component);

        // Unauthorized removal is rejected with the omitted indices.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![]),
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("allow_deletion"), "{error}");
        assert!(error.contains("[0, 1]"), "{error}");

        let removed = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![]),
                allow_deletion: true,
            })
            .unwrap();
        let (footprints, preserved) = footprint_projection(&removed);
        assert!(footprints.is_empty());
        assert_eq!(preserved, 1, "the SIM implementation survives");
        assert_eq!(sim_chain_records(&removed), sim_before);
        assert_eq!(
            removed
                .component
                .records
                .iter()
                .filter(|record| matches!(record, SchRecord::ImplementationList(_)))
                .count(),
            1,
            "the shared ImplementationList container is preserved"
        );
        // No footprint subtree record survives: both implementations, their
        // maps, definer, parameter list, and implementation parameter are
        // gone (8 records: 2 impl + 2 maps + 1 definer + 2 lists + 1 param).
        assert_eq!(
            component.component.records.len() - removed.component.records.len(),
            8
        );
        let replaced = snapshot.replace_component(0, removed.clone()).unwrap();
        assert_eq!(save_reopen(&replaced), replaced);
        // Weight follows cardinality.
        assert_eq!(
            i64::from(replaced.materialize().unwrap().header.weight),
            i64::from(library.header.weight) - 8
        );

        // Removing one of two footprints keeps the sibling and the SIM chain.
        let removed_one = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(1)]),
                allow_deletion: true,
            })
            .unwrap_err();
        // Keep(1) alone leaves ZERO current footprints (FP_B stored false):
        // the membership change enforces the single-current invariant.
        assert!(
            removed_one.to_string().contains("0 current footprints"),
            "{removed_one}"
        );
        let removed_one = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(0)]),
                allow_deletion: true,
            })
            .unwrap();
        let (footprints, preserved) = footprint_projection(&removed_one);
        assert_eq!(footprints.len(), 1);
        assert_eq!(footprints[0].0, "FP_A");
        assert_eq!(preserved, 1);
        assert_eq!(sim_chain_records(&removed_one), sim_before);
        let replaced = snapshot.replace_component(0, removed_one).unwrap();
        assert_eq!(save_reopen(&replaced), replaced);
    }

    #[test]
    fn footprint_reorder_permutes_only_footprint_slots() {
        let library = footprint_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let (footprints_before, _) = footprint_projection(&component);
        let kinds_before: Vec<&'static str> = component
            .component
            .records
            .iter()
            .map(record_kind_for_test)
            .collect();

        // A pure Keep permutation: FP_B takes slot 0, FP_A slot 1. This is
        // legal even though both currents stay stored as-is (no value
        // changes), and the SIM chain keeps its interleaved position.
        let reordered = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(1), SchFootprintEdit::Keep(0)]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, preserved) = footprint_projection(&reordered);
        assert_eq!(preserved, 1);
        assert_eq!(footprints[0], footprints_before[1]);
        assert_eq!(footprints[1], footprints_before[0]);
        // The record-kind sequence is unchanged EXCEPT the footprint slot
        // contents swapped: FP_B's 3-record subtree now sits where FP_A's
        // 5-record subtree began and vice versa; the list and the SIM chain
        // keep their relative positions.
        let kinds_after: Vec<&'static str> = reordered
            .component
            .records
            .iter()
            .map(record_kind_for_test)
            .collect();
        let names: Vec<&str> = reordered
            .component
            .records
            .iter()
            .filter_map(|record| match record {
                SchRecord::Implementation(i) => Some(i.model_name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["FP_B", "RESISTOR", "FP_A"]);
        assert_eq!(kinds_before.len(), kinds_after.len());
        assert_eq!(sim_chain_records(&reordered), sim_chain_records(&component));
        let replaced = snapshot.replace_component(0, reordered.clone()).unwrap();
        assert_eq!(save_reopen(&replaced), replaced);

        // Restoring the order hands back the exact original component, and
        // the all-positional-Keep no-op is an identity.
        let restored = reordered
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(1), SchFootprintEdit::Keep(0)]),
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(restored, component);
        let noop = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(0), SchFootprintEdit::Keep(1)]),
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(noop, component);

        // Duplicate and out-of-range member references are conflicts.
        for edits in [
            vec![SchFootprintEdit::Keep(0), SchFootprintEdit::Keep(0)],
            vec![SchFootprintEdit::Keep(2), SchFootprintEdit::Keep(1)],
        ] {
            let error = component
                .with_children(SchChildBatch {
                    parameters: None,
                    graphics: None,
                    pins: None,
                    footprints: Some(edits),
                    allow_deletion: true,
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains("footprints()["), "{error}");
        }
    }

    fn record_kind_for_test(record: &SchRecord) -> &'static str {
        match record {
            SchRecord::ImplementationList(_) => "List",
            SchRecord::Implementation(_) => "Impl",
            SchRecord::ImplementationMap(_) => "Map",
            SchRecord::MapDefiner(_) => "Definer",
            SchRecord::ParameterList(_) => "ParamList",
            SchRecord::Parameter(_) => "Param",
            SchRecord::Designator(_) => "Designator",
            _ => "Other",
        }
    }

    #[test]
    fn additional_stream_chain_records_freeze_footprint_edits() {
        // Additional-stream chain records are programmatically constructible
        // (zero corpus occurrences; the native exporter cannot emit them), so
        // the collection freezes to the all-positional-Keep no-op — the same
        // conservative shape as the Additional-pin freeze.
        let mut library = footprint_library();
        {
            let aggregate = &mut library.components[0];
            let mut chain = Vec::new();
            let list_native = 0i32; // additional[0]
            chain.push(SchRecord::ImplementationList(
                SchImplementationList::from_params(&mut ParameterCollection::new()).unwrap(),
            ));
            let mut implementation =
                SchImplementation::from_params(&mut ParameterCollection::new()).unwrap();
            implementation.base.owner_index = list_native;
            implementation.base.owner_index_additional_list = true;
            implementation.model_type = "PCBLIB".to_owned();
            implementation.model_name = "ADDITIONAL_FP".to_owned();
            chain.push(SchRecord::Implementation(implementation));
            aggregate.additional_records = chain;
        }
        let library = finish(library);
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let (footprints, _) = footprint_projection(&component);
        assert_eq!(footprints.len(), 3, "the Additional footprint projects");

        // The positional all-Keep arrangement passes as an identity.
        let noop = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some((0..3).map(SchFootprintEdit::Keep).collect()),
                allow_deletion: false,
            })
            .unwrap();
        assert_eq!(noop, component);

        // Everything else is frozen: reorder, Replace, New, and deletion.
        let frozen_init = SchFootprintInit {
            model_name: "X".to_owned(),
            description: String::new(),
            is_current: false,
            pin_pad_maps: None,
        };
        for edits in [
            vec![
                SchFootprintEdit::Keep(1),
                SchFootprintEdit::Keep(0),
                SchFootprintEdit::Keep(2),
            ],
            vec![
                SchFootprintEdit::Replace(0, frozen_init.clone()),
                SchFootprintEdit::Keep(1),
                SchFootprintEdit::Keep(2),
            ],
            (0..3)
                .map(SchFootprintEdit::Keep)
                .chain([SchFootprintEdit::New(frozen_init.clone())])
                .collect(),
            vec![SchFootprintEdit::Keep(0), SchFootprintEdit::Keep(1)],
        ] {
            let error = component
                .with_children(SchChildBatch {
                    parameters: None,
                    graphics: None,
                    pins: None,
                    footprints: Some(edits),
                    allow_deletion: true,
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains("frozen"), "{error}");
        }
    }

    #[test]
    fn footprint_conflicts_and_wild_current_states() {
        // A pin inside a footprint subtree is unaddressable wild state.
        let mut library = footprint_library();
        {
            let records = &mut library.components[0].records;
            let implementation_native = records
                .iter()
                .position(|record| {
                    matches!(record, SchRecord::Implementation(i) if i.model_name == "FP_A")
                })
                .unwrap() as i32
                + 1;
            let mut wild_pin =
                crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
            wild_pin.owner_index = implementation_native;
            wild_pin.owner_part_id = 1;
            wild_pin.designator = "W".to_owned();
            records.push(SchRecord::Pin(wild_pin));
        }
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(0), SchFootprintEdit::Keep(1)]),
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("owns a Pin record"), "{error}");

        // A graphics member inside a footprint subtree conflicts with a
        // graphics edit in the same batch (and only then).
        let mut library = footprint_library();
        {
            let records = &mut library.components[0].records;
            let implementation_native = records
                .iter()
                .position(|record| {
                    matches!(record, SchRecord::Implementation(i) if i.model_name == "FP_A")
                })
                .unwrap() as i32
                + 1;
            records.push(line(implementation_native, 0));
        }
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        let graphic_count = component.graphics().len();
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: Some((0..graphic_count).map(SchGraphicEdit::Keep).collect()),
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(0), SchFootprintEdit::Keep(1)]),
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("both a graphics() member"), "{error}");
        // Without the graphics edit, the footprint edit owns the record: a
        // member deletion removes the wild graphic with the whole subtree
        // (FP_A carried the sole current, so the survivor takes the flag).
        let removed = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Replace(
                    1,
                    SchFootprintInit {
                        model_name: "FP_B".to_owned(),
                        description: "FP_B description".to_owned(),
                        is_current: true,
                        pin_pad_maps: None,
                    },
                )]),
                allow_deletion: true,
            })
            .unwrap();
        assert_eq!(removed.graphics().len(), graphic_count - 1);

        // Wild multi-current input: pure Keep arrangements (reorders
        // included) pass — they cannot change any is_current value — while a
        // collection-changing batch must land coherent.
        let mut library = footprint_library();
        for record in &mut library.components[0].records {
            if let SchRecord::Implementation(implementation) = record {
                if implementation.model_name == "FP_B" {
                    implementation.is_current = true; // second current
                }
            }
        }
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        let reordered = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(1), SchFootprintEdit::Keep(0)]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&reordered);
        assert!(
            footprints[0].2 && footprints[1].2,
            "wild currents preserved"
        );
        // A non-current New passes through the wild state untouched (no
        // current value is written, so AD26's load repair is unchanged) …
        let extended = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Keep(0),
                    SchFootprintEdit::Keep(1),
                    SchFootprintEdit::New(SchFootprintInit {
                        model_name: "X".to_owned(),
                        description: String::new(),
                        is_current: false,
                        pin_pad_maps: None,
                    }),
                ]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&extended);
        assert_eq!(
            footprints.iter().map(|f| f.2).collect::<Vec<_>>(),
            [true, true, false],
            "wild double-current preserved verbatim under a non-current New"
        );
        // … while an ACTIVE current claim into the incoherent state blocks.
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Keep(0),
                    SchFootprintEdit::Keep(1),
                    SchFootprintEdit::New(SchFootprintInit {
                        model_name: "X".to_owned(),
                        description: String::new(),
                        is_current: true,
                        pin_pad_maps: None,
                    }),
                ]),
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("3 current footprints"), "{error}");
        // The wild state is resolvable in the same batch.
        let resolved = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Keep(0),
                    SchFootprintEdit::Replace(
                        1,
                        SchFootprintInit {
                            model_name: "FP_B".to_owned(),
                            description: "FP_B description".to_owned(),
                            is_current: false,
                            pin_pad_maps: None,
                        },
                    ),
                ]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&resolved);
        assert!(footprints[0].2 && !footprints[1].2);

        // The zero-current wild class (the arthurbenemann-STM32F4 shape: no
        // ISCURRENT key on any footprint): value-preserving Replaces and
        // non-current News pass through verbatim; flipping a value into a
        // still-incoherent state blocks; authoring coherence resolves it.
        let mut library = footprint_library();
        for record in &mut library.components[0].records {
            if let SchRecord::Implementation(implementation) = record {
                implementation.is_current = false;
            }
        }
        let library = finish(library);
        let component = library.snapshot().unwrap().component(0).unwrap();
        let edited = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Replace(
                        0,
                        SchFootprintInit {
                            model_name: "FP_A".to_owned(),
                            description: "zero-current description edit".to_owned(),
                            is_current: false,
                            pin_pad_maps: None,
                        },
                    ),
                    SchFootprintEdit::Keep(1),
                    SchFootprintEdit::New(SchFootprintInit {
                        model_name: "PROBE".to_owned(),
                        description: String::new(),
                        is_current: false,
                        pin_pad_maps: None,
                    }),
                ]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&edited);
        assert_eq!(
            footprints.iter().map(|f| f.2).collect::<Vec<_>>(),
            [false, false, false],
            "the zero-current wild state passes through untouched edits"
        );
        // Deleting from the zero-current set writes no current value: the
        // (still zero-current) remainder passes through.
        let shrunk = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![SchFootprintEdit::Keep(1)]),
                allow_deletion: true,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&shrunk);
        assert_eq!(footprints.len(), 1);
        assert!(!footprints[0].2);
        // Active writes must land coherent even over a wild input: two
        // authored currents block …
        let error = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Replace(
                        0,
                        SchFootprintInit {
                            model_name: "FP_A".to_owned(),
                            description: "FP_A description".to_owned(),
                            is_current: true,
                            pin_pad_maps: None,
                        },
                    ),
                    SchFootprintEdit::Keep(1),
                    SchFootprintEdit::New(SchFootprintInit {
                        model_name: "PROBE".to_owned(),
                        description: String::new(),
                        is_current: true,
                        pin_pad_maps: None,
                    }),
                ]),
                allow_deletion: false,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("2 current footprints"), "{error}");
        // … while one authored current resolves the wild state (exactly the
        // repair AD26's own load would apply to the sole/first footprint).
        let resolved = component
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::Replace(
                        0,
                        SchFootprintInit {
                            model_name: "FP_A".to_owned(),
                            description: "FP_A description".to_owned(),
                            is_current: true,
                            pin_pad_maps: None,
                        },
                    ),
                    SchFootprintEdit::Keep(1),
                ]),
                allow_deletion: false,
            })
            .unwrap();
        let (footprints, _) = footprint_projection(&resolved);
        assert!(footprints[0].2 && !footprints[1].2);
    }
}
