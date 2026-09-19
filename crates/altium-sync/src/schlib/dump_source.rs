//! Dump-direction spec source generation (step-3 increment C).
//!
//! [`generate_schlib_spec_source`] renders one `.schlib-spec` text from a
//! typed [`SchLibSnapshot`], emitting exactly the values elaboration
//! compares, so that compiling the generated text and elaborating it against
//! the same library is an EMPTY patch by construction (the directional
//! convergence law). Every value is rendered losslessly (`raw` coordinate
//! units where mils are inexact, exact strings, exhaustive enum names); any
//! content the intent language cannot represent or the renderer cannot
//! express exactly is SKIPPED with a recorded diagnostic — never silently —
//! and stays preserved in the library because unauthored content is outside
//! elaboration scope:
//!
//! - graphics groups stop at the first intent-unrepresentable member
//!   (Symbol/Hyperlink/other) or unrenderable value (non-24-bit color,
//!   non-finite/exponent-formatted angle, string with unescapable control
//!   characters) — a partial positional scope preserves the tail;
//! - pin groups are skipped whole on empty or duplicate designators (the
//!   comment-preserving merge keys pins by designator, so duplicates cannot
//!   be reconciled) or unrenderable strings;
//! - unaddressable owner groups (owner-0 pins of a single-part component,
//!   owner-0 graphics, `PartIndependent`/`OutOfRange` owners) are never
//!   authored;
//! - parameters/footprints collections are skipped whole on duplicate keys
//!   under their matching equivalence;
//! - fields whose authored form would differ from preservation (`font_id`,
//!   image `source`, pin `hidden_net_name`/`part_swap_group`/
//!   `pair_swap_group`, footprint pin-pad maps) are never emitted: the
//!   unauthored field preserves the stored value byte-verbatim.
//!
//! Output is deterministic (projection order, no randomness, no
//! annotations) and LF-newlined; the byte-preserving merge honors an
//! existing source's own newline style when editing.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use altium_format::api::{SchGraphicDetail, SchLibComponentSnapshot, SchLibSnapshot};
use altium_format_types::{
    Color, ComponentKind, Coord, CoordPoint, HorizontalAlign, LineShape, LineStyle, PenWidth,
    PinElectricalType, RotationBy90, TextJustification,
};

use super::{SchLibPatchError, parameter_key_fold};

/// One generated spec text plus the recorded skips (surfaced as non-blocking
/// plan diagnostics).
#[derive(Debug, Clone)]
pub struct GeneratedSchLibSpec {
    pub text: String,
    pub skips: Vec<String>,
}

fn format_error(
    context: String,
) -> impl FnOnce(altium_format::AltiumFormatError) -> SchLibPatchError {
    move |source| SchLibPatchError::Format { context, source }
}

/// Renders a string literal with the escapes the spec lexer supports
/// (`\\ \" \n \r \t`). Returns `None` for strings containing other control
/// characters, which the language cannot express.
fn quote(value: &str) -> Option<String> {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch == '\u{7f}' => return None,
            ch => out.push(ch),
        }
    }
    out.push('"');
    Some(out)
}

/// Exact coordinate text: whole mils when exact, `raw` internal units
/// otherwise.
fn coord_text(value: Coord) -> String {
    let raw = value.raw();
    let mil = Coord::ONE_MIL.raw();
    if raw % mil == 0 {
        format!("{}mil", raw / mil)
    } else {
        format!("{raw}raw")
    }
}

fn point_text(value: CoordPoint) -> String {
    format!("({}, {})", coord_text(value.x), coord_text(value.y))
}

/// Exact 24-bit color text; `None` when the stored raw value carries bits
/// outside `#RRGGBB`.
fn color_text(value: Color) -> Option<String> {
    if value.raw() & !0x00FF_FFFF != 0 {
        return None;
    }
    Some(format!(
        "#{:02X}{:02X}{:02X}",
        value.r(),
        value.g(),
        value.b()
    ))
}

/// Exact angle text; `None` for values whose shortest round-trip rendering
/// the spec lexer cannot read back (exponent form, non-finite).
fn angle_text(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let text = format!("{value}");
    if text.contains(['e', 'E']) {
        return None;
    }
    Some(text)
}

