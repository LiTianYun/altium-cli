//! Read-only child projections over a `SchLibComponentSnapshot`.
//!
//! Views are cheap projections over intact typed state: they borrow strings and
//! slices and copy only small scalar fields. No native record type appears in
//! any public signature. Projection is raw-truthful over loaded data
//! (resolutions memo `docs/worklogs/next-child-contracts-2026-09-16.md` §1):
//! wild owner values classify as their raw meaning class and are never
//! repaired, filtered out, or remapped to intent scopes here.
//!
//! Child encounter order is the Data stream in record order followed by the
//! Additional stream in record order. The corpus carries zero Additional-stream
//! records, so on every real fixture this equals plain Data encounter order.

use super::SchLibComponentSnapshot;
use crate::sch_records::{SchRecord, split_swap_id_part_and_part_pin};
use crate::schlib::ownership::{RecordAddress, RecordStream, owner_of};
use crate::{Result, ResultExt};
use altium_format_types::{
    Color, Coord, CoordPoint, HorizontalAlign, LineShape, LineStyle, PenWidth, PinElectricalType,
    RotationBy90, TextJustification,
};

/// Raw-truthful owner-group classification of a child's native `owner_part_id`
/// (resolutions §1). The native value domain has exactly three meaning classes
/// plus preserved out-of-range content:
///
/// - `PartIndependent` — native −1, the "no part" sentinel carried by root text
///   children (Designator/Parameter); the part filter never filters them.
/// - `SharedPins` — native 0, the common-to-all-parts wildcard. Natively this
///   is a wildcard for PINS ONLY; a non-pin child carrying 0 (outside the
///   implementation chain) has no shared rendering form and is preserved-only
///   content.
/// - `Part(n)` — a numbered part, `1..=effective` (native count − 1). Intent
///   `part N` maps to owner_part_id `N` directly; the native/effective +1
///   offset lives only in the count field, never on child records.
/// - `OutOfRange(raw)` — any other loaded value (`≥ native count` or `< −1`).
///   Such children exist in the wild, must keep loading, and are
///   intent-unaddressable preserved content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchOwnerGroup {
    PartIndependent,
    SharedPins,
    Part(i32),
    OutOfRange(i32),
}

impl SchOwnerGroup {
    /// Classifies a raw native `owner_part_id` against a component's NATIVE
    /// part count (effective + 1). Total over all inputs; never an error and
    /// never a repair.
    pub fn classify(owner_part_id: i32, native_part_count: i32) -> Self {
        match owner_part_id {
            -1 => Self::PartIndependent,
            0 => Self::SharedPins,
            n if n >= 1 && n < native_part_count => Self::Part(n),
            n => Self::OutOfRange(n),
        }
    }
}

/// Derived part swap-group state (resolutions §5 rule 1). Never stored: the
/// group is the unanimous `SwapIdPart` prefix of the part's non-owner-0 pins
/// across all display modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchPartSwapGroup {
    /// No eligible pins, or a unanimous empty prefix (natively "no group").
    None,
    /// All eligible pins carry this same non-empty prefix.
    Group(String),
    /// Eligible pins disagree; the file holds a distinction that a part-level
    /// value cannot express. Elaboration must block on this state rather than
    /// overwrite it.
    Mixed,
}

/// One effective part slot (1-based numbering).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchPartView {
    pub number: i32,
    pub swap_group: SchPartSwapGroup,
}

/// One pin in child encounter order. Pins align positionally; `designator` is
/// the authored matching aid, not an identity key.
#[derive(Debug, Clone, PartialEq)]
pub struct SchPinView<'a> {
    pub owner_group: SchOwnerGroup,
    pub owner_part_display_mode: i32,
    pub designator: &'a str,
    pub name: &'a str,
    pub electrical: PinElectricalType,
    pub length: Coord,
    pub location: CoordPoint,
    pub orientation: RotationBy90,
    pub is_hidden: bool,
    pub hidden_net_name: &'a str,
    /// `SwapIdPin` (pin swap group).
    pub swap_group: &'a str,
    /// `SwapIdPart` — the split prefix of the raw combined field.
    pub part_swap_group: &'a str,
    /// `SwapIdPartPin` — present only when the raw combined field carries a
    /// literal separator (which may enclose an empty half; resolutions §5).
    pub part_pin_swap_suffix: Option<&'a str>,
    /// `PairSwapID` (PinMiscData sidecar).
    pub pair_swap_group: &'a str,
}

/// One ROOT-owned component parameter (owner resolves to the component root).
/// `name` is the parent-scoped alignment key.
#[derive(Debug, Clone, PartialEq)]
pub struct SchParameterView<'a> {
    pub owner_group: SchOwnerGroup,
    pub owner_part_display_mode: i32,
    pub name: &'a str,
    pub text: &'a str,
    pub is_hidden: bool,
}

/// One graphic-like child in child encounter order (positional alignment).
#[derive(Debug, Clone, PartialEq)]
pub struct SchGraphicView<'a> {
    pub owner_group: SchOwnerGroup,
    pub owner_part_display_mode: i32,
    /// Native unique id where the record kind carries one.
    pub unique_id: Option<&'a str>,
    pub detail: SchGraphicDetail<'a>,
}

