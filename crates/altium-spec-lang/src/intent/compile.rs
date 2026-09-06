use super::*;
use crate::ast::*;
use crate::{ScopeStack, SpecError, SpecErrorCode, Value, eval_expr};
use altium_format_types::Color;
use indexmap::IndexMap;
use std::collections::BTreeMap;

pub(super) fn error(message: impl Into<String>, span: Span) -> SpecError {
    SpecError::at(SpecErrorCode::TypeMismatch, message, span)
}
fn duplicate(name: &str, previous: Span, span: Span) -> SpecError {
    SpecError::at(
        SpecErrorCode::DuplicateEntity,
        format!(
            "duplicate '{name}'; first authored at {}..{}",
            previous.start, previous.end
        ),
        span,
    )
}
fn unique<K: Ord + std::fmt::Display>(
    seen: &mut BTreeMap<K, Span>,
    key: K,
    span: Span,
) -> Result<(), SpecError> {
    if let Some(previous) = seen.get(&key) {
        return Err(duplicate(&key.to_string(), *previous, span));
    }
    seen.insert(key, span);
    Ok(())
}

/// Compile a standalone definition file. Imports must be captured and resolved;
/// silently accepting an import without loading its source is forbidden.
pub fn compile_schlib(file: &SpecFile) -> Result<SchLibIntent, SpecError> {
    if let Some(item) = file
        .items
        .iter()
        .find(|i| matches!(i.node, SpecItem::Import(_)))
    {
        return Err(error(
            "imports require captured import compilation",
            item.span,
        ));
    }
    compile_schlib_with_scope(file, ScopeStack::new())
}

/// Compile with explicitly resolved expression bindings. The caller must validate
/// and capture imports before using this lower-level entry point.
pub fn compile_schlib_with_scope(
    file: &SpecFile,
    mut scope: ScopeStack,
) -> Result<SchLibIntent, SpecError> {
    scope.push();
    let mut bindings = BTreeMap::new();
    // Swap groups are lexically scoped named bindings, not native collections.
    for item in &file.items {
        if let SpecItem::SwapGroup(group) = &item.node {
            bind_swap(group, &mut scope, &mut bindings)?;
        }
    }
    for item in &file.items {
        if let SpecItem::LetBinding(binding) = &item.node {
            bind_let(binding, &mut scope, &mut bindings)?;
        }
    }
    let mut result = SchLibIntent {
        schema_version: INTENT_SCHEMA_VERSION,
        components: Collection::default(),
    };
    let mut names = BTreeMap::new();
    for item in &file.items {
        match &item.node {
            SpecItem::Component(decl) => {
                unique(&mut names, decl.name.node.as_str(), decl.name.span)?;
                result.components.items.push(component(decl, &scope)?);
            }
            SpecItem::Collection(decl) if decl.kind.node == CollectionKind::Components => {
                set_scope(&mut result.components.scope, decl, item.span)?
            }
            SpecItem::Import(_) | SpecItem::LetBinding(_) | SpecItem::SwapGroup(_) => {}
            _ => {
                return Err(error(
                    "item is not legal at a SchLib library owner",
                    item.span,
                ));
            }
        }
    }
    Ok(result)
}

