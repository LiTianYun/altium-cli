//! Batch structural editing of the `parameters()` and `graphics()` child
//! collections (step-2 increment B; contract authority
//! `docs/worklogs/next-child-contracts-2026-09-16.md`, cited as "memo §N").
//!
//! # Batch model
//!
//! [`SchLibComponentSnapshot::with_children`] takes, per edited collection, the
//! FINAL ordered sequence of items. Every `Keep`/`Replace` index is a position
//! in the corresponding A2 projection of the IMMUTABLE input snapshot
//! (`parameters()[i]` / `graphics()[i]`); multiple edits never shift each
//! other's targets. Referencing the same original slot twice is a conflict.
//! Omission of an original member from the final sequence is a deletion and
//! requires the explicit per-call `allow_deletion` authorization.
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
//! empty collection (parameters extras before graphics extras when both
//! collections append to an empty tail). The resulting A2 projection of the
//! edited collection equals the requested final sequence exactly.
//!
//! Originals never migrate between streams (verified loader restriction), and
//! creating NEW records in the Additional stream is unsupported: a final
//! arrangement that would require either is a hard error.
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
//! parameters (DefaultNet/HiddenNetName/PinSelectionMemory/PinUniqueId), and
//! the implementation compatibility chain (ImplementationList, Implementation,
//! ImplementationMap, MapDefiner, ParameterList — implementations are removed
//! from the objectList before AD26's save-time numbering). Newly created field
//! objects are written with −1 (memo §6 rule 1); other new children receive
//! their final sibling position through the renumbering above.
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
//! `SchParameterInit` (name/text/is_hidden) and `SchGraphicInit` (owner part,
//! display mode, and the 13 representable kinds' view fields). Replace
//! preserves every non-addressed field of the original record verbatim
//! (unique_id, style_id, unrepresentable extras); a Replace target's kind must
//! equal the replacement's kind — no cross-kind mapping exists, and
//! intent-unrepresentable members (Symbol/Hyperlink/other) can only be kept or
//! deleted. New records get typed wire defaults, a fresh UniqueId (where the
//! kind carries one), `owner_part_id` per memo §2 (parameters → −1; graphics →
//! the part they are created for, 1..=effective), `owner_part_display_mode`
//! per memo §4 (default 0, bounded by `display_mode_count`), and style_id 0
//! (authoring a style id is the memo §8b hard error; no init exposes one).
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
//! default entry is a library-level namespace change owned by increment D's
//! declared-transition ops, so it is a hard error here until D lands. NOTE:
//! `SchParameterInit` mirrors `SchParameterView` and therefore has no font
//! field, so in a library whose captured table lacks the native default
//! entry, parameter creation is impossible until increment D lands (the hard
//! error names that limitation). Embedded image records must reference an
//! existing captured storage asset (memo §8c rule 2); external-path images
//! are exempt.

use std::collections::{HashMap, HashSet};

use super::views::is_graphic_like;
use super::{SchGraphicDetail, SchGraphicView, SchLibComponentSnapshot, SchOwnerGroup};
use crate::param_collection::ParameterCollection;
use crate::param_value::SchAngle;
use crate::sch_records::{
    SchArc, SchBezier, SchEllipse, SchEllipticalArc, SchImage, SchLabel, SchLibComponent, SchLine,
    SchParameter, SchPie, SchPolygon, SchPolyline, SchRecord, SchRectangle, SchRoundRectangle,
    SchTextFrame,
};
use crate::schlib::ownership::{
    RecordAddress, RecordItem, RecordOwner, RecordStream, assemble_records, owner_of,
};
use crate::schlib::validation::{
    WrittenOwnerPartClass, check_embedded_image_reference, check_font_id_exists,
    check_style_id_write, check_written_display_mode, check_written_owner_part_id,
    find_or_create_default_font,
};
use crate::util::generate_unique_id;
use crate::{AltiumFormatError, Result, ResultExt};
use altium_format_types::{
    Color, Coord, CoordPoint, HorizontalAlign, LineShape, LineStyle, PenWidth, RotationBy90,
    TextJustification,
};

/// One batch of structural edits over the parameters and graphics collections,
/// applied through a single assembly pass. `None` leaves a collection
/// completely untouched; `Some(vec![])` deletes every member (authorization
/// required).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchChildBatch {
    /// Final ordered `parameters()` sequence, or `None` to leave it untouched.
    pub parameters: Option<Vec<SchParameterEdit>>,
    /// Final ordered `graphics()` sequence, or `None` to leave it untouched.
    pub graphics: Option<Vec<SchGraphicEdit>>,
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

/// The two structurally editable collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Collection {
    Parameters,
    Graphics,
}

impl Collection {
    fn name(self) -> &'static str {
        match self {
            Self::Parameters => "parameters",
            Self::Graphics => "graphics",
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
/// batch-local id of a created record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Identity {
    Original(RecordStream2, usize),
    New(usize),
}

/// `RecordStream` mirror with derives needed for identity ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum RecordStream2 {
    Data,
    Additional,
}