/// Intent-addressable fields per graphic kind, plus explicit unrepresentable
/// variants. `Symbol` and `Hyperlink` (and any other graphic-like record kind)
/// are preserved-only content: they appear in the ordered collection so that
/// positional addressing and managed-scope checks can see them, but no intent
/// can author or replace them.
#[derive(Debug, Clone, PartialEq)]
pub enum SchGraphicDetail<'a> {
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
        vertices: &'a [CoordPoint],
        line_width: PenWidth,
        line_style: LineStyle,
        start_line_shape: LineShape,
        end_line_shape: LineShape,
        line_shape_size: PenWidth,
        color: Color,
    },
    Polygon {
        vertices: &'a [CoordPoint],
        line_width: PenWidth,
        color: Color,
        area_color: Color,
        is_solid: bool,
        transparent: bool,
    },
    Bezier {
        vertices: &'a [CoordPoint],
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
        file_name: &'a str,
    },
    Label {
        at: CoordPoint,
        orientation: RotationBy90,
        justification: TextJustification,
        color: Color,
        font_id: i32,
        text: &'a str,
        is_mirrored: bool,
        url: &'a str,
    },
    TextFrame {
        from: CoordPoint,
        to: CoordPoint,
        line_width: PenWidth,
        color: Color,
        area_color: Color,
        text_color: Color,
        font_id: i32,
        is_solid: bool,
        show_border: bool,
        alignment: HorizontalAlign,
        word_wrap: bool,
        clip_to_rect: bool,
        text: &'a str,
        text_margin: Coord,
        transparent: bool,
    },
    /// Intent-unrepresentable; preserved verbatim through any patch.
    Symbol,
    /// Intent-unrepresentable; preserved verbatim through any patch.
    Hyperlink,
    /// Any other graphic-like record kind: intent-unrepresentable, preserved
    /// verbatim. `kind` is a diagnostic label, never an addressing key.
    Other { kind: &'static str },
}

impl SchGraphicDetail<'_> {
    /// Whether an authored intent can represent (and therefore replace) this
    /// child. Unrepresentable children block managed complete/positional scope
    /// over the collection at elaboration.
    pub fn is_intent_representable(&self) -> bool {
        !matches!(self, Self::Symbol | Self::Hyperlink | Self::Other { .. })
    }

    /// Stable diagnostic label for the kind.
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
            Self::Symbol => "Symbol",
            Self::Hyperlink => "Hyperlink",
            Self::Other { kind } => kind,
        }
    }
}

/// The footprint collection projection: PCBLIB-model implementations only,
/// with an explicit count of implementation records preserved outside the
/// collection (non-PCBLIB models such as SIM; resolutions/plan A2 §2).
#[derive(Debug, Clone, PartialEq)]
pub struct SchFootprintMapsView<'a> {
    pub footprints: Vec<SchFootprintMapView<'a>>,
    /// Implementation records whose model type is not PCBLIB. They are
    /// outside-collection preserved content; complete-scope footprint edits
    /// never touch them.
    pub preserved_non_pcblib_implementations: usize,
}

/// One PCBLIB implementation. `model_name` is the parent-scoped alignment key.
#[derive(Debug, Clone, PartialEq)]
pub struct SchFootprintMapView<'a> {
    pub owner_group: SchOwnerGroup,
    pub owner_part_display_mode: i32,
    pub model_name: &'a str,
    pub description: &'a str,
    pub is_current: bool,
    pub unique_id: &'a str,
    pub pin_pad_maps: Vec<SchPinPadMapView<'a>>,
}

/// One MapDefiner entry under a footprint's ImplementationMap.
#[derive(Debug, Clone, PartialEq)]
pub struct SchPinPadMapView<'a> {
    /// Pin designator interface string (`DesIntf`).
    pub pin: &'a str,
    /// Pad designators (`DesImp{i}`); native files may carry several.
    pub pads: &'a [String],
}

/// One captured font-table entry from the reference namespace, projecting
/// every native font axis (ids are 1-based positional).
#[derive(Debug, Clone, PartialEq)]
pub struct SchFontEntryView<'a> {
    pub id: i32,
    pub name: &'a str,
    pub size: i32,
    pub rotation: i32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

impl SchLibComponentSnapshot {
    /// Enumerates children in canonical child encounter order: Data stream
    /// records, then Additional stream records.
    fn child_records(&self) -> impl Iterator<Item = (RecordAddress, &SchRecord)> {
        let data = self.component.records.iter().enumerate().map(|(index, r)| {
            (
                RecordAddress {
                    stream: RecordStream::Data,
                    index,
                },
                r,
            )
        });
        let additional = self
            .component
            .additional_records
            .iter()
            .enumerate()
            .map(|(index, r)| {
                (
                    RecordAddress {
                        stream: RecordStream::Additional,
                        index,
                    },
                    r,
                )
            });
        data.chain(additional)
    }

    fn native_part_count(&self) -> i32 {
        self.component.component.part_count
    }

    /// Effective part slots `1..=effective_part_count()`, each with its derived
    /// swap-group projection (resolutions §5 rule 1): the unanimous
    /// `SwapIdPart` prefix of the part's non-owner-0 pins across all display
    /// modes; disagreement projects `Mixed`; no eligible pins (or a unanimous
    /// empty prefix) projects `None`.
    pub fn parts(&self) -> Result<Vec<SchPartView>> {
        let effective = self
            .effective_part_count()
            .with_context(|| format!("projecting parts of component '{}'", self.lib_reference()))?;
        Ok((1..=effective)
            .map(|number| SchPartView {
                number,
                swap_group: self.derived_part_swap_group(number),
            })
            .collect())
    }

    fn derived_part_swap_group(&self, part: i32) -> SchPartSwapGroup {
        let mut unanimous: Option<&str> = None;
        for (_, record) in self.child_records() {
            let SchRecord::Pin(pin) = record else {
                continue;
            };
            // Owner-0 (shared) pins are excluded from part swap semantics
            // natively; membership is exact part equality.
            if pin.owner_part_id != part {
                continue;
            }
            let (prefix, _) = split_swap_id_part_and_part_pin(&pin.swap_id_part);
            match unanimous {
                None => unanimous = Some(prefix),
                Some(existing) if existing == prefix => {}
                Some(_) => return SchPartSwapGroup::Mixed,
            }
        }
        match unanimous {
            None | Some("") => SchPartSwapGroup::None,
            Some(prefix) => SchPartSwapGroup::Group(prefix.to_owned()),
        }
    }

