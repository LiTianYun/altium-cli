//! Strict projection of a captured footprint's pad namespace. Geometry is checked
//! while expanding layouts, but native footprint materialization remains step 6.
use super::compile::{coord, error, finite_number, object_scope, point, string};
use crate::ast::*;
use crate::eval::DefinitionReference;
use crate::{ScopeStack, SpecError, SpecErrorCode, Value, eval_expr};
use altium_format_types::pcb::HoleType;
use altium_format_types::{Coord, CoordPoint, PadShape, PadStackMode, PlaneConnectionStyle};
use indexmap::IndexMap;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

// An evaluator resource bound, not a limit of the Altium format. Reject before
// allocation/iteration so malformed negative or enormous counts cannot exhaust RAM.
const MAX_GENERATED_POSITIONS: usize = 1_000_000;

#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}
struct Anchor {
    from: CoordPoint,
    to: CoordPoint,
    edge: Edge,
}
type BoxBindings = BTreeMap<String, Option<(CoordPoint, CoordPoint)>>;

struct Inputs {
    values: IndexMap<String, Value>,
    anchor: Option<Anchor>,
    span: Span,
}
impl Inputs {
    fn take(&mut self, key: &str) -> Option<Value> {
        self.values.shift_remove(key)
    }
    fn integer(&mut self, key: &str, default: i32) -> Result<i32, SpecError> {
        match self.take(key) {
            None => Ok(default),
            Some(Value::Integer(value)) => Ok(value),
            Some(_) => Err(error(format!("{key} must be an integer"), self.span)),
        }
    }
    fn dimension(&mut self, key: &str, default: Coord) -> Result<Coord, SpecError> {
        match self.take(key) {
            None => Ok(default),
            Some(value) => coord(value, self.span),
        }
    }
    fn text(&mut self, key: &str, default: &str) -> Result<String, SpecError> {
        match self.take(key) {
            None => Ok(default.to_string()),
            Some(value) => string(value, self.span),
        }
    }
    fn boolean(&mut self, key: &str, default: bool) -> Result<bool, SpecError> {
        match self.take(key) {
            None => Ok(default),
            Some(Value::Bool(value)) => Ok(value),
            Some(_) => Err(error(format!("{key} must be boolean"), self.span)),
        }
    }
    fn skip(&mut self) -> Result<BTreeSet<String>, SpecError> {
        let mut result = BTreeSet::new();
        if let Some(value) = self.take("skip") {
            let Value::Array(values) = value else {
                return Err(error("skip must be an array of pad names", self.span));
            };
            for value in values {
                let name = match value {
                    Value::String(value) if !value.is_empty() => value,
                    Value::Integer(value) => value.to_string(),
                    _ => {
                        return Err(error(
                            "skip entries must be nonempty strings or integers",
                            self.span,
                        ));
                    }
                };
                if !result.insert(name.clone()) {
                    return Err(error(format!("duplicate skip entry '{name}'"), self.span));
                }
            }
        }
        Ok(result)
    }
    fn template(&mut self) -> Result<(), SpecError> {
        if let Some(value) = self.take("pad") {
            let Value::Object(values) = value else {
                return Err(error("pad template must be an object", self.span));
            };
            validate_pad(values.into_values(), self.span, true)?;
        }
        Ok(())
    }
    fn finish(self) -> Result<(), SpecError> {
        if let Some((key, _)) = self.values.first() {
            return Err(SpecError::at(
                SpecErrorCode::UnknownProperty,
                format!("unknown or unsupported pad generator property '{key}'"),
                self.span,
            ));
        }
        Ok(())
    }
}

