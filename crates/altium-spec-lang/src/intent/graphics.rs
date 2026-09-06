//! Graphic field policies for the authored-intent schema.
//!
//! Defaults follow the current Rust `graphic_from_spec` creation behavior and
//! `api::schlib_types` surface. They are compatibility policies, not claims of
//! independent AD26 source verification. Resource/font validity is checked later.
use super::compile::{
    Properties, boolean, color, coord, finite_number, point, positive_integer, string,
};
use super::{FieldIntent, FieldPolicy};
use crate::ast::GraphicDecl;
use crate::diagnostic::Span;
use crate::{ScopeStack, SpecError, Value};
use altium_format_types::constants::visual::{
    SCH_PEN_WIDTH_LARGE_INTERNAL, SCH_PEN_WIDTH_MEDIUM_INTERNAL, SCH_PEN_WIDTH_SMALL_INTERNAL,
    SCH_PEN_WIDTH_ZERO_INTERNAL,
};
use altium_format_types::sch::{
    HorizontalAlign, LineShape, LineStyle, PenWidth, TextJustification,
};
use altium_format_types::{Color, Coord, CoordPoint, RotationBy90};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicKind {
    Line,
    Rectangle,
    Arc,
    EllipticalArc,
    Ellipse,
    Polyline,
    Polygon,
    Bezier,
    Pie,
    RoundRectangle,
    Label,
    TextFrame,
    Image,
}

/// Positive source font-table reference. Default 1 is a creation policy, not
/// evidence that the destination library actually contains that font entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontReference(pub u32);

