//! Typed SchLib child addresses bound to an exact input snapshot.
//!
//! An address names a child slot as (component, owner path, collection kind,
//! index). The owner path encodes the part slot; the index is a position in
//! the per-owner-group FILTERED projection of that collection, never a raw
//! record offset. Resolution validates every layer against one immutable
//! `SchLibSnapshot`.
//!
//! **Binding**: indices bind to an exact input snapshot. A resolved address is
//! meaningful only against snapshots whose complete typed state equals the
//! snapshot it was resolved with; after any replacement, patch, or reopen that
//! changes typed state, addresses must be re-resolved. No persistent identity
//! or hash is attached here — the transaction layer owns physical input
//! binding.
//!
//! Owner-path ↔ native mapping (resolutions memo
//! `docs/worklogs/next-child-contracts-2026-09-16.md` §1, adopted by plan A2):
//!
//! - `part N` maps to native `owner_part_id == N` directly (1-based effective
//!   numbering; the native/effective +1 offset lives only in the component's
//!   count field, never on child records).
//! - Component-scope children form their own owner group. For **pins**:
//!   effective part count 1 ⇒ the `Part(1)` group (native placement assigns
//!   the current part, which is 1); effective count > 1 ⇒ the natively defined
//!   shared-pin group `SharedPins` (owner_part_id 0). For **graphics**:
//!   effective count 1 ⇒ `Part(1)`; effective count > 1 ⇒ a hard error — no
//!   shared graphics owner exists natively (0 is not a wildcard for non-pins);
//!   author the graphic inside a part block.
//! - In a single-part component, `ComponentScope` and `Part(1)` therefore
//!   resolve to the same filtered projection; authoring-level constraints on
//!   which spelling is allowed belong to elaboration, not addressing.
//! - `OutOfRange` owner groups and owner-0 graphics are intent-unaddressable
//!   preserved content: no owner path reaches them, and their presence in a
//!   collection is invisible to the filtered index space.
//!
//! **Consumption status (step 2 → step 3)**: this vocabulary is landed and
//! unit-tested but NOT yet consumed by intent elaboration — saved plans
//! (NEXT.md step 3) are its intended consumer. Elaboration's scope targeting
//! lives independently in `crate::schlib::positional`
//! (`component_scope_pins_target` / `component_scope_graphics_target` /
//! `group_member`), which matches by RAW owner id — deliberately treating an
//! `OutOfRange(n)` group as a member of target `n` so a pre-growth projection
//! selects the members the post-growth projection will — while resolution
//! here filters by exact `SchOwnerGroup` equality and rejects parts beyond
//! the effective count, so the two coincide on every resolvable address.
//! When step 3 wires addresses into saved plans, either make one of the two
//! mappings delegate to the other or add a cross-implementation equivalence
//! test over the owner/effective-count matrix (including the OutOfRange
//! pre-growth convention).

use altium_format::api::{SchLibComponentSnapshot, SchLibSnapshot, SchOwnerGroup};
use altium_spec_lang::ast::CollectionKind;
use altium_spec_lang::intent::PartNumber;

/// Address resolution failures. Every variant names the offending layer.
#[derive(Debug, thiserror::Error)]
pub enum SchLibAddressError {
    #[error("component index {index} exceeds component count {count}")]
    ComponentOutOfRange { index: usize, count: usize },
    #[error("{context}: {source}")]
    Format {
        context: String,
        #[source]
        source: altium_format::AltiumFormatError,
    },
    #[error("part {part} exceeds effective part count {effective} of component '{component}'")]
    PartOutOfRange {
        component: String,
        part: u32,
        effective: i32,
    },
    #[error(
        "component '{component}' is multi-part; no shared graphics owner exists natively \
         (owner 0 is not a wildcard for non-pins); author the graphic inside a part block"
    )]
    NoSharedGraphicsOwner { component: String },
    #[error("{collection:?} children cannot be addressed under {owner:?}: {detail}")]
    OwnerMismatch {
        collection: CollectionKind,
        owner: SchLibChildOwner,
        detail: String,
    },
    #[error("footprint index {index} exceeds {count} PCBLIB footprints of component '{component}'")]
    FootprintOutOfRange {
        component: String,
        index: usize,
        count: usize,
    },
    #[error(
        "child index {index} exceeds {count} items in the {collection:?} projection under \
         {owner:?} of component '{component}'"
    )]
    IndexOutOfRange {
        component: String,
        owner: SchLibChildOwner,
        collection: CollectionKind,
        index: usize,
        count: usize,
    },
}