    /// All pin children in child encounter order, with raw-truthful owner
    /// groups and the native swap-string split applied on projection only (the
    /// stored combined string stays raw on untouched pins).
    pub fn pins(&self) -> Vec<SchPinView<'_>> {
        let native = self.native_part_count();
        self.child_records()
            .filter_map(|(_, record)| match record {
                SchRecord::Pin(pin) => {
                    let (part_swap_group, part_pin_swap_suffix) =
                        split_swap_id_part_and_part_pin(&pin.swap_id_part);
                    Some(SchPinView {
                        owner_group: SchOwnerGroup::classify(pin.owner_part_id, native),
                        owner_part_display_mode: i32::from(pin.owner_part_display_mode),
                        designator: &pin.designator,
                        name: &pin.name,
                        electrical: pin.electrical,
                        length: pin.pin_length,
                        location: pin.location,
                        orientation: pin.orientation,
                        is_hidden: pin.is_hidden,
                        hidden_net_name: &pin.hidden_net_name,
                        swap_group: &pin.swap_id_pin,
                        part_swap_group,
                        part_pin_swap_suffix,
                        pair_swap_group: &pin.swap_id_pair,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// ROOT-owned Parameter records only, in child encounter order: a record
    /// participates exactly when its owner resolves to the component root.
    /// Parameters owned by a ParameterList (or any other record) are
    /// implementation/preserved content, not component parameters — even when
    /// their names collide with root parameters.
    pub fn parameters(&self) -> Result<Vec<SchParameterView<'_>>> {
        let native = self.native_part_count();
        let mut views = Vec::new();
        for (address, record) in self.child_records() {
            let SchRecord::Parameter(parameter) = record else {
                continue;
            };
            let owner = owner_of(&self.component, address, record).with_context(|| {
                format!(
                    "projecting parameters of component '{}'",
                    self.lib_reference()
                )
            })?;
            if owner.is_none() {
                views.push(SchParameterView {
                    owner_group: SchOwnerGroup::classify(parameter.base.owner_part_id, native),
                    owner_part_display_mode: parameter.base.owner_part_display_mode,
                    name: &parameter.name,
                    text: &parameter.text,
                    is_hidden: parameter.is_hidden,
                });
            }
        }
        Ok(views)
    }

    /// One ordered collection over child encounter order including ALL
    /// graphic-like kinds. Pin, Designator, Parameter, and the implementation
    /// compatibility chain (ImplementationList/Implementation/
    /// ImplementationMap/MapDefiner/ParameterList) are not graphic-like and
    /// are excluded; every other child kind appears, with Symbol/Hyperlink and
    /// unknown kinds marked intent-unrepresentable.
    pub fn graphics(&self) -> Vec<SchGraphicView<'_>> {
        let native = self.native_part_count();
        self.child_records()
            .filter_map(|(_, record)| graphic_view(record, native))
            .collect()
    }

    /// Per-component alias names from the library index entry, in stored
    /// order. The alias string itself is the alignment key.
    pub fn aliases(&self) -> impl ExactSizeIterator<Item = &str> {
        self.index.aliases.iter().map(String::as_str)
    }

    /// PCBLIB-model implementations flattened through the native compatibility
    /// chain (ImplementationList → Implementation → ImplementationMap →
    /// MapDefiner), plus an explicit count of preserved non-PCBLIB
    /// implementations.
    pub fn footprint_maps(&self) -> Result<SchFootprintMapsView<'_>> {
        let native = self.native_part_count();
        // Owner edges over both streams in encounter order.
        let mut children: Vec<(RecordAddress, Option<RecordAddress>, &SchRecord)> = Vec::new();
        for (address, record) in self.child_records() {
            let owner = owner_of(&self.component, address, record).with_context(|| {
                format!(
                    "projecting footprint maps of component '{}'",
                    self.lib_reference()
                )
            })?;
            children.push((address, owner, record));
        }
        let children_of = |parent: RecordAddress| {
            children
                .iter()
                .filter(move |(_, owner, _)| *owner == Some(parent))
        };
        let mut footprints = Vec::new();
        let mut preserved_non_pcblib_implementations = 0usize;
        for (address, _, record) in &children {
            let SchRecord::Implementation(implementation) = record else {
                continue;
            };
            if !implementation.model_type.eq_ignore_ascii_case("PCBLIB") {
                preserved_non_pcblib_implementations += 1;
                continue;
            }
            let mut pin_pad_maps = Vec::new();
            for (map_address, _, map_record) in children_of(*address) {
                if !matches!(map_record, SchRecord::ImplementationMap(_)) {
                    continue;
                }
                for (_, _, definer_record) in children_of(*map_address) {
                    if let SchRecord::MapDefiner(definer) = definer_record {
                        pin_pad_maps.push(SchPinPadMapView {
                            pin: &definer.des_intf,
                            pads: &definer.des_imps,
                        });
                    }
                }
            }
            footprints.push(SchFootprintMapView {
                owner_group: SchOwnerGroup::classify(implementation.base.owner_part_id, native),
                owner_part_display_mode: implementation.base.owner_part_display_mode,
                model_name: &implementation.model_name,
                description: &implementation.description,
                is_current: implementation.is_current,
                unique_id: &implementation.unique_id,
                pin_pad_maps,
            });
        }
        Ok(SchFootprintMapsView {
            footprints,
            preserved_non_pcblib_implementations,
        })
    }

    /// Captured font-table entries (every native axis) from the reference
    /// namespace this snapshot was taken in.
    pub fn captured_fonts(&self) -> impl ExactSizeIterator<Item = SchFontEntryView<'_>> {
        self.reference_context
            .fonts
            .iter()
            .map(|font| SchFontEntryView {
                id: font.id,
                name: &font.name,
                size: font.size,
                rotation: font.rotation,
                bold: font.bold,
                italic: font.italic,
                underline: font.underline,
                strikeout: font.strikeout,
            })
    }

    /// Captured image-asset file names, in `/Storage` order.
    pub fn captured_image_asset_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.reference_context
            .image_assets
            .iter()
            .map(|(name, _)| name.as_str())
    }

