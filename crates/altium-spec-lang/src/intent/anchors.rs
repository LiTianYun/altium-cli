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
    let mut auto_margins: BTreeMap<(GraphicSourceRef, AnchorEdge), Coord> = BTreeMap::new();
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
            Some(i64::from(AUTO_PADDING.definition(&auto.padding)?.raw()))
        } else {
            None
        };
        if explicit_padding.is_some_and(|padding| padding < 0) {
            return Err(error("auto-size padding must be nonnegative", auto.span));
        }
        let mut extents: BTreeMap<AnchorEdge, (i64, i64)> = BTreeMap::new();
        for pin_index in &order {
            if let Some(p) = &placements[*pin_index] {
                if p.graphic.node == target {
                    let gap = i64::from(ANCHOR_GAP.definition(&p.gap)?.raw());
                    if gap < 0 {
                        return Err(error("anchor gap must be nonnegative", p.span));
                    }
                    if let Some(entry) = extents.get_mut(&p.edge.node) {
                        entry.1 = entry
                            .1
                            .checked_add(gap)
                            .ok_or_else(|| error("auto-size extent overflow", auto.span))?;
                    } else {
                        let padding = explicit_padding.unwrap_or(gap);
                        extents.insert(p.edge.node, (padding, 0));
                    }
                }
            }
        }
        for (edge, (padding, _)) in &extents {
            let padding = i32::try_from(*padding)
                .map(Coord::new)
                .map_err(|_| error("auto-size margin overflow", auto.span))?;
            auto_margins.insert((target, *edge), padding);
        }
        let extent = |edge| -> Result<i64, SpecError> {
            let (padding, gaps) = extents.get(&edge).copied().unwrap_or((0, 0));
            padding
                .checked_mul(2)
                .and_then(|v| v.checked_add(gaps))
                .ok_or_else(|| error("auto-size extent overflow", auto.span))
        };
        let min_width = i64::from(AUTO_MIN_WIDTH.definition(&auto.min_width)?.raw());
        let min_height = i64::from(AUTO_MIN_HEIGHT.definition(&auto.min_height)?.raw());
        if min_width < 0 || min_height < 0 || extents.values().any(|(padding, _)| *padding < 0) {
            return Err(error("auto-size dimensions must be nonnegative", auto.span));
        }
        let width = min_width
            .max(extent(AnchorEdge::Top)?)
            .max(extent(AnchorEdge::Bottom)?);
        let height = min_height
            .max(extent(AnchorEdge::Left)?)
            .max(extent(AnchorEdge::Right)?);
        let existing = effective_graphics[index].ok_or_else(|| {
            error(
                "auto-size source slot has no effective box geometry",
                auto.span,
            )
        })?;
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
        if let Some(margin) = auto_margins.get(&(p.graphic.node, edge)) {
            let varying = i64::from(if edge.vertical() {
                location.y.raw()
            } else {
                location.x.raw()
            });
            let margin = i64::from(margin.raw());
            if varying < min + margin || varying > max - margin {
                return Err(error(
                    "auto-size cannot contain this anchor chain within its edge margins; author explicit box bounds or revise the start/end, gap, offset, or sequence constraints",
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
        assert_eq!(geometry.from.y.to_mils(), -22.5);
        assert_eq!(geometry.to.y.to_mils(), 22.5);
        assert_eq!(evaluated.pins[1].unwrap().location.y.to_mils(), 12.5);
        assert_eq!(evaluated.pins[2].unwrap().location.y.to_mils(), -7.5);
        assert_eq!(evaluated.pins[0].unwrap().location.y.to_mils(), -2.5);
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
    #[test]
    fn unsupported_centered_or_outward_auto_size_chains_block_with_source_context() {
        for body in [
            "root=pin 1 {on:$body.left,at:center,gap:10mil} a=pin 2 {after:$root,gap:50mil} b=pin 3 {after:$a,gap:50mil}",
            "root=pin 1 {on:$body.left,at:start,gap:10mil} pin 2 {before:$root,gap:5mil}",
        ] {
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
                error.message.contains("auto-size cannot contain"),
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
