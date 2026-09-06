use super::{FieldIntent, PartCount};
use crate::{SpecError, SpecErrorCode};
use altium_format_types::{ComponentKind, Coord, CoordPoint, PinElectricalType, RotationBy90};

/// Typed policy; absence of a default or clear value blocks that operation.
#[derive(Clone, Copy)]
pub struct FieldPolicy<T> {
    pub name: &'static str,
    pub default: Option<fn() -> T>,
    pub clear: Option<fn() -> T>,
}
impl<T: Clone> FieldPolicy<T> {
    /// Definition normalization never grants write permission. Keep the original
    /// FieldIntent alongside the normalized result when planning against a target.
    pub fn definition(&self, intent: &FieldIntent<T>) -> Result<T, SpecError> {
        match intent {
            FieldIntent::Set(value) => Ok(value.node.clone()),
            FieldIntent::Clear(_) => self.clear_value(intent),
            _ => self.default_value(intent),
        }
    }
    /// Resolve a placed instance field using an already resolved base. Existing
    /// target wins only for absence; reset explicitly bypasses the base.
    pub fn instance(
        &self,
        intent: &FieldIntent<T>,
        target: Option<&T>,
        base: Option<&T>,
    ) -> Result<T, SpecError> {
        match intent {
            FieldIntent::Absent => target
                .or(base)
                .cloned()
                .ok_or_else(|| self.error(intent, "has neither target nor resolved base")),
            FieldIntent::Set(value) => Ok(value.node.clone()),
            FieldIntent::Inherit(_) => base
                .cloned()
                .ok_or_else(|| self.error(intent, "requires a resolved base for inherit")),
            FieldIntent::Reset(_) => self.default_value(intent),
            FieldIntent::Clear(_) => self.clear_value(intent),
        }
    }
    pub(crate) fn validate(&self, intent: &FieldIntent<T>) -> Result<(), SpecError> {
        match intent {
            FieldIntent::Clear(_) if self.clear.is_none() => {
                Err(self.error(intent, "cannot be cleared"))
            }
            FieldIntent::Reset(_) | FieldIntent::Inherit(_) if self.default.is_none() => {
                Err(self.error(intent, "has no declared default"))
            }
            _ => Ok(()),
        }
    }
    fn default_value(&self, intent: &FieldIntent<T>) -> Result<T, SpecError> {
        self.default
            .map(|f| f())
            .ok_or_else(|| self.error(intent, "has no declared default"))
    }
    fn clear_value(&self, intent: &FieldIntent<T>) -> Result<T, SpecError> {
        self.clear
            .map(|f| f())
            .ok_or_else(|| self.error(intent, "cannot be cleared"))
    }
    fn error(&self, intent: &FieldIntent<T>, reason: &str) -> SpecError {
        SpecError::new(
            SpecErrorCode::TypeMismatch,
            format!("field '{}' {reason}", self.name),
            intent.span(),
        )
    }
}

pub const PIN_NAME: FieldPolicy<String> = FieldPolicy {
    name: "pin.name",
    default: Some(String::new),
    clear: Some(String::new),
};
pub const PIN_HIDDEN_NET_NAME: FieldPolicy<String> = FieldPolicy {
    name: "pin.hidden_net_name",
    ..PIN_NAME
};
pub const SWAP_GROUP: FieldPolicy<String> = FieldPolicy {
    name: "swap_group",
    ..PIN_NAME
};
pub const PARAMETER_TEXT: FieldPolicy<String> = FieldPolicy {
    name: "parameter.text",
    ..PIN_NAME
};
pub const OPTIONAL_DESCRIPTION: FieldPolicy<Option<String>> = FieldPolicy {
    name: "description",
    default: Some(|| None),
    clear: Some(|| None),
};
pub const COMPONENT_DESIGNATOR: FieldPolicy<Option<String>> = FieldPolicy {
    name: "component.designator",
    ..OPTIONAL_DESCRIPTION
};
pub const PIN_LENGTH: FieldPolicy<Coord> = FieldPolicy {
    name: "pin.length",
    default: Some(|| Coord::from_mils_f64(25.0)),
    clear: None,
};
pub const PIN_LOCATION: FieldPolicy<CoordPoint> = FieldPolicy {
    name: "pin.location",
    default: Some(|| CoordPoint::new(Coord::ZERO, Coord::ZERO)),
    clear: None,
};
pub const PIN_ORIENTATION: FieldPolicy<RotationBy90> = FieldPolicy {
    name: "pin.orientation",
    default: Some(|| RotationBy90::Rotate0),
    clear: None,
};
pub const PIN_ELECTRICAL: FieldPolicy<PinElectricalType> = FieldPolicy {
    name: "pin.electrical",
    default: Some(|| PinElectricalType::Passive),
    clear: None,
};
pub const HIDDEN: FieldPolicy<bool> = FieldPolicy {
    name: "is_hidden",
    default: Some(|| false),
    clear: None,
};
pub const COMPONENT_KIND: FieldPolicy<ComponentKind> = FieldPolicy {
    name: "component_kind",
    default: Some(|| ComponentKind::Standard),
    clear: None,
};
/// Count normalization needs the final managed and preserved slots. It cannot be
/// replaced by a constant one when reset/inherit is authored.
pub fn part_count_policy(resulting_count: PartCount) -> PartCountPolicy {
    PartCountPolicy { resulting_count }
}
pub struct PartCountPolicy {
    resulting_count: PartCount,
}
impl PartCountPolicy {
    pub fn definition(&self, intent: &FieldIntent<PartCount>) -> Result<PartCount, SpecError> {
        match intent {
            FieldIntent::Set(value) => Ok(value.node),
            FieldIntent::Clear(span) => Err(SpecError::at(
                SpecErrorCode::TypeMismatch,
                "part_count cannot be cleared",
                *span,
            )),
            _ => Ok(self.resulting_count),
        }
    }
}