fn set_scope(
    target: &mut CollectionScope,
    decl: &CollectionDecl,
    span: Span,
) -> Result<(), SpecError> {
    if let Some(previous) = target.span {
        return Err(duplicate("collection metadata", previous, span));
    }
    if decl.delete.node == CollectionDelete::Allow && decl.mode.node != CollectionMode::Complete {
        return Err(error(
            "delete: allow requires mode: complete",
            decl.delete.span,
        ));
    }
    *target = CollectionScope {
        mode: decl.mode.node,
        delete: decl.delete.node,
        span: Some(span),
    };
    Ok(())
}
fn bind_let(
    binding: &LetBinding,
    scope: &mut ScopeStack,
    seen: &mut BTreeMap<String, Span>,
) -> Result<(), SpecError> {
    unique(seen, binding.name.node.clone(), binding.name.span)?;
    scope.mark_evaluating(&binding.name.node);
    let value = eval_expr(&binding.value, scope)?;
    scope.define(binding.name.node.clone(), value);
    Ok(())
}
pub(super) fn bind_swap(
    group: &SwapGroupDecl,
    scope: &mut ScopeStack,
    seen: &mut BTreeMap<String, Span>,
) -> Result<(), SpecError> {
    // Group properties currently have no native meaning; never silently ignore.
    let props = Properties::object(&group.body, scope)?;
    props.finish("swap_group")?;
    let name = group.name.node.as_str();
    let binding = group
        .binding
        .as_ref()
        .map(|b| b.node.clone())
        .unwrap_or_else(|| name.clone());
    unique(seen, binding.clone(), group.name.span)?;
    scope.define(binding.clone(), Value::SwapGroup(name.clone()));
    if binding != name {
        unique(seen, name.clone(), group.name.span)?;
        scope.define(name.clone(), Value::SwapGroup(name));
    }
    Ok(())
}
fn named(name: &Spanned<EntityName>) -> Result<Spanned<String>, SpecError> {
    let value = name.node.as_str();
    if value.is_empty() {
        return Err(error("required identifier cannot be empty", name.span));
    }
    Ok(Spanned::new(value, name.span))
}