macro_rules! graphic_fields {
    ($($field:ident: $ty:ty),* $(,)?) => {
        /// Native-facing typed fields. Unsupported fields remain absent and the
        /// compiler rejects authoring them for an inapplicable graphic kind.
        #[derive(Debug, Clone, PartialEq, Default)]
        pub struct GraphicFields { $(pub $field: FieldIntent<$ty>,)* }
        /// Each present policy declares applicability, default and clearability.
        /// Defaults never grant implicit write scope over an existing graphic.
        #[derive(Default)]
        pub struct GraphicPolicies { $(pub $field: Option<FieldPolicy<$ty>>,)* }
    };
}
graphic_fields! {
    from: CoordPoint,
    to: CoordPoint,
    center: CoordPoint,
    at: CoordPoint,
    radius: Coord,
    secondary_radius: Coord,
    corner_x_radius: Coord,
    corner_y_radius: Coord,
    start_angle: f64,
    end_angle: Option<f64>,
    vertices: Vec<CoordPoint>,
    color: Color,
    area_color: Color,
    text_color: Color,
    line_width: PenWidth,
    line_style: LineStyle,
    start_line_shape: LineShape,
    end_line_shape: LineShape,
    line_shape_size: PenWidth,
    is_solid: bool,
    transparent: bool,
    show_border: bool,
    text: String,
    font_id: FontReference,
    file_name: String,
    orientation: RotationBy90,
    justification: TextJustification,
    alignment: HorizontalAlign,
    is_mirrored: bool,
    url: String,
    word_wrap: bool,
    clip_to_rect: bool,
    text_margin: Coord,
    keep_aspect: bool,
    embed_image: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphicIntent {
    pub auto_size: Option<super::AutoSizeIntent>,
    pub kind: crate::diagnostic::Spanned<GraphicKind>,
    pub fields: GraphicFields,
}

impl GraphicKind {
    /// Language-schema policies derived from current Rust creation behavior.
    /// Optional arc end angles default/clear to None rather than an invented
    /// full-circle endpoint. Physical line widths use the verified AD26 size table.
    pub fn field_policies(self) -> GraphicPolicies {
        use GraphicKind::*;
        let mut policies = GraphicPolicies::default();
        macro_rules! policy {
            ($field:ident, $default:expr) => {
                policies.$field = Some(FieldPolicy {
                    name: concat!("graphic.", stringify!($field)),
                    default: Some($default),
                    clear: None,
                });
            };
            ($field:ident, $default:expr, clear) => {
                policies.$field = Some(FieldPolicy {
                    name: concat!("graphic.", stringify!($field)),
                    default: Some($default),
                    clear: Some($default),
                });
            };
        }
        policy!(color, || Color::BLACK);
        if self != Label {
            policy!(line_width, || PenWidth::Zero);
        }
        match self {
            Line | Rectangle | RoundRectangle | TextFrame | Image => {
                policy!(from, CoordPoint::zero);
                policy!(to, CoordPoint::zero);
            }
            Arc | EllipticalArc | Ellipse | Pie => {
                policy!(center, CoordPoint::zero);
                policy!(radius, || Coord::ZERO);
            }
            Polyline | Polygon | Bezier => {
                policy!(vertices, Vec::new);
            }
            Label => {
                policy!(at, CoordPoint::zero);
            }
        }
        if matches!(
            self,
            Rectangle | RoundRectangle | Ellipse | Polygon | Pie | TextFrame
        ) {
            policy!(is_solid, || true);
            policy!(area_color, || Color::BLACK);
        }
        if matches!(self, Rectangle | Ellipse | Polygon | TextFrame) {
            policy!(transparent, || false);
        }
        if matches!(self, Arc | EllipticalArc | Pie) {
            policy!(start_angle, || 0.0);
            policy!(end_angle, || None, clear);
        }
        if matches!(self, EllipticalArc | Ellipse) {
            policy!(secondary_radius, || Coord::ZERO);
        }
        if self == RoundRectangle {
            policy!(corner_x_radius, || Coord::ZERO);
            policy!(corner_y_radius, || Coord::ZERO);
        }
        if matches!(self, Line | Rectangle | Polyline) {
            policy!(line_style, || LineStyle::Solid);
        }
        if self == Polyline {
            policy!(start_line_shape, || LineShape::None);
            policy!(end_line_shape, || LineShape::None);
            policy!(line_shape_size, || PenWidth::Zero);
        }
        if matches!(self, Label | TextFrame) {
            policy!(text, String::new, clear);
            policy!(font_id, || FontReference(1));
        }
        if matches!(self, Label | Image) {
            policy!(orientation, || RotationBy90::Rotate0);
        }
        if self == Label {
            policy!(justification, || TextJustification::BottomLeft);
            policy!(is_mirrored, || false);
            policy!(url, String::new, clear);
        }
        if self == TextFrame {
            policy!(text_color, || Color::BLACK);
            policy!(show_border, || true);
            policy!(alignment, || HorizontalAlign::Left);
            policy!(word_wrap, || true);
            policy!(clip_to_rect, || false);
            policy!(text_margin, || Coord::ZERO);
        }
        if self == Image {
            // Clearing the filename is field intent, not proof that an embedded
            // image has a resolvable resource. Image assets are a separate gate.
            policy!(file_name, String::new, clear);
            policy!(is_solid, || false);
            policy!(keep_aspect, || true);
            policy!(embed_image, || true);
        }
        policies
    }
}

pub(super) fn compile_graphic(
    decl: &GraphicDecl,
    scope: &ScopeStack,
    auto_size: Option<super::AutoSizeIntent>,
) -> Result<GraphicIntent, SpecError> {
    use GraphicKind::*;
    let kind = match decl.graphic_type.node.as_str() {
        "line" => Line,
        "rectangle" => Rectangle,
        "arc" => Arc,
        "elliptical_arc" => EllipticalArc,
        "ellipse" => Ellipse,
        "polyline" => Polyline,
        "polygon" => Polygon,
        "bezier" => Bezier,
        "pie" => Pie,
        "round_rectangle" => RoundRectangle,
        "label" => Label,
        "text_frame" => TextFrame,
        "image" => Image,
        name => {
            return Err(super::compile::error(
                format!("unsupported SchLib graphic '{name}'"),
                decl.graphic_type.span,
            ));
        }
    };
    let mut props = Properties::object(&decl.body, scope)?;
    if auto_size.is_some() {
        // The layout compiler has already represented these properties as typed intent.
        props.consume_layout(&["padding", "min_width", "min_height"]);
    }
    let policies = kind.field_policies();
    let mut fields = GraphicFields::default();
    macro_rules! field {
        ($field:ident, $parse:expr) => {
            if let Some(policy) = policies.$field {
                fields.$field = props.take(stringify!($field), &[], policy, $parse)?;
            }
        };
    }
    field!(from, point);
    field!(to, point);
    field!(center, point);
    field!(at, point);
    field!(radius, coord);
    field!(secondary_radius, coord);
    field!(corner_x_radius, coord);
    field!(corner_y_radius, coord);
    field!(start_angle, finite_number);
    field!(end_angle, |value, span| finite_number(value, span)
        .map(Some));
    field!(vertices, |value, span| match value {
        Value::Array(values) => values.into_iter().map(|value| point(value, span)).collect(),
        _ => Err(super::compile::error(
            "vertices must be an array of coordinates",
            span
        )),
    });
    field!(color, color);
    field!(area_color, color);
    field!(text_color, color);
    field!(line_width, pen_width);
    field!(line_style, line_style);
    field!(start_line_shape, line_shape);
    field!(end_line_shape, line_shape);
    field!(line_shape_size, named_endpoint_size);
    field!(is_solid, boolean);
    field!(transparent, boolean);
    field!(show_border, boolean);
    field!(text, string);
    field!(font_id, |value, span| positive_integer(value, span)
        .map(|v| FontReference(v as u32)));
    field!(file_name, string);
    field!(orientation, rotation);
    field!(justification, justification);
    field!(alignment, alignment);
    field!(is_mirrored, boolean);
    field!(url, string);
    field!(word_wrap, boolean);
    field!(clip_to_rect, boolean);
    field!(text_margin, coord);
    field!(keep_aspect, boolean);
    field!(embed_image, boolean);
    props.finish(&format!("{} graphic", decl.graphic_type.node))?;
    Ok(GraphicIntent {
        auto_size,
        kind: crate::diagnostic::Spanned::new(kind, decl.graphic_type.span),
        fields,
    })
}

fn enum_name(value: Value, span: Span) -> Result<String, SpecError> {
    string(value, span).map(|value| value.to_ascii_lowercase())
}
/// Compare canonical coordinates exactly; never round a width to a nearby enum.
fn pen_width(value: Value, span: Span) -> Result<PenWidth, SpecError> {
    if matches!(value, Value::String(_)) {
        return named_pen_width(value, span);
    }
    match coord(value, span)?.raw() {
        SCH_PEN_WIDTH_ZERO_INTERNAL => Ok(PenWidth::Zero),
        SCH_PEN_WIDTH_SMALL_INTERNAL => Ok(PenWidth::Small),
        SCH_PEN_WIDTH_MEDIUM_INTERNAL => Ok(PenWidth::Medium),
        SCH_PEN_WIDTH_LARGE_INTERNAL => Ok(PenWidth::Large),
        _ => Err(super::compile::error(
            "physical schematic line width must be exactly 0, 10, 30, or 50 mil; use zero, small, medium, or large for named widths",
            span,
        )),
    }
}
fn named_endpoint_size(value: Value, span: Span) -> Result<PenWidth, SpecError> {
    if !matches!(value, Value::String(_)) {
        return Err(super::compile::error(
            "line_shape_size requires a named endpoint size: zero, small, medium, or large; physical endpoint-size conversion is not verified",
            span,
        ));
    }
    named_pen_width(value, span)
}
fn named_pen_width(value: Value, span: Span) -> Result<PenWidth, SpecError> {
    match enum_name(value, span)?.as_str() {
        "zero" => Ok(PenWidth::Zero),
        "small" => Ok(PenWidth::Small),
        "medium" => Ok(PenWidth::Medium),
        "large" => Ok(PenWidth::Large),
        _ => Err(super::compile::error(
            "pen width must be zero, small, medium, or large",
            span,
        )),
    }
}
fn line_style(value: Value, span: Span) -> Result<LineStyle, SpecError> {
    match enum_name(value, span)?.as_str() {
        "solid" => Ok(LineStyle::Solid),
        "dashed" => Ok(LineStyle::Dashed),
        "dotted" => Ok(LineStyle::Dotted),
        "dash_dotted" | "dashdotted" => Ok(LineStyle::DashDotted),
        _ => Err(super::compile::error("unknown schematic line style", span)),
    }
}
fn line_shape(value: Value, span: Span) -> Result<LineShape, SpecError> {
    match enum_name(value, span)?.as_str() {
        "none" => Ok(LineShape::None),
        "arrow" => Ok(LineShape::Arrow),
        "solid_arrow" | "solidarrow" => Ok(LineShape::SolidArrow),
        "tail" => Ok(LineShape::Tail),
        "solid_tail" | "solidtail" => Ok(LineShape::SolidTail),
        "circle" => Ok(LineShape::Circle),
        "square" => Ok(LineShape::Square),
        _ => Err(super::compile::error(
            "unknown schematic line endpoint shape",
            span,
        )),
    }
}
fn rotation(value: Value, span: Span) -> Result<RotationBy90, SpecError> {
    let name = match value {
        Value::Integer(value) => value.to_string(),
        value => enum_name(value, span)?,
    };
    match name.as_str() {
        "0" | "rotate0" | "right" | "east" => Ok(RotationBy90::Rotate0),
        "90" | "rotate90" | "up" | "north" => Ok(RotationBy90::Rotate90),
        "180" | "rotate180" | "left" | "west" => Ok(RotationBy90::Rotate180),
        "270" | "rotate270" | "down" | "south" => Ok(RotationBy90::Rotate270),
        _ => Err(super::compile::error(
            "orientation must be a quarter turn",
            span,
        )),
    }
}
fn alignment(value: Value, span: Span) -> Result<HorizontalAlign, SpecError> {
    match enum_name(value, span)?.as_str() {
        "left" => Ok(HorizontalAlign::Left),
        "center" => Ok(HorizontalAlign::Center),
        "right" => Ok(HorizontalAlign::Right),
        _ => Err(super::compile::error(
            "alignment must be left, center, or right",
            span,
        )),
    }
}
fn justification(value: Value, span: Span) -> Result<TextJustification, SpecError> {
    match enum_name(value, span)?.as_str() {
        "bottom_left" | "bottomleft" => Ok(TextJustification::BottomLeft),
        "bottom_center" | "bottomcenter" => Ok(TextJustification::BottomCenter),
        "bottom_right" | "bottomright" => Ok(TextJustification::BottomRight),
        "center_left" | "centerleft" => Ok(TextJustification::CenterLeft),
        "center" => Ok(TextJustification::Center),
        "center_right" | "centerright" => Ok(TextJustification::CenterRight),
        "top_left" | "topleft" => Ok(TextJustification::TopLeft),
        "top_center" | "topcenter" => Ok(TextJustification::TopCenter),
        "top_right" | "topright" => Ok(TextJustification::TopRight),
        _ => Err(super::compile::error("unknown text justification", span)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Spanned;
    use crate::intent::{FieldIntent, compile_schlib};

    fn graphic(source: &str) -> Result<GraphicIntent, SpecError> {
        let ast = crate::parser::parse_spec(&format!("component R {{{source}}}"))
            .expect("valid graphic syntax");
        Ok(compile_schlib(&ast)?
            .components
            .items
            .remove(0)
            .graphics
            .items
            .remove(0))
    }
    #[test]
    fn registered_defaults_match_current_rust_creation_policies_without_authoring_fields() {
        let rectangle = graphic("rectangle {}").unwrap();
        let policies = rectangle.kind.node.field_policies();
        assert_eq!(rectangle.fields.from, FieldIntent::Absent);
        assert_eq!(rectangle.fields.is_solid, FieldIntent::Absent);
        assert_eq!(
            policies
                .from
                .unwrap()
                .definition(&rectangle.fields.from)
                .unwrap(),
            CoordPoint::zero()
        );
        assert!(
            policies
                .is_solid
                .unwrap()
                .definition(&rectangle.fields.is_solid)
                .unwrap()
        );
        assert_eq!(
            policies
                .line_width
                .unwrap()
                .definition(&rectangle.fields.line_width)
                .unwrap(),
            PenWidth::Zero
        );
        assert_eq!(
            policies
                .line_style
                .unwrap()
                .definition(&rectangle.fields.line_style)
                .unwrap(),
            LineStyle::Solid
        );
        assert_eq!(
            policies
                .color
                .unwrap()
                .definition(&rectangle.fields.color)
                .unwrap(),
            Color::BLACK
        );
        let label = GraphicKind::Label.field_policies();
        assert!(label.line_width.is_none(), "native label has no pen width");
        assert_eq!(
            label
                .font_id
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap(),
            FontReference(1)
        );
        assert_eq!(
            label
                .justification
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap(),
            TextJustification::BottomLeft
        );
        let frame = GraphicKind::TextFrame.field_policies();
        assert!(
            frame
                .show_border
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap()
        );
        assert!(
            frame
                .word_wrap
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap()
        );
        assert!(
            !frame
                .clip_to_rect
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap()
        );
        assert_eq!(
            frame
                .alignment
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap(),
            HorizontalAlign::Left
        );
        let image = GraphicKind::Image.field_policies();
        assert!(
            !image
                .is_solid
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap()
        );
        assert!(
            image
                .keep_aspect
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap()
        );
        assert!(
            image
                .embed_image
                .unwrap()
                .definition(&FieldIntent::Absent)
                .unwrap()
        );
    }
    #[test]
    fn optional_arc_endpoint_distinguishes_absence_set_clear_inherit_and_reset() {
        for kind in ["arc", "elliptical_arc", "pie"] {
            let absent = graphic(&format!("{kind} {{}}")).unwrap();
            let policy = absent.kind.node.field_policies().end_angle.unwrap();
            assert_eq!(absent.fields.end_angle, FieldIntent::Absent);
            assert_eq!(policy.definition(&absent.fields.end_angle).unwrap(), None);
            let set = graphic(&format!("{kind} {{end_angle:360}}")).unwrap();
            assert!(matches!(
                set.fields.end_angle,
                FieldIntent::Set(Spanned {
                    node: Some(360.0),
                    ..
                })
            ));
            let clear = graphic(&format!("{kind} {{end_angle:null}}")).unwrap();
            assert!(matches!(clear.fields.end_angle, FieldIntent::Clear(_)));
            assert_eq!(policy.definition(&clear.fields.end_angle).unwrap(), None);
            let inherited = graphic(&format!("{kind} {{end_angle:inherit}}")).unwrap();
            let reset = graphic(&format!("{kind} {{end_angle:reset}}")).unwrap();
            let target = Some(270.0);
            let base = Some(180.0);
            assert_eq!(
                policy
                    .instance(&absent.fields.end_angle, Some(&target), Some(&base))
                    .unwrap(),
                target
            );
            assert_eq!(
                policy
                    .instance(&inherited.fields.end_angle, Some(&target), Some(&base))
                    .unwrap(),
                base
            );
            assert_eq!(
                policy
                    .instance(&reset.fields.end_angle, Some(&target), Some(&base))
                    .unwrap(),
                None
            );
            assert!(
                policy
                    .instance(&inherited.fields.end_angle, Some(&target), None)
                    .is_err()
            );
        }
    }
    #[test]
    fn geometry_reset_and_text_clear_preserve_explicit_operations() {
        let rectangle = graphic("rectangle {from:reset, to:inherit, is_solid:reset}").unwrap();
        assert!(matches!(rectangle.fields.from, FieldIntent::Reset(_)));
        assert!(matches!(rectangle.fields.to, FieldIntent::Inherit(_)));
        let label = graphic("label {text:null, url:null}").unwrap();
        assert!(matches!(label.fields.text, FieldIntent::Clear(_)));
        assert_eq!(
            GraphicKind::Label
                .field_policies()
                .text
                .unwrap()
                .definition(&label.fields.text)
                .unwrap(),
            ""
        );
        let image = graphic("image {file_name:null}").unwrap();
        assert!(matches!(image.fields.file_name, FieldIntent::Clear(_)));
        for source in [
            "rectangle {from:null}",
            "arc {start_angle:null}",
            "polyline {vertices:null}",
            "label {font_id:null}",
            "image {embed_image:null}",
        ] {
            assert!(
                graphic(source).is_err(),
                "required native field must reject clear: {source}"
            );
        }
    }
    #[test]
    fn named_and_exact_physical_pen_widths_are_typed() {
        for (name, width) in [
            ("zero", PenWidth::Zero),
            ("small", PenWidth::Small),
            ("Medium", PenWidth::Medium),
            ("large", PenWidth::Large),
            ("0mil", PenWidth::Zero),
            ("10mil", PenWidth::Small),
            ("30mil", PenWidth::Medium),
            ("50mil", PenWidth::Large),
            ("0raw", PenWidth::Zero),
            ("100000raw", PenWidth::Small),
            ("300000raw", PenWidth::Medium),
            ("500000raw", PenWidth::Large),
            ("0.254mm", PenWidth::Small),
            ("0.762mm", PenWidth::Medium),
            ("1.27mm", PenWidth::Large),
            ("0", PenWidth::Zero),
            ("10", PenWidth::Small),
            ("30.0", PenWidth::Medium),
            ("50", PenWidth::Large),
        ] {
            let line = graphic(&format!("line {{line_width:{name}}}")).unwrap();
            assert!(matches!(line.fields.line_width, FieldIntent::Set(value) if value.node==width));
        }
        for value in [
            "1mil",
            "0.2mm",
            "1",
            "0.5",
            "-10mil",
            "-1raw",
            "99999raw",
            "100001raw",
            "49.9999mil",
            "50.0001mil",
        ] {
            let error = graphic(&format!("line {{line_width:{value}}}")).unwrap_err();
            assert!(error.message.contains("zero, small, medium, or large"));
            assert!(error.span.is_some());
        }
        assert!(graphic("line {line_width:wide}").is_err());
        assert!(graphic("line {line_width:null}").is_err());
    }
    #[test]
    fn physical_widths_keep_field_policies_and_checked_numeric_errors() {
        let policy = GraphicKind::Line.field_policies().line_width.unwrap();
        for (body, expected) in [
            ("", PenWidth::Large),
            ("line_width:inherit", PenWidth::Medium),
            ("line_width:reset", PenWidth::Zero),
            ("line_width:10mil", PenWidth::Small),
        ] {
            let line = graphic(&format!("line {{{body}}}")).unwrap();
            assert_eq!(
                policy
                    .instance(
                        &line.fields.line_width,
                        Some(&PenWidth::Large),
                        Some(&PenWidth::Medium)
                    )
                    .unwrap(),
                expected
            );
        }
        for value in [
            "999999999999mil",
            "2147483648raw",
            "999999999999.0",
            "1.0 / 0.0",
        ] {
            let error = graphic(&format!("line {{line_width:{value}}}")).unwrap_err();
            assert!(
                error.span.is_some(),
                "numeric failure must retain source context: {value}"
            );
        }
        let error = pen_width(Value::Float(f64::INFINITY), Span::new(0, 1)).unwrap_err();
        assert!(error.span.is_some());
        let error = graphic("polyline {line_shape_size:10mil}").unwrap_err();
        assert!(error.message.contains("named endpoint size"));
    }
    #[test]
    fn native_applicability_and_existing_named_enums_are_checked() {
        let line = graphic("line {line_style:dash_dotted}").unwrap();
        assert!(
            matches!(line.fields.line_style, FieldIntent::Set(value) if value.node==LineStyle::DashDotted)
        );
        let polyline = graphic(
            "polyline {start_line_shape:solid_arrow,end_line_shape:circle,line_shape_size:large}",
        )
        .unwrap();
        assert!(
            matches!(polyline.fields.start_line_shape, FieldIntent::Set(value) if value.node==LineShape::SolidArrow)
        );
        let label = graphic("label {orientation:90,justification:top_right,is_mirrored:true,url:\"https://example.test\"}").unwrap();
        assert!(
            matches!(label.fields.orientation, FieldIntent::Set(value) if value.node==RotationBy90::Rotate90)
        );
        assert!(
            matches!(label.fields.justification, FieldIntent::Set(value) if value.node==TextJustification::TopRight)
        );
        let frame = graphic("text_frame {alignment:right,word_wrap:false,text_margin:2mil,transparent:true,text_color:#123456}").unwrap();
        assert!(
            matches!(frame.fields.alignment, FieldIntent::Set(value) if value.node==HorizontalAlign::Right)
        );
        let image =
            graphic("image {orientation:180,is_solid:true,keep_aspect:false,embed_image:false}")
                .unwrap();
        assert!(matches!(image.fields.keep_aspect, FieldIntent::Set(value) if !value.node));
        for source in [
            "polyline {closed:true}",
            "image {show_border:true}",
            "label {line_width:small}",
            "arc {area_color:#FFFFFF}",
            "round_rectangle {transparent:true}",
            "polygon {line_style:dashed}",
            "bezier {start_line_shape:arrow}",
            "text_frame {orientation:90}",
            "ellipse {end_angle:180}",
            "label {alignment:left}",
            "image {url:\"x\"}",
            "line {line_style:unknown}",
            "polyline {end_line_shape:unknown}",
            "label {justification:unknown}",
            "text_frame {alignment:unknown}",
            "image {orientation:45}",
        ] {
            assert!(
                graphic(source).is_err(),
                "inapplicable or invalid native field must reject: {source}"
            );
        }
    }
}