pub const PIN_LOCATION_X: FieldPolicy<Coord> = FieldPolicy {
    name: "pin.x",
    default: Some(|| Coord::ZERO),
    clear: None,
};
pub const PIN_LOCATION_Y: FieldPolicy<Coord> = FieldPolicy {
    name: "pin.y",
    ..PIN_LOCATION_X
};

/// Definition normalization and instance inheritance have distinct authority.
/// Existing target values are preserved only for absent fields/axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PinLocationContext {
    Definition {
        target: Option<CoordPoint>,
    },
    Instance {
        target: Option<CoordPoint>,
        base: Option<CoordPoint>,
    },
}

/// Resolve effective native location before applying any retained anchor layout.
/// This function returns values, not write scope: callers retain the authored
/// pair/axis operations when deciding which native coordinate axes may change.
pub fn resolve_pin_location(
    pin: &super::PinIntent,
    context: PinLocationContext,
) -> Result<CoordPoint, SpecError> {
    validate_pin_location(pin)?;
    let (target, base, instance) = match context {
        PinLocationContext::Definition { target } => (target, None, false),
        PinLocationContext::Instance { target, base } => (target, base, true),
    };
    if pin.location.is_authored() {
        return resolve_location_field(PIN_LOCATION, &pin.location, target, base, instance);
    }
    let x = resolve_location_field(
        PIN_LOCATION_X,
        &pin.location_x,
        target.map(|point| point.x),
        base.map(|point| point.x),
        instance,
    )?;
    let y = resolve_location_field(
        PIN_LOCATION_Y,
        &pin.location_y,
        target.map(|point| point.y),
        base.map(|point| point.y),
        instance,
    )?;
    Ok(CoordPoint::new(x, y))
}
fn resolve_location_field<T: Clone>(
    policy: FieldPolicy<T>,
    intent: &FieldIntent<T>,
    target: Option<T>,
    base: Option<T>,
    instance: bool,
) -> Result<T, SpecError> {
    if instance {
        policy.instance(intent, target.as_ref(), base.as_ref())
    } else if matches!(intent, FieldIntent::Absent) {
        match target {
            Some(value) => Ok(value),
            None => policy.definition(intent),
        }
    } else {
        policy.definition(intent)
    }
}

pub(crate) fn validate_pin_location(pin: &super::PinIntent) -> Result<(), SpecError> {
    let axis_span = pin.location_x.span().or(pin.location_y.span());
    if let (Some(pair), Some(axis)) = (pin.location.span(), axis_span) {
        return Err(SpecError::at(
            SpecErrorCode::TypeMismatch,
            format!(
                "pin at/location cannot be combined with x/y; pair authored at {}..{}",
                pair.start, pair.end
            ),
            axis,
        ));
    }
    if let Some(placement) = &pin.placement {
        if let Some(span) = axis_span.or(pin.location.span()) {
            return Err(SpecError::at(
                SpecErrorCode::TypeMismatch,
                format!(
                    "pin coordinates cannot be combined with anchored placement at {}..{}",
                    placement.span.start, placement.span.end
                ),
                span,
            ));
        }
    }
    Ok(())
}