#[allow(unreachable_patterns)]
fn electrical_text(value: PinElectricalType) -> Option<&'static str> {
    match value {
        PinElectricalType::Input => Some("input"),
        PinElectricalType::InputOutput => Some("input_output"),
        PinElectricalType::Output => Some("output"),
        PinElectricalType::OpenCollector => Some("open_collector"),
        PinElectricalType::Passive => Some("passive"),
        PinElectricalType::HiZ => Some("hiz"),
        PinElectricalType::OpenEmitter => Some("open_emitter"),
        PinElectricalType::Power => Some("power"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn orientation_text(value: RotationBy90) -> Option<&'static str> {
    match value {
        RotationBy90::Rotate0 => Some("0"),
        RotationBy90::Rotate90 => Some("90"),
        RotationBy90::Rotate180 => Some("180"),
        RotationBy90::Rotate270 => Some("270"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn component_kind_text(value: ComponentKind) -> Option<&'static str> {
    match value {
        ComponentKind::Standard => Some("standard"),
        ComponentKind::Mechanical => Some("mechanical"),
        ComponentKind::Graphical => Some("graphical"),
        ComponentKind::NetTieBom => Some("net_tie_bom"),
        ComponentKind::NetTieNoBom => Some("net_tie_no_bom"),
        ComponentKind::StandardNoBom => Some("standard_no_bom"),
        ComponentKind::Jumper => Some("jumper"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn pen_width_text(value: PenWidth) -> Option<&'static str> {
    match value {
        PenWidth::Zero => Some("zero"),
        PenWidth::Small => Some("small"),
        PenWidth::Medium => Some("medium"),
        PenWidth::Large => Some("large"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn line_style_text(value: LineStyle) -> Option<&'static str> {
    match value {
        LineStyle::Solid => Some("solid"),
        LineStyle::Dashed => Some("dashed"),
        LineStyle::Dotted => Some("dotted"),
        LineStyle::DashDotted => Some("dash_dotted"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn line_shape_text(value: LineShape) -> Option<&'static str> {
    match value {
        LineShape::None => Some("none"),
        LineShape::Arrow => Some("arrow"),
        LineShape::SolidArrow => Some("solid_arrow"),
        LineShape::Tail => Some("tail"),
        LineShape::SolidTail => Some("solid_tail"),
        LineShape::Circle => Some("circle"),
        LineShape::Square => Some("square"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn justification_text(value: TextJustification) -> Option<&'static str> {
    match value {
        TextJustification::BottomLeft => Some("bottom_left"),
        TextJustification::BottomCenter => Some("bottom_center"),
        TextJustification::BottomRight => Some("bottom_right"),
        TextJustification::CenterLeft => Some("center_left"),
        TextJustification::Center => Some("center"),
        TextJustification::CenterRight => Some("center_right"),
        TextJustification::TopLeft => Some("top_left"),
        TextJustification::TopCenter => Some("top_center"),
        TextJustification::TopRight => Some("top_right"),
        _ => None,
    }
}

#[allow(unreachable_patterns)]
fn alignment_text(value: HorizontalAlign) -> Option<&'static str> {
    match value {
        HorizontalAlign::Left => Some("left"),
        HorizontalAlign::Center => Some("center"),
        HorizontalAlign::Right => Some("right"),
        _ => None,
    }
}

fn vertices_text(vertices: &[CoordPoint]) -> String {
    let mut out = String::from("[");
    for (index, vertex) in vertices.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&point_text(*vertex));
    }
    out.push(']');
    out
}

/// One graphic's rendered property list, or `None` (with a reason) when the
/// member cannot be expressed exactly. `font_id` and image `source` are
/// deliberately never emitted (unauthored = preserved verbatim).
fn graphic_lines(detail: &SchGraphicDetail<'_>) -> Result<(&'static str, Vec<String>), String> {
    let color = |value: Color, name: &str| {
        color_text(value).ok_or_else(|| format!("{name} is not a 24-bit RGB value"))
    };
    let angle = |value: f64, name: &str| {
        angle_text(value).ok_or_else(|| format!("{name} has no exact literal rendering"))
    };
    let text_value = |value: &str, name: &str| {
        quote(value).ok_or_else(|| format!("{name} contains unescapable control characters"))
    };
    let named = |value: Option<&'static str>, name: &str| {
        value.ok_or_else(|| format!("{name} has no exact enum rendering"))
    };
    let mut properties: Vec<String> = Vec::new();
    let keyword = match detail {
        SchGraphicDetail::Line {
            from,
            to,
            line_width,
            line_style,
            color: line_color,
        } => {
            properties.push(format!("from: {}", point_text(*from)));
            properties.push(format!("to: {}", point_text(*to)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!(
                "line_style: \"{}\"",
                named(line_style_text(*line_style), "line_style")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            "line"
        }
        SchGraphicDetail::Rectangle {
            from,
            to,
            line_width,
            line_style,
            color: line_color,
            area_color,
            is_solid,
            transparent,
        } => {
            properties.push(format!("from: {}", point_text(*from)));
            properties.push(format!("to: {}", point_text(*to)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!(
                "line_style: \"{}\"",
                named(line_style_text(*line_style), "line_style")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("area_color: {}", color(*area_color, "area_color")?));
            properties.push(format!("is_solid: {is_solid}"));
            properties.push(format!("transparent: {transparent}"));
            "rectangle"
        }
        SchGraphicDetail::RoundRectangle {
            from,
            to,
            corner_x_radius,
            corner_y_radius,
            line_width,
            color: line_color,
            area_color,
            is_solid,
        } => {
            properties.push(format!("from: {}", point_text(*from)));
            properties.push(format!("to: {}", point_text(*to)));
            properties.push(format!("corner_x_radius: {}", coord_text(*corner_x_radius)));
            properties.push(format!("corner_y_radius: {}", coord_text(*corner_y_radius)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("area_color: {}", color(*area_color, "area_color")?));
            properties.push(format!("is_solid: {is_solid}"));
            "round_rectangle"
        }
        SchGraphicDetail::Arc {
            center,
            radius,
            line_width,
            start_angle,
            end_angle,
            color: line_color,
        } => {
            properties.push(format!("center: {}", point_text(*center)));
            properties.push(format!("radius: {}", coord_text(*radius)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!(
                "start_angle: {}",
                angle(*start_angle, "start_angle")?
            ));
            if let Some(end) = end_angle {
                properties.push(format!("end_angle: {}", angle(*end, "end_angle")?));
            }
            properties.push(format!("color: {}", color(*line_color, "color")?));
            "arc"
        }
        SchGraphicDetail::EllipticalArc {
            center,
            radius,
            secondary_radius,
            line_width,
            start_angle,
            end_angle,
            color: line_color,
        } => {
            properties.push(format!("center: {}", point_text(*center)));
            properties.push(format!("radius: {}", coord_text(*radius)));
            properties.push(format!(
                "secondary_radius: {}",
                coord_text(*secondary_radius)
            ));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!(
                "start_angle: {}",
                angle(*start_angle, "start_angle")?
            ));
            if let Some(end) = end_angle {
                properties.push(format!("end_angle: {}", angle(*end, "end_angle")?));
            }
            properties.push(format!("color: {}", color(*line_color, "color")?));
            "elliptical_arc"
        }
        SchGraphicDetail::Ellipse {
            center,
            radius,
            secondary_radius,
            line_width,
            color: line_color,
            area_color,
            is_solid,
            transparent,
        } => {
            properties.push(format!("center: {}", point_text(*center)));
            properties.push(format!("radius: {}", coord_text(*radius)));
            properties.push(format!(
                "secondary_radius: {}",
                coord_text(*secondary_radius)
            ));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("area_color: {}", color(*area_color, "area_color")?));
            properties.push(format!("is_solid: {is_solid}"));
            properties.push(format!("transparent: {transparent}"));
            "ellipse"
        }
        SchGraphicDetail::Pie {
            center,
            radius,
            line_width,
            start_angle,
            end_angle,
            color: line_color,
            area_color,
            is_solid,
        } => {
            properties.push(format!("center: {}", point_text(*center)));
            properties.push(format!("radius: {}", coord_text(*radius)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!(
                "start_angle: {}",
                angle(*start_angle, "start_angle")?
            ));
            if let Some(end) = end_angle {
                properties.push(format!("end_angle: {}", angle(*end, "end_angle")?));
            }
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("area_color: {}", color(*area_color, "area_color")?));
            properties.push(format!("is_solid: {is_solid}"));
            "pie"
        }
        SchGraphicDetail::Polyline {
            vertices,
            line_width,
            line_style,
            start_line_shape,
            end_line_shape,
            line_shape_size,
            color: line_color,
        } => {
            properties.push(format!("vertices: {}", vertices_text(vertices)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!(
                "line_style: \"{}\"",
                named(line_style_text(*line_style), "line_style")?
            ));
            properties.push(format!(
                "start_line_shape: \"{}\"",
                named(line_shape_text(*start_line_shape), "start_line_shape")?
            ));
            properties.push(format!(
                "end_line_shape: \"{}\"",
                named(line_shape_text(*end_line_shape), "end_line_shape")?
            ));
            properties.push(format!(
                "line_shape_size: \"{}\"",
                named(pen_width_text(*line_shape_size), "line_shape_size")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            "polyline"
        }
        SchGraphicDetail::Polygon {
            vertices,
            line_width,
            color: line_color,
            area_color,
            is_solid,
            transparent,
        } => {
            properties.push(format!("vertices: {}", vertices_text(vertices)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("area_color: {}", color(*area_color, "area_color")?));
            properties.push(format!("is_solid: {is_solid}"));
            properties.push(format!("transparent: {transparent}"));
            "polygon"
        }
        SchGraphicDetail::Bezier {
            vertices,
            line_width,
            color: line_color,
        } => {
            properties.push(format!("vertices: {}", vertices_text(vertices)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            "bezier"
        }
        SchGraphicDetail::Image {
            from,
            to,
            orientation,
            line_width,
            color: line_color,
            is_solid,
            keep_aspect,
            embed_image,
            file_name: _,
        } => {
            // `file_name` and `source` are never authored: the embedded
            // resource pairing and body stay preserved verbatim.
            properties.push(format!("from: {}", point_text(*from)));
            properties.push(format!("to: {}", point_text(*to)));
            properties.push(format!(
                "orientation: {}",
                named(orientation_text(*orientation), "orientation")?
            ));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("is_solid: {is_solid}"));
            properties.push(format!("keep_aspect: {keep_aspect}"));
            properties.push(format!("embed_image: {embed_image}"));
            "image"
        }
        SchGraphicDetail::Label {
            at,
            orientation,
            justification,
            color: line_color,
            font_id: _,
            text,
            is_mirrored,
            url,
        } => {
            properties.push(format!("at: {}", point_text(*at)));
            properties.push(format!(
                "orientation: {}",
                named(orientation_text(*orientation), "orientation")?
            ));
            properties.push(format!(
                "justification: \"{}\"",
                named(justification_text(*justification), "justification")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("text: {}", text_value(text, "text")?));
            properties.push(format!("is_mirrored: {is_mirrored}"));
            if !url.is_empty() {
                properties.push(format!("url: {}", text_value(url, "url")?));
            }
            "label"
        }
        SchGraphicDetail::TextFrame {
            from,
            to,
            line_width,
            color: line_color,
            area_color,
            text_color,
            font_id: _,
            is_solid,
            show_border,
            alignment,
            word_wrap,
            clip_to_rect,
            text,
            text_margin,
            transparent,
        } => {
            properties.push(format!("from: {}", point_text(*from)));
            properties.push(format!("to: {}", point_text(*to)));
            properties.push(format!(
                "line_width: \"{}\"",
                named(pen_width_text(*line_width), "line_width")?
            ));
            properties.push(format!("color: {}", color(*line_color, "color")?));
            properties.push(format!("area_color: {}", color(*area_color, "area_color")?));
            properties.push(format!("text_color: {}", color(*text_color, "text_color")?));
            properties.push(format!("is_solid: {is_solid}"));
            properties.push(format!("show_border: {show_border}"));
            properties.push(format!(
                "alignment: \"{}\"",
                named(alignment_text(*alignment), "alignment")?
            ));
            properties.push(format!("word_wrap: {word_wrap}"));
            properties.push(format!("clip_to_rect: {clip_to_rect}"));
            properties.push(format!("text: {}", text_value(text, "text")?));
            properties.push(format!("text_margin: {}", coord_text(*text_margin)));
            properties.push(format!("transparent: {transparent}"));
            "text_frame"
        }
        SchGraphicDetail::Symbol | SchGraphicDetail::Hyperlink | SchGraphicDetail::Other { .. } => {
            return Err(format!(
                "graphic kind {} is intent-unrepresentable",
                detail.kind_name()
            ));
        }
    };
    Ok((keyword, properties))
}

struct ComponentEmitter<'a> {
    component: &'a SchLibComponentSnapshot,
    name: String,
    skips: Vec<String>,
}

impl ComponentEmitter<'_> {
    fn skip(&mut self, what: impl Into<String>) {
        self.skips
            .push(format!("component '{}': {}", self.name, what.into()));
    }

    /// Renders the pin blocks of one owner group at `indent`, or nothing
    /// (with a recorded skip) when the group cannot be reconciled or
    /// rendered exactly.
    fn pin_group_lines(&mut self, target: i32, group_desc: &str, indent: &str) -> Vec<String> {
        let pins: Vec<_> = self
            .component
            .pins()
            .into_iter()
            .filter(|view| crate::schlib::positional::group_member(view.owner_group, target))
            .map(|view| {
                (
                    view.designator.to_owned(),
                    view.name.to_owned(),
                    view.electrical,
                    view.length,
                    view.location,
                    view.orientation,
                    view.is_hidden,
                    view.swap_group.to_owned(),
                )
            })
            .collect();
        if pins.is_empty() {
            return Vec::new();
        }
        let mut designators = BTreeSet::new();
        for (designator, ..) in &pins {
            if designator.is_empty() {
                self.skip(format!(
                    "{group_desc}: a pin has an empty designator; the group is preserved \
                     unauthored"
                ));
                return Vec::new();
            }
            if !designators.insert(designator.clone()) {
                self.skip(format!(
                    "{group_desc}: duplicate pin designator '{designator}'; the \
                     comment-preserving merge cannot key the group — preserved unauthored"
                ));
                return Vec::new();
            }
        }
        let mut lines = Vec::with_capacity(pins.len());
        for (designator, name, electrical, length, location, orientation, is_hidden, swap_group) in
            &pins
        {
            let Some(designator_text) = quote(designator) else {
                self.skip(format!(
                    "{group_desc}: pin designator contains unescapable control characters; \
                     the group is preserved unauthored"
                ));
                return Vec::new();
            };
            let mut properties = Vec::new();
            if !name.is_empty() {
                match quote(name) {
                    Some(text) => properties.push(format!("name: {text}")),
                    None => {
                        self.skip(format!(
                            "{group_desc}: pin '{designator}' name contains unescapable \
                             control characters; the group is preserved unauthored"
                        ));
                        return Vec::new();
                    }
                }
            }
            let (Some(electrical_name), Some(orientation_name)) =
                (electrical_text(*electrical), orientation_text(*orientation))
            else {
                self.skip(format!(
                    "{group_desc}: pin '{designator}' has an enum value with no exact \
                     rendering; the group is preserved unauthored"
                ));
                return Vec::new();
            };
            properties.push(format!("electrical: \"{electrical_name}\""));
            properties.push(format!("length: {}", coord_text(*length)));
            properties.push(format!("at: {}", point_text(*location)));
            properties.push(format!("orientation: {orientation_name}"));
            if *is_hidden {
                properties.push("is_hidden: true".to_owned());
            }
            if !swap_group.is_empty() {
                match quote(swap_group) {
                    Some(text) => properties.push(format!("swap_group: {text}")),
                    None => {
                        self.skip(format!(
                            "{group_desc}: pin '{designator}' swap group contains \
                             unescapable control characters; the group is preserved \
                             unauthored"
                        ));
                        return Vec::new();
                    }
                }
            }
            lines.push(format!(
                "{indent}pin {designator_text} {{ {} }}",
                properties.join(", ")
            ));
        }
        lines
    }

    /// Renders the representable PREFIX of one owner group's graphics at
    /// `indent`; the unauthored tail (from the first unrepresentable or
    /// unrenderable member on) stays preserved, with a recorded skip.
    fn graphic_group_lines(&mut self, target: i32, group_desc: &str, indent: &str) -> Vec<String> {
        let graphics = self.component.graphics();
        let members: Vec<_> = graphics
            .iter()
            .filter(|view| crate::schlib::positional::group_member(view.owner_group, target))
            .collect();
        let mut lines = Vec::new();
        for (position, view) in members.iter().enumerate() {
            match graphic_lines(&view.detail) {
                Ok((keyword, properties)) => {
                    if properties.is_empty() {
                        lines.push(format!("{indent}{keyword} {{}}"));
                    } else {
                        lines.push(format!("{indent}{keyword} {{ {} }}", properties.join(", ")));
                    }
                }
                Err(reason) => {
                    let remaining = members.len() - position;
                    self.skip(format!(
                        "{group_desc}: graphics entry #{position}: {reason}; this and the \
                         {remaining} trailing member(s) of the group are preserved unauthored"
                    ));
                    break;
                }
            }
        }
        lines
    }
}

/// Generates the deterministic `.schlib-spec` source of a library snapshot.
/// See the module documentation for the exact emission and skip contract.
pub fn generate_schlib_spec_source(
    input: &SchLibSnapshot,
) -> Result<GeneratedSchLibSpec, SchLibPatchError> {
    let mut out = String::new();
    let mut skips = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index).map_err(format_error(format!(
            "resolving component #{index} for dump generation"
        )))?;
        let name = component.lib_reference().to_owned();
        let mut emitter = ComponentEmitter {
            component: &component,
            name: name.clone(),
            skips: Vec::new(),
        };
        let context = format!("projecting component '{name}' for dump generation");
        let Some(name_text) = quote(&name) else {
            skips.push(format!(
                "component '{name}': lib reference contains unescapable control characters; \
                 the component is not represented in the generated source"
            ));
            continue;
        };
        if index > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "component {name_text} {{");

        // ── Scalars ──
        if let Some(description) = component.description() {
            match quote(description) {
                Some(text) => {
                    let _ = writeln!(out, "    description: {text}");
                }
                None => emitter.skip(
                    "description contains unescapable control characters; preserved unauthored",
                ),
            }
        }
        if let Some(designator) = component
            .designator()
            .map_err(format_error(context.clone()))?
        {
            match quote(&designator) {
                Some(text) => {
                    let _ = writeln!(out, "    designator: {text}");
                }
                None => emitter.skip(
                    "designator contains unescapable control characters; preserved unauthored",
                ),
            }
        }
        match component_kind_text(component.component_kind()) {
            Some(kind_name) => {
                let _ = writeln!(out, "    component_kind: \"{kind_name}\"");
            }
            None => emitter.skip(format!(
                "component kind {:?} has no spec rendering; preserved unauthored",
                component.component_kind()
            )),
        }
        let _ = writeln!(
            out,
            "    show_hidden_pins: {}",
            component.show_hidden_pins()
        );
        let effective = component
            .effective_part_count()
            .map_err(format_error(context.clone()))?;
        if effective > 1 {
            let _ = writeln!(out, "    part_count: {effective}");
        }

        // ── Pins and graphics per owner group ──
        if effective <= 1 {
            // Single-part: component scope addresses Part(1). Owner-0 pins
            // of a single-part component are intent-unaddressable.
            let shared_pins = component
                .pins()
                .iter()
                .filter(|view| crate::schlib::positional::group_member(view.owner_group, 0))
                .count();
            if shared_pins > 0 {
                emitter.skip(format!(
                    "{shared_pins} owner-0 pin(s) on a single-part component are \
                     intent-unaddressable; preserved unauthored"
                ));
            }
            for line in emitter.graphic_group_lines(1, "component-scope graphics", "    ") {
                let _ = writeln!(out, "{line}");
            }
            for line in emitter.pin_group_lines(1, "component-scope pins", "    ") {
                let _ = writeln!(out, "{line}");
            }
        } else {
            // Multi-part: component scope addresses the shared owner-0 pin
            // group; each part gets its own block. Owner-0 graphics have no
            // authorable owner.
            for line in emitter.pin_group_lines(0, "shared (owner-0) pins", "    ") {
                let _ = writeln!(out, "{line}");
            }
            let shared_graphics = component
                .graphics()
                .iter()
                .filter(|view| crate::schlib::positional::group_member(view.owner_group, 0))
                .count();
            if shared_graphics > 0 {
                emitter.skip(format!(
                    "{shared_graphics} owner-0 graphic(s) on a multi-part component have no \
                     authorable owner; preserved unauthored"
                ));
            }
            for part in 1..=effective {
                let graphics =
                    emitter.graphic_group_lines(part, &format!("part {part} graphics"), "        ");
                let pins = emitter.pin_group_lines(part, &format!("part {part} pins"), "        ");
                if graphics.is_empty() && pins.is_empty() {
                    continue;
                }
                let _ = writeln!(out, "    part {part} {{");
                for line in graphics.into_iter().chain(pins) {
                    let _ = writeln!(out, "{line}");
                }
                let _ = writeln!(out, "    }}");
            }
        }
        // Unaddressable owner groups beyond the shared/part targets.
        let unaddressable_pins = component
            .pins()
            .iter()
            .filter(|view| {
                !(0..=effective)
                    .any(|target| crate::schlib::positional::group_member(view.owner_group, target))
            })
            .count();
        if unaddressable_pins > 0 {
            emitter.skip(format!(
                "{unaddressable_pins} pin(s) in unaddressable owner groups \
                 (PartIndependent/OutOfRange); preserved unauthored"
            ));
        }
        let unaddressable_graphics = component
            .graphics()
            .iter()
            .filter(|view| {
                !(1..=effective)
                    .any(|target| crate::schlib::positional::group_member(view.owner_group, target))
                    && !crate::schlib::positional::group_member(view.owner_group, 0)
            })
            .count();
        if unaddressable_graphics > 0 {
            emitter.skip(format!(
                "{unaddressable_graphics} graphic(s) in unaddressable owner groups; \
                 preserved unauthored"
            ));
        }

        // ── Parameters (keyed, case-insensitive fold) ──
        let parameters = component
            .parameters()
            .map_err(format_error(context.clone()))?;
        let mut folded = BTreeSet::new();
        let duplicate = parameters
            .iter()
            .find(|view| !folded.insert(parameter_key_fold(view.name)));
        if let Some(view) = duplicate {
            emitter.skip(format!(
                "duplicate parameter name '{}' under the case-insensitive fold; the \
                 parameters collection is preserved unauthored",
                view.name
            ));
        } else {
            let mut parameter_lines = Vec::with_capacity(parameters.len());
            let mut renderable = true;
            for view in &parameters {
                let (Some(name_text), Some(text_text)) = (quote(view.name), quote(view.text))
                else {
                    emitter.skip(format!(
                        "parameter '{}' contains unescapable control characters; the \
                         parameters collection is preserved unauthored",
                        view.name
                    ));
                    renderable = false;
                    break;
                };
                parameter_lines.push(format!(
                    "    parameter {name_text} {{ text: {text_text}, is_hidden: {} }}",
                    view.is_hidden
                ));
            }
            if renderable {
                for line in parameter_lines {
                    let _ = writeln!(out, "{line}");
                }
            }
        }

        // ── Aliases (keyed, exact) ──
        for alias in component.aliases() {
            match quote(alias) {
                Some(text) => {
                    let _ = writeln!(out, "    alias {text}");
                }
                None => emitter.skip(format!(
                    "alias '{alias}' contains unescapable control characters; preserved \
                     unauthored"
                )),
            }
        }

        // ── Footprints (keyed by model name; definers never authored) ──
        let maps = component
            .footprint_maps()
            .map_err(format_error(context.clone()))?;
        let mut models = BTreeSet::new();
        let duplicate_model = maps
            .footprints
            .iter()
            .find(|view| !models.insert(view.model_name.to_owned()));
        if let Some(view) = duplicate_model {
            emitter.skip(format!(
                "duplicate footprint model name '{}'; the footprints collection is \
                 preserved unauthored",
                view.model_name
            ));
        } else {
            for view in &maps.footprints {
                let Some(model_text) = quote(view.model_name) else {
                    emitter.skip(format!(
                        "footprint '{}' contains unescapable control characters; preserved \
                         unauthored",
                        view.model_name
                    ));
                    continue;
                };
                if view.description.is_empty() {
                    let _ = writeln!(out, "    footprint {model_text}");
                } else {
                    match quote(view.description) {
                        Some(description_text) => {
                            let _ = writeln!(
                                out,
                                "    footprint {model_text} {{ description: {description_text} }}"
                            );
                        }
                        None => {
                            emitter.skip(format!(
                                "footprint '{}' description contains unescapable control \
                                 characters; the footprint is preserved unauthored",
                                view.model_name
                            ));
                        }
                    }
                }
            }
        }

        out.push_str("}\n");
        skips.extend(emitter.skips);
    }
    Ok(GeneratedSchLibSpec { text: out, skips })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::schlib::SchLibPatch;
    use altium_format::SchLib;
    use altium_format::api::{
        SchAliasOp, SchChildBatch, SchFootprintEdit, SchFootprintInit, SchGraphicEdit,
        SchGraphicInit, SchGraphicShape, SchLibEdit, SchParameterEdit, SchParameterInit,
        SchPinEdit, SchPinInit, SchPinPadMapInit,
    };
    use altium_format_types::CoordPoint;

    /// A blank AD26 library extended across every emitted collection class:
    /// parameters, aliases, a footprint (with untouchable definers), pins,
    /// and several graphic kinds with non-default values.
    pub(crate) fn seeded_snapshot() -> SchLibSnapshot {
        let doc = SchLib::new_blank_ad26().unwrap();
        let snapshot = doc.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Value".to_owned(),
                        text: "10k \"quoted\"\ttabbed".to_owned(),
                        is_hidden: false,
                    }),
                ]),
                graphics: Some(vec![
                    SchGraphicEdit::New(SchGraphicInit {
                        owner_part_id: 1,
                        owner_part_display_mode: None,
                        shape: SchGraphicShape::Rectangle {
                            from: CoordPoint::zero(),
                            to: CoordPoint {
                                x: Coord::from_internal(123_457),
                                y: Coord::from_mils(60).unwrap(),
                            },
                            line_width: PenWidth::Medium,
                            line_style: LineStyle::Dashed,
                            color: Color::from_rgb(0x12, 0x34, 0x56),
                            area_color: Color::from_rgb(0xAB, 0xCD, 0xEF),
                            is_solid: true,
                            transparent: false,
                        },
                    }),
                    SchGraphicEdit::New(SchGraphicInit {
                        owner_part_id: 1,
                        owner_part_display_mode: None,
                        shape: SchGraphicShape::Arc {
                            center: CoordPoint {
                                x: Coord::from_mils(-10).unwrap(),
                                y: Coord::from_internal(-5),
                            },
                            radius: Coord::from_mils(50).unwrap(),
                            line_width: PenWidth::Small,
                            start_angle: 12.5,
                            end_angle: Some(270.25),
                            color: Color::from_rgb(1, 2, 3),
                        },
                    }),
                    SchGraphicEdit::New(SchGraphicInit {
                        owner_part_id: 1,
                        owner_part_display_mode: None,
                        shape: SchGraphicShape::Polyline {
                            vertices: vec![
                                CoordPoint::zero(),
                                CoordPoint {
                                    x: Coord::from_mils(10).unwrap(),
                                    y: Coord::from_internal(7),
                                },
                            ],
                            line_width: PenWidth::Zero,
                            line_style: LineStyle::Dotted,
                            start_line_shape: LineShape::Arrow,
                            end_line_shape: LineShape::Circle,
                            line_shape_size: PenWidth::Large,
                            color: Color::from_rgb(0, 0, 0),
                        },
                    }),
                ]),
                pins: Some(vec![
                    SchPinEdit::New(SchPinInit {
                        owner_part_id: 1,
                        owner_part_display_mode: Some(0),
                        designator: "1".to_owned(),
                        name: "IN".to_owned(),
                        electrical: PinElectricalType::Input,
                        length: Coord::from_mils(300).unwrap(),
                        location: CoordPoint {
                            x: Coord::from_mils(-100).unwrap(),
                            y: Coord::from_internal(12_345),
                        },
                        orientation: RotationBy90::Rotate180,
                        is_hidden: false,
                        hidden_net_name: String::new(),
                        swap_group: "grp A".to_owned(),
                        part_swap_group: Some(String::new()),
                        part_pin_swap_suffix: Some(String::new()),
                        pair_swap_group: String::new(),
                    }),
                    SchPinEdit::New(SchPinInit {
                        owner_part_id: 1,
                        owner_part_display_mode: Some(0),
                        designator: "A\"2".to_owned(),
                        name: String::new(),
                        electrical: PinElectricalType::Power,
                        length: Coord::from_internal(1),
                        location: CoordPoint::zero(),
                        orientation: RotationBy90::Rotate90,
                        is_hidden: true,
                        hidden_net_name: String::new(),
                        swap_group: String::new(),
                        part_swap_group: Some(String::new()),
                        part_pin_swap_suffix: Some(String::new()),
                        pair_swap_group: String::new(),
                    }),
                ]),
                footprints: Some(vec![SchFootprintEdit::New(SchFootprintInit {
                    model_name: "FP-0603".to_owned(),
                    description: "footprint description".to_owned(),
                    is_current: true,
                    pin_pad_maps: Some(vec![SchPinPadMapInit {
                        pin: "1".to_owned(),
                        pads: vec!["A1".to_owned(), "A2".to_owned()],
                    }]),
                })]),
                allow_deletion: false,
            })
            .unwrap();
        let with_children = snapshot.replace_component(0, edited).unwrap();
        with_children
            .with_library_edit(SchLibEdit {
                alias_ops: vec![SchAliasOp::Add {
                    component: "Component_1".to_owned(),
                    alias: "ALIAS_A".to_owned(),
                }],
                ..Default::default()
            })
            .unwrap()
    }

    #[test]
    fn generated_source_parses_compiles_and_elaborates_to_an_empty_patch() {
        let input = seeded_snapshot();
        let generated = generate_schlib_spec_source(&input).unwrap();
        assert!(generated.skips.is_empty(), "{:?}", generated.skips);

        // The comment-preserving merge machinery must parse the text.
        altium_spec_lang::cst::SpecTree::parse(generated.text.clone()).unwrap();

        // Compile through the AST parser and elaborate: the convergence law.
        let ast = altium_spec_lang::parser::parse_spec(&generated.text).unwrap();
        let intent = altium_spec_lang::intent::compile_schlib(&ast).unwrap();
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert!(
            patch.is_empty(),
            "generated dump must elaborate to an empty patch; replacements: {:?}, text:\n{}",
            patch.replacements().len(),
            generated.text
        );

        // Determinism: two generations are byte-identical.
        assert_eq!(
            generate_schlib_spec_source(&input).unwrap().text,
            generated.text
        );
    }

    #[test]
    fn unrepresentable_content_is_skipped_with_recorded_diagnostics() {
        let doc = SchLib::new_blank_ad26().unwrap();
        let snapshot = doc.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        // Duplicate (case-folded) parameter names block the collection.
        let edited = component
            .with_children(SchChildBatch {
                parameters: Some(vec![
                    SchParameterEdit::Keep(0),
                    SchParameterEdit::New(SchParameterInit {
                        name: "Dup".to_owned(),
                        text: "a".to_owned(),
                        is_hidden: false,
                    }),
                    SchParameterEdit::New(SchParameterInit {
                        name: "DUP".to_owned(),
                        text: "b".to_owned(),
                        is_hidden: false,
                    }),
                ]),
                graphics: None,
                pins: None,
                footprints: None,
                allow_deletion: false,
            })
            .unwrap();
        let input = snapshot.replace_component(0, edited).unwrap();
        let generated = generate_schlib_spec_source(&input).unwrap();
        assert!(
            generated
                .skips
                .iter()
                .any(|skip| skip.contains("duplicate parameter name")),
            "{:?}",
            generated.skips
        );
        assert!(!generated.text.contains("parameter \"Dup\""));

        // The reduced dump still converges.
        let ast = altium_spec_lang::parser::parse_spec(&generated.text).unwrap();
        let intent = altium_spec_lang::intent::compile_schlib(&ast).unwrap();
        let patch = SchLibPatch::from_intent(&input, &intent).unwrap();
        assert!(patch.is_empty());
    }
}