fn insert(values: &mut IndexMap<String, Value>, key: String, value: Value) {
    values.shift_remove(&key);
    values.insert(key, value);
}
fn inputs(
    body: &Spanned<Object>,
    parent: &ScopeStack,
    allow_anchor: bool,
    boxes: &BoxBindings,
) -> Result<Inputs, SpecError> {
    let scope = object_scope(body, parent)?;
    let mut values = IndexMap::new();
    let mut direct = BTreeMap::new();
    let mut anchor = None;
    for item in &body.node.items {
        match &item.node {
            ObjectItem::LetBinding(_) => {} // Evaluated by object_scope, including cycle checks.
            ObjectItem::Property(property) => {
                if let Some(previous) = direct.insert(property.key.node.clone(), property.key.span)
                {
                    return Err(error(
                        format!(
                            "duplicate property '{}'; first at {}..{}",
                            property.key.node, previous.start, previous.end
                        ),
                        property.key.span,
                    ));
                }
                if property.key.node == "on" && allow_anchor {
                    anchor = Some(resolve_anchor(&property.value, &scope, boxes)?);
                } else {
                    let value = if property.key.node == "pad" {
                        if let Expr::Object(object) = &property.value.node {
                            let template = inputs(
                                &Spanned::new(object.clone(), property.value.span),
                                &scope,
                                false,
                                boxes,
                            )?;
                            Value::Object(template.values.into())
                        } else {
                            eval_expr(&property.value, &scope)?
                        }
                    } else {
                        eval_expr(&property.value, &scope)?
                    };
                    insert(&mut values, property.key.node.clone(), value);
                }
            }
            ObjectItem::Spread(expr) => {
                let Value::Object(object) = eval_expr(expr, &scope)? else {
                    return Err(error("generator spread requires an object", expr.span));
                };
                for (key, value) in object {
                    if key == "on" {
                        return Err(error(
                            "row on: must be a direct symbolic edge reference",
                            expr.span,
                        ));
                    }
                    insert(&mut values, key, value);
                }
            }
        }
    }
    Ok(Inputs {
        values,
        anchor,
        span: body.span,
    })
}
fn resolve_anchor(
    expr: &Spanned<Expr>,
    scope: &ScopeStack,
    boxes: &BoxBindings,
) -> Result<Anchor, SpecError> {
    let Expr::Path(base, field) = &expr.node else {
        return Err(error(
            "on must be a bound box edge such as $body.left",
            expr.span,
        ));
    };
    let Expr::DollarIdent(binding) = &base.node else {
        return Err(error(
            "on requires a dollar-prefixed box binding",
            expr.span,
        ));
    };
    // The row's own let scope shadows footprint graphic bindings. A local
    // non-box graphic blocks an outer same-name box instead of falling through.
    let geometry = if scope.lookup_current(binding).is_none() {
        boxes.get(binding)
    } else {
        None
    };
    let (from, to) = if let Some(geometry) = geometry {
        geometry.ok_or_else(|| {
            error(
                format!("graphic '${binding}' is not an explicit box"),
                expr.span,
            )
        })?
    } else {
        let value = scope
            .lookup_dollar(binding)
            .ok_or_else(|| error(format!("undefined box binding '${binding}'"), base.span))??;
        let Value::Object(values) = value else {
            return Err(error(
                "row box binding must be an object with from/to coordinates",
                expr.span,
            ));
        };
        let required = |key: &str| {
            values
                .get(key)
                .cloned()
                .ok_or_else(|| {
                    error(
                        format!("row box binding '${binding}' has no {key}"),
                        expr.span,
                    )
                })
                .and_then(|value| point(value, expr.span))
        };
        (required("from")?, required("to")?)
    };
    let edge = match field.node.as_str() {
        "left" => Edge::Left,
        "right" => Edge::Right,
        "top" => Edge::Top,
        "bottom" => Edge::Bottom,
        _ => return Err(error("row edge must be left/right/top/bottom", field.span)),
    };
    Ok(Anchor { from, to, edge })
}
fn count(value: i32, key: &str, span: Span) -> Result<usize, SpecError> {
    let count = usize::try_from(value)
        .ok()
        .filter(|count| *count > 0 && *count <= MAX_GENERATED_POSITIONS)
        .ok_or_else(|| {
            error(
                format!("{key} must be positive and at most {MAX_GENERATED_POSITIONS} positions"),
                span,
            )
        })?;
    Ok(count)
}
fn position(x: i64, y: i64, span: Span) -> Result<CoordPoint, SpecError> {
    let checked = |value| {
        i32::try_from(value).map(Coord::from_internal).map_err(|_| {
            SpecError::at(
                SpecErrorCode::ArithmeticOverflow,
                "generated pad coordinate exceeds the internal coordinate range",
                span,
            )
        })
    };
    Ok(CoordPoint::new(checked(x)?, checked(y)?))
}
fn nonnegative(value: Coord, key: &str, span: Span) -> Result<Coord, SpecError> {
    if value.raw() < 0 {
        return Err(error(format!("{key} must be nonnegative"), span));
    }
    Ok(value)
}
fn row(
    body: &Spanned<Object>,
    scope: &ScopeStack,
    boxes: &BoxBindings,
) -> Result<Vec<String>, SpecError> {
    let mut input = inputs(body, scope, true, boxes)?;
    let span = input.span;
    let count = count(input.integer("count", 1)?, "count", span)?;
    let start = input.integer("start", 1)?;
    let pitch =
        i64::from(nonnegative(input.dimension("pitch", Coord::ZERO)?, "pitch", span)?.raw());
    let skip = input.skip()?;
    input.template()?;
    let (first_x, first_y, step_x, step_y) = if let Some(anchor) = input.anchor.take() {
        let direction = input.text("direction", "forward")?.to_ascii_lowercase();
        let reverse = match direction.as_str() {
            "forward" => false,
            "reverse" => true,
            _ => {
                return Err(error(
                    "anchored row direction must be forward/reverse",
                    span,
                ));
            }
        };
        let at = input.text("at", "center")?.to_ascii_lowercase();
        let x0 = i64::from(anchor.from.x.raw().min(anchor.to.x.raw()));
        let x1 = i64::from(anchor.from.x.raw().max(anchor.to.x.raw()));
        let y0 = i64::from(anchor.from.y.raw().min(anchor.to.y.raw()));
        let y1 = i64::from(anchor.from.y.raw().max(anchor.to.y.raw()));
        let (fixed, low, high, forward, vertical) = match anchor.edge {
            Edge::Left => (x0, y0, y1, -1, true),
            Edge::Right => (x1, y0, y1, 1, true),
            Edge::Top => (y1, x0, x1, 1, false),
            Edge::Bottom => (y0, x0, x1, -1, false),
        };
        let center = match at.as_str() {
            "center" => (low + high) / 2,
            "start" => {
                if forward > 0 {
                    low
                } else {
                    high
                }
            }
            "end" => {
                if forward > 0 {
                    high
                } else {
                    low
                }
            }
            _ => return Err(error("anchored row at must be start/center/end", span)),
        };
        // Preserve the legacy row equation: first offset is negative half-span;
        // subsequent pitch follows the selected edge direction.
        let first = center - pitch * (count as i64 - 1) / 2;
        let step = pitch * if reverse { -forward } else { forward };
        if vertical {
            (fixed, first, 0, step)
        } else {
            (first, fixed, step, 0)
        }
    } else {
        let at = match input.take("at") {
            None => CoordPoint::zero(),
            Some(value) => point(value, span)?,
        };
        let (dx, dy) = match input
            .text("direction", "right")?
            .to_ascii_lowercase()
            .as_str()
        {
            "right" => (pitch, 0),
            "left" => (-pitch, 0),
            "up" => (0, pitch),
            "down" => (0, -pitch),
            _ => {
                return Err(error(
                    "absolute row direction must be up/down/left/right",
                    span,
                ));
            }
        };
        (i64::from(at.x.raw()), i64::from(at.y.raw()), dx, dy)
    };
    input.finish()?;
    // Coordinate progression is linear; validating both endpoints bounds all pads.
    position(first_x, first_y, span)?;
    position(
        first_x + step_x * (count as i64 - 1),
        first_y + step_y * (count as i64 - 1),
        span,
    )?;
    let mut names = Vec::with_capacity(count);
    let mut next = i64::from(start);
    for _ in 0..count {
        loop {
            let number = i32::try_from(next).map_err(|_| {
                SpecError::at(
                    SpecErrorCode::ArithmeticOverflow,
                    "generated row pad number exceeds signed integer range",
                    span,
                )
            })?;
            next += 1;
            let name = number.to_string();
            if !skip.contains(&name) {
                names.push(name);
                break;
            }
        }
    }
    Ok(names)
}
fn grid(body: &Spanned<Object>, scope: &ScopeStack) -> Result<Vec<String>, SpecError> {
    let mut input = inputs(body, scope, false, &BoxBindings::new())?;
    let span = input.span;
    let rows = count(input.integer("rows", 1)?, "rows", span)?;
    let cols = count(input.integer("cols", 1)?, "cols", span)?;
    let cells = rows
        .checked_mul(cols)
        .filter(|cells| *cells <= MAX_GENERATED_POSITIONS)
        .ok_or_else(|| {
            error(
                format!("grid exceeds {MAX_GENERATED_POSITIONS} positions"),
                span,
            )
        })?;
    let pitch = nonnegative(input.dimension("pitch", Coord::ZERO)?, "pitch", span)?;
    let pitch_x =
        i64::from(nonnegative(input.dimension("pitch_x", pitch)?, "pitch_x", span)?.raw());
    let pitch_y =
        i64::from(nonnegative(input.dimension("pitch_y", pitch)?, "pitch_y", span)?.raw());
    let origin = match input.take("origin") {
        None => CoordPoint::zero(),
        Some(value) => point(value, span)?,
    };
    let alpha = match input
        .text("naming", "numeric")?
        .to_ascii_lowercase()
        .as_str()
    {
        "numeric" => false,
        "alphanumeric" => true,
        _ => return Err(error("grid naming must be numeric/alphanumeric", span)),
    };
    let perimeter = input.boolean("perimeter_only", false)?;
    let skip = input.skip()?;
    input.template()?;
    input.finish()?;
    let mut names = Vec::with_capacity(cells);
    for row in 0..rows {
        for col in 0..cols {
            let name = if alpha {
                format!("{}{}", bga_row(row), col + 1)
            } else {
                (row * cols + col + 1).to_string()
            };
            if (perimeter && row != 0 && row + 1 != rows && col != 0 && col + 1 != cols)
                || skip.contains(&name)
            {
                continue;
            }
            position(
                i64::from(origin.x.raw()) + pitch_x * (2 * col as i64 - (cols as i64 - 1)) / 2,
                i64::from(origin.y.raw()) - pitch_y * (2 * row as i64 - (rows as i64 - 1)) / 2,
                span,
            )?;
            names.push(name);
        }
    }
    Ok(names)
}
// Altium-spec legacy BGA naming alphabet, not native record tags/constants.
const BGA_LETTERS: &[u8] = b"ABCDEFGHJKLMNPRTUVWY";
fn bga_row(mut row: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(char::from(BGA_LETTERS[row % BGA_LETTERS.len()]));
        if row < BGA_LETTERS.len() {
            break;
        }
        row = row / BGA_LETTERS.len() - 1;
    }
    letters.into_iter().rev().collect()
}

