use super::compile::{
    Properties, boolean, color, coord, finite_number, point, positive_integer, string,
};
use super::{FieldIntent, FieldPolicy};
use crate::ast::GraphicDecl;
use crate::{ScopeStack, SpecError};
use altium_format_types::{Color, Coord, CoordPoint};

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

/// Positive source font-table reference; native font data remains document-owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontReference(pub u32);

/// Geometry remains typed and source-scoped. Inapplicable properties are rejected
/// per kind; this is not a bag of string-keyed runtime values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphicFields {
    pub from: FieldIntent<CoordPoint>,
    pub to: FieldIntent<CoordPoint>,
    pub center: FieldIntent<CoordPoint>,
    pub at: FieldIntent<CoordPoint>,
    pub radius: FieldIntent<Coord>,
    pub secondary_radius: FieldIntent<Coord>,
    pub corner_x_radius: FieldIntent<Coord>,
    pub corner_y_radius: FieldIntent<Coord>,
    pub start_angle: FieldIntent<f64>,
    pub end_angle: FieldIntent<f64>,
    pub vertices: FieldIntent<Vec<CoordPoint>>,
    pub color: FieldIntent<Color>,
    pub area_color: FieldIntent<Color>,
    pub line_width: FieldIntent<Coord>,
    pub is_solid: FieldIntent<bool>,
    pub closed: FieldIntent<bool>,
    pub show_border: FieldIntent<bool>,
    pub text: FieldIntent<String>,
    pub font_id: FieldIntent<FontReference>,
    pub file_name: FieldIntent<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct GraphicIntent {
    pub auto_size: Option<super::AutoSizeIntent>,
    pub kind: crate::diagnostic::Spanned<GraphicKind>,
    pub fields: GraphicFields,
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
        // The layout compiler has already parsed these into typed authored intent.
        props.consume_layout(&["padding", "min_width", "min_height"]);
    }
    let mut fields = GraphicFields::default();
    // No default/clear policy has been accepted for graphic fields yet. Explicit
    // values are supported; reset/inherit/clear produce a contextual hard error.
    macro_rules! field {
        ($field:ident, $parse:expr) => {
            fields.$field = props.take(
                stringify!($field),
                &[],
                FieldPolicy {
                    name: concat!("graphic.", stringify!($field)),
                    default: None,
                    clear: None,
                },
                $parse,
            )?;
        };
    }
    field!(color, color);
    field!(line_width, coord);
    match kind {
        Line | Rectangle | RoundRectangle | TextFrame | Image => {
            field!(from, point);
            field!(to, point);
        }
        Arc | EllipticalArc | Ellipse | Pie => {
            field!(center, point);
            field!(radius, coord);
        }
        Polyline | Polygon | Bezier => {
            field!(vertices, |value, span| match value {
                crate::Value::Array(values) => values.into_iter().map(|v| point(v, span)).collect(),
                _ => Err(super::compile::error(
                    "vertices must be an array of coordinates",
                    span
                )),
            });
        }
        Label => {
            field!(at, point);
        }
    }
    if matches!(
        kind,
        Rectangle | RoundRectangle | Ellipse | Polygon | Pie | TextFrame
    ) {
        field!(is_solid, boolean);
        field!(area_color, color);
    }
    if matches!(kind, Arc | EllipticalArc | Pie) {
        field!(start_angle, finite_number);
        field!(end_angle, finite_number);
    }
    if matches!(kind, EllipticalArc | Ellipse) {
        field!(secondary_radius, coord);
    }
    if kind == RoundRectangle {
        field!(corner_x_radius, coord);
        field!(corner_y_radius, coord);
    }
    if matches!(kind, Label | TextFrame) {
        field!(text, string);
        field!(font_id, |value, span| positive_integer(value, span)
            .map(|v| FontReference(v as u32)));
    }
    if matches!(kind, TextFrame | Image) {
        field!(show_border, boolean);
    }
    if kind == Polyline {
        field!(closed, boolean);
    }
    if kind == Image {
        field!(file_name, string);
    }
    props.finish(&format!("{} graphic", decl.graphic_type.node))?;
    Ok(GraphicIntent {
        auto_size,
        kind: crate::diagnostic::Spanned::new(kind, decl.graphic_type.span),
        fields,
    })
}