/// A validated component slot in one immutable snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentIndex(usize);

impl ComponentIndex {
    /// Validates a raw component position against the snapshot.
    pub fn resolve(snapshot: &SchLibSnapshot, index: usize) -> Result<Self, SchLibAddressError> {
        let count = snapshot.component_count();
        if index >= count {
            return Err(SchLibAddressError::ComponentOutOfRange { index, count });
        }
        Ok(Self(index))
    }

    pub fn get(self) -> usize {
        self.0
    }
}

/// A validated position in the per-owner-group filtered projection of one
/// collection. Produced only by address resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChildIndex(usize);

impl ChildIndex {
    pub fn get(self) -> usize {
        self.0
    }
}

/// A validated position in the PCBLIB footprint projection. Produced only by
/// address resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FootprintIndex(usize);

impl FootprintIndex {
    pub fn get(self) -> usize {
        self.0
    }
}

/// The owner path of a child collection inside one component. See the module
/// docs for the exact native mapping of `ComponentScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchLibChildOwner {
    /// Children authored at component scope (outside any `part` block).
    ComponentScope,
    /// Children authored inside `part N` (1-based effective numbering).
    Part(PartNumber),
    /// The pin-pad map collection under one footprint, addressed by its
    /// position in the PCBLIB footprint projection. Valid only for
    /// `CollectionKind::Maps`.
    Footprint(usize),
}

/// An authored (unvalidated) child address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchLibChildAddress {
    pub component: usize,
    pub owner: SchLibChildOwner,
    pub collection: CollectionKind,
    pub index: usize,
}

/// A snapshot-validated child address. Construction is only possible through
/// [`SchLibChildAddress::resolve`], so holding one proves every layer resolved
/// against the binding snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedSchLibChildAddress {
    component: ComponentIndex,
    owner: SchLibChildOwner,
    collection: CollectionKind,
    index: ChildIndex,
    /// Present exactly when `owner` is `Footprint(_)`.
    footprint: Option<FootprintIndex>,
}

impl ResolvedSchLibChildAddress {
    pub fn component(&self) -> ComponentIndex {
        self.component
    }
    pub fn owner(&self) -> SchLibChildOwner {
        self.owner
    }
    pub fn collection(&self) -> CollectionKind {
        self.collection
    }
    pub fn index(&self) -> ChildIndex {
        self.index
    }
    pub fn footprint(&self) -> Option<FootprintIndex> {
        self.footprint
    }
}

/// Maps a pin owner path to the owner group whose filtered projection the
/// index addresses (resolutions §1; see module docs). `effective` must be the
/// component's effective part count (≥ 1).
fn pin_owner_group(
    component: &str,
    owner: SchLibChildOwner,
    effective: i32,
) -> Result<SchOwnerGroup, SchLibAddressError> {
    match owner {
        SchLibChildOwner::ComponentScope => Ok(if effective == 1 {
            SchOwnerGroup::Part(1)
        } else {
            SchOwnerGroup::SharedPins
        }),
        SchLibChildOwner::Part(part) => part_owner_group(component, part, effective),
        SchLibChildOwner::Footprint(_) => Err(SchLibAddressError::OwnerMismatch {
            collection: CollectionKind::Pins,
            owner,
            detail: "pins live under the component or a part, not a footprint".to_owned(),
        }),
    }
}

