//! Source-bound layout dependencies. Compilation retains authored operands;
//! evaluation receives effective target/base/default values from the planner.
use super::compile::{coord, error, point};
use super::{FieldIntent, FieldPolicy, PartNumber};
use crate::ast::*;
use crate::{ScopeStack, SpecError, SpecErrorCode, Value, eval_expr};
use altium_format_types::{Coord, CoordPoint, RotationBy90};
use std::collections::BTreeMap;

/// Source owner and ordinal are lexical links, never native record identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayoutOwner {
    Component,
    Part(PartNumber),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphicSourceRef {
    pub owner: LayoutOwner,
    pub index: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PinSourceRef {
    pub owner: LayoutOwner,
    pub index: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnchorEdge {
    Left,
    Right,
    Top,
    Bottom,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementSide {
    Outside,
    Inside,
    Center,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorPosition {
    Start,
    Center,
    End,
    After(PinSourceRef),
    Before(PinSourceRef),
}
#[derive(Debug, Clone, PartialEq)]
pub struct PinPlacementIntent {
    pub graphic: Spanned<GraphicSourceRef>,
    pub edge: Spanned<AnchorEdge>,
    pub position: Spanned<AnchorPosition>,
    pub gap: FieldIntent<Coord>,
    pub side: FieldIntent<PlacementSide>,
    pub offset: FieldIntent<CoordPoint>,
    pub span: Span,
}
#[derive(Debug, Clone, PartialEq)]
pub struct AutoSizeIntent {
    /// Omission derives a separate margin from each chain root's gap. Set uses
    /// the authored common margin; explicit inherit/reset selects the versioned
    /// AUTO_PADDING default (20mil), independent of source declaration order.
    pub padding: FieldIntent<Coord>,
    pub min_width: FieldIntent<Coord>,
    pub min_height: FieldIntent<Coord>,
    /// These source-absent corners are explicitly inferred by the bound body.
    pub infer_from: bool,
    pub infer_to: bool,
    pub span: Span,
}
pub const ANCHOR_GAP: FieldPolicy<Coord> = FieldPolicy {
    name: "pin.gap",
    default: Some(|| Coord::from_mils(50).expect("50 mil fits Coord")),
    clear: None,
};
pub const ANCHOR_SIDE: FieldPolicy<PlacementSide> = FieldPolicy {
    name: "pin.side",
    default: Some(|| PlacementSide::Outside),
    clear: None,
};
pub const ANCHOR_OFFSET: FieldPolicy<CoordPoint> = FieldPolicy {
    name: "pin.offset",
    default: Some(CoordPoint::zero),
    clear: None,
};
pub const AUTO_PADDING: FieldPolicy<Coord> = FieldPolicy {
    name: "graphic.padding",
    default: Some(|| Coord::from_mils(20).expect("20 mil fits Coord")),
    clear: None,
};
pub const AUTO_MIN_WIDTH: FieldPolicy<Coord> = FieldPolicy {
    name: "graphic.min_width",
    default: Some(|| Coord::from_mils(50).expect("50 mil fits Coord")),
    clear: None,
};
pub const AUTO_MIN_HEIGHT: FieldPolicy<Coord> = FieldPolicy {
    name: "graphic.min_height",
    default: Some(|| Coord::from_mils(50).expect("50 mil fits Coord")),
    clear: None,
};

pub(super) type GraphicBindings = BTreeMap<String, Spanned<GraphicSourceRef>>;
pub(super) struct CompiledLayout {
    pub pins: Vec<PinDecl>,
    pub graphics: Vec<GraphicDecl>,
    pub placements: Vec<Option<PinPlacementIntent>>,
    pub auto_sizes: Vec<Option<AutoSizeIntent>>,
    pub bindings: GraphicBindings,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxGeometry {
    pub from: CoordPoint,
    pub to: CoordPoint,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectivePinGeometry {
    pub length: Coord,
    pub location: CoordPoint,
    pub orientation: RotationBy90,
    /// Explicit orientation intent overrides the edge-derived orientation.
    pub orientation_is_authored: bool,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPinPlacement {
    pub location: CoordPoint,
    pub orientation: RotationBy90,
}
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedLayout {
    /// None means no layout write permission; preserve the field-level result.
    pub pins: Vec<Option<ResolvedPinPlacement>>,
    pub graphics: Vec<Option<BoxGeometry>>,
    pub geometry: BTreeMap<GraphicSourceRef, BoxGeometry>,
}

const PIN_KEYS: &[&str] = &["on", "after", "before", "gap", "side", "offset"];
const SIZE_KEYS: &[&str] = &["padding", "min_width", "min_height"];
fn properties(body: &Spanned<Object>) -> Result<BTreeMap<String, Spanned<Expr>>, SpecError> {
    let mut result: BTreeMap<String, Spanned<Expr>> = BTreeMap::new();
    for item in &body.node.items {
        if let ObjectItem::Property(p) = &item.node {
            if let Some(previous) = result.insert(p.key.node.clone(), p.value.clone()) {
                return Err(error(
                    format!(
                        "duplicate '{}' in layout; first supplied at {}..{}",
                        p.key.node, previous.span.start, previous.span.end
                    ),
                    p.key.span,
                ));
            }
        }
    }
    Ok(result)
}
// Expand spreads only for metadata analysis. Original declarations, including
// nested spread provenance, are returned unchanged to the native field pass.
fn expand(body: &Spanned<Object>, scope: &ScopeStack) -> Result<Spanned<Object>, SpecError> {
    // Literal duplicate properties are errors; deliberate spread overlays retain
    // their established left-to-right precedence.
    properties(body)?;
    let mut result = indexmap::IndexMap::new();
    let mut lets = Vec::new();
    for item in &body.node.items {
        match &item.node {
            ObjectItem::LetBinding(_) => lets.push(item.clone()),
            ObjectItem::Property(property) => {
                result.insert(property.key.node.clone(), item.clone());
            }
            ObjectItem::Spread(expr) => {
                let value = eval_expr(expr, scope)?.into_object(Some(expr.span))?;
                for key in value.keys() {
                    let span = expr.span;
                    result.insert(
                        key.clone(),
                        Spanned::new(
                            ObjectItem::Property(Property {
                                key: Spanned::new(key.clone(), span),
                                value: Spanned::new(
                                    Expr::Path(
                                        Box::new(expr.clone()),
                                        Spanned::new(key.clone(), span),
                                    ),
                                    span,
                                ),
                            }),
                            span,
                        ),
                    );
                }
            }
        }
    }
    lets.extend(result.into_values());
    Ok(Spanned::new(Object { items: lets }, body.span))
}

fn value_field<T: Clone>(
    props: &BTreeMap<String, Spanned<Expr>>,
    key: &str,
    policy: FieldPolicy<T>,
    scope: &ScopeStack,
    convert: impl Fn(Value, Span) -> Result<T, SpecError>,
) -> Result<FieldIntent<T>, SpecError> {
    let value = match props.get(key) {
        None => FieldIntent::Absent,
        Some(expr) => match &expr.node {
            Expr::Inherit => FieldIntent::Inherit(expr.span),
            Expr::Reset => FieldIntent::Reset(expr.span),
            _ => match eval_expr(expr, scope)? {
                Value::Null => FieldIntent::Clear(expr.span),
                value => FieldIntent::Set(Spanned::new(convert(value, expr.span)?, expr.span)),
            },
        },
    };
    policy.validate(&value)?;
    Ok(value)
}
fn nonnegative(value: Value, span: Span) -> Result<Coord, SpecError> {
    let value = coord(value, span)?;
    if value.raw() < 0 {
        return Err(error("layout dimension must be nonnegative", span));
    }
    Ok(value)
}
fn side(value: Value, span: Span) -> Result<PlacementSide, SpecError> {
    match value {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "outside" => Ok(PlacementSide::Outside),
            "inside" => Ok(PlacementSide::Inside),
            "center" => Ok(PlacementSide::Center),
            _ => Err(error("side must be inside/outside/center", span)),
        },
        _ => Err(error("side requires a string", span)),
    }
}
fn position(value: Value, span: Span) -> Result<AnchorPosition, SpecError> {
    match value {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "start" => Ok(AnchorPosition::Start),
            "center" => Ok(AnchorPosition::Center),
            "end" => Ok(AnchorPosition::End),
            _ => Err(error("anchor at must be start/center/end", span)),
        },
        _ => Err(error("anchor at requires a position name", span)),
    }
}
fn edge(name: &str, span: Span) -> Result<AnchorEdge, SpecError> {
    match name {
        "left" => Ok(AnchorEdge::Left),
        "right" => Ok(AnchorEdge::Right),
        "top" => Ok(AnchorEdge::Top),
        "bottom" => Ok(AnchorEdge::Bottom),
        _ => Err(error("anchor edge must be left/right/top/bottom", span)),
    }
}
fn sequence(expr: &Spanned<Expr>) -> Result<String, SpecError> {
    match &expr.node {
        Expr::DollarIdent(name) => Ok(name.clone()),
        _ => Err(error(
            "after/before requires a pin binding such as $p1",
            expr.span,
        )),
    }
}
#[derive(Clone)]
struct PendingPlacement {
    edge: Option<(Spanned<GraphicSourceRef>, Spanned<AnchorEdge>)>,
    sequence: Option<(Spanned<PinSourceRef>, bool)>,
    position: Spanned<AnchorPosition>,
    gap: FieldIntent<Coord>,
    side: FieldIntent<PlacementSide>,
    offset: FieldIntent<CoordPoint>,
    span: Span,
}
fn resolve_pending(
    index: usize,
    pending: &mut [Option<PendingPlacement>],
    colors: &mut [u8],
) -> Result<(), SpecError> {
    if colors[index] == 2 {
        return Ok(());
    }
    let Some(current) = pending[index].clone() else {
        colors[index] = 2;
        return Ok(());
    };
    if colors[index] == 1 {
        return Err(error("circular pin placement dependency", current.span));
    }
    colors[index] = 1;
    if let Some((reference, _)) = &current.sequence {
        let dep = reference.node.index;
        resolve_pending(dep, pending, colors)?;
        let other = pending[dep].as_ref().and_then(|p| p.edge.clone());
        match (&current.edge, other) {
            (None, Some(other)) => {
                pending[index].as_mut().expect("pending exists").edge = Some(other)
            }
            (None, None) => {
                return Err(error(
                    "cannot infer an edge from an unanchored pin",
                    reference.span,
                ));
            }
            (Some((g, e)), Some((og, oe))) if g.node != og.node || e.node != oe.node => {
                return Err(SpecError::at(
                    SpecErrorCode::CrossEdgeReference,
                    "sequenced pins must reference the same graphic and edge",
                    reference.span,
                ));
            }
            _ => {}
        }
    }
    colors[index] = 2;
    Ok(())
}

/// Elaboration resolves lexical references and consumes metadata into typed
/// intent, but does not compute coordinates or fill absent native fields.
pub(super) fn compile_layout(
    owner: LayoutOwner,
    pins: &[&PinDecl],
    graphics: &[&GraphicDecl],
    inherited: &GraphicBindings,
    scope: &ScopeStack,
) -> Result<CompiledLayout, SpecError> {
    let original_pins = pins;
    let original_graphics = graphics;
    let mut pins: Vec<PinDecl> = pins.iter().map(|p| (*p).clone()).collect();
    let mut graphics: Vec<GraphicDecl> = graphics.iter().map(|g| (*g).clone()).collect();
    let mut pin_scopes = Vec::new();
    let mut graphic_scopes = Vec::new();
    for pin in &mut pins {
        let local = super::compile::object_scope(&pin.body, scope)?;
        pin.body = expand(&pin.body, &local)?;
        pin_scopes.push(local);
    }
    for graphic in &mut graphics {
        let local = super::compile::object_scope(&graphic.body, scope)?;
        graphic.body = expand(&graphic.body, &local)?;
        graphic_scopes.push(local);
    }
    let mut bindings = inherited.clone();
    // A value binding shadows inherited source geometry; it must never silently
    // fall through to a parent's box with the same lexical name.
    bindings.retain(|name, _| scope.lookup_current(name).is_none());
    for pin in &pins {
        if let Some(binding) = &pin.binding {
            bindings.remove(&binding.node);
        }
    }

    let mut local_names = BTreeMap::new();
    let mut auto_sizes = Vec::new();
    for (index, graphic) in graphics.iter_mut().enumerate() {
        let props = properties(&graphic.body)?;
        let span = graphic.body.span;
        let box_kind = matches!(
            graphic.graphic_type.node.as_str(),
            "rectangle" | "round_rectangle" | "text_frame" | "image"
        );
        if let Some(binding) = &graphic.binding {
            if let Some(previous) = local_names.insert(binding.node.clone(), binding.span) {
                return Err(error(
                    format!(
                        "duplicate graphic binding '{}'; first at {}..{}",
                        binding.node, previous.start, previous.end
                    ),
                    binding.span,
                ));
            }
            bindings.remove(&binding.node);
            if box_kind {
                bindings.insert(
                    binding.node.clone(),
                    Spanned::new(GraphicSourceRef { owner, index }, binding.span),
                );
            }
        }
        let infer_from = !props.contains_key("from");
        let infer_to = !props.contains_key("to");
        let auto = box_kind && graphic.binding.is_some() && (infer_from || infer_to);
        if !auto {
            if let Some(key) = SIZE_KEYS.iter().find(|key| props.contains_key(**key)) {
                return Err(error(
                    format!("{key} requires an auto-sized bound box"),
                    props[*key].span,
                ));
            }
            auto_sizes.push(None);
            graphic.body = original_graphics[index].body.clone();
            continue;
        }
        let local = &graphic_scopes[index];
        auto_sizes.push(Some(AutoSizeIntent {
            padding: value_field(&props, "padding", AUTO_PADDING, local, nonnegative)?,
            min_width: value_field(&props, "min_width", AUTO_MIN_WIDTH, local, nonnegative)?,
            min_height: value_field(&props, "min_height", AUTO_MIN_HEIGHT, local, nonnegative)?,
            infer_from,
            infer_to,
            span,
        }));
        graphic.body = original_graphics[index].body.clone();
    }
    let mut names: BTreeMap<String, Vec<Spanned<PinSourceRef>>> = BTreeMap::new();
    let mut explicit_names: BTreeMap<String, Span> = BTreeMap::new();
    for (index, pin) in pins.iter().enumerate() {
        if let Some(binding) = &pin.binding {
            if let Some(previous) = explicit_names.insert(binding.node.clone(), binding.span) {
                return Err(error(
                    format!(
                        "duplicate pin binding '{}'; first at {}..{}",
                        binding.node, previous.start, previous.end
                    ),
                    binding.span,
                ));
            }
        }
        let name = pin
            .binding
            .as_ref()
            .map(|b| b.node.clone())
            .unwrap_or_else(|| format!("pin{}", pin.name.node.as_str()));
        names
            .entry(name)
            .or_default()
            .push(Spanned::new(PinSourceRef { owner, index }, pin.name.span));
    }
    let mut pending = Vec::new();
    for (index, pin) in pins.iter().enumerate() {
        let props = properties(&pin.body)?;
        let span = pin.body.span;
        let local = &pin_scopes[index];
        let anchored =
            props.contains_key("on") || props.contains_key("after") || props.contains_key("before");
        if !anchored {
            if let Some(key) = PIN_KEYS.iter().find(|key| props.contains_key(**key)) {
                return Err(error(
                    format!("{key} requires on/after/before placement"),
                    props[*key].span,
                ));
            }
            pending.push(None);
            continue;
        }
        if props.contains_key("x") || props.contains_key("y") || props.contains_key("location") {
            return Err(error(
                "anchor placement conflicts with absolute location/x/y",
                span,
            ));
        }
        let edge = props
            .get("on")
            .map(|expr| {
                if let Expr::Path(base, field) = &expr.node {
                    if let Expr::DollarIdent(name) = &base.node {
                        if local.lookup_current(name).is_some() {
                            return Err(error(
                                format!("local value binding '${name}' is not a graphic edge"),
                                base.span,
                            ));
                        }
                        let reference = bindings.get(name).ok_or_else(|| {
                            error(format!("undefined bound graphic '${name}'"), base.span)
                        })?;
                        return Ok((
                            Spanned::new(reference.node, expr.span),
                            Spanned::new(edge(&field.node, field.span)?, field.span),
                        ));
                    }
                }
                Err(error(
                    "on requires a bound graphic edge such as $body.left",
                    expr.span,
                ))
            })
            .transpose()?;
        if usize::from(props.contains_key("at"))
            + usize::from(props.contains_key("after"))
            + usize::from(props.contains_key("before"))
            > 1
        {
            return Err(error(
                "at, after and before are mutually exclusive in anchor mode",
                span,
            ));
        }
        let position = props
            .get("at")
            .map(|expr| {
                position(eval_expr(expr, local)?, expr.span).map(|p| Spanned::new(p, expr.span))
            })
            .transpose()?
            .unwrap_or(Spanned::new(AnchorPosition::Center, span));
        let sequence = props
            .get("after")
            .map(|expr| (expr, true))
            .or_else(|| props.get("before").map(|expr| (expr, false)))
            .map(|(expr, after)| {
                let name = sequence(expr)?;
                if local.lookup_current(&name).is_some(){return Err(error(format!("local value binding '${name}' is not a pin layout reference"),expr.span));}
                let reference = names
                    .get(&name)
                    .ok_or_else(|| error(format!("undefined pin binding '${name}'"), expr.span))?;
                if reference.len() != 1 {
                    return Err(error(format!("ambiguous pin binding '${name}' at {}..{} and {}..{}; use explicit bindings", reference[0].span.start, reference[0].span.end, reference[1].span.start, reference[1].span.end),expr.span));
                }
                Ok::<_, SpecError>((Spanned::new(reference[0].node, expr.span), after))
            })
            .transpose()?;
        pending.push(Some(PendingPlacement {
            edge,
            sequence,
            position,
            gap: value_field(&props, "gap", ANCHOR_GAP, local, nonnegative)?,
            side: value_field(&props, "side", ANCHOR_SIDE, local, side)?,
            offset: value_field(&props, "offset", ANCHOR_OFFSET, local, point)?,
            span,
        }));
    }
    let mut colors = vec![0; pending.len()];
    for index in 0..pending.len() {
        resolve_pending(index, &mut pending, &mut colors)?;
    }
    let mut placements = Vec::new();
    for (index, pending) in pending.into_iter().enumerate() {
        let Some(pending) = pending else {
            placements.push(None);
            pins[index].body = original_pins[index].body.clone();
            continue;
        };
        let (graphic, edge) = pending
            .edge
            .ok_or_else(|| error("anchor has no resolved graphic edge", pending.span))?;
        let position = pending
            .sequence
            .map(|(reference, after)| {
                Spanned::new(
                    if after {
                        AnchorPosition::After(reference.node)
                    } else {
                        AnchorPosition::Before(reference.node)
                    },
                    reference.span,
                )
            })
            .unwrap_or(pending.position);
        placements.push(Some(PinPlacementIntent {
            graphic,
            edge,
            position,
            gap: pending.gap,
            side: pending.side,
            offset: pending.offset,
            span: pending.span,
        }));
        pins[index].body = original_pins[index].body.clone();
    }
    Ok(CompiledLayout {
        pins,
        graphics,
        placements,
        auto_sizes,
        bindings,
    })
}
impl AnchorEdge {
    fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
    fn direction(self) -> i64 {
        if matches!(self, Self::Left | Self::Bottom) {
            -1
        } else {
            1
        }
    }
    fn outward(self) -> i64 {
        if matches!(self, Self::Left | Self::Bottom) {
            -1
        } else {
            1
        }
    }
    fn rotation(self) -> RotationBy90 {
        match self {
            Self::Left => RotationBy90::Rotate0,
            Self::Right => RotationBy90::Rotate180,
            Self::Top => RotationBy90::Rotate270,
            Self::Bottom => RotationBy90::Rotate90,
        }
    }
    fn bounds(self, geom: &BoxGeometry) -> (i64, i64, i64) {
        let x0 = i64::from(geom.from.x.raw().min(geom.to.x.raw()));
        let x1 = i64::from(geom.from.x.raw().max(geom.to.x.raw()));
        let y0 = i64::from(geom.from.y.raw().min(geom.to.y.raw()));
        let y1 = i64::from(geom.from.y.raw().max(geom.to.y.raw()));
        match self {
            Self::Left => (x0, y0, y1),
            Self::Right => (x1, y0, y1),
            Self::Top => (y1, x0, x1),
            Self::Bottom => (y0, x0, x1),
        }
    }
}
fn xy(x: i64, y: i64, span: Span) -> Result<CoordPoint, SpecError> {
    let convert = |value| {
        i32::try_from(value)
            .map(Coord::new)
            .map_err(|_| error("layout coordinate overflow", span))
    };
    Ok(CoordPoint::new(convert(x)?, convert(y)?))
}
fn dependency(position: AnchorPosition) -> Option<(PinSourceRef, bool)> {
    match position {
        AnchorPosition::After(pin) => Some((pin, true)),
        AnchorPosition::Before(pin) => Some((pin, false)),
        _ => None,
    }
}
fn eval_order(
    index: usize,
    owner: LayoutOwner,
    placements: &[Option<PinPlacementIntent>],
    colors: &mut [u8],
    order: &mut Vec<usize>,
) -> Result<(), SpecError> {
    if colors[index] == 2 {
        return Ok(());
    }
    let Some(current) = &placements[index] else {
        colors[index] = 2;
        order.push(index);
        return Ok(());
    };
    if colors[index] == 1 {
        return Err(error("circular pin placement dependency", current.span));
    }
    colors[index] = 1;
    if let Some((reference, _)) = dependency(current.position.node) {
        if reference.owner != owner || reference.index >= placements.len() {
            return Err(error(
                "placement dependency is outside the source pin owner/range",
                current.position.span,
            ));
        }
        eval_order(reference.index, owner, placements, colors, order)?;
        if let Some(dep) = &placements[reference.index] {
            if current.graphic.node != dep.graphic.node || current.edge.node != dep.edge.node {
                return Err(SpecError::at(
                    SpecErrorCode::CrossEdgeReference,
                    "sequenced pins must reference the same graphic and edge",
                    current.position.span,
                ));
            }
        }
    }
    colors[index] = 2;
    order.push(index);
    Ok(())
}

// Each dependency chain is an exact boundary/center expression plus a checked
// translation. Same-edge references keep width and height constraints separable.
#[derive(Debug, Clone, Copy)]
enum AlongBase {
    Low,
    High,
    Center,
    Absolute(Coord),
}
#[derive(Debug, Clone, Copy)]
struct CoordinateExpr {
    base: AlongBase,
    offset: i64,
    span: Span,
}
impl CoordinateExpr {
    fn value(self, low: i64, high: i64) -> Result<i64, SpecError> {
        let base = match self.base {
            AlongBase::Low => low,
            AlongBase::High => high,
            AlongBase::Center => (low + high) / 2,
            AlongBase::Absolute(value) => i64::from(value.raw()),
        };
        base.checked_add(self.offset)
            .ok_or_else(|| error("layout chain coordinate overflow", self.span))
    }
    fn translated(self, offset: i64, span: Span) -> Result<Self, SpecError> {
        Ok(Self {
            base: self.base,
            offset: self
                .offset
                .checked_add(offset)
                .ok_or_else(|| error("layout chain offset overflow", span))?,
            span,
        })
    }
}
#[derive(Debug, Clone, Copy)]
struct AlongConstraint {
    position: CoordinateExpr,
    margin: Coord,
}
#[derive(Debug, Default)]
struct AxisConstraints {
    inside: Vec<AlongConstraint>,
    coordinates: Vec<CoordinateExpr>,
}
#[derive(Debug, Clone, Copy)]
enum AxisOrigin {
    Centered,
    Minimum(Coord),
    Maximum(Coord),
}
impl AxisOrigin {
    fn bounds(self, size: i64) -> (i64, i64) {
        match self {
            Self::Centered => (-size / 2, size - size / 2),
            Self::Minimum(value) => (i64::from(value.raw()), i64::from(value.raw()) + size),
            Self::Maximum(value) => (i64::from(value.raw()) - size, i64::from(value.raw())),
        }
    }
    fn maximum_size(self) -> i64 {
        match self {
            Self::Centered => 2 * i64::from(i32::MAX),
            Self::Minimum(value) => i64::from(i32::MAX) - i64::from(value.raw()),
            Self::Maximum(value) => i64::from(value.raw()) - i64::from(i32::MIN),
        }
    }
}
fn first_satisfying(
    mut low: i64,
    mut high: i64,
    mut predicate: impl FnMut(i64) -> Result<bool, SpecError>,
) -> Result<i64, SpecError> {
    // Caller proves the high endpoint satisfies a monotone predicate.
    while low < high {
        let middle = low + (high - low) / 2;
        if predicate(middle)? {
            high = middle
        } else {
            low = middle + 1
        }
    }
    Ok(low)
}
fn last_satisfying(
    mut low: i64,
    mut high: i64,
    mut predicate: impl FnMut(i64) -> Result<bool, SpecError>,
) -> Result<i64, SpecError> {
    // Caller proves the low endpoint satisfies a monotone decreasing predicate.
    while low < high {
        let middle = low + (high - low + 1) / 2;
        if predicate(middle)? {
            low = middle
        } else {
            high = middle - 1
        }
    }
    Ok(low)
}
/// Compute the exact feasible integer-size interval for a monotone coordinate,
/// accounting for pin connection offsets as well as the box's own Coord bounds.
fn coordinate_size_interval(
    origin: AxisOrigin,
    expr: CoordinateExpr,
) -> Result<(i64, i64), SpecError> {
    let maximum = origin.maximum_size();
    let value = |size| {
        let (low, high) = origin.bounds(size);
        expr.value(low, high)
    };
    let first = value(0)?;
    let last = value(maximum)?;
    let min = i64::from(i32::MIN);
    let max = i64::from(i32::MAX);
    let impossible = || {
        error(
            "auto-size constraints have no representable pin coordinate",
            expr.span,
        )
    };
    if first <= last {
        if last < min || first > max {
            return Err(impossible());
        }
        let lower = if first >= min {
            0
        } else {
            first_satisfying(0, maximum, |size| Ok(value(size)? >= min))?
        };
        let upper = if last <= max {
            maximum
        } else {
            last_satisfying(0, maximum, |size| Ok(value(size)? <= max))?
        };
        Ok((lower, upper))
    } else {
        if first < min || last > max {
            return Err(impossible());
        }
        let lower = if first <= max {
            0
        } else {
            first_satisfying(0, maximum, |size| Ok(value(size)? <= max))?
        };
        let upper = if last >= min {
            maximum
        } else {
            last_satisfying(0, maximum, |size| Ok(value(size)? >= min))?
        };
        Ok((lower, upper))
    }
}
fn solve_axis(
    origin: AxisOrigin,
    minimum: Coord,
    constraints: &AxisConstraints,
    span: Span,
) -> Result<i64, SpecError> {
    if minimum.raw() < 0 {
        return Err(error("auto-size minimum must be nonnegative", span));
    }
    let mut lower = i64::from(minimum.raw());
    let mut upper = origin.maximum_size();
    for coordinate in &constraints.coordinates {
        let (low, high) = coordinate_size_interval(origin, *coordinate)?;
        lower = lower.max(low);
        upper = upper.min(high);
        if lower > upper {
            return Err(error(
                "auto-size constraints have no common representable size",
                coordinate.span,
            ));
        }
    }
    if lower > upper {
        return Err(error(
            "auto-size minimum exceeds representable box bounds",
            span,
        ));
    }
    let violated = |size| -> Result<Option<Span>, SpecError> {
        let (low, high) = origin.bounds(size);
        for constraint in &constraints.inside {
            let position = constraint.position.value(low, high)?;
            let margin = i64::from(constraint.margin.raw());
            if position < low + margin || position > high - margin {
                return Ok(Some(constraint.position.span));
            }
        }
        Ok(None)
    };
    if let Some(source) = violated(upper)? {
        return Err(error(
            "auto-size constraints cannot satisfy this anchor's edge margins at any representable size; revise its start/end, gap, offset, or sequence",
            source,
        ));
    }
    // Every containment distance is monotone in size for Low/High/Center/
    // Absolute roots, including truncation-toward-zero integer center rounding.
    first_satisfying(lower, upper, |size| Ok(violated(size)?.is_none()))
}
fn auto_constraints(
    target: GraphicSourceRef,
    placements: &[Option<PinPlacementIntent>],
    effective_pins: &[EffectivePinGeometry],
    order: &[usize],
    padding: Option<Coord>,
) -> Result<(AxisConstraints, AxisConstraints, Vec<(usize, Coord)>), SpecError> {
    let mut x = AxisConstraints::default();
    let mut y = AxisConstraints::default();
    let mut along: Vec<Option<AlongConstraint>> = vec![None; placements.len()];
    let mut margins = Vec::new();
    for index in order {
        let Some(p) = &placements[*index] else {
            continue;
        };
        if p.graphic.node != target {
            continue;
        }
        let edge = p.edge.node;
        let gap = ANCHOR_GAP.definition(&p.gap)?;
        let offset = ANCHOR_OFFSET.definition(&p.offset)?;
        let length = effective_pins[*index].length;
        if gap.raw() < 0 || length.raw() < 0 {
            return Err(error(
                "anchor gap and effective pin length must be nonnegative",
                p.span,
            ));
        }
        let along_offset = i64::from(if edge.vertical() {
            offset.y.raw()
        } else {
            offset.x.raw()
        });
        let fixed_offset = i64::from(if edge.vertical() {
            offset.x.raw()
        } else {
            offset.y.raw()
        });
        let side = match ANCHOR_SIDE.definition(&p.side)? {
            PlacementSide::Outside => 1,
            PlacementSide::Inside => -1,
            PlacementSide::Center => 0,
        };
        let fixed = CoordinateExpr {
            base: if edge.outward() < 0 {
                AlongBase::Low
            } else {
                AlongBase::High
            },
            offset: edge.outward() * side * i64::from(length.raw()) + fixed_offset,
            span: p.span,
        };
        let (position, margin) = if let Some((reference, after)) = dependency(p.position.node) {
            let dependency = along[reference.index];
            let base = dependency.map(|value| value.position).unwrap_or_else(|| {
                let location = effective_pins[reference.index].location;
                CoordinateExpr {
                    base: AlongBase::Absolute(if edge.vertical() {
                        location.y
                    } else {
                        location.x
                    }),
                    offset: 0,
                    span: p.span,
                }
            });
            let translation = edge.direction() * i64::from(gap.raw()) * if after { 1 } else { -1 };
            (
                base.translated(translation, p.span)?
                    .translated(along_offset, p.span)?,
                padding.unwrap_or_else(|| dependency.map(|value| value.margin).unwrap_or(gap)),
            )
        } else {
            let (base, translation) = match p.position.node {
                AnchorPosition::Start => {
                    if edge.direction() > 0 {
                        (AlongBase::Low, i64::from(gap.raw()))
                    } else {
                        (AlongBase::High, -i64::from(gap.raw()))
                    }
                }
                AnchorPosition::End => {
                    if edge.direction() > 0 {
                        (AlongBase::High, -i64::from(gap.raw()))
                    } else {
                        (AlongBase::Low, i64::from(gap.raw()))
                    }
                }
                _ => (AlongBase::Center, 0),
            };
            (
                CoordinateExpr {
                    base,
                    offset: translation,
                    span: p.span,
                }
                .translated(along_offset, p.span)?,
                padding.unwrap_or(gap),
            )
        };
        let constraint = AlongConstraint { position, margin };
        along[*index] = Some(constraint);
        margins.push((*index, margin));
        if edge.vertical() {
            y.inside.push(constraint);
            x.coordinates.push(fixed)
        } else {
            x.inside.push(constraint);
            y.coordinates.push(fixed)
        }
    }
    Ok((x, y, margins))
}

/// Evaluate only after the caller resolves each native field against target,
/// imported base, and defaults. In particular an absent pin length is supplied
/// from its target, rather than silently replaced by the creation default.
/// This function grants no writes for slots without authored layout intent.
pub fn evaluate_layout(
    owner: LayoutOwner,
    placements: &[Option<PinPlacementIntent>],
    auto_sizes: &[Option<AutoSizeIntent>],
    effective_pins: &[EffectivePinGeometry],
    effective_graphics: &[Option<BoxGeometry>],
    inherited: &BTreeMap<GraphicSourceRef, BoxGeometry>,
) -> Result<EvaluatedLayout, SpecError> {
    if placements.len() != effective_pins.len() || auto_sizes.len() != effective_graphics.len() {
        return Err(SpecError::no_span(
            SpecErrorCode::TypeMismatch,
            "layout source slots do not match effective geometry slots",
        ));
    }
    let mut colors = vec![0; placements.len()];
    let mut order = Vec::new();
    for index in 0..placements.len() {
        eval_order(index, owner, placements, &mut colors, &mut order)?;
    }
    // Parent geometry cannot stand in for a missing local source slot.
    let mut geometry = inherited
        .iter()
        .filter(|(reference, _)| reference.owner != owner)
        .map(|(reference, geometry)| (*reference, *geometry))
        .collect::<BTreeMap<_, _>>();
    for (index, geom) in effective_graphics.iter().enumerate() {
        if let Some(geom) = geom {
            geometry.insert(GraphicSourceRef { owner, index }, *geom);
        }
    }
    for placement in placements.iter().flatten() {
        let reference = placement.graphic.node;
        if reference.owner == owner && reference.index >= effective_graphics.len() {
            return Err(error(
                "graphic reference is outside the local source range",
                placement.graphic.span,
            ));
        }
        if reference.owner != owner
            && !(matches!(owner, LayoutOwner::Part(_)) && reference.owner == LayoutOwner::Component)
        {
            return Err(error(
                "graphic reference is outside this source owner or its component",
                placement.graphic.span,
            ));
        }
        if !geometry.contains_key(&placement.graphic.node) {
            return Err(error(
                "effective bound graphic geometry is missing",
                placement.graphic.span,
            ));
        }
    }
    let mut graphics = vec![None; auto_sizes.len()];
    let mut auto_margins: BTreeMap<usize, Coord> = BTreeMap::new();
    for (index, auto) in auto_sizes.iter().enumerate() {
        let Some(auto) = auto else { continue };
        if !auto.infer_from && !auto.infer_to {
            return Err(error(
                "auto-size must infer at least one source-absent corner",
                auto.span,
            ));
        }
        let target = GraphicSourceRef { owner, index };
        let explicit_padding = if auto.padding.is_authored() {
            Some(AUTO_PADDING.definition(&auto.padding)?)
        } else {
            None
        };
        if explicit_padding.is_some_and(|padding| padding.raw() < 0) {
            return Err(error("auto-size padding must be nonnegative", auto.span));
        }
        let (x_constraints, y_constraints, margins) =
            auto_constraints(target, placements, effective_pins, &order, explicit_padding)?;
        for (index, margin) in margins {
            auto_margins.insert(index, margin);
        }
        let existing = effective_graphics[index].ok_or_else(|| {
            error(
                "auto-size source slot has no effective box geometry",
                auto.span,
            )
        })?;
        let (x_origin, y_origin) = match (auto.infer_from, auto.infer_to) {
            (true, true) => (AxisOrigin::Centered, AxisOrigin::Centered),
            (false, true) => (
                AxisOrigin::Minimum(existing.from.x),
                AxisOrigin::Minimum(existing.from.y),
            ),
            (true, false) => (
                AxisOrigin::Maximum(existing.to.x),
                AxisOrigin::Maximum(existing.to.y),
            ),
            _ => unreachable!("validated inferred corner"),
        };
        let width = solve_axis(
            x_origin,
            AUTO_MIN_WIDTH.definition(&auto.min_width)?,
            &x_constraints,
            auto.span,
        )?;
        let height = solve_axis(
            y_origin,
            AUTO_MIN_HEIGHT.definition(&auto.min_height)?,
            &y_constraints,
            auto.span,
        )?;
        let geom = match (auto.infer_from, auto.infer_to) {
            (true, true) => BoxGeometry {
                from: xy(-width / 2, -height / 2, auto.span)?,
                to: xy(width - width / 2, height - height / 2, auto.span)?,
            },
            (false, true) => BoxGeometry {
                from: existing.from,
                to: xy(
                    i64::from(existing.from.x.raw()) + width,
                    i64::from(existing.from.y.raw()) + height,
                    auto.span,
                )?,
            },
            (true, false) => BoxGeometry {
                from: xy(
                    i64::from(existing.to.x.raw()) - width,
                    i64::from(existing.to.y.raw()) - height,
                    auto.span,
                )?,
                to: existing.to,
            },
            _ => unreachable!("validated inferred corner"),
        };
        graphics[index] = Some(geom);
        geometry.insert(target, geom);
    }
    let mut pins = vec![None; placements.len()];
    let mut positions = effective_pins
        .iter()
        .map(|pin| pin.location)
        .collect::<Vec<_>>();
    for index in order {
        let Some(p) = &placements[index] else {
            continue;
        };
        let geom = geometry.get(&p.graphic.node).ok_or_else(|| {
            error(
                "effective bound graphic geometry is missing",
                p.graphic.span,
            )
        })?;
        let edge = p.edge.node;
        let (fixed, min, max) = edge.bounds(geom);
        let gap = i64::from(ANCHOR_GAP.definition(&p.gap)?.raw());
        let length = i64::from(effective_pins[index].length.raw());
        if gap < 0 || length < 0 {
            return Err(error(
                "anchor gap and effective pin length must be nonnegative",
                p.span,
            ));
        }
        let along = if let Some((reference, after)) = dependency(p.position.node) {
            let position = positions[reference.index];
            let along = i64::from(if edge.vertical() {
                position.y.raw()
            } else {
                position.x.raw()
            });
            along + edge.direction() * gap * if after { 1 } else { -1 }
        } else {
            match p.position.node {
                AnchorPosition::Start => {
                    if edge.direction() > 0 {
                        min + gap
                    } else {
                        max - gap
                    }
                }
                AnchorPosition::End => {
                    if edge.direction() > 0 {
                        max - gap
                    } else {
                        min + gap
                    }
                }
                _ => (min + max) / 2,
            }
        };
        let side = match ANCHOR_SIDE.definition(&p.side)? {
            PlacementSide::Outside => 1,
            PlacementSide::Inside => -1,
            PlacementSide::Center => 0,
        };
        let fixed = fixed + edge.outward() * side * length;
        let offset = ANCHOR_OFFSET.definition(&p.offset)?;
        let (x, y) = if edge.vertical() {
            (fixed, along)
        } else {
            (along, fixed)
        };
        let location = xy(
            x + i64::from(offset.x.raw()),
            y + i64::from(offset.y.raw()),
            p.span,
        )?;
        if let Some(margin) = auto_margins.get(&index) {
            let varying = i64::from(if edge.vertical() {
                location.y.raw()
            } else {
                location.x.raw()
            });
            let margin = i64::from(margin.raw());
            if varying < min + margin || varying > max - margin {
                return Err(error(
                    "auto-size solver invariant failed: evaluated pin violates solved edge margins",
                    p.span,
                ));
            }
        }
        let effective = effective_pins[index];
        let orientation = if effective.orientation_is_authored {
            effective.orientation
        } else {
            edge.rotation()
        };
        positions[index] = location;
        pins[index] = Some(ResolvedPinPlacement {
            location,
            orientation,
        });
    }
    Ok(EvaluatedLayout {
        pins,
        graphics,
        geometry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mil(value: i32) -> Coord {
        Coord::from_mils(value).unwrap()
    }
    fn location(x: i32, y: i32) -> CoordPoint {
        CoordPoint::new(mil(x), mil(y))
    }
    fn effective(length: i32) -> EffectivePinGeometry {
        EffectivePinGeometry {
            length: mil(length),
            location: CoordPoint::zero(),
            orientation: RotationBy90::Rotate0,
            orientation_is_authored: false,
        }
    }
    fn compile(source: &str) -> CompiledLayout {
        let ast = crate::parser::parse_spec(source).unwrap();
        let SpecItem::Component(component) = &ast.items[0].node else {
            panic!("component")
        };
        let pins = component
            .body
            .iter()
            .filter_map(|item| {
                if let ComponentItem::Pin(pin) = &item.node {
                    Some(pin)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let graphics = component
            .body
            .iter()
            .filter_map(|item| {
                if let ComponentItem::Graphic(graphic) = &item.node {
                    Some(graphic)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        compile_layout(
            LayoutOwner::Component,
            &pins,
            &graphics,
            &BTreeMap::new(),
            &ScopeStack::new(),
        )
        .unwrap()
    }
    fn box100() -> BoxGeometry {
        BoxGeometry {
            from: location(0, 0),
            to: location(100, 100),
        }
    }
    #[test]
    fn effective_target_length_is_retained_until_layout_evaluation() {
        let source = "component R { body = rectangle { from:(0mil,0mil), to:(100mil,100mil) } pin 1 { on:$body.left } }";
        let compiled = compile(source);
        assert!(
            !properties(&compiled.pins[0].body)
                .unwrap()
                .contains_key("length")
        );
        assert!(
            !properties(&compiled.pins[0].body)
                .unwrap()
                .contains_key("at")
        );
        let creation = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(25)],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap();
        let existing = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(60)],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(creation.pins[0].unwrap().location, location(-25, 50));
        assert_eq!(existing.pins[0].unwrap().location, location(-60, 50));
        assert!(
            existing.graphics[0].is_none(),
            "explicit box has no inferred geometry write"
        );
    }
    #[test]
    fn four_edges_match_existing_default_pin_orientation_and_outside_coordinates() {
        let compiled = compile(
            "component R { body=rectangle {from:(0mil,0mil),to:(100mil,100mil)} pin 1 {on:$body.left} pin 2 {on:$body.right} pin 3 {on:$body.top} pin 4 {on:$body.bottom} pin 5 {name:\"unplaced\"} }",
        );
        let evaluated = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(25); 5],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap();
        for (index, (point, orientation)) in [
            (location(-25, 50), RotationBy90::Rotate0),
            (location(125, 50), RotationBy90::Rotate180),
            (location(50, 125), RotationBy90::Rotate270),
            (location(50, -25), RotationBy90::Rotate90),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                evaluated.pins[index],
                Some(ResolvedPinPlacement {
                    location: point,
                    orientation
                })
            );
        }
        assert_eq!(
            evaluated.pins[4], None,
            "absence never grants a location write"
        );
    }
    #[test]
    fn inferred_after_before_chains_auto_size_and_preserve_raw_absence() {
        let compiled = compile(
            "component R { body=rectangle {min_height:1mil,min_width:1mil} b=pin 2 {before:$a,gap:5mil} root=pin 1 {on:$body.left,at:start,gap:10mil} a=pin 3 {after:$root,gap:20mil} }",
        );
        assert!(
            !properties(&compiled.graphics[0].body)
                .unwrap()
                .contains_key("from")
        );
        assert!(
            !properties(&compiled.graphics[0].body)
                .unwrap()
                .contains_key("to")
        );
        let evaluated = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(25); 3],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap();
        let geometry = evaluated.graphics[0].unwrap();
        assert_eq!(geometry.from.y.to_mils(), -20.0);
        assert_eq!(geometry.to.y.to_mils(), 20.0);
        assert_eq!(evaluated.pins[1].unwrap().location.y.to_mils(), 10.0);
        assert_eq!(evaluated.pins[2].unwrap().location.y.to_mils(), -10.0);
        assert_eq!(evaluated.pins[0].unwrap().location.y.to_mils(), -5.0);
    }
    #[test]
    fn offsets_inside_center_and_explicit_orientation_survive_chain_resolution() {
        let compiled = compile(
            "component R {body=rectangle {from:(0mil,0mil),to:(100mil,100mil)} a=pin 1 {on:$body.left,at:start,gap:10mil,side:inside} pin 2 {after:$a,gap:20mil,side:center,offset:(3mil,4mil),orientation:90} }",
        );
        let mut second = effective(25);
        second.orientation = RotationBy90::Rotate90;
        second.orientation_is_authored = true;
        let evaluated = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(25), second],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(evaluated.pins[0].unwrap().location, location(25, 90));
        assert_eq!(
            evaluated.pins[1].unwrap(),
            ResolvedPinPlacement {
                location: location(3, 74),
                orientation: RotationBy90::Rotate90
            }
        );
    }
    #[test]
    fn parts_use_inherited_graphic_source_slots_and_can_shadow_locally() {
        let parent = compile("component R {body=rectangle {from:(0mil,0mil),to:(100mil,100mil)}}");
        let ast =
            crate::parser::parse_spec("component R {part 2 {pin 1 {on:$body.right}}}").unwrap();
        let SpecItem::Component(c) = &ast.items[0].node else {
            panic!()
        };
        let ComponentItem::Part(part) = &c.body[0].node else {
            panic!()
        };
        let PartItem::Pin(pin) = &part.body[0].node else {
            panic!()
        };
        let owner = LayoutOwner::Part(PartNumber::new(2).unwrap());
        let compiled =
            compile_layout(owner, &[pin], &[], &parent.bindings, &ScopeStack::new()).unwrap();
        assert_eq!(
            compiled.placements[0].as_ref().unwrap().graphic.node.owner,
            LayoutOwner::Component
        );
        let inherited = BTreeMap::from([(
            GraphicSourceRef {
                owner: LayoutOwner::Component,
                index: 0,
            },
            box100(),
        )]);
        let evaluated = evaluate_layout(
            owner,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(60)],
            &[],
            &inherited,
        )
        .unwrap();
        assert_eq!(evaluated.pins[0].unwrap().location, location(160, 50));
        let shadow = compile_layout(
            owner,
            &[pin],
            &[&parent.graphics[0]],
            &parent.bindings,
            &ScopeStack::new(),
        )
        .unwrap();
        assert_eq!(
            shadow.placements[0].as_ref().unwrap().graphic.node.owner,
            owner
        );
        let local = BoxGeometry {
            from: location(200, 0),
            to: location(300, 100),
        };
        let evaluated = evaluate_layout(
            owner,
            &shadow.placements,
            &shadow.auto_sizes,
            &[effective(25)],
            &[Some(local)],
            &inherited,
        )
        .unwrap();
        assert_eq!(evaluated.pins[0].unwrap().location, location(325, 50));
        assert_eq!(
            evaluated.geometry[&GraphicSourceRef {
                owner: LayoutOwner::Component,
                index: 0
            }],
            box100()
        );
    }
    #[test]
    fn local_nonbox_or_value_shadow_never_falls_through_to_parent_box() {
        let parent = compile("component R {body=rectangle {from:(0mil,0mil),to:(100mil,100mil)}}");
        let local = compile("component R {body=line {from:(0mil,0mil),to:(1mil,1mil)}}");
        let ast = crate::parser::parse_spec("component R {pin 1 {on:$body.left}}").unwrap();
        let SpecItem::Component(component) = &ast.items[0].node else {
            panic!()
        };
        let ComponentItem::Pin(pin) = &component.body[0].node else {
            panic!()
        };
        let owner = LayoutOwner::Part(PartNumber::new(2).unwrap());
        assert!(
            compile_layout(
                owner,
                &[pin],
                &[&local.graphics[0]],
                &parent.bindings,
                &ScopeStack::new()
            )
            .is_err()
        );
        let mut scope = ScopeStack::new();
        scope.push();
        scope.define("body".into(), Value::Integer(1));
        assert!(compile_layout(owner, &[pin], &[], &parent.bindings, &scope).is_err());
    }
    #[test]
    fn duplicate_designators_are_positional_until_an_ambiguous_alias_is_referenced() {
        let compiled = compile("component R {pin 1 {} pin 1 {}}");
        assert_eq!(compiled.placements, vec![None, None]);
    }
    #[test]
    fn layout_errors_are_not_defaults_or_silently_discarded() {
        for body in [
            "pin 1 {on:$missing.left}",
            "body=rectangle {} pin 1 {on:$body.corner}",
            "body=rectangle {} a=pin 1 {after:$b} b=pin 2 {after:$a}",
            "body=rectangle {} pin 1 {on:$body.left,gap:1mil/0}",
            "body=rectangle {} pin 1 {on:$body.left,gap:null}",
            "body=rectangle {} pin 1 {on:$body.left,at:start,after:$pin1}",
            "body=rectangle {} a=pin 1 {on:$body.left} pin 2 {on:$body.right,after:$a}",
            "body=rectangle {} other=rectangle {} a=pin 1 {on:$body.left} pin 2 {on:$other.left,after:$a}",
            "pin 1 {side:outside}",
            "body=rectangle {} pin 1 {let body=1, on:$body.left}",
            "body=rectangle {} a=pin 1 {on:$body.left} pin 2 {let a=1, after:$a}",
            "body=rectangle {} pin 1 {on:$body.left,location:(0mil,0mil)}",
            "body=rectangle {} pin 1 {on:$body.left} pin 1 {on:$body.left} pin 2 {after:$pin1}",
        ] {
            let source = format!("component R {{{body}}}");
            let ast = crate::parser::parse_spec(&source).unwrap();
            let SpecItem::Component(c) = &ast.items[0].node else {
                panic!()
            };
            let pins = c
                .body
                .iter()
                .filter_map(|i| {
                    if let ComponentItem::Pin(p) = &i.node {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let graphics = c
                .body
                .iter()
                .filter_map(|i| {
                    if let ComponentItem::Graphic(g) = &i.node {
                        Some(g)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert!(
                compile_layout(
                    LayoutOwner::Component,
                    &pins,
                    &graphics,
                    &BTreeMap::new(),
                    &ScopeStack::new()
                )
                .is_err(),
                "accepted {source}"
            );
        }
    }
    fn run_auto(source: &str) -> EvaluatedLayout {
        let compiled = compile(source);
        let pins = vec![effective(25); compiled.placements.len()];
        let graphics = vec![Some(box100()); compiled.auto_sizes.len()];
        evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &pins,
            &graphics,
            &BTreeMap::new(),
        )
        .unwrap()
    }
    #[test]
    fn centered_branching_chains_solve_exact_extrema_instead_of_summing_gaps() {
        let evaluated = run_auto(
            "component R {body=rectangle {min_height:1mil} root=pin 1 {on:$body.left,at:center,gap:10mil} a=pin 2 {after:$root,gap:50mil} b=pin 3 {after:$a,gap:50mil} c=pin 4 {before:$root,gap:30mil} }",
        );
        let bounds = evaluated.graphics[0].unwrap();
        assert_eq!(bounds.from.y, mil(-110));
        assert_eq!(bounds.to.y, mil(110));
        assert_eq!(
            evaluated
                .pins
                .iter()
                .map(|pin| pin.unwrap().location.y.to_mils())
                .collect::<Vec<_>>(),
            vec![0.0, -50.0, -100.0, 30.0]
        );
        // The negative branch requires 100mil plus its root's 10mil margin.
        // A one-internal-unit smaller height would violate that exact bound.
        assert_eq!(bounds.to.y.raw() - bounds.from.y.raw(), mil(220).raw());
    }
    #[test]
    fn multiple_root_margins_are_independent_of_declaration_order_and_minima_hold() {
        let first = "a=pin 1 {on:$body.left,at:start,gap:10mil} b=pin 2 {on:$body.left,at:center,gap:50mil}";
        let second = "b=pin 2 {on:$body.left,at:center,gap:50mil} a=pin 1 {on:$body.left,at:start,gap:10mil}";
        let a = run_auto(&format!(
            "component R {{body=rectangle {{min_width:130mil,min_height:1mil}} {first}}}"
        ));
        let b = run_auto(&format!(
            "component R {{body=rectangle {{min_width:130mil,min_height:1mil}} {second}}}"
        ));
        assert_eq!(a.graphics, b.graphics);
        let bounds = a.graphics[0].unwrap();
        assert_eq!(bounds.to.y.raw() - bounds.from.y.raw(), mil(100).raw());
        assert_eq!(bounds.to.x.raw() - bounds.from.x.raw(), mil(130).raw());
        assert_eq!(a.pins[0], b.pins[1]);
        assert_eq!(a.pins[1], b.pins[0]);
        let reset = run_auto(
            "component R {body=rectangle {padding:reset,min_height:1mil,min_width:1mil} pin 1 {on:$body.left,at:center,gap:50mil}}",
        );
        let bounds = reset.graphics[0].unwrap();
        assert_eq!(bounds.to.y.raw() - bounds.from.y.raw(), mil(40).raw());
    }
    #[test]
    fn explicit_corners_and_absolute_effective_dependencies_are_preserved() {
        for (corner, value, infer_from) in [
            ("from", location(-40, -30), false),
            ("to", location(40, 30), true),
        ] {
            let source = format!(
                "component R {{body=rectangle {{{corner}:({}mil,{}mil),min_width:1mil,min_height:1mil}} anchor=pin 1 {{}} pin 2 {{on:$body.left,after:$anchor,gap:10mil,offset:(0mil,5mil)}}}}",
                value.x.to_mils(),
                value.y.to_mils()
            );
            let compiled = compile(&source);
            let mut reference = effective(25);
            reference.location = location(0, 70);
            let existing = if infer_from {
                BoxGeometry {
                    from: CoordPoint::zero(),
                    to: value,
                }
            } else {
                BoxGeometry {
                    from: value,
                    to: CoordPoint::zero(),
                }
            };
            let result = evaluate_layout(
                LayoutOwner::Component,
                &compiled.placements,
                &compiled.auto_sizes,
                &[reference, effective(25)],
                &[Some(existing)],
                &BTreeMap::new(),
            );
            if infer_from {
                // The fixed upper Y=30mil cannot contain absolute dependency Y65
                // with its 10mil margin for any size.
                assert!(result.is_err());
            } else {
                let evaluated = result.unwrap();
                let bounds = evaluated.graphics[0].unwrap();
                assert_eq!(bounds.from, value);
                assert_eq!(bounds.to.y, mil(75));
                assert_eq!(evaluated.pins[1].unwrap().location.y, mil(65));
            }
        }
        let compiled = compile(
            "component R {body=rectangle {to:(40mil,30mil),min_height:1mil} root=pin 1 {on:$body.right,at:center,gap:10mil} pin 2 {after:$root,gap:40mil}}",
        );
        let existing = BoxGeometry {
            from: CoordPoint::zero(),
            to: location(40, 30),
        };
        let evaluated = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[effective(25); 2],
            &[Some(existing)],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(evaluated.graphics[0].unwrap().to, location(40, 30));
        assert_eq!(evaluated.graphics[0].unwrap().from.y, mil(-70));
    }
    #[test]
    fn effective_pin_length_and_offsets_constrain_representable_auto_width() {
        let compiled = compile("component R {body=rectangle {} pin 1 {on:$body.left,side:inside}}");
        let mut placements = compiled.placements.clone();
        placements[0].as_mut().unwrap().offset = FieldIntent::Set(Spanned::new(
            CoordPoint::new(Coord::new(i32::MAX), Coord::ZERO),
            Span::new(1, 2),
        ));
        let mut pin = effective(25);
        pin.length = Coord::new(i32::MAX);
        let evaluated = evaluate_layout(
            LayoutOwner::Component,
            &placements,
            &compiled.auto_sizes,
            &[pin],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(evaluated.graphics[0].unwrap().from.x, Coord::new(-i32::MAX));
        assert_eq!(evaluated.graphics[0].unwrap().to.x, Coord::new(i32::MAX));
        assert_eq!(evaluated.pins[0].unwrap().location.x, Coord::new(i32::MAX));
    }
    #[test]
    fn individually_representable_pin_constraints_with_empty_intersection_error() {
        let compiled = compile(
            "component R {body=rectangle {min_width:0mil} pin 1 {on:$body.left,side:inside,offset:(0.01mil,0mil),gap:0mil} pin 2 {on:$body.right,offset:(-0.001mil,0mil),gap:0mil}}",
        );
        let mut pin = effective(25);
        pin.length = Coord::new(i32::MAX);
        let error = evaluate_layout(
            LayoutOwner::Component,
            &compiled.placements,
            &compiled.auto_sizes,
            &[pin; 2],
            &[Some(box100())],
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(
            error.message.contains("no common representable size"),
            "{error}"
        );
        assert!(error.span.is_some());
    }
    #[test]
    fn exact_integer_solver_matches_exhaustive_center_and_corner_rounding() {
        let span = Span::new(1, 2);
        for origin in [
            AxisOrigin::Centered,
            AxisOrigin::Minimum(Coord::new(-7)),
            AxisOrigin::Maximum(Coord::new(9)),
        ] {
            for base in [
                AlongBase::Low,
                AlongBase::High,
                AlongBase::Center,
                AlongBase::Absolute(Coord::new(13)),
            ] {
                for offset in -15..=15 {
                    for margin in 0..=3 {
                        let constraint = AlongConstraint {
                            position: CoordinateExpr { base, offset, span },
                            margin: Coord::new(margin),
                        };
                        let expected = (0..=200).find(|size| {
                            let (low, high) = origin.bounds(*size);
                            let value = constraint.position.value(low, high).unwrap();
                            value >= low + i64::from(margin) && value <= high - i64::from(margin)
                        });
                        let result = solve_axis(
                            origin,
                            Coord::ZERO,
                            &AxisConstraints {
                                inside: vec![constraint],
                                coordinates: Vec::new(),
                            },
                            span,
                        );
                        assert_eq!(
                            result.ok(),
                            expected,
                            "{origin:?} {base:?} offset={offset} margin={margin}"
                        );
                    }
                }
            }
        }
        for origin in [
            AxisOrigin::Minimum(Coord::new(i32::MAX - 100)),
            AxisOrigin::Maximum(Coord::new(i32::MIN + 100)),
        ] {
            for base in [AlongBase::Low, AlongBase::High, AlongBase::Center] {
                for offset in [-150, -1, 0, 1, 150] {
                    let expr = CoordinateExpr { base, offset, span };
                    let sizes = (0..=100)
                        .filter(|size| {
                            let (low, high) = origin.bounds(*size);
                            let value = expr.value(low, high).unwrap();
                            i32::try_from(value).is_ok()
                        })
                        .collect::<Vec<_>>();
                    let expected = sizes
                        .first()
                        .zip(sizes.last())
                        .map(|(first, last)| (*first, *last));
                    assert_eq!(
                        coordinate_size_interval(origin, expr).ok(),
                        expected,
                        "{origin:?} {base:?} offset={offset}"
                    );
                }
            }
        }
    }
    #[test]
    fn impossible_outward_auto_size_chains_block_with_source_context() {
        for body in ["root=pin 1 {on:$body.left,at:start,gap:10mil} pin 2 {before:$root,gap:5mil}"]
        {
            let compiled = compile(&format!("component R {{body=rectangle {{}} {body}}}"));
            let effective = vec![effective(25); compiled.placements.len()];
            let error = evaluate_layout(
                LayoutOwner::Component,
                &compiled.placements,
                &compiled.auto_sizes,
                &effective,
                &[Some(box100())],
                &BTreeMap::new(),
            )
            .unwrap_err();
            assert!(
                error
                    .message
                    .contains("auto-size constraints cannot satisfy"),
                "{error}"
            );
            assert!(
                error.span.is_some(),
                "layout failure must identify authored source"
            );
        }
    }
    #[test]
    fn inherited_geometry_never_supplies_a_missing_or_out_of_range_local_slot() {
        let mut compiled = compile(
            "component R {body=rectangle {from:(0mil,0mil),to:(100mil,100mil)} pin 1 {on:$body.left}}",
        );
        let mut inherited = BTreeMap::from([(
            GraphicSourceRef {
                owner: LayoutOwner::Component,
                index: 0,
            },
            box100(),
        )]);
        assert!(
            evaluate_layout(
                LayoutOwner::Component,
                &compiled.placements,
                &compiled.auto_sizes,
                &[effective(25)],
                &[None],
                &inherited
            )
            .is_err()
        );
        compiled.placements[0].as_mut().unwrap().graphic.node.index = 99;
        inherited.insert(
            GraphicSourceRef {
                owner: LayoutOwner::Component,
                index: 99,
            },
            box100(),
        );
        assert!(
            evaluate_layout(
                LayoutOwner::Component,
                &compiled.placements,
                &compiled.auto_sizes,
                &[effective(25)],
                &[Some(box100())],
                &inherited
            )
            .is_err()
        );
    }
    #[test]
    fn evaluator_rechecks_source_ranges_cycles_and_numeric_overflow() {
        let mut compiled = compile(
            "component R {body=rectangle {from:(0mil,0mil),to:(100mil,100mil)} pin 1 {on:$body.right}}",
        );
        let evaluate = |placements: &[Option<PinPlacementIntent>],
                        geometry: &[Option<BoxGeometry>]| {
            evaluate_layout(
                LayoutOwner::Component,
                placements,
                &compiled.auto_sizes,
                &[effective(25)],
                geometry,
                &BTreeMap::new(),
            )
        };
        let huge = BoxGeometry {
            from: CoordPoint::zero(),
            to: CoordPoint::new(Coord::new(i32::MAX), Coord::ZERO),
        };
        assert!(evaluate(&compiled.placements, &[Some(huge)]).is_err());
        let p = compiled.placements[0].as_mut().unwrap();
        p.position.node = AnchorPosition::After(PinSourceRef {
            owner: LayoutOwner::Component,
            index: 1,
        });
        assert!(evaluate(&compiled.placements, &[Some(box100())]).is_err());
        compiled.placements[0].as_mut().unwrap().position.node =
            AnchorPosition::After(PinSourceRef {
                owner: LayoutOwner::Component,
                index: 0,
            });
        assert!(evaluate(&compiled.placements, &[Some(box100())]).is_err());
        compiled.placements[0].as_mut().unwrap().position.node = AnchorPosition::Center;
        compiled.placements[0].as_mut().unwrap().graphic.node.index = 99;
        assert!(evaluate(&compiled.placements, &[Some(box100())]).is_err());
    }
}
