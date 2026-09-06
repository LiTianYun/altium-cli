//! Typed authored SchLib intent. Defaults are resolved only after write scope and
//! destination context are known; an absent field is never an implicit write.

mod compile;
mod fields;
mod graphics;
mod imports;

pub use compile::{compile_schlib, compile_schlib_with_scope};
pub use fields::*;
pub use graphics::*;
pub use imports::{CompiledSchLib, compile_captured_schlib};

use crate::ast::{BlockAnnotation, CollectionDelete, CollectionMode};
use crate::diagnostic::{Span, Spanned};
use altium_format_types::{ComponentKind, Coord, CoordPoint, PinElectricalType, RotationBy90};

/// The version of the default registry and authored-intent contract.
pub const INTENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum FieldIntent<T> {
    Absent,
    Set(Spanned<T>),
    Inherit(Span),
    Reset(Span),
    Clear(Span),
}
impl<T> Default for FieldIntent<T> {
    fn default() -> Self {
        Self::Absent
    }
}
impl<T> FieldIntent<T> {
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::Absent => None,
            Self::Set(value) => Some(value.span),
            Self::Inherit(span) | Self::Reset(span) | Self::Clear(span) => Some(*span),
        }
    }
    pub fn is_authored(&self) -> bool {
        !matches!(self, Self::Absent)
    }
}

/// Collection scope is independent of child field scope. Declaration absence is
/// preserved as well as its effective partial/deny permissions.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionScope {
    pub mode: CollectionMode,
    pub delete: CollectionDelete,
    pub span: Option<Span>,
}
impl Default for CollectionScope {
    fn default() -> Self {
        Self {
            mode: CollectionMode::Partial,
            delete: CollectionDelete::Deny,
            span: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Collection<T> {
    pub scope: CollectionScope,
    pub items: Vec<T>,
}
impl<T> Default for Collection<T> {
    fn default() -> Self {
        Self {
            scope: CollectionScope::default(),
            items: Vec::new(),
        }
    }
}

/// A positive numbered part slot, not a record offset or generated identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartNumber(u32);
impl PartNumber {
    pub fn new(number: i32) -> Option<Self> {
        (number > 0).then_some(Self(number as u32))
    }
    pub fn get(self) -> u32 {
        self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartCount(u32);
impl PartCount {
    pub fn new(count: i32) -> Option<Self> {
        (count > 0).then_some(Self(count as u32))
    }
    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchLibIntent {
    pub schema_version: u32,
    pub components: Collection<ComponentIntent>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentIntent {
    pub name: Spanned<String>,
    /// Source metadata only; never a cross-artifact execution binding.
    pub annotation: Option<Spanned<BlockAnnotation>>,
    pub designator: FieldIntent<Option<String>>,
    pub description: FieldIntent<Option<String>>,
    pub component_kind: FieldIntent<ComponentKind>,
    pub part_count: FieldIntent<PartCount>,
    pub show_hidden_pins: FieldIntent<bool>,
    /// Owner of new top-level children is chosen from effective resulting count,
    /// not eagerly assigned from the source's absent/default part_count.
    pub pins: Collection<PinIntent>,
    pub parameters: Collection<ParameterIntent>,
    pub aliases: Collection<Spanned<String>>,
    pub footprints: Collection<FootprintIntent>,
    pub graphics: Collection<GraphicIntent>,
    pub parts: Collection<PartIntent>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PartIntent {
    pub number: Spanned<PartNumber>,
    pub part_swap_group: FieldIntent<String>,
    pub pins: Collection<PinIntent>,
    pub graphics: Collection<GraphicIntent>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PinIntent {
    /// Desired designator; pins align positionally, not by this name.
    pub designator: Spanned<String>,
    pub name: FieldIntent<String>,
    pub electrical: FieldIntent<PinElectricalType>,
    pub length: FieldIntent<Coord>,
    pub location: FieldIntent<CoordPoint>,
    pub orientation: FieldIntent<RotationBy90>,
    pub is_hidden: FieldIntent<bool>,
    pub hidden_net_name: FieldIntent<String>,
    pub swap_group: FieldIntent<String>,
    pub part_swap_group: FieldIntent<String>,
    pub pair_swap_group: FieldIntent<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterIntent {
    pub name: Spanned<String>,
    pub text: FieldIntent<String>,
    pub is_hidden: FieldIntent<bool>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct FootprintIntent {
    /// Captured source provenance; never a native-object matching key.
    pub source: Option<SourceDefinitionRef>,
    pub model_name: Spanned<String>,
    pub description: FieldIntent<Option<String>>,
    pub maps: Collection<PinPadIntent>,
    /// Distinguishes an omitted map body from an explicitly authored empty body.
    pub explicit_maps: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDefinitionRef {
    pub source: std::path::PathBuf,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PinPadIntent {
    pub pin: Spanned<String>,
    pub pad: Spanned<String>,
}

#[cfg(test)]
mod tests;