/// Maps a graphics owner path to its owner group. Component-scope graphics
/// exist natively only in single-part components (resolutions §2).
fn graphic_owner_group(
    component: &str,
    owner: SchLibChildOwner,
    effective: i32,
) -> Result<SchOwnerGroup, SchLibAddressError> {
    match owner {
        SchLibChildOwner::ComponentScope => {
            if effective == 1 {
                Ok(SchOwnerGroup::Part(1))
            } else {
                Err(SchLibAddressError::NoSharedGraphicsOwner {
                    component: component.to_owned(),
                })
            }
        }
        SchLibChildOwner::Part(part) => part_owner_group(component, part, effective),
        SchLibChildOwner::Footprint(_) => Err(SchLibAddressError::OwnerMismatch {
            collection: CollectionKind::Graphics,
            owner,
            detail: "graphics live under the component or a part, not a footprint".to_owned(),
        }),
    }
}

fn part_owner_group(
    component: &str,
    part: PartNumber,
    effective: i32,
) -> Result<SchOwnerGroup, SchLibAddressError> {
    let number = part.get();
    if i32::try_from(number).is_ok_and(|number| number <= effective) {
        Ok(SchOwnerGroup::Part(number as i32))
    } else {
        Err(SchLibAddressError::PartOutOfRange {
            component: component.to_owned(),
            part: number,
            effective,
        })
    }
}

fn format_error(
    context: impl Into<String>,
) -> impl FnOnce(altium_format::AltiumFormatError) -> SchLibAddressError {
    let context = context.into();
    move |source| SchLibAddressError::Format { context, source }
}

fn require_component_scope(
    collection: CollectionKind,
    owner: SchLibChildOwner,
) -> Result<(), SchLibAddressError> {
    if matches!(owner, SchLibChildOwner::ComponentScope) {
        return Ok(());
    }
    Err(SchLibAddressError::OwnerMismatch {
        collection,
        owner,
        detail: format!("{collection:?} is a component-scope collection"),
    })
}

impl SchLibChildAddress {
    /// Resolves every layer of the address against `snapshot`, computing the
    /// per-owner-group filtered projection the index binds into. The returned
    /// address is valid exactly as long as the input snapshot's typed state.
    pub fn resolve(
        &self,
        snapshot: &SchLibSnapshot,
    ) -> Result<ResolvedSchLibChildAddress, SchLibAddressError> {
        let component_index = ComponentIndex::resolve(snapshot, self.component)?;
        let component = snapshot
            .component(self.component)
            .map_err(format_error(format!(
                "resolving input component #{}",
                self.component
            )))?;
        let name = component.lib_reference().to_owned();
        let mut footprint = None;
        let count = match self.collection {
            CollectionKind::Components => {
                return Err(SchLibAddressError::OwnerMismatch {
                    collection: self.collection,
                    owner: self.owner,
                    detail: "components are addressed by ComponentIndex, not child addresses"
                        .to_owned(),
                });
            }
            CollectionKind::Parts => {
                require_component_scope(self.collection, self.owner)?;
                let effective = self.effective_part_count(&component, &name)?;
                // A failed conversion would mean the effective_part_count()
                // "always >= 1 on success" contract was broken; surface that
                // as the internal bug it is instead of defaulting.
                usize::try_from(effective)
                    .expect("effective_part_count() contract guarantees >= 1 on success")
            }
            CollectionKind::Pins => {
                let effective = self.effective_part_count(&component, &name)?;
                let group = pin_owner_group(&name, self.owner, effective)?;
                component
                    .pins()
                    .iter()
                    .filter(|pin| pin.owner_group == group)
                    .count()
            }
            CollectionKind::Graphics => {
                let effective = self.effective_part_count(&component, &name)?;
                let group = graphic_owner_group(&name, self.owner, effective)?;
                component
                    .graphics()
                    .iter()
                    .filter(|graphic| graphic.owner_group == group)
                    .count()
            }
            CollectionKind::Parameters => {
                require_component_scope(self.collection, self.owner)?;
                component
                    .parameters()
                    .map_err(format_error(format!(
                        "projecting parameters of component '{name}'"
                    )))?
                    .len()
            }
            CollectionKind::Aliases => {
                require_component_scope(self.collection, self.owner)?;
                component.aliases().len()
            }
            CollectionKind::Footprints => {
                require_component_scope(self.collection, self.owner)?;
                component
                    .footprint_maps()
                    .map_err(format_error(format!(
                        "projecting footprints of component '{name}'"
                    )))?
                    .footprints
                    .len()
            }
            CollectionKind::Maps => {
                let SchLibChildOwner::Footprint(footprint_position) = self.owner else {
                    return Err(SchLibAddressError::OwnerMismatch {
                        collection: self.collection,
                        owner: self.owner,
                        detail: "pin-pad maps live under one footprint owner".to_owned(),
                    });
                };
                let maps = component.footprint_maps().map_err(format_error(format!(
                    "projecting footprints of component '{name}'"
                )))?;
                let Some(entry) = maps.footprints.get(footprint_position) else {
                    return Err(SchLibAddressError::FootprintOutOfRange {
                        component: name,
                        index: footprint_position,
                        count: maps.footprints.len(),
                    });
                };
                footprint = Some(FootprintIndex(footprint_position));
                entry.pin_pad_maps.len()
            }
        };
        if self.index >= count {
            return Err(SchLibAddressError::IndexOutOfRange {
                component: name,
                owner: self.owner,
                collection: self.collection,
                index: self.index,
                count,
            });
        }
        Ok(ResolvedSchLibChildAddress {
            component: component_index,
            owner: self.owner,
            collection: self.collection,
            index: ChildIndex(self.index),
            footprint,
        })
    }