    /// Whether the captured reference namespace carries any display-style
    /// table entries. Style semantics are unproven (resolutions §8b); only
    /// presence is projected.
    pub fn captured_display_styles_present(&self) -> bool {
        !self.reference_context.display_settings.styles.is_empty()
    }
}

/// Membership predicate of the ordered `graphics()` collection. Not
/// graphic-like: pins, root text projections, and the implementation
/// compatibility chain, each covered by its own projection or scalar API.
/// The structural batch engine (`children.rs`) uses the same predicate so the
/// edited collection and the projection agree exactly.
pub(super) fn is_graphic_like(record: &SchRecord) -> bool {
    !matches!(
        record,
        SchRecord::Pin(_)
            | SchRecord::Designator(_)
            | SchRecord::Parameter(_)
            | SchRecord::ImplementationList(_)
            | SchRecord::Implementation(_)
            | SchRecord::ImplementationMap(_)
            | SchRecord::MapDefiner(_)
            | SchRecord::ParameterList(_)
    )
}

fn graphic_view<'a>(record: &'a SchRecord, native_part_count: i32) -> Option<SchGraphicView<'a>> {
    if !is_graphic_like(record) {
        return None;
    }
    let detail = match record {
        SchRecord::Line(line) => SchGraphicDetail::Line {
            from: line.location,
            to: line.corner,
            line_width: line.line_width,
            line_style: line.line_style,
            color: line.color,
        },
        SchRecord::Rectangle(rectangle) => SchGraphicDetail::Rectangle {
            from: rectangle.location,
            to: rectangle.corner,
            line_width: rectangle.line_width,
            line_style: rectangle.line_style,
            color: rectangle.color,
            area_color: rectangle.area_color,
            is_solid: rectangle.is_solid,
            transparent: rectangle.transparent,
        },
        SchRecord::RoundRectangle(rounded) => SchGraphicDetail::RoundRectangle {
            from: rounded.location,
            to: rounded.corner,
            corner_x_radius: rounded.corner_x_radius,
            corner_y_radius: rounded.corner_y_radius,
            line_width: rounded.line_width,
            color: rounded.color,
            area_color: rounded.area_color,
            is_solid: rounded.is_solid,
        },
        SchRecord::Arc(arc) => SchGraphicDetail::Arc {
            center: arc.location,
            radius: arc.radius,
            line_width: arc.line_width,
            start_angle: arc.start_angle.0,
            end_angle: arc.end_angle.map(|angle| angle.0),
            color: arc.color,
        },
        SchRecord::EllipticalArc(arc) => SchGraphicDetail::EllipticalArc {
            center: arc.location,
            radius: arc.radius,
            secondary_radius: arc.secondary_radius,
            line_width: arc.line_width,
            start_angle: arc.start_angle.0,
            end_angle: arc.end_angle.map(|angle| angle.0),
            color: arc.color,
        },
        SchRecord::Ellipse(ellipse) => SchGraphicDetail::Ellipse {
            center: ellipse.location,
            radius: ellipse.radius,
            secondary_radius: ellipse.secondary_radius,
            line_width: ellipse.line_width,
            color: ellipse.color,
            area_color: ellipse.area_color,
            is_solid: ellipse.is_solid,
            transparent: ellipse.transparent,
        },
        SchRecord::Pie(pie) => SchGraphicDetail::Pie {
            center: pie.location,
            radius: pie.radius,
            line_width: pie.line_width,
            start_angle: pie.start_angle.0,
            end_angle: pie.end_angle.map(|angle| angle.0),
            color: pie.color,
            area_color: pie.area_color,
            is_solid: pie.is_solid,
        },
        SchRecord::Polyline(polyline) => SchGraphicDetail::Polyline {
            vertices: &polyline.vertices,
            line_width: polyline.line_width,
            line_style: polyline.line_style,
            start_line_shape: polyline.start_line_shape,
            end_line_shape: polyline.end_line_shape,
            line_shape_size: polyline.line_shape_size,
            color: polyline.color,
        },
        SchRecord::Polygon(polygon) => SchGraphicDetail::Polygon {
            vertices: &polygon.vertices,
            line_width: polygon.line_width,
            color: polygon.color,
            area_color: polygon.area_color,
            is_solid: polygon.is_solid,
            transparent: polygon.transparent,
        },
        SchRecord::Bezier(bezier) => SchGraphicDetail::Bezier {
            vertices: &bezier.vertices,
            line_width: bezier.line_width,
            color: bezier.color,
        },
        SchRecord::Image(image) => SchGraphicDetail::Image {
            from: image.location,
            to: image.corner,
            orientation: image.orientation,
            line_width: image.line_width,
            color: image.color,
            is_solid: image.is_solid,
            keep_aspect: image.keep_aspect,
            embed_image: image.embed_image,
            file_name: &image.file_name,
        },
        SchRecord::Label(label) => SchGraphicDetail::Label {
            at: label.location,
            orientation: label.orientation,
            justification: label.justification,
            color: label.color,
            font_id: label.font_id,
            text: &label.text,
            is_mirrored: label.is_mirrored,
            url: &label.url,
        },
        SchRecord::TextFrame(frame) => SchGraphicDetail::TextFrame {
            from: frame.location,
            to: frame.corner,
            line_width: frame.line_width,
            color: frame.color,
            area_color: frame.area_color,
            text_color: frame.text_color,
            font_id: frame.font_id,
            is_solid: frame.is_solid,
            show_border: frame.show_border,
            alignment: frame.alignment,
            word_wrap: frame.word_wrap,
            clip_to_rect: frame.clip_to_rect,
            text: &frame.text,
            text_margin: frame.text_margin,
            transparent: frame.transparent,
        },
        SchRecord::Symbol(_) => SchGraphicDetail::Symbol,
        SchRecord::Hyperlink(_) => SchGraphicDetail::Hyperlink,
        other => SchGraphicDetail::Other {
            kind: record_kind_label(other),
        },
    };
    let (owner_part_id, owner_part_display_mode) = match record.base() {
        Some(base) => (base.owner_part_id, base.owner_part_display_mode),
        // Component is the only base-less non-pin kind; a nested Component
        // record never passes snapshot ownership validation, but the raw
        // projection stays total rather than panicking.
        None => match record {
            SchRecord::Component(component) => {
                (component.owner_part_id, component.owner_part_display_mode)
            }
            _ => unreachable!("Pin records are filtered out above"),
        },
    };
    Some(SchGraphicView {
        owner_group: SchOwnerGroup::classify(owner_part_id, native_part_count),
        owner_part_display_mode,
        unique_id: record.unique_id(),
        detail,
    })
}