fn component(decl: &ComponentDecl, parent: &ScopeStack) -> Result<ComponentIntent, SpecError> {
    let mut scope = parent.clone();
    scope.push();
    let mut bindings = BTreeMap::new();
    for item in &decl.body {
        if let ComponentItem::SwapGroup(group) = &item.node {
            bind_swap(group, &mut scope, &mut bindings)?;
        }
    }
    for item in &decl.body {
        if let ComponentItem::LetBinding(binding) = &item.node {
            bind_let(binding, &mut scope, &mut bindings)?;
        }
    }
    for item in &decl.body {
        if let ComponentItem::Graphic(graphic) = &item.node {
            if let Some(binding) = &graphic.binding {
                unique(&mut bindings, binding.node.clone(), binding.span)?;
            }
        }
    }
    bind_pins(
        decl.body.iter().filter_map(|item| {
            if let ComponentItem::Pin(pin) = &item.node {
                Some(pin)
            } else {
                None
            }
        }),
        &mut scope,
        &mut bindings,
    )?;
    let mut props = Properties::properties(
        decl.body.iter().filter_map(|i| {
            if let ComponentItem::Property(p) = &i.node {
                Some(p)
            } else {
                None
            }
        }),
        &scope,
    )?;
    let mut result = ComponentIntent {
        name: named(&decl.name)?,
        annotation: decl.annotation.clone(),
        designator: props.take("designator", &[], COMPONENT_DESIGNATOR, optional_string)?,
        description: props.take("description", &[], OPTIONAL_DESCRIPTION, optional_string)?,
        component_kind: props.take("component_kind", &[], COMPONENT_KIND, component_kind)?,
        part_count: props.take_count()?,
        show_hidden_pins: props.take("show_hidden_pins", &[], HIDDEN, boolean)?,
        pins: Collection::default(),
        parameters: Collection::default(),
        aliases: Collection::default(),
        footprints: Collection::default(),
        graphics: Collection::default(),
        parts: Collection::default(),
    };
    props.finish("component")?;
    let (pins, graphics, graphic_bindings) = compile_children(
        LayoutOwner::Component,
        &decl
            .body
            .iter()
            .filter_map(|item| {
                if let ComponentItem::Pin(pin) = &item.node {
                    Some(pin)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        &decl
            .body
            .iter()
            .filter_map(|item| {
                if let ComponentItem::Graphic(graphic) = &item.node {
                    Some(graphic)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        &super::anchors::GraphicBindings::new(),
        &scope,
    )?;
    result.pins.items = pins;
    result.graphics.items = graphics;
    let (mut params, mut aliases, mut footprints, mut parts) = (
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    for item in &decl.body {
        match &item.node {
            ComponentItem::Pin(_) | ComponentItem::Graphic(_) => {}
            ComponentItem::Parameter(decl) => {
                unique(&mut params, decl.name.node.as_str(), decl.name.span)?;
                result.parameters.items.push(parameter(decl, &scope)?);
            }
            ComponentItem::Alias(decl) => {
                let name = named(&decl.name)?;
                unique(&mut aliases, name.node.clone(), name.span)?;
                result.aliases.items.push(name);
            }
            ComponentItem::FootprintMap(decl) => {
                let value = footprint(decl, &scope)?;
                unique(
                    &mut footprints,
                    value.model_name.node.clone(),
                    value.model_name.span,
                )?;
                result.footprints.items.push(value);
            }
            ComponentItem::Part(decl) => {
                unique(&mut parts, decl.number.node, decl.number.span)?;
                result
                    .parts
                    .items
                    .push(part(decl, &scope, &graphic_bindings)?);
            }
            ComponentItem::Collection(decl) => {
                let target = match decl.kind.node {
                    CollectionKind::Parts => &mut result.parts.scope,
                    CollectionKind::Pins => &mut result.pins.scope,
                    CollectionKind::Parameters => &mut result.parameters.scope,
                    CollectionKind::Aliases => &mut result.aliases.scope,
                    CollectionKind::Footprints => &mut result.footprints.scope,
                    CollectionKind::Graphics => &mut result.graphics.scope,
                    _ => {
                        return Err(error(
                            "collection is not legal at component owner",
                            item.span,
                        ));
                    }
                };
                set_scope(target, decl, item.span)?;
            }
            ComponentItem::Property(_)
            | ComponentItem::LetBinding(_)
            | ComponentItem::SwapGroup(_) => {}
            _ => {
                return Err(error(
                    "item is not legal in a SchLib component definition",
                    item.span,
                ));
            }
        }
    }
    validate_parts(&result)?;
    Ok(result)
}
fn validate_parts(component: &ComponentIntent) -> Result<(), SpecError> {
    if component.parts.scope.mode == CollectionMode::Complete {
        if component.parts.items.is_empty() {
            return Err(error(
                "complete parts collection must retain at least part 1",
                component.parts.scope.span.unwrap_or(component.name.span),
            ));
        }
        for (index, part) in component.parts.items.iter().enumerate() {
            if part.number.node.get() as usize != index + 1 {
                return Err(error(
                    "complete parts must be an ascending contiguous prefix starting at 1",
                    part.number.span,
                ));
            }
        }
    }
    if let FieldIntent::Set(count) = &component.part_count {
        let highest = component
            .parts
            .items
            .iter()
            .map(|p| p.number.node.get())
            .max()
            .unwrap_or(1);
        if count.node.get() < highest {
            return Err(error(
                "part_count cannot exclude an authored part",
                count.span,
            ));
        }
        if component.parts.scope.mode == CollectionMode::Complete
            && count.node.get() as usize != component.parts.items.len()
        {
            return Err(error(
                "part_count must agree with complete parts collection",
                count.span,
            ));
        }
    }
    Ok(())
}
fn bind_pins<'a>(
    pins: impl Iterator<Item = &'a PinDecl>,
    scope: &mut ScopeStack,
    seen: &mut BTreeMap<String, Span>,
) -> Result<(), SpecError> {
    for pin in pins {
        if let Some(binding) = &pin.binding {
            unique(seen, binding.node.clone(), binding.span)?;
            scope.define(binding.node.clone(), Value::String(pin.name.node.as_str()));
        }
    }
    Ok(())
}
fn part(
    decl: &PartBlock,
    parent: &ScopeStack,
    inherited: &super::anchors::GraphicBindings,
) -> Result<PartIntent, SpecError> {
    let number = PartNumber::new(decl.number.node)
        .ok_or_else(|| error("part number must be positive", decl.number.span))?;
    let mut scope = parent.clone();
    scope.push();
    let mut bindings = BTreeMap::new();
    for item in &decl.body {
        if let PartItem::LetBinding(binding) = &item.node {
            bind_let(binding, &mut scope, &mut bindings)?;
        }
    }
    for item in &decl.body {
        if let PartItem::Graphic(graphic) = &item.node {
            if let Some(binding) = &graphic.binding {
                unique(&mut bindings, binding.node.clone(), binding.span)?;
            }
        }
    }
    bind_pins(
        decl.body.iter().filter_map(|item| {
            if let PartItem::Pin(pin) = &item.node {
                Some(pin)
            } else {
                None
            }
        }),
        &mut scope,
        &mut bindings,
    )?;
    let mut props = Properties::properties(
        decl.body.iter().filter_map(|i| {
            if let PartItem::Property(p) = &i.node {
                Some(p)
            } else {
                None
            }
        }),
        &scope,
    )?;
    let mut result = PartIntent {
        number: Spanned::new(number, decl.number.span),
        part_swap_group: props.take("part_swap_group", &["swap_group"], SWAP_GROUP, swap_string)?,
        pins: Collection::default(),
        graphics: Collection::default(),
    };
    props.finish("part")?;
    let (pins, graphics, _) = compile_children(
        LayoutOwner::Part(number),
        &decl
            .body
            .iter()
            .filter_map(|item| {
                if let PartItem::Pin(pin) = &item.node {
                    Some(pin)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        &decl
            .body
            .iter()
            .filter_map(|item| {
                if let PartItem::Graphic(graphic) = &item.node {
                    Some(graphic)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        inherited,
        &scope,
    )?;
    result.pins.items = pins;
    result.graphics.items = graphics;
    for item in &decl.body {
        match &item.node {
            PartItem::Pin(_) | PartItem::Graphic(_) => {}
            PartItem::Collection(decl) => {
                let target = match decl.kind.node {
                    CollectionKind::Pins => &mut result.pins.scope,
                    CollectionKind::Graphics => &mut result.graphics.scope,
                    _ => return Err(error("collection is not legal at a part owner", item.span)),
                };
                set_scope(target, decl, item.span)?;
            }
            PartItem::Property(_) | PartItem::LetBinding(_) => {}
        }
    }
    Ok(result)
}
fn compile_children(
    owner: LayoutOwner,
    pins: &[&PinDecl],
    graphics: &[&GraphicDecl],
    inherited: &super::anchors::GraphicBindings,
    scope: &ScopeStack,
) -> Result<
    (
        Vec<PinIntent>,
        Vec<GraphicIntent>,
        super::anchors::GraphicBindings,
    ),
    SpecError,
> {
    let layout = super::anchors::compile_layout(owner, pins, graphics, inherited, scope)?;
    if layout.pins.len() != layout.placements.len()
        || layout.graphics.len() != layout.auto_sizes.len()
    {
        return Err(SpecError::no_span(
            SpecErrorCode::TypeMismatch,
            "layout compilation changed source collection cardinality",
        ));
    }
    let pins = layout
        .pins
        .iter()
        .zip(layout.placements)
        .map(|(decl, placement)| pin(decl, scope, placement))
        .collect::<Result<Vec<_>, SpecError>>()?;
    let graphics = layout
        .graphics
        .iter()
        .zip(layout.auto_sizes)
        .map(|(decl, auto_size)| super::graphics::compile_graphic(decl, scope, auto_size))
        .collect::<Result<Vec<_>, SpecError>>()?;
    Ok((pins, graphics, layout.bindings))
}

fn pin(
    decl: &PinDecl,
    scope: &ScopeStack,
    placement: Option<PinPlacementIntent>,
) -> Result<PinIntent, SpecError> {
    let mut props = Properties::object(&decl.body, scope)?;
    if placement.is_some() {
        // Already parsed into typed placement by compile_layout; only the native
        // field pass is filtered. No authored property is silently discarded.
        props.consume_layout(&["on", "after", "before", "at", "gap", "side", "offset"]);
    }
    let result = PinIntent {
        placement,
        designator: named(&decl.name)?,
        name: props.take("name", &[], PIN_NAME, string)?,
        electrical: props.take("electrical", &[], PIN_ELECTRICAL, electrical)?,
        length: props.take("length", &[], PIN_LENGTH, coord)?,
        location: props.take("at", &["location"], PIN_LOCATION, point)?,
        location_x: props.take("x", &[], PIN_LOCATION_X, coord)?,
        location_y: props.take("y", &[], PIN_LOCATION_Y, coord)?,
        orientation: props.take("orientation", &[], PIN_ORIENTATION, rotation)?,
        is_hidden: props.take("is_hidden", &[], HIDDEN, boolean)?,
        hidden_net_name: props.take("hidden_net_name", &[], PIN_HIDDEN_NET_NAME, string)?,
        swap_group: props.take("swap_group", &[], SWAP_GROUP, swap_string)?,
        part_swap_group: props.take("part_swap_group", &[], SWAP_GROUP, swap_string)?,
        pair_swap_group: props.take("pair_swap_group", &[], SWAP_GROUP, swap_string)?,
    };
    props.finish("pin")?;
    super::fields::validate_pin_location(&result)?;
    Ok(result)
}
fn parameter(decl: &ParameterDecl, scope: &ScopeStack) -> Result<ParameterIntent, SpecError> {
    let mut props = Properties::object(&decl.body, scope)?;
    let result = ParameterIntent {
        name: named(&decl.name)?,
        text: props.take("text", &["value"], PARAMETER_TEXT, string)?,
        is_hidden: props.take("is_hidden", &[], HIDDEN, boolean)?,
    };
    props.finish("parameter")?;
    Ok(result)
}
fn path_expr(path: &DollarPath, span: Span) -> Spanned<Expr> {
    let mut expr = Spanned::new(Expr::DollarIdent(path.root.node.clone()), path.root.span);
    for step in &path.steps {
        expr = Spanned::new(
            match &step.node {
                PathStep::Field(field) => {
                    Expr::Path(Box::new(expr), Spanned::new(field.clone(), step.span))
                }
                PathStep::Index(index) => Expr::Index(
                    Box::new(expr),
                    Box::new(Spanned::new(index.clone(), step.span)),
                ),
            },
            span,
        );
    }
    expr
}
fn ref_string(value: Value, span: Span) -> Result<String, SpecError> {
    match value {
        Value::String(value) => Ok(value),
        Value::Integer(value) => Ok(value.to_string()),
        Value::ImportRef { name, .. } => Ok(name),

        _ => Err(error(
            "reference must resolve to an existing named entity",
            span,
        )),
    }
}
fn footprint(decl: &FootprintMapDecl, scope: &ScopeStack) -> Result<FootprintIntent, SpecError> {
    let (model_name, source) = match &decl.name.node {
        FootprintRef::Name(name) => (name.as_str(), None),
        FootprintRef::DollarPath(path) => match eval_expr(&path_expr(path, decl.name.span), scope)?
        {
            Value::Definition(crate::eval::DefinitionReference::Footprint {
                name, source, ..
            }) => (name.clone(), Some(SourceDefinitionRef { source, name })),
            _ => {
                return Err(error(
                    "footprint reference must resolve to a footprint definition",
                    decl.name.span,
                ));
            }
        },
    };
    if model_name.is_empty() {
        return Err(error(
            "footprint model name cannot be empty",
            decl.name.span,
        ));
    }
    let mut result = FootprintIntent {
        source,
        model_name: Spanned::new(model_name, decl.name.span),
        description: match &decl.description {
            Some(expr) => operation(expr, scope, OPTIONAL_DESCRIPTION, optional_string)?,
            None => FieldIntent::Absent,
        },
        maps: Collection::default(),
        explicit_maps: decl.maps.is_some(),
    };
    for decl in &decl.collections {
        if decl.node.kind.node != CollectionKind::Maps {
            return Err(error(
                "only maps collection is legal at footprint owner",
                decl.span,
            ));
        }
        set_scope(&mut result.maps.scope, &decl.node, decl.span)?;
    }
    if let Some(maps) = &decl.maps {
        for pair in maps {
            let resolve = |reference: &PinPadRef| -> Result<Spanned<String>, SpecError> {
                match reference {
                    PinPadRef::Literal(value) => {
                        if value.node.is_empty() {
                            return Err(error("pin/pad identifier cannot be empty", value.span));
                        }
                        Ok(value.clone())
                    }
                    PinPadRef::Dollar(path) => Ok(Spanned::new(
                        ref_string(
                            eval_expr(&path_expr(&path.node, path.span), scope)?,
                            path.span,
                        )?,
                        path.span,
                    )),
                }
            };
            result.maps.items.push(PinPadIntent {
                pin: resolve(&pair.node.pin)?,
                pad: resolve(&pair.node.pad)?,
            });
        }
    }
    Ok(result)
}

pub(super) fn object_scope(
    object: &Spanned<Object>,
    parent: &ScopeStack,
) -> Result<ScopeStack, SpecError> {
    let mut scope = parent.clone();
    scope.push();
    let mut seen = BTreeMap::new();
    for item in &object.node.items {
        if let ObjectItem::LetBinding(binding) = &item.node {
            bind_let(binding, &mut scope, &mut seen)?;
        }
    }
    Ok(scope)
}

/// Short-lived property staging only; no untyped map escapes into authored intent.
/// Track duplicates before expression evaluation can collapse object keys.
pub(super) struct Properties {
    values: IndexMap<String, (Spanned<Expr>, ScopeStack, Span, usize)>,
    explicit: BTreeMap<String, Span>,
    spread_keys: Vec<BTreeMap<String, Span>>,
    group: usize,
}
impl Properties {
    pub(super) fn properties<'a>(
        properties: impl Iterator<Item = &'a Property>,
        scope: &ScopeStack,
    ) -> Result<Self, SpecError> {
        let mut result = Self {
            values: IndexMap::new(),
            explicit: BTreeMap::new(),
            spread_keys: Vec::new(),
            group: 0,
        };
        for property in properties {
            result.group += 1;
            result.insert(
                property.key.node.clone(),
                property.value.clone(),
                scope.clone(),
                property.key.span,
                true,
            )?;
        }
        Ok(result)
    }
    fn insert(
        &mut self,
        key: String,
        expr: Spanned<Expr>,
        scope: ScopeStack,
        key_span: Span,
        explicit: bool,
    ) -> Result<(), SpecError> {
        if explicit {
            unique(&mut self.explicit, key.clone(), key_span)?;
        }
        self.values.insert(key, (expr, scope, key_span, self.group));
        Ok(())
    }
    pub(super) fn object(object: &Spanned<Object>, parent: &ScopeStack) -> Result<Self, SpecError> {
        let scope = object_scope(object, parent)?;
        let mut result = Self {
            values: IndexMap::new(),
            explicit: BTreeMap::new(),
            spread_keys: Vec::new(),
            group: 0,
        };
        for item in &object.node.items {
            result.group += 1;
            match &item.node {
                ObjectItem::Property(property) => result.insert(
                    property.key.node.clone(),
                    property.value.clone(),
                    scope.clone(),
                    property.key.span,
                    true,
                )?,
                ObjectItem::Spread(expr) => {
                    let Value::Object(values) = eval_expr(expr, &scope)? else {
                        return Err(error("spread requires an object", expr.span));
                    };
                    result
                        .spread_keys
                        .extend(values.direct_groups().iter().map(|group| {
                            group
                                .iter()
                                .map(|(key, _)| (key.clone(), expr.span))
                                .collect()
                        }));
                    for (key, value) in values {
                        result.group += 1;
                        // Preserve the spread's source span and evaluate each typed
                        // value through a private bound expression, without lossy
                        // conversion back into source literals.
                        let mut value_scope = ScopeStack::new();
                        value_scope.push();
                        value_scope.define("spread_value".into(), value);
                        result.insert(
                            key,
                            Spanned::new(Expr::Ident("spread_value".into()), expr.span),
                            value_scope,
                            expr.span,
                            false,
                        )?;
                    }
                }
                ObjectItem::LetBinding(_) => {}
            }
        }
        Ok(result)
    }
    pub(super) fn take<T: Clone>(
        &mut self,
        name: &str,
        synonyms: &[&str],
        policy: FieldPolicy<T>,
        parse: impl Fn(Value, Span) -> Result<T, SpecError>,
    ) -> Result<FieldIntent<T>, SpecError> {
        for spread in &self.spread_keys {
            let mut previous = None;
            for key in std::iter::once(name).chain(synonyms.iter().copied()) {
                if let Some(span) = spread.get(key) {
                    if let Some(first) = previous {
                        return Err(duplicate(name, first, *span));
                    }
                    previous = Some(*span);
                }
            }
        }
        let mut found: Option<(Spanned<Expr>, ScopeStack, Span, usize)> = None;
        let mut direct = None;
        for key in std::iter::once(name).chain(synonyms.iter().copied()) {
            if let Some(span) = self.explicit.get(key) {
                if let Some(previous) = direct {
                    return Err(duplicate(name, previous, *span));
                }
                direct = Some(*span);
            }
            if let Some(value) = self.values.shift_remove(key) {
                if let Some(previous) = &found {
                    if previous.3 == value.3 {
                        return Err(duplicate(name, previous.2, value.2));
                    }
                    if previous.3 > value.3 {
                        continue;
                    }
                }
                found = Some(value);
            }
        }
        match found {
            None => Ok(FieldIntent::Absent),
            Some((expr, scope, _, _)) => operation(&expr, &scope, policy, parse),
        }
    }
    fn take_count(&mut self) -> Result<FieldIntent<PartCount>, SpecError> {
        let Some((expr, scope, _, _)) = self.values.shift_remove("part_count") else {
            return Ok(FieldIntent::Absent);
        };
        match expr.node {
            Expr::Inherit => Ok(FieldIntent::Inherit(expr.span)),
            Expr::Reset => Ok(FieldIntent::Reset(expr.span)),
            _ => {
                let count = positive_integer(eval_expr(&expr, &scope)?, expr.span)?;
                Ok(FieldIntent::Set(Spanned::new(
                    PartCount::new(count).ok_or_else(|| error("invalid part count", expr.span))?,
                    expr.span,
                )))
            }
        }
    }
    /// Called only after the layout pass has represented these properties in
    /// typed PinPlacementIntent or AutoSizeIntent.
    pub(super) fn consume_layout(&mut self, keys: &[&str]) {
        for key in keys {
            self.values.shift_remove(*key);
        }
    }
    pub(super) fn finish(self, owner: &str) -> Result<(), SpecError> {
        if let Some((key, (_, _, span, _))) = self.values.first() {
            return Err(SpecError::at(
                SpecErrorCode::UnknownProperty,
                format!("unknown or unsupported {owner} property '{key}'"),
                *span,
            ));
        }
        Ok(())
    }
}
fn operation<T: Clone>(
    expr: &Spanned<Expr>,
    scope: &ScopeStack,
    policy: FieldPolicy<T>,
    parse: impl Fn(Value, Span) -> Result<T, SpecError>,
) -> Result<FieldIntent<T>, SpecError> {
    let result = match expr.node {
        Expr::Inherit => FieldIntent::Inherit(expr.span),
        Expr::Reset => FieldIntent::Reset(expr.span),
        _ => match eval_expr(expr, scope)? {
            Value::Null => FieldIntent::Clear(expr.span),
            value => FieldIntent::Set(Spanned::new(parse(value, expr.span)?, expr.span)),
        },
    };
    policy.validate(&result)?;
    Ok(result)
}
pub(super) fn string(value: Value, span: Span) -> Result<String, SpecError> {
    match value {
        Value::String(value) => Ok(value),
        _ => Err(error("expected string", span)),
    }
}
fn optional_string(value: Value, span: Span) -> Result<Option<String>, SpecError> {
    string(value, span).map(Some)
}
fn swap_string(value: Value, span: Span) -> Result<String, SpecError> {
    match value {
        Value::SwapGroup(value) => Ok(value),
        value => string(value, span),
    }
}
pub(super) fn boolean(value: Value, span: Span) -> Result<bool, SpecError> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(error("expected boolean", span)),
    }
}
pub(super) fn coord(value: Value, span: Span) -> Result<Coord, SpecError> {
    value.to_dim(Some(span)).map(Coord::from_internal)
}
pub(super) fn point(value: Value, span: Span) -> Result<CoordPoint, SpecError> {
    match value {
        Value::CoordPoint(x, y) => Ok(CoordPoint::new(
            Coord::from_internal(x),
            Coord::from_internal(y),
        )),
        _ => Err(error("expected coordinate pair", span)),
    }
}
pub(super) fn finite_number(value: Value, span: Span) -> Result<f64, SpecError> {
    match value {
        Value::Integer(value) => Ok(value as f64),
        Value::Float(value) if value.is_finite() => Ok(value),
        _ => Err(error("expected finite number", span)),
    }
}
pub(super) fn positive_integer(value: Value, span: Span) -> Result<i32, SpecError> {
    match value {
        Value::Integer(value) if value > 0 => Ok(value),
        _ => Err(error("expected positive integer", span)),
    }
}
pub(super) fn color(value: Value, span: Span) -> Result<Color, SpecError> {
    match value {
        Value::Color(r, g, b) => Ok(Color::from_rgb(r, g, b)),
        _ => Err(error("expected color", span)),
    }
}
fn electrical(value: Value, span: Span) -> Result<PinElectricalType, SpecError> {
    let name = string(value, span)?;
    match name.to_ascii_lowercase().as_str() {
        "input" => Ok(PinElectricalType::Input),
        "input_output" | "inputoutput" | "io" | "bidir" => Ok(PinElectricalType::InputOutput),
        "output" => Ok(PinElectricalType::Output),
        "open_collector" | "opencollector" | "oc" => Ok(PinElectricalType::OpenCollector),
        "passive" => Ok(PinElectricalType::Passive),
        "hiz" | "hi_z" | "tristate" => Ok(PinElectricalType::HiZ),
        "open_emitter" | "openemitter" | "oe" => Ok(PinElectricalType::OpenEmitter),
        "power" => Ok(PinElectricalType::Power),
        _ => Err(error(format!("unknown electrical type '{name}'"), span)),
    }
}
fn rotation(value: Value, span: Span) -> Result<RotationBy90, SpecError> {
    let name = match value {
        Value::Integer(value) => value.to_string(),
        value => string(value, span)?,
    };
    match name.to_ascii_lowercase().as_str() {
        "0" | "rotate0" | "right" | "east" => Ok(RotationBy90::Rotate0),
        "90" | "rotate90" | "up" | "north" => Ok(RotationBy90::Rotate90),
        "180" | "rotate180" | "left" | "west" => Ok(RotationBy90::Rotate180),
        "270" | "rotate270" | "down" | "south" => Ok(RotationBy90::Rotate270),
        _ => Err(error("orientation must be a quarter turn", span)),
    }
}
fn component_kind(value: Value, span: Span) -> Result<ComponentKind, SpecError> {
    let name = string(value, span)?;
    match name.to_ascii_lowercase().as_str() {
        "standard" => Ok(ComponentKind::Standard),
        "mechanical" => Ok(ComponentKind::Mechanical),
        "graphical" => Ok(ComponentKind::Graphical),
        "net_tie_bom" | "nettie_bom" | "nettiebom" => Ok(ComponentKind::NetTieBom),
        "net_tie_no_bom" | "nettie_no_bom" | "nettienobom" => Ok(ComponentKind::NetTieNoBom),
        "standard_no_bom" | "standardnobom" => Ok(ComponentKind::StandardNoBom),
        "jumper" => Ok(ComponentKind::Jumper),
        _ => Err(error(format!("unknown component kind '{name}'"), span)),
    }
}