fn pad_shape(value: Value, span: Span) -> Result<PadShape, SpecError> {
    match string(value, span)?.to_ascii_lowercase().as_str() {
        "no_shape" | "none" => Ok(PadShape::NoShape),
        "round" | "circle" => Ok(PadShape::Round),
        "rectangular" | "rect" | "square" => Ok(PadShape::Rectangular),
        "octagonal" | "octagon" => Ok(PadShape::Octagonal),
        "arc" => Ok(PadShape::Arc),
        "terminator" => Ok(PadShape::Terminator),
        "round_rect" | "roundrect" => Ok(PadShape::RoundRect),
        "rotated_rect" | "rotatedrect" => Ok(PadShape::RotatedRect),
        "rounded_rectangular" | "roundedrectangular" => Ok(PadShape::RoundedRectangular),
        "custom" => Ok(PadShape::Custom),
        _ => Err(error("unknown pad shape", span)),
    }
}
fn validate_pad(
    values: IndexMap<String, Value>,
    span: Span,
    template: bool,
) -> Result<(), SpecError> {
    if values.contains_key("at") && (values.contains_key("x") || values.contains_key("y")) {
        return Err(error("pad at conflicts with x/y", span));
    }
    for (key, value) in values {
        let validate = || -> Result<(), SpecError> {
            match key.as_str() {
                "at" | "x" | "y" if template => {
                    return Err(error(
                        "pad template cannot specify coordinates owned by its generator",
                        span,
                    ));
                }
                "at" => {
                    point(value.clone(), span)?;
                }
                "x" | "y" | "solder_mask_expansion" | "paste_mask_expansion" => {
                    coord(value.clone(), span)?;
                }
                "x_size"
                | "y_size"
                | "hole_size"
                | "relief_conductor_width"
                | "relief_air_gap"
                | "mid_x_size"
                | "mid_y_size"
                | "bot_x_size"
                | "bot_y_size"
                | "slot_size" => {
                    nonnegative(coord(value.clone(), span)?, &key, span)?;
                }
                "shape" | "mid_shape" | "bot_shape" => {
                    pad_shape(value.clone(), span)?;
                }
                "rotation" => {
                    finite_number(value.clone(), span)?;
                }
                "is_plated" => {
                    if !matches!(value, Value::Bool(_)) {
                        return Err(error("is_plated must be boolean", span));
                    }
                }
                "relief_entries" => {
                    if !matches!(value, Value::Integer(n) if n > 0) {
                        return Err(error("relief_entries must be a positive integer", span));
                    }
                }
                "layer" => {
                    if string(value.clone(), span)?.is_empty() {
                        return Err(error("layer name must not be empty", span));
                    }
                }
                "pad_mode" => {
                    let _: PadStackMode =
                        match string(value.clone(), span)?.to_ascii_lowercase().as_str() {
                            "simple" => PadStackMode::Simple,
                            "local_stack" | "localstack" => PadStackMode::LocalStack,
                            "external_stack" | "externalstack" => PadStackMode::ExternalStack,
                            _ => return Err(error("unknown pad stack mode", span)),
                        };
                }
                "plane_connection" => {
                    let _: PlaneConnectionStyle =
                        match string(value.clone(), span)?.to_ascii_lowercase().as_str() {
                            "no_connect" | "noconnect" | "none" => PlaneConnectionStyle::NoConnect,
                            "relief" => PlaneConnectionStyle::Relief,
                            "direct" => PlaneConnectionStyle::Direct,
                            _ => return Err(error("unknown plane connection", span)),
                        };
                }
                "hole_shape" => {
                    let _: HoleType =
                        match string(value.clone(), span)?.to_ascii_lowercase().as_str() {
                            "round" => HoleType::Round,
                            "square" => HoleType::Square,
                            "slot" => HoleType::Slot,
                            _ => return Err(error("unknown hole shape", span)),
                        };
                }
                _ => {
                    return Err(SpecError::at(
                        SpecErrorCode::UnknownProperty,
                        format!("unknown pad property '{key}'"),
                        span,
                    ));
                }
            }
            Ok(())
        };
        validate().map_err(|mut error| {
            error.message = format!("pad property '{key}': {}", error.message);
            error
        })?;
    }
    Ok(())
}
fn declare(names: &mut BTreeMap<String, Span>, name: &str, span: Span) -> Result<(), SpecError> {
    if name.is_empty() {
        return Err(error("pad/binding name cannot be empty", span));
    }
    if let Some(first) = names.insert(name.to_string(), span) {
        return Err(SpecError::at(
            SpecErrorCode::DuplicateEntity,
            format!(
                "duplicate pad/binding '{name}', first declared at {}..{}",
                first.start, first.end
            ),
            span,
        ));
    }
    Ok(())
}