fn identity_of(address: RecordAddress) -> Identity {
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
    /// Applies one batch of structural edits to the parameters and graphics
    /// collections; see the module documentation for the complete contract
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

        // Working copy with Replace field-mutations applied in place: indices
        // and the ownership graph are unchanged, so every later resolution
        // still refers to the immutable input addressing.
        let mut working = component.clone();
        for finals in [&param_finals, &graphic_finals].into_iter().flatten() {
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

        let plan = plan_streams(
            component,
            &param_members,
            &graphic_members,
            param_finals,
            graphic_finals,
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
                             stream; creating new parameters/graphics in the Additional stream \
                             is unsupported (no verified native creation path exists)"
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
                "SchParameterInit has no font field, so parameter creation in this library \
                 is impossible until increment D lands",
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
    /// namespace change owned by increment D and hard-errors here, with
    /// caller-specific advice (parameters have no authorable font field, so
    /// their advice cannot suggest one).
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
                     change that requires increment D's declared namespace transition — \
                     {advice}",
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

/// Planned final content of one stream.
enum PlannedEntry {
    /// An original input record (possibly field-replaced in the working copy).
    Original(RecordAddress),
    /// A record created by this batch.
    Created(SchRecord),
}

struct PlannedStreams {
    data: Vec<PlannedEntry>,
    additional: Vec<PlannedEntry>,
}

/// Applies the stream-order rule: non-members keep their original relative
/// order; each edited collection's final sequence occupies its members'
/// original stream positions in encounter order, extras following the last
/// original member (or the end of the Data stream for an empty membership).
fn plan_streams(
    component: &SchLibComponent,
    param_members: &[RecordAddress],
    graphic_members: &[RecordAddress],
    param_finals: Option<Vec<FinalRecord>>,
    graphic_finals: Option<Vec<FinalRecord>>,
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

    let mut data = Vec::new();
    let mut additional = Vec::new();
    for (address, _) in all_records(component) {
        let output = match address.stream {
            RecordStream::Data => &mut data,
            RecordStream::Additional => &mut additional,
        };
        let slot = if let Some(ordinal) = param_ordinals.get(&address) {
            params.as_mut().map(|collection| (collection, *ordinal))
        } else if let Some(ordinal) = graphic_ordinals.get(&address) {
            graphics.as_mut().map(|collection| (collection, *ordinal))
        } else {
            None
        };
        let Some((collection, ordinal)) = slot else {
            // Not a member of an edited collection: preserved in place.
            output.push(PlannedEntry::Original(address));
            continue;
        };
        if let Some(item) = collection.finals[ordinal].take() {
            output.push(planned(item));
        }
        // Extras follow the last original member of their collection.
        if ordinal + 1 == collection.members.len() {
            for item in std::mem::take(&mut collection.extras) {
                output.push(planned(item));
            }
        }
    }
    // Previously empty collections append their extras at the end of the Data
    // stream, parameters before graphics.
    for collection in [&mut params, &mut graphics].into_iter().flatten() {
        if collection.members.is_empty() {
            for item in std::mem::take(&mut collection.extras) {
                data.push(planned(item));
            }
        }
    }
    Ok(PlannedStreams { data, additional })
}

fn planned(item: FinalRecord) -> PlannedEntry {
    match item {
        FinalRecord::Existing { address, .. } => PlannedEntry::Original(address),
        FinalRecord::Created(record) => PlannedEntry::Created(record),
    }
}

/// Converts one planned stream into assembly items plus the parallel identity
/// list used by the IndexInSheet renumbering.
fn to_items(entries: &[PlannedEntry]) -> Result<(Vec<RecordItem>, Vec<Identity>)> {
    let mut items = Vec::with_capacity(entries.len());
    let mut ids = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        match entry {
            PlannedEntry::Original(address) => {
                items.push(RecordItem::Original(*address));
                ids.push(identity_of(*address));
            }
            PlannedEntry::Created(record) => {
                items.push(RecordItem::New {
                    id: position,
                    record: record.clone(),
                    owner: RecordOwner::Root,
                });
                ids.push(Identity::New(position));
            }
        }
    }
    Ok((items, ids))
}

/// Memo §6 rule 2: renumber ALL in-objectList children of owners whose child
/// sequence the batch structurally touched to final 0-based sibling positions;
/// untouched owners keep their stored values verbatim.
fn renumber_touched_owners(
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
        let owner_is_root = owner_address.is_none();
        let owner_is_pin = owner_address.is_some_and(|owner_address| {
            matches!(
                record_slot(final_component, owner_address),
                SchRecord::Pin(_)
            )
        });
        if !occupies_object_list_slot(record, owner_is_root, owner_is_pin) {
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
/// implementation compatibility chain, and pin-owned special parameters never
/// do — AD26 either never attaches them to an objectList or removes them
/// before numbering (memo §6; SchDataParameter/SchDataDesignator UpdateOwner;
/// SchDataComponent.AddToListForSave). The name checks are deliberately
/// owner-conditional, NARROWER than AD26's name-first import dispatch: a
/// special-named parameter whose owner lacks the matching interface is
/// attached nowhere by AD26 and silently dropped on its next save; we
/// preserve such records and number them as ordinary siblings instead (see
/// the module-level IndexInSheet deviation note).
fn occupies_object_list_slot(record: &SchRecord, owner_is_root: bool, owner_is_pin: bool) -> bool {
    match record {
        SchRecord::Designator(_) => false,
        SchRecord::ImplementationList(_)
        | SchRecord::Implementation(_)
        | SchRecord::ImplementationMap(_)
        | SchRecord::MapDefiner(_)
        | SchRecord::ParameterList(_) => false,
        SchRecord::Parameter(parameter) => {
            if owner_is_root && parameter.name.eq_ignore_ascii_case("Comment") {
                return false;
            }
            if owner_is_pin
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
        // font until increment D's namespace transitions.
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
        assert!(error.contains("increment D"), "{error}");
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
}