    fn effective_part_count(
        &self,
        component: &SchLibComponentSnapshot,
        name: &str,
    ) -> Result<i32, SchLibAddressError> {
        component
            .effective_part_count()
            .map_err(format_error(format!(
                "resolving effective part count of component '{name}'"
            )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_format::SchLib;

    fn address(
        owner: SchLibChildOwner,
        collection: CollectionKind,
        index: usize,
    ) -> SchLibChildAddress {
        SchLibChildAddress {
            component: 0,
            owner,
            collection,
            index,
        }
    }

    fn part(number: i32) -> PartNumber {
        PartNumber::new(number).unwrap()
    }

    #[test]
    fn component_indices_validate_against_the_snapshot() {
        let snapshot = SchLib::new_blank_ad26().unwrap().snapshot().unwrap();
        assert_eq!(ComponentIndex::resolve(&snapshot, 0).unwrap().get(), 0);
        assert!(matches!(
            ComponentIndex::resolve(&snapshot, 1),
            Err(SchLibAddressError::ComponentOutOfRange { index: 1, count: 1 })
        ));
    }

    #[test]
    fn part_and_parameter_addresses_resolve_within_filtered_projections() {
        // The blank library: one component, effective part count 1, a root
        // Comment parameter, no pins/graphics/footprints/aliases.
        let snapshot = SchLib::new_blank_ad26().unwrap().snapshot().unwrap();

        let parts = address(SchLibChildOwner::ComponentScope, CollectionKind::Parts, 0)
            .resolve(&snapshot)
            .unwrap();
        assert_eq!(parts.component().get(), 0);
        assert_eq!(parts.index().get(), 0);
        assert_eq!(parts.footprint(), None);
        assert!(matches!(
            address(SchLibChildOwner::ComponentScope, CollectionKind::Parts, 1).resolve(&snapshot),
            Err(SchLibAddressError::IndexOutOfRange {
                index: 1,
                count: 1,
                ..
            })
        ));

        let parameter = address(
            SchLibChildOwner::ComponentScope,
            CollectionKind::Parameters,
            0,
        )
        .resolve(&snapshot)
        .unwrap();
        assert_eq!(parameter.collection(), CollectionKind::Parameters);
        assert!(matches!(
            address(
                SchLibChildOwner::Part(part(1)),
                CollectionKind::Parameters,
                0
            )
            .resolve(&snapshot),
            Err(SchLibAddressError::OwnerMismatch { .. })
        ));
    }

    #[test]
    fn empty_collections_part_bounds_and_owner_shapes_reject_with_named_layers() {
        let snapshot = SchLib::new_blank_ad26().unwrap().snapshot().unwrap();

        // Pins: single-part component-scope maps to the Part(1) group, whose
        // filtered projection is empty here.
        assert!(matches!(
            address(SchLibChildOwner::ComponentScope, CollectionKind::Pins, 0).resolve(&snapshot),
            Err(SchLibAddressError::IndexOutOfRange { count: 0, .. })
        ));
        // Part(1) is the aliased spelling of the same projection.
        assert!(matches!(
            address(SchLibChildOwner::Part(part(1)), CollectionKind::Pins, 0).resolve(&snapshot),
            Err(SchLibAddressError::IndexOutOfRange { count: 0, .. })
        ));
        assert!(matches!(
            address(SchLibChildOwner::Part(part(2)), CollectionKind::Pins, 0).resolve(&snapshot),
            Err(SchLibAddressError::PartOutOfRange {
                part: 2,
                effective: 1,
                ..
            })
        ));
        assert!(matches!(
            address(
                SchLibChildOwner::ComponentScope,
                CollectionKind::Graphics,
                0
            )
            .resolve(&snapshot),
            Err(SchLibAddressError::IndexOutOfRange { count: 0, .. })
        ));
        assert!(matches!(
            address(SchLibChildOwner::ComponentScope, CollectionKind::Aliases, 0)
                .resolve(&snapshot),
            Err(SchLibAddressError::IndexOutOfRange { count: 0, .. })
        ));
        assert!(matches!(
            address(
                SchLibChildOwner::ComponentScope,
                CollectionKind::Footprints,
                0
            )
            .resolve(&snapshot),
            Err(SchLibAddressError::IndexOutOfRange { count: 0, .. })
        ));
        assert!(matches!(
            address(SchLibChildOwner::Footprint(0), CollectionKind::Maps, 0).resolve(&snapshot),
            Err(SchLibAddressError::FootprintOutOfRange {
                index: 0,
                count: 0,
                ..
            })
        ));
        assert!(matches!(
            address(SchLibChildOwner::ComponentScope, CollectionKind::Maps, 0).resolve(&snapshot),
            Err(SchLibAddressError::OwnerMismatch { .. })
        ));
        assert!(matches!(
            address(SchLibChildOwner::Footprint(0), CollectionKind::Pins, 0).resolve(&snapshot),
            Err(SchLibAddressError::OwnerMismatch { .. })
        ));
        assert!(matches!(
            address(
                SchLibChildOwner::ComponentScope,
                CollectionKind::Components,
                0
            )
            .resolve(&snapshot),
            Err(SchLibAddressError::OwnerMismatch { .. })
        ));
        assert!(matches!(
            address(SchLibChildOwner::ComponentScope, CollectionKind::Pins, 0)
                .resolve(&SchLib::new_blank_ad26().unwrap().snapshot().unwrap()),
            Err(SchLibAddressError::IndexOutOfRange { .. })
        ));
    }

    #[test]
    fn multi_part_owner_mapping_is_shared_pins_for_pins_and_an_error_for_graphics() {
        // The mapping layer is pure over the effective count, so multi-part
        // behavior is testable without a multi-part fixture (the public API
        // cannot build one yet).
        assert_eq!(
            pin_owner_group("C", SchLibChildOwner::ComponentScope, 1).unwrap(),
            SchOwnerGroup::Part(1)
        );
        assert_eq!(
            pin_owner_group("C", SchLibChildOwner::ComponentScope, 2).unwrap(),
            SchOwnerGroup::SharedPins
        );
        assert_eq!(
            pin_owner_group("C", SchLibChildOwner::Part(part(2)), 2).unwrap(),
            SchOwnerGroup::Part(2)
        );
        assert!(matches!(
            pin_owner_group("C", SchLibChildOwner::Part(part(3)), 2),
            Err(SchLibAddressError::PartOutOfRange {
                part: 3,
                effective: 2,
                ..
            })
        ));

        assert_eq!(
            graphic_owner_group("C", SchLibChildOwner::ComponentScope, 1).unwrap(),
            SchOwnerGroup::Part(1)
        );
        let error = graphic_owner_group("C", SchLibChildOwner::ComponentScope, 2).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("author the graphic inside a part block"),
            "{error}"
        );
        assert_eq!(
            graphic_owner_group("C", SchLibChildOwner::Part(part(1)), 2).unwrap(),
            SchOwnerGroup::Part(1)
        );
    }
}