/// Diagnostic labels for graphic-like kinds without a dedicated projection.
fn record_kind_label(record: &SchRecord) -> &'static str {
    match record {
        SchRecord::Sheet(_) => "Sheet",
        SchRecord::Template(_) => "Template",
        SchRecord::Wire(_) => "Wire",
        SchRecord::Bus(_) => "Bus",
        SchRecord::NetLabel(_) => "NetLabel",
        SchRecord::PowerObject(_) => "PowerObject",
        SchRecord::Port(_) => "Port",
        SchRecord::NoConnect(_) => "NoConnect",
        SchRecord::Junction(_) => "Junction",
        SchRecord::SheetName(_) => "SheetName",
        SchRecord::SheetFileName(_) => "SheetFileName",
        SchRecord::SheetSymbol(_) => "SheetSymbol",
        SchRecord::SheetEntry(_) => "SheetEntry",
        SchRecord::BusEntry(_) => "BusEntry",
        SchRecord::ParameterSet(_) => "ParameterSet",
        SchRecord::Note(_) => "Note",
        SchRecord::Probe(_) => "Probe",
        SchRecord::CompileMask(_) => "CompileMask",
        SchRecord::Blanket(_) => "Blanket",
        SchRecord::Component(_) => "Component",
        SchRecord::Pin(_) => "Pin",
        SchRecord::Symbol(_) => "Symbol",
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
        SchRecord::Hyperlink(_) => "Hyperlink",
        SchRecord::Designator(_) => "Designator",
        SchRecord::Parameter(_) => "Parameter",
        SchRecord::TextFrame(_) => "TextFrame",
        SchRecord::ImplementationList(_) => "ImplementationList",
        SchRecord::Implementation(_) => "Implementation",
        SchRecord::ImplementationMap(_) => "ImplementationMap",
        SchRecord::MapDefiner(_) => "MapDefiner",
        SchRecord::ParameterList(_) => "ParameterList",
        SchRecord::HarnessConnector(_) => "HarnessConnector",
        SchRecord::HarnessEntry(_) => "HarnessEntry",
        SchRecord::HarnessConnectorType(_) => "HarnessConnectorType",
        SchRecord::SignalHarness(_) => "SignalHarness",
        SchRecord::HighLevelCodeSymbol(_) => "HighLevelCodeSymbol",
        SchRecord::HighLevelCodeEntry(_) => "HighLevelCodeEntry",
        SchRecord::HighLevelCodeName(_) => "HighLevelCodeName",
        SchRecord::HighLevelCodeFileName(_) => "HighLevelCodeFileName",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param_collection::ParameterCollection;
    use crate::sch_records::{
        SchImage, SchImplementation, SchImplementationList, SchImplementationMap, SchLabel,
        SchLine, SchMapDefiner, SchParameter, SchParameterList, SchRectangle, SchSymbol,
        encode_swap_id_part_and_part_pin, parse_text_pin,
    };
    use crate::schlib::{SchLib, SchLibEmbeddedImage, compute_weight};

    fn pin(
        owner_part_id: i32,
        display_mode: u8,
        designator: &str,
        swap_combined: &str,
    ) -> SchRecord {
        let mut pin = parse_text_pin(&mut ParameterCollection::new()).unwrap();
        pin.owner_part_id = owner_part_id;
        pin.owner_part_display_mode = display_mode;
        pin.designator = designator.to_owned();
        pin.name = format!("N{designator}");
        pin.swap_id_part = swap_combined.to_owned();
        pin.swap_id_pin = "pin-swap".to_owned();
        pin.swap_id_pair = "pair-swap".to_owned();
        SchRecord::Pin(pin)
    }

    /// A component with native PARTCOUNT=3 (effective 2), two display modes,
    /// shared and per-part pins, a corpus-wild out-of-range owner
    /// (OWNERPARTID == PARTCOUNT), unrepresentable graphics, a root parameter
    /// shadowed by an implementation-owned parameter of the same name, a
    /// PCBLIB footprint chain, a preserved SIM implementation, and an
    /// Additional-stream graphic.
    fn library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            let root = &mut aggregate.component;
            root.part_count = 3;
            root.display_mode_count = 2;
            root.custom_display_mode_names = vec![String::new(), "alternate".to_owned()];
            let records = &mut aggregate.records;

            records.push(pin(1, 0, "1", "G1|&|s1"));
            records.push(pin(1, 1, "2", "G1|&|s2"));
            records.push(pin(0, 0, "3", "ZZZ"));
            records.push(pin(2, 0, "4", "A"));
            records.push(pin(2, 0, "5", "B"));

            let mut rectangle = SchRectangle::from_params(&mut ParameterCollection::new()).unwrap();
            rectangle.base.owner_part_id = 1;
            rectangle.base.owner_part_display_mode = 1;
            rectangle.unique_id = "RECT0001".to_owned();
            rectangle.is_solid = true;
            records.push(SchRecord::Rectangle(rectangle));

            // The corpus wild class: OWNERPARTID == native PARTCOUNT.
            let mut wild = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            wild.base.owner_part_id = 3;
            records.push(SchRecord::Line(wild));

            let mut symbol = SchSymbol::from_params(&mut ParameterCollection::new()).unwrap();
            symbol.base.owner_part_id = 1;
            records.push(SchRecord::Symbol(symbol));

            let mut hyperlink = SchLabel::from_params(&mut ParameterCollection::new()).unwrap();
            hyperlink.base.owner_part_id = 2;
            hyperlink.url = "https://example.invalid".to_owned();
            records.push(SchRecord::Hyperlink(hyperlink));

            let mut root_parameter =
                SchParameter::from_params(&mut ParameterCollection::new()).unwrap();
            root_parameter.base.owner_part_id = -1;
            root_parameter.name = "Value".to_owned();
            root_parameter.text = "10k".to_owned();
            root_parameter.is_hidden = true;
            records.push(SchRecord::Parameter(root_parameter));

            let list_native = records.len() as i32 + 1;
            records.push(SchRecord::ImplementationList(
                SchImplementationList::from_params(&mut ParameterCollection::new()).unwrap(),
            ));

            let footprint_native = records.len() as i32 + 1;
            let mut footprint =
                SchImplementation::from_params(&mut ParameterCollection::new()).unwrap();
            footprint.base.owner_index = list_native;
            footprint.model_type = "PcbLib".to_owned();
            footprint.model_name = "R0805".to_owned();
            footprint.description = "resistor body".to_owned();
            footprint.is_current = true;
            footprint.unique_id = "IMPL0001".to_owned();
            records.push(SchRecord::Implementation(footprint));

            let map_native = records.len() as i32 + 1;
            let mut map =
                SchImplementationMap::from_params(&mut ParameterCollection::new()).unwrap();
            map.base.owner_index = footprint_native;
            records.push(SchRecord::ImplementationMap(map));

            let mut definer = SchMapDefiner::from_params(&mut ParameterCollection::new()).unwrap();
            definer.base.owner_index = map_native;
            definer.des_intf = "1".to_owned();
            definer.des_imps = vec!["A1".to_owned(), "B1".to_owned()];
            records.push(SchRecord::MapDefiner(definer));

            let parameter_list_native = records.len() as i32 + 1;
            let mut parameter_list =
                SchParameterList::from_params(&mut ParameterCollection::new()).unwrap();
            parameter_list.base.owner_index = footprint_native;
            records.push(SchRecord::ParameterList(parameter_list));

            // Implementation-owned parameter deliberately named like the root
            // parameter: it must NOT appear in parameters().
            let mut shadowed = SchParameter::from_params(&mut ParameterCollection::new()).unwrap();
            shadowed.base.owner_index = parameter_list_native;
            shadowed.name = "Value".to_owned();
            shadowed.text = "implementation-owned".to_owned();
            records.push(SchRecord::Parameter(shadowed));

            let mut sim = SchImplementation::from_params(&mut ParameterCollection::new()).unwrap();
            sim.base.owner_index = list_native;
            sim.model_type = "SIM".to_owned();
            sim.model_name = "RESISTOR".to_owned();
            records.push(SchRecord::Implementation(sim));

            let mut additional_line =
                SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            additional_line.base.owner_part_id = 1;
            aggregate
                .additional_records
                .push(SchRecord::Line(additional_line));
        }
        library.header.components[0].part_count = 3;
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        library
    }

    #[test]
    fn wild_owner_values_snapshot_fine_and_classify_raw_truthfully() {
        // Regression (resolutions §1/§9): the corpus wild class — a record
        // whose OWNERPARTID equals native PARTCOUNT — and part-0 pins must
        // keep capturing without any new validation.
        let component = library().snapshot().unwrap().component(0).unwrap();

        let pins = component.pins();
        assert_eq!(
            pins.iter().map(|pin| pin.owner_group).collect::<Vec<_>>(),
            vec![
                SchOwnerGroup::Part(1),
                SchOwnerGroup::Part(1),
                SchOwnerGroup::SharedPins,
                SchOwnerGroup::Part(2),
                SchOwnerGroup::Part(2),
            ]
        );
        assert_eq!(pins[1].owner_part_display_mode, 1);
        assert_eq!(pins[0].designator, "1");
        assert_eq!(pins[0].name, "N1");
        assert_eq!(pins[0].swap_group, "pin-swap");
        assert_eq!(pins[0].pair_swap_group, "pair-swap");

        let graphics = component.graphics();
        assert_eq!(
            graphics
                .iter()
                .map(|graphic| (graphic.detail.kind_name(), graphic.owner_group))
                .collect::<Vec<_>>(),
            vec![
                ("Rectangle", SchOwnerGroup::Part(1)),
                ("Line", SchOwnerGroup::OutOfRange(3)),
                ("Symbol", SchOwnerGroup::Part(1)),
                ("Hyperlink", SchOwnerGroup::Part(2)),
                ("Line", SchOwnerGroup::Part(1)),
            ]
        );
        assert_eq!(graphics[0].unique_id, Some("RECT0001"));
        assert_eq!(graphics[0].owner_part_display_mode, 1);
        assert!(matches!(
            graphics[0].detail,
            SchGraphicDetail::Rectangle { is_solid: true, .. }
        ));

        assert_eq!(
            SchOwnerGroup::classify(-1, 3),
            SchOwnerGroup::PartIndependent
        );
        assert_eq!(SchOwnerGroup::classify(0, 3), SchOwnerGroup::SharedPins);
        assert_eq!(SchOwnerGroup::classify(2, 3), SchOwnerGroup::Part(2));
        assert_eq!(SchOwnerGroup::classify(3, 3), SchOwnerGroup::OutOfRange(3));
        assert_eq!(
            SchOwnerGroup::classify(-5, 3),
            SchOwnerGroup::OutOfRange(-5)
        );
    }

    #[test]
    fn symbol_and_hyperlink_project_as_unrepresentable() {
        let component = library().snapshot().unwrap().component(0).unwrap();
        let graphics = component.graphics();
        let representable: Vec<bool> = graphics
            .iter()
            .map(|graphic| graphic.detail.is_intent_representable())
            .collect();
        assert_eq!(representable, vec![true, true, false, false, true]);
        assert!(matches!(graphics[2].detail, SchGraphicDetail::Symbol));
        assert!(matches!(graphics[3].detail, SchGraphicDetail::Hyperlink));
    }

    #[test]
    fn image_graphic_projects_all_intent_axes() {
        // Every authorable Image axis from the intent GraphicFields policies
        // must be visible in the projection — including color, line_width, and
        // is_solid, which the native SchImage record carries alongside the
        // geometry/asset fields.
        let mut library = SchLib::new_blank_ad26().unwrap();
        let mut image = SchImage::from_params(&mut ParameterCollection::new()).unwrap();
        image.base.owner_part_id = 1;
        image.corner = CoordPoint {
            x: Coord::from_mils(300).unwrap(),
            y: Coord::from_mils(200).unwrap(),
        };
        image.orientation = RotationBy90::Rotate90;
        image.line_width = PenWidth::Medium;
        image.color = Color::new(0x0012_3456);
        image.is_solid = true;
        image.keep_aspect = true;
        image.embed_image = false;
        image.file_name = "body.png".to_owned();
        image.unique_id = "IMG00001".to_owned();
        library.components[0].records.push(SchRecord::Image(image));
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();

        let component = library.snapshot().unwrap().component(0).unwrap();
        let graphics = component.graphics();
        assert_eq!(graphics.len(), 1);
        assert_eq!(graphics[0].unique_id, Some("IMG00001"));
        assert_eq!(
            graphics[0].detail,
            SchGraphicDetail::Image {
                from: CoordPoint::zero(),
                to: CoordPoint {
                    x: Coord::from_mils(300).unwrap(),
                    y: Coord::from_mils(200).unwrap(),
                },
                orientation: RotationBy90::Rotate90,
                line_width: PenWidth::Medium,
                color: Color::new(0x0012_3456),
                is_solid: true,
                keep_aspect: true,
                embed_image: false,
                file_name: "body.png",
            }
        );
    }

    #[test]
    fn parts_project_unanimous_swap_prefixes_and_typed_mixed_states() {
        let snapshot = library().snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        // Part 1 pins agree on prefix G1 (across both display modes, with
        // differing suffixes); the owner-0 pin's ZZZ prefix never
        // participates. Part 2 pins disagree → Mixed.
        assert_eq!(
            component.parts().unwrap(),
            vec![
                SchPartView {
                    number: 1,
                    swap_group: SchPartSwapGroup::Group("G1".to_owned()),
                },
                SchPartView {
                    number: 2,
                    swap_group: SchPartSwapGroup::Mixed,
                },
            ]
        );

        // A unanimous EMPTY prefix projects "no group", including the wild
        // literal-separator form whose prefix half is empty.
        let mut cleared = component.clone();
        for record in &mut cleared.component.records {
            if let SchRecord::Pin(pin) = record {
                if pin.owner_part_id == 2 {
                    pin.swap_id_part = if pin.designator == "4" {
                        String::new()
                    } else {
                        "|&|suffix-only".to_owned()
                    };
                }
            }
        }
        assert_eq!(
            cleared.parts().unwrap()[1].swap_group,
            SchPartSwapGroup::None
        );

        // Mojibake separators are NOT repaired: the whole string is the
        // prefix, and unanimity over it projects a defined group.
        let mut mojibake = component.clone();
        for record in &mut mojibake.component.records {
            if let SchRecord::Pin(pin) = record {
                if pin.owner_part_id == 1 {
                    pin.swap_id_part = "?&?group".to_owned();
                }
            }
        }
        assert_eq!(
            mojibake.parts().unwrap()[0].swap_group,
            SchPartSwapGroup::Group("?&?group".to_owned())
        );

        // A part with no eligible pins projects "no group".
        let mut empty = component.clone();
        for record in &mut empty.component.records {
            if let SchRecord::Pin(pin) = record {
                if pin.owner_part_id == 2 {
                    pin.owner_part_id = 1;
                }
            }
        }
        assert_eq!(empty.parts().unwrap()[1].swap_group, SchPartSwapGroup::None);
    }

    #[test]
    fn swap_split_and_encode_follow_native_rules() {
        let split = split_swap_id_part_and_part_pin;
        assert_eq!(split(""), ("", None));
        assert_eq!(split("G1"), ("G1", None));
        assert_eq!(split("G1|&|p"), ("G1", Some("p")));
        assert_eq!(split("|&|"), ("", Some("")));
        assert_eq!(split("|&|x"), ("", Some("x")));
        // First-occurrence split, like the native ordinal IndexOf.
        assert_eq!(split("a|&|b|&|c"), ("a", Some("b|&|c")));
        // Mojibake separators are an ordinary prefix, never repaired.
        assert_eq!(split("?&?g"), ("?&?g", None));

        assert_eq!(encode_swap_id_part_and_part_pin("G1", "p"), "G1|&|p");
        assert_eq!(encode_swap_id_part_and_part_pin("G1", ""), "G1");
        // The native concat cannot regenerate a bare literal separator; wild
        // "|&|" strings survive only because untouched pins are never
        // re-encoded.
        assert_eq!(encode_swap_id_part_and_part_pin("", ""), "");
        assert_eq!(encode_swap_id_part_and_part_pin("", "x"), "|&|x");
    }

    #[test]
    fn literal_separator_swap_strings_survive_projection_and_save_reopen_verbatim() {
        // Minimal library: the rich builder carries a Hyperlink, which the
        // SchLib Data dispatcher does not reopen (never occurs in SchLib
        // corpus); the swap wire path needs only one pin.
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.components[0].records.push(pin(1, 0, "1", "|&|"));
        library.header.weight = compute_weight(&library.header, &library.components);
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let pins = component.pins();
        assert_eq!(pins[0].part_swap_group, "");
        assert_eq!(pins[0].part_pin_swap_suffix, Some(""));
        // Projection never rewrites the stored combined string.
        assert_eq!(snapshot.materialize().unwrap(), library);

        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        let SchRecord::Pin(pin) = &reopened.components[0].records[2] else {
            panic!("reopened pin slot")
        };
        assert_eq!(pin.swap_id_part, "|&|");
    }

    #[test]
    fn parameters_are_root_owned_only_excluding_implementation_owned_same_name() {
        let component = library().snapshot().unwrap().component(0).unwrap();
        let parameters = component.parameters().unwrap();
        // The blank library's root Comment parameter plus the builder's root
        // Value parameter; the implementation-owned Value never appears even
        // though its name collides.
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| parameter.name)
                .collect::<Vec<_>>(),
            vec!["Comment", "Value"]
        );
        assert!(
            parameters
                .iter()
                .all(|parameter| parameter.text != "implementation-owned")
        );
        assert_eq!(parameters[1].text, "10k");
        assert!(parameters[1].is_hidden);
        assert_eq!(parameters[1].owner_group, SchOwnerGroup::PartIndependent);
        assert_eq!(parameters[1].owner_part_display_mode, 0);
    }

    #[test]
    fn footprint_maps_are_pcblib_only_with_explicit_preserved_count() {
        let component = library().snapshot().unwrap().component(0).unwrap();
        let maps = component.footprint_maps().unwrap();
        assert_eq!(maps.preserved_non_pcblib_implementations, 1);
        assert_eq!(maps.footprints.len(), 1);
        let footprint = &maps.footprints[0];
        assert_eq!(footprint.model_name, "R0805");
        assert_eq!(footprint.description, "resistor body");
        assert!(footprint.is_current);
        assert_eq!(footprint.unique_id, "IMPL0001");
        assert_eq!(footprint.pin_pad_maps.len(), 1);
        assert_eq!(footprint.pin_pad_maps[0].pin, "1");
        assert_eq!(
            footprint.pin_pad_maps[0].pads,
            &["A1".to_owned(), "B1".to_owned()]
        );
    }

    #[test]
    fn alias_and_captured_namespace_projections_expose_reference_context() {
        let mut library = library();
        let name = library.header.components[0].lib_ref.clone();
        library.header.components[0]
            .aliases
            .push("ALIAS_A".to_owned());
        library.aliases.push(crate::schlib::SchLibAlias {
            alias_name: "ALIAS_A".to_owned(),
            canonical_name: name,
        });
        library.embedded_images = vec![SchLibEmbeddedImage {
            file_name: "logo.png".to_owned(),
            data: vec![1, 2, 3],
        }];
        library.header.weight = compute_weight(&library.header, &library.components);
        let component = library.snapshot().unwrap().component(0).unwrap();

        assert_eq!(component.aliases().collect::<Vec<_>>(), vec!["ALIAS_A"]);
        let fonts: Vec<_> = component.captured_fonts().collect();
        assert_eq!(fonts.len(), 1);
        assert_eq!(
            fonts[0],
            SchFontEntryView {
                id: 1,
                name: "Times New Roman",
                size: 10,
                rotation: 0,
                bold: false,
                italic: false,
                underline: false,
                strikeout: false,
            }
        );
        assert_eq!(
            component.captured_image_asset_names().collect::<Vec<_>>(),
            vec!["logo.png"]
        );
        assert!(!component.captured_display_styles_present());

        let mut styled = library;
        styled
            .header
            .display_settings
            .styles
            .push(altium_format_types::sch::SchDisplayStyle {
                id: 1,
                ..Default::default()
            });
        let styled_component = styled.snapshot().unwrap().component(0).unwrap();
        assert!(styled_component.captured_display_styles_present());
    }
}