pub(super) fn export(
    source: &Path,
    decl: &FootprintDecl,
    parent: &ScopeStack,
) -> Result<Value, SpecError> {
    let mut scope = parent.clone();
    scope.push();
    let mut binding_names = BTreeMap::new();
    let mut explicit = IndexMap::new();
    for item in &decl.body {
        if let FootprintItem::LetBinding(binding) = &item.node {
            declare(&mut binding_names, &binding.name.node, binding.name.span)?;
            scope.mark_evaluating(&binding.name.node);
            scope.define(
                binding.name.node.clone(),
                eval_expr(&binding.value, &scope)?,
            );
        }
    }
    // Collect all bindings before generators so textual forward references to
    // explicit box graphics have the same result as backward references.
    let mut boxes = BoxBindings::new();
    for item in &decl.body {
        let FootprintItem::Graphic(graphic) = &item.node else {
            continue;
        };
        let Some(binding) = &graphic.binding else {
            continue;
        };
        declare(&mut binding_names, &binding.node, binding.span)?;
        let geometry = if matches!(
            graphic.graphic_type.node.as_str(),
            "rectangle" | "round_rectangle" | "text_frame" | "image" | "fill"
        ) {
            let mut properties = inputs(&graphic.body, &scope, false, &BoxBindings::new())?;
            let (from_key, to_key) = if graphic.graphic_type.node == "fill" {
                ("corner1", "corner2")
            } else {
                ("from", "to")
            };
            match (properties.take(from_key), properties.take(to_key)) {
                (Some(from), Some(to)) => Some((
                    point(from, graphic.body.span)?,
                    point(to, graphic.body.span)?,
                )),
                _ => None,
            }
        } else {
            None
        };
        boxes.insert(binding.node.clone(), geometry);
    }
    for item in &decl.body {
        if let FootprintItem::Pad(pad) = &item.node {
            let name = pad.name.node.as_str();
            if let Some(previous) = explicit.insert(name.clone(), pad) {
                return Err(error(
                    format!(
                        "duplicate explicit pad '{name}', first at {}..{}",
                        previous.name.span.start, previous.name.span.end
                    ),
                    pad.name.span,
                ));
            }
            if name.is_empty() {
                return Err(error("pad name cannot be empty", pad.name.span));
            }
            let pad_values = inputs(&pad.body, &scope, false, &boxes)?;
            validate_pad(pad_values.values, pad.body.span, false)?;
        }
    }
    let mut pads = IndexMap::new();
    let mut generated = BTreeMap::new();
    for item in &decl.body {
        let names = match &item.node {
            FootprintItem::Row(decl) | FootprintItem::Column(decl) => {
                row(&decl.body, &scope, &boxes)?
            }
            FootprintItem::Grid(decl) => grid(&decl.body, &scope)?,
            _ => continue, // These are not generator declarations; explicit pads handled below.
        };
        if generated
            .len()
            .checked_add(names.len())
            .is_none_or(|count| count > MAX_GENERATED_POSITIONS)
        {
            return Err(error(
                "footprint exceeds the generated position resource limit",
                item.span,
            ));
        }
        for name in names {
            declare(&mut generated, &name, item.span)?;
            declare(&mut binding_names, &name, item.span)?;
            pads.insert(name.clone(), name);
        }
    }
    // Explicit same-name pads override generated geometry without adding another
    // pad. Their distinct source binding remains available for SchLib mappings.
    for (name, pad) in explicit {
        if !pads.contains_key(&name) {
            declare(&mut binding_names, &name, pad.name.span)?;
            pads.insert(name.clone(), name.clone());
        }
        if let Some(binding) = &pad.binding {
            if binding.node != name {
                declare(&mut binding_names, &binding.node, binding.span)?;
                pads.insert(binding.node.clone(), name);
            }
        }
    }
    Ok(Value::Definition(DefinitionReference::Footprint {
        source: source.to_path_buf(),
        name: decl.name.node.as_str(),
        pads,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pads(body: &str) -> Result<IndexMap<String, String>, SpecError> {
        let ast = crate::parser::parse_spec(&format!("footprint F {{{body}}}")).unwrap();
        let SpecItem::Footprint(decl) = &ast.items[0].node else {
            panic!("footprint")
        };
        let Value::Definition(DefinitionReference::Footprint { pads, source, name }) = export(
            Path::new("/captured/pads.pcblib-spec"),
            decl,
            &ScopeStack::new(),
        )?
        else {
            panic!("typed footprint")
        };
        assert_eq!(source, Path::new("/captured/pads.pcblib-spec"));
        assert_eq!(name, "F");
        Ok(pads)
    }
    #[test]
    fn row_column_skip_and_explicit_override_bindings() {
        let result = pads("row {count:3, skip:[2], pitch:10mil, direction:Up} special=pad 3 {shape:rectangular} column {count:2,start:10}").unwrap();
        assert_eq!(
            result.keys().map(String::as_str).collect::<Vec<_>>(),
            ["1", "3", "4", "10", "11", "special"]
        );
        assert_eq!(result["special"], "3");
    }
    #[test]
    fn grid_perimeter_skip_and_bga_names() {
        let numeric = pads("grid {rows:3,cols:3,perimeter_only:true,skip:[1]}").unwrap();
        assert_eq!(
            numeric.keys().map(String::as_str).collect::<Vec<_>>(),
            ["2", "3", "4", "6", "7", "8", "9"]
        );
        let bga = pads("grid {rows:21,cols:1,naming:alphanumeric}").unwrap();
        assert_eq!(bga.len(), 21);
        assert!(bga.contains_key("Y1") && bga.contains_key("AA1"));
        assert!(!bga.contains_key("I1"));
    }
    #[test]
    fn bound_graphic_forward_reference_and_all_edges() {
        for edge in ["left", "right", "top", "bottom"] {
            let row =
                format!("row {{on:$body.{edge},count:3,pitch:5mil,at:Center,direction:Reverse}}");
            let graphic = "body=rectangle {from:(-20mil,-20mil),to:(20mil,20mil)}";
            assert_eq!(
                pads(&format!("{row} {graphic}")).unwrap(),
                pads(&format!("{graphic} {row}")).unwrap()
            );
        }
        assert!(pads("body=fill {corner1:(0,0),corner2:(10,10)} row {on:$body.left}").is_ok());
        assert!(pads("let body={from:(0,0),to:(10,10)} row {on:$body.left}").is_ok());
        assert!(pads("body=line {from:(0,0),to:(10,10)} row {on:$body.left}").is_err());
        assert!(pads("body=rectangle {} row {on:$body.left}").is_err());
        assert!(
            pads("body=rectangle {from:(0,0),to:(10,10)} row {let body=42, on:$body.left}")
                .is_err()
        );
    }
    #[test]
    fn anchored_edge_traversal_checks_the_correct_signed_endpoint() {
        for (edge, forward_fits) in [
            ("left", false),
            ("bottom", false),
            ("right", true),
            ("top", true),
        ] {
            for (direction, fits) in [("forward", forward_fits), ("reverse", !forward_fits)] {
                let source = format!(
                    "body=rectangle {{from:(-2147483646raw,-2147483646raw),to:(-2147483644raw,-2147483644raw)}} row {{on:$body.{edge}, count:2,pitch:4raw,direction:{direction}}}"
                );
                let result = pads(&source);
                assert_eq!(result.is_ok(), fits, "{edge}/{direction}: {result:?}");
                if let Err(error) = result {
                    assert_eq!(error.code, SpecErrorCode::ArithmeticOverflow);
                }
            }
        }
    }
    #[test]
    fn malformed_generators_fail_without_fallback_names() {
        for body in [
            "row {count:0}",
            "row {count:-1}",
            "row {count:1.5}",
            "row {count:1000001}",
            "grid {rows:1001,cols:1000}",
            "row {pitch:-1mil}",
            "row {skip:true}",
            "row {skip:[true]}",
            "row {skip:[1,1]}",
            "row {unknown:1}",
            "row {direction:diagonal}",
            "row {pad:42}",
            "row {pad:{x:10mil}}",
            "row {pad:{shape:triangle}}",
            "row {pad:{x_size:-1mil}}",
            "row {pad:{unknown:1}}",
            "row {count:2,start:2147483647}",
            "row {count:2,pitch:2147483647raw,at:(1raw,0raw)}",
            "grid {rows:3,cols:3,pitch:2147483647raw,origin:(1raw,0raw)}",
            "row {count:2} column {start:2}",
            "pad 1 {} pad 1 {}",
            "let a=1 a=pad 1 {}",
            "body=rectangle {} body=pad 1 {}",
            "row {count:2,count:3}",
            "row {...{pad:{shape:round,shape:rectangular}}}",
        ] {
            assert!(pads(body).is_err(), "accepted {body}");
        }
        assert!(
            pads("row {...{count:2,pad:{shape:round}},count:3,pad:{shape:rectangular}}").is_ok()
        );
    }
    #[test]
    fn captured_transitive_generator_exports_preserve_provenance_without_reread() {
        let dir = tempfile::tempdir().unwrap();
        let leaf = dir.path().join("constants.pcblib-spec");
        let footprint = dir.path().join("pads.pcblib-spec");
        let root = dir.path().join("root.schlib-spec");
        std::fs::write(
            &leaf,
            "let template={shape:rectangular,x_size:10mil,y_size:10mil}",
        )
        .unwrap();
        std::fs::write(&footprint, "import \"constants.pcblib-spec\" as shared\nfootprint F {row {count:3,pad:{...$shared.template}} terminal=pad 2 {}}").unwrap();
        std::fs::write(&root, "import \"pads.pcblib-spec\" as fp\ncomponent C { p=pin 1 {} footprint $fp.F {$p:$fp.F.terminal} }").unwrap();
        let graph = crate::import::capture_imports(&root).unwrap();
        let first = crate::intent::compile_captured_schlib(&graph).unwrap();
        let intent = &first.root.components.items[0].footprints.items[0];
        assert_eq!(intent.maps.items[0].pad.node, "2");
        assert_eq!(
            intent.source.as_ref().unwrap().source,
            footprint.canonicalize().unwrap()
        );
        std::fs::remove_file(&leaf).unwrap();
        std::fs::remove_file(&footprint).unwrap();
        std::fs::remove_file(&root).unwrap();
        assert_eq!(
            first,
            crate::intent::compile_captured_schlib(&graph).unwrap()
        );
    }
    #[test]
    fn imported_nested_template_duplicate_is_contextual_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("constants.pcblib-spec"),
            "let template={shape:round,shape:rectangular}",
        )
        .unwrap();
        std::fs::write(dir.path().join("pads.pcblib-spec"), "import \"constants.pcblib-spec\" as shared\nfootprint F {row {pad:{...$shared.template}}}").unwrap();
        let root = dir.path().join("root.schlib-spec");
        std::fs::write(
            &root,
            "import \"pads.pcblib-spec\" as fp\ncomponent C {footprint $fp.F}",
        )
        .unwrap();
        let graph = crate::import::capture_imports(&root).unwrap();
        let error = crate::intent::compile_captured_schlib(&graph).unwrap_err();
        assert!(error.message.contains("constants.pcblib-spec"), "{error}");
        assert!(error.message.contains("shape"), "{error}");
    }
}
