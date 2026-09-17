//! Positional (per-owner-group) elaboration of pins, graphics, parts, layout,
//! and part swap fan-out (step-2 increment E2).
//!
//! # Owner-group scoping (contracts memo §1/§2, consumed via the A2 views)
//!
//! Positional collections align WITHIN one owner group's filtered projection,
//! never across the whole `pins()`/`graphics()` sequence:
//!
//! - a `part N` block addresses the `Part(N)` group;
//! - component-scope pins address `Part(1)` on a single-part component and
//!   `SharedPins` (owner 0) on a multi-part one — an intent that moves the
//!   component across the single/multi boundary while managing component-scope
//!   pins is a hard error (the mapping would silently retarget the authored
//!   items between groups mid-patch); a CREATED component maps by its
//!   resulting count alone (there are no existing pins to retarget);
//! - component-scope graphics address `Part(1)` on a single-part component;
//!   on a multi-part one (destination or resulting) they are a hard error —
//!   no shared graphics owner exists natively (memo §2);
//! - two scopes addressing the same group (e.g. component scope and a
//!   `part 1` block on a single-part component) are a hard error.
//!
//! Authored item K describes FILTERED index K of its group; the mapping to
//! full-projection indices (the recorded B/C obligation) is the group's
//! member-index list, injective by construction. Partial scope updates the
//! addressed prefix and appends overflow items (owner fields forced to the
//! group's owner id); complete scope defines the final membership/order of
//! THAT group only — members of every other group, including the
//! intent-unaddressable `PartIndependent`/`OutOfRange` owners and owner-0
//! graphics (memo §1), keep their slots verbatim. Those unaddressable owners
//! can never appear inside a targetable group's filtered projection
//! (classification puts them in their own groups), so the only
//! intent-unrepresentable members a scope can meet are unrepresentable
//! graphic KINDS (Symbol/Hyperlink/other): a complete scope over a group
//! containing one always blocks, a partial scope blocks only when it would
//! address one (tail preservation is fine), and a positional replacement
//! whose authored kind differs from the destination kind at that position
//! blocks naming both kinds.
//!
//! # Field semantics
//!
//! Unauthored fields preserve the destination value (creation resolves the
//! registry default); authored fields elaborate through their declared
//! policies. `inherit` on a native pin/graphic/part field is a hard error:
//! it requires an imported instance base, and definition-scope SchLib
//! elaboration has none — instance semantics land in step 7 (SchDoc scope).
//! Layout metadata fields (anchor gap/side/offset, auto-size padding and
//! minima) keep their documented `inherit ≡ reset ≡ versioned default`
//! resolution from the anchors module. Swap halves follow memo §5 rule 3:
//! an unauthored prefix leaves the stored combined string byte-verbatim; an
//! authored prefix re-encodes through the native rule with the stored suffix
//! preserved through the split. `hidden_net_name` may only elaborate to the
//! stored value or the empty default (the C restriction: the binary pin body
//! has no hidden-net field and the pin-owned Parameter storage form is
//! preserved-only) — an unsatisfiable `Set` hard-errors at elaboration.
//!
//! # Parts
//!
//! Parts are the keyed structural collection (numbers are the keys). Partial
//! scope: the resulting effective count is
//! `max(destination, highest authored part number)`, an explicit `part_count`
//! must accommodate it, and lowering is inexpressible. Complete scope: the
//! authored numbers must be exactly the contiguous prefix `1..=n` (re-checked
//! here even though the compiler validates it), `part_count` must agree, at
//! least one slot must remain, and `n < destination` is a trailing shrink
//! requiring `delete: allow` — lowered to `with_parts_removed` with the
//! descendant-deletion authorization, its typed report surfaced through
//! `SchLibPatch::parts_shrinks`. Growth lowers to `with_parts_appended`
//! before any child batch resolves, so new-slot children validate against
//! the grown count (and previously out-of-range owners reclassify into the
//! new numbered groups — raw classification, never a record edit).
//!
//! `PartIntent::part_swap_group` fans out per memo §5 rule 2: every
//! non-owner-0 pin of that part in the FINAL pin sequence gets the target
//! prefix (suffix preserved); pins already carrying it stay byte-verbatim.
//! A fan-out against a destination `Mixed` projection (the `parts()` view)
//! is a hard block, and a pin-level authored `part_swap_group` that
//! contradicts its part's fan-out target is a hard error.
use std::collections::BTreeMap;

use altium_format::AltiumFormatError;
use altium_format::api::{
    SchGraphicDetail, SchGraphicEdit, SchGraphicInit, SchGraphicShape, SchGraphicView,
    SchLibComponentSnapshot, SchOwnerGroup, SchPartSwapGroup, SchPinEdit, SchPinInit, SchPinView,
};
use altium_format_types::{
    Color, Coord, CoordPoint, HorizontalAlign, LineShape, LineStyle, PenWidth, RotationBy90,
    TextJustification,
};
use altium_spec_lang::ast::{CollectionDelete, CollectionKind, CollectionMode};
use altium_spec_lang::intent::{
    BoxGeometry, Collection, ComponentIntent, EffectivePinGeometry, FieldIntent, FieldPolicy,
    GraphicIntent, GraphicKind, GraphicSourceRef, HIDDEN, ImageAssetSource, LayoutOwner,
    PIN_ELECTRICAL, PIN_HIDDEN_NET_NAME, PIN_LENGTH, PIN_NAME, PIN_ORIENTATION, PartCount,
    PinIntent, PinLocationContext, SWAP_GROUP, evaluate_layout, part_count_policy,
    resolve_pin_location,
};

use crate::collection::{
    CollectionContext, CollectionError, CollectionRequest, FinalItem, IndexedEdit, PlannedItem,
    plan_collection,
};

use super::SchLibPatchError;

const PIN_PART_SWAP: FieldPolicy<String> = FieldPolicy {
    name: "pin.part_swap_group",
    default: Some(String::new),
    clear: Some(String::new),
};
const PIN_PAIR_SWAP: FieldPolicy<String> = FieldPolicy {
    name: "pin.pair_swap_group",
    ..PIN_PART_SWAP
};
const PART_SWAP: FieldPolicy<String> = FieldPolicy {
    name: "part.part_swap_group",
    ..PIN_PART_SWAP
};

fn collection_error(error: CollectionError) -> SchLibPatchError {
    SchLibPatchError::Collection {
        detail: error.to_string(),
    }
}

fn format_error(context: String) -> impl FnOnce(AltiumFormatError) -> SchLibPatchError {
    move |source| SchLibPatchError::Format { context, source }
}

fn elaboration(context: impl Into<String>, detail: impl Into<String>) -> SchLibPatchError {
    SchLibPatchError::Elaboration {
        context: context.into(),
        detail: detail.into(),
    }
}

/// The step-7 boundary: `inherit` on a native field needs an imported
/// instance base, which definition-scope SchLib elaboration does not have.
fn inherit_error(context: &str, field: &str) -> SchLibPatchError {
    elaboration(
        context,
        format!(
            "field '{field}' authors 'inherit', which resolves against an imported instance \
             base; definition-scope SchLib elaboration has no base symbol — imported-base \
             instance semantics land in step 7 (SchDoc scope)"
        ),
    )
}

fn managed<T>(collection: &Collection<T>) -> bool {
    !collection.items.is_empty() || collection.scope.span.is_some()
}

/// Whether a projected member belongs to the raw owner-id `target` group.
/// Matching is by RAW owner id so a pre-growth projection (where a future
/// part slot still classifies `OutOfRange`) selects the same members the
/// post-growth projection will.
fn group_member(group: SchOwnerGroup, target: i32) -> bool {
    match group {
        SchOwnerGroup::SharedPins => target == 0,
        SchOwnerGroup::Part(n) | SchOwnerGroup::OutOfRange(n) => n == target,
        SchOwnerGroup::PartIndependent => false,
    }
}

// ── Parts structure ─────────────────────────────────────────────────────────

/// The structural outcome of the parts collection + `part_count` field.
#[derive(Debug)]
pub(super) struct PartsResolution {
    pub resulting_effective: i32,
    /// Effective slots to append (`with_parts_appended`).
    pub growth: i32,
    /// Trailing effective slots to remove (`with_parts_removed`), ascending.
    pub removed_slots: Vec<i32>,
}

fn part_numbers(owner: &str, intent: &ComponentIntent) -> Result<Vec<i32>, SchLibPatchError> {
    let context = format!("component '{owner}' Parts");
    let mut numbers = Vec::new();
    for (position, item) in intent.parts.items.iter().enumerate() {
        let number = i32::try_from(item.number.node.get()).map_err(|_| {
            elaboration(
                &context,
                format!(
                    "authored part number {} at entry #{position} exceeds the representable \
                     native range",
                    item.number.node.get()
                ),
            )
        })?;
        if let Some(previous) = numbers.iter().position(|existing| *existing == number) {
            return Err(elaboration(
                &context,
                format!(
                    "part {number} is authored more than once (entries #{previous} and \
                     #{position}); part numbers are the collection's keys"
                ),
            ));
        }
        numbers.push(number);
    }
    Ok(numbers)
}

pub(super) fn resolve_parts_structure(
    owner: &str,
    destination_effective: i32,
    intent: &ComponentIntent,
) -> Result<PartsResolution, SchLibPatchError> {
    let context = format!("component '{owner}' Parts");
    let numbers = part_numbers(owner, intent)?;
    let spec_error = |source| SchLibPatchError::Spec {
        context: format!("component '{owner}' part_count"),
        source,
    };
    let count_value = |resulting: i32| -> Result<i32, SchLibPatchError> {
        let resulting_count = PartCount::new(resulting).ok_or_else(|| {
            elaboration(
                &context,
                format!("resulting effective part count {resulting} is not positive"),
            )
        })?;
        let value = part_count_policy(resulting_count)
            .definition(&intent.part_count)
            .map_err(spec_error)?;
        i32::try_from(value.get()).map_err(|_| {
            elaboration(
                &context,
                format!(
                    "authored part_count {} exceeds the representable native range",
                    value.get()
                ),
            )
        })
    };
    match intent.parts.scope.mode {
        CollectionMode::Partial => {
            let highest = numbers.iter().copied().max().unwrap_or(0);
            let implied = destination_effective.max(highest);
            let value = count_value(implied)?;
            if value < implied {
                return Err(elaboration(
                    &context,
                    format!(
                        "explicit part_count {value} cannot accommodate the authored and \
                         preserved parts (destination effective {destination_effective}, \
                         highest authored part {highest}); lowering the count requires a \
                         complete parts scope with delete: allow"
                    ),
                ));
            }
            Ok(PartsResolution {
                resulting_effective: value,
                growth: value - destination_effective,
                removed_slots: Vec::new(),
            })
        }
        CollectionMode::Complete => {
            let n = i32::try_from(numbers.len()).unwrap_or(i32::MAX);
            if n == 0 {
                return Err(elaboration(
                    &context,
                    "a complete parts scope must retain at least one effective part slot",
                ));
            }
            let max = numbers.iter().copied().max().unwrap_or(0);
            if max != n {
                return Err(elaboration(
                    &context,
                    format!(
                        "complete parts scope must author the contiguous prefix 1..={n}, but \
                         the highest authored part is {max}; omitting an interior slot while \
                         retaining a higher one is not expressible"
                    ),
                ));
            }
            let value = count_value(n)?;
            if value != n {
                return Err(elaboration(
                    &context,
                    format!(
                        "explicit part_count {value} disagrees with the complete parts scope \
                         authoring {n} part(s)"
                    ),
                ));
            }
            if n < destination_effective && intent.parts.scope.delete != CollectionDelete::Allow {
                return Err(elaboration(
                    &context,
                    format!(
                        "complete parts scope omits trailing slots \
                         {}..={destination_effective}; removing them (and their descendants) \
                         requires explicit collection delete: allow",
                        n + 1
                    ),
                ));
            }
            Ok(PartsResolution {
                resulting_effective: n,
                growth: (n - destination_effective).max(0),
                removed_slots: ((n + 1)..=destination_effective).collect(),
            })
        }
    }
}

// ── Scope targeting ─────────────────────────────────────────────────────────

fn component_scope_pins_target(
    owner: &str,
    destination_effective: i32,
    resulting_effective: i32,
    created: bool,
) -> Result<i32, SchLibPatchError> {
    let resulting_multi = resulting_effective > 1;
    if created {
        // No existing pins can be retargeted; map by the resulting count.
        return Ok(if resulting_multi { 0 } else { 1 });
    }
    let destination_multi = destination_effective > 1;
    if destination_multi != resulting_multi {
        return Err(elaboration(
            format!("component '{owner}' Pins"),
            format!(
                "component-scope pins map to Part(1) on a single-part component and to the \
                 shared owner 0 on a multi-part one, but this intent changes the effective \
                 part count from {destination_effective} to {resulting_effective} across \
                 that boundary — the mapping would silently retarget the authored items; \
                 author the pins inside part blocks, or split the count change into its own \
                 patch"
            ),
        ));
    }
    Ok(if resulting_multi { 0 } else { 1 })
}

fn component_scope_graphics_target(
    owner: &str,
    destination_effective: i32,
    resulting_effective: i32,
    created: bool,
) -> Result<i32, SchLibPatchError> {
    let multi = resulting_effective > 1 || (!created && destination_effective > 1);
    if multi {
        return Err(elaboration(
            format!("component '{owner}' Graphics"),
            "no shared graphics owner exists natively on a multi-part component (owner 0 is \
             a pins-only wildcard, memo §1/§2); author the graphic inside a part block",
        ));
    }
    Ok(1)
}

// ── Image asset source discovery (pre-pass; consumed by elaborate.rs) ──────

/// One authored image-asset source with its resolved storage resource name.
pub(super) struct AuthoredImageSource<'a> {
    pub context: String,
    pub source: &'a ImageAssetSource,
    pub resource_name: String,
}

/// Collects every authored `source:` on the intent's image graphics with the
/// storage resource name it binds to: the authored `file_name` when present,
/// else the positionally addressed destination image's stored name. Pure —
/// no filesystem access; the caller resolves the captured bytes.
pub(super) fn collect_authored_image_sources<'a>(
    owner: &str,
    destination: Option<&SchLibComponentSnapshot>,
    intent: &'a ComponentIntent,
) -> Result<Vec<AuthoredImageSource<'a>>, SchLibPatchError> {
    let destination_effective = match destination {
        Some(component) => component
            .effective_part_count()
            .map_err(format_error(format!(
                "resolving the effective part count of component '{owner}'"
            )))?,
        // The Blank creation factory shape has exactly one effective part.
        None => 1,
    };
    let resolution = resolve_parts_structure(owner, destination_effective, intent)?;
    let scopes = graphic_scopes(
        owner,
        intent,
        destination_effective,
        resolution.resulting_effective,
        destination.is_none(),
    )?;
    let full = destination.map(SchLibComponentSnapshot::graphics);
    let mut authored = Vec::new();
    for scope in &scopes {
        let filtered: Vec<usize> = full
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter(|(_, view)| group_member(view.owner_group, scope.target))
            .map(|(index, _)| index)
            .collect();
        for (position, item) in scope.collection.items.iter().enumerate() {
            let FieldIntent::Set(spanned) = &item.fields.source else {
                continue;
            };
            let Some(source) = spanned.node.as_ref() else {
                continue;
            };
            let context = format!("{} graphics entry #{position}", scope.desc);
            let resource_name = if item.fields.file_name.is_authored() {
                let policies = item.kind.node.field_policies();
                let policy = policies.file_name.as_ref().ok_or_else(|| {
                    elaboration(
                        &context,
                        format!(
                            "property 'file_name' does not apply to {:?} graphics",
                            item.kind.node
                        ),
                    )
                })?;
                let name = policy
                    .definition(&item.fields.file_name)
                    .map_err(|source| SchLibPatchError::Spec {
                        context: format!("{context} file_name"),
                        source,
                    })?;
                if name.is_empty() {
                    return Err(elaboration(
                        &context,
                        "an authored image source needs a non-empty file_name to name its \
                         storage resource; resource identity is never derived from the \
                         bundle path",
                    ));
                }
                name
            } else {
                let stored = filtered.get(position).and_then(|full_index| {
                    let view = &full.as_deref().unwrap_or(&[])[*full_index];
                    match &view.detail {
                        SchGraphicDetail::Image { file_name, .. } if !file_name.is_empty() => {
                            Some((*file_name).to_owned())
                        }
                        _ => None,
                    }
                });
                stored.ok_or_else(|| {
                    elaboration(
                        &context,
                        "an authored image source needs a resolvable storage resource name: \
                         author file_name, or address an existing embedded image whose \
                         stored file_name is non-empty",
                    )
                })?
            };
            authored.push(AuthoredImageSource {
                context,
                source,
                resource_name,
            });
        }
    }
    Ok(authored)
}

// ── Owner units ─────────────────────────────────────────────────────────────

struct GraphicScope<'a> {
    desc: String,
    layout_owner: LayoutOwner,
    collection: &'a Collection<GraphicIntent>,
    target: i32,
}

fn graphic_scopes<'a>(
    owner: &str,
    intent: &'a ComponentIntent,
    destination_effective: i32,
    resulting_effective: i32,
    created: bool,
) -> Result<Vec<GraphicScope<'a>>, SchLibPatchError> {
    let mut scopes: Vec<GraphicScope<'a>> = Vec::new();
    let mut claim =
        |desc: String, layout_owner, collection, target| -> Result<(), SchLibPatchError> {
            if let Some(previous) = scopes.iter().find(|scope| scope.target == target) {
                return Err(elaboration(
                    format!("component '{owner}' Graphics"),
                    format!(
                        "both {} and {desc} address the Part({target}) graphics group; a group \
                     may be managed by exactly one scope",
                        previous.desc
                    ),
                ));
            }
            scopes.push(GraphicScope {
                desc,
                layout_owner,
                collection,
                target,
            });
            Ok(())
        };
    if managed(&intent.graphics) {
        let target = component_scope_graphics_target(
            owner,
            destination_effective,
            resulting_effective,
            created,
        )?;
        claim(
            format!("component '{owner}' (component scope)"),
            LayoutOwner::Component,
            &intent.graphics,
            target,
        )?;
    }
    let mut parts: Vec<&altium_spec_lang::intent::PartIntent> = intent.parts.items.iter().collect();
    parts.sort_by_key(|item| item.number.node.get());
    for part in parts {
        if !managed(&part.graphics) {
            continue;
        }
        let number = i32::try_from(part.number.node.get()).expect("checked by resolve_parts");
        claim(
            format!("component '{owner}' part {number}"),
            LayoutOwner::Part(part.number.node),
            &part.graphics,
            number,
        )?;
    }
    Ok(scopes)
}

struct PinScope<'a> {
    desc: String,
    layout_owner: LayoutOwner,
    collection: &'a Collection<PinIntent>,
    target: i32,
}

fn pin_scopes<'a>(
    owner: &str,
    intent: &'a ComponentIntent,
    destination_effective: i32,
    resulting_effective: i32,
    created: bool,
) -> Result<Vec<PinScope<'a>>, SchLibPatchError> {
    let mut scopes: Vec<PinScope<'a>> = Vec::new();
    let mut claim =
        |desc: String, layout_owner, collection, target| -> Result<(), SchLibPatchError> {
            if let Some(previous) = scopes.iter().find(|scope| scope.target == target) {
                let group = if target == 0 {
                    "shared (owner 0)".to_owned()
                } else {
                    format!("Part({target})")
                };
                return Err(elaboration(
                    format!("component '{owner}' Pins"),
                    format!(
                        "both {} and {desc} address the {group} pins group; a group may be \
                     managed by exactly one scope",
                        previous.desc
                    ),
                ));
            }
            scopes.push(PinScope {
                desc,
                layout_owner,
                collection,
                target,
            });
            Ok(())
        };
    if managed(&intent.pins) {
        let target = component_scope_pins_target(
            owner,
            destination_effective,
            resulting_effective,
            created,
        )?;
        claim(
            format!("component '{owner}' (component scope)"),
            LayoutOwner::Component,
            &intent.pins,
            target,
        )?;
    }
    let mut parts: Vec<&altium_spec_lang::intent::PartIntent> = intent.parts.items.iter().collect();
    parts.sort_by_key(|item| item.number.node.get());
    for part in parts {
        if !managed(&part.pins) {
            continue;
        }
        let number = i32::try_from(part.number.node.get()).expect("checked by resolve_parts");
        claim(
            format!("component '{owner}' part {number}"),
            LayoutOwner::Part(part.number.node),
            &part.pins,
            number,
        )?;
    }
    Ok(scopes)
}

// ── Pin elaboration ─────────────────────────────────────────────────────────

/// The byte-preserving baseline for a positional Replace: every view value
/// copied, every "unaddressed" optional left `None` so a value we do not
/// write is neither re-encoded nor re-validated.
fn preserving_pin_init(view: &SchPinView<'_>) -> Result<SchPinInit, AltiumFormatError> {
    let mut init = SchPinInit::from_view(view)?;
    init.owner_part_display_mode = None;
    init.part_swap_group = None;
    init.part_pin_swap_suffix = None;
    Ok(init)
}

fn check_pin_inherit(context: &str, item: &PinIntent) -> Result<(), SchLibPatchError> {
    macro_rules! check_all {
        ($($field:ident),* $(,)?) => {{
            // Exhaustive destructure (no `..`): a `FieldIntent` axis added to
            // `PinIntent` fails this build until its step-7 inherit boundary
            // is decided here. `placement` (layout metadata with documented
            // inherit≡reset semantics, resolved by the anchors module) and
            // `designator` (the positional alignment key, not a FieldIntent)
            // are the deliberate exceptions.
            let PinIntent { placement: _, designator: _, $($field),* } = item;
            $(
                if matches!($field, FieldIntent::Inherit(_)) {
                    return Err(inherit_error(context, concat!("pin.", stringify!($field))));
                }
            )*
        }};
    }
    check_all!(
        name,
        electrical,
        length,
        location,
        location_x,
        location_y,
        orientation,
        is_hidden,
        hidden_net_name,
        swap_group,
        part_swap_group,
        pair_swap_group,
    );
    Ok(())
}

fn elaborate_pin_init(
    context: &str,
    item: &PinIntent,
    existing: Option<&SchPinView<'_>>,
    owner_part_id: i32,
) -> Result<SchPinInit, SchLibPatchError> {
    check_pin_inherit(context, item)?;
    let spec_error = |field: &'static str| {
        let context = format!("{context} {field}");
        move |source| SchLibPatchError::Spec { context, source }
    };
    let creation = existing.is_none();
    let name = if item.name.is_authored() || creation {
        PIN_NAME
            .definition(&item.name)
            .map_err(spec_error("name"))?
    } else {
        existing
            .map(|view| view.name.to_owned())
            .unwrap_or_default()
    };
    let electrical = if item.electrical.is_authored() || creation {
        PIN_ELECTRICAL
            .definition(&item.electrical)
            .map_err(spec_error("electrical"))?
    } else {
        existing.expect("not creation").electrical
    };
    let length = if item.length.is_authored() || creation {
        PIN_LENGTH
            .definition(&item.length)
            .map_err(spec_error("length"))?
    } else {
        existing.expect("not creation").length
    };
    // Pair/axis/placement mutual exclusion and independent x/y preservation
    // both live inside resolve_pin_location; the unauthored axes of an
    // existing pin come back from the target.
    let location = resolve_pin_location(
        item,
        PinLocationContext::Definition {
            target: existing.map(|view| view.location),
        },
    )
    .map_err(spec_error("location"))?;
    let orientation = if item.orientation.is_authored() || creation {
        PIN_ORIENTATION
            .definition(&item.orientation)
            .map_err(spec_error("orientation"))?
    } else {
        existing.expect("not creation").orientation
    };
    let is_hidden = if item.is_hidden.is_authored() || creation {
        HIDDEN
            .definition(&item.is_hidden)
            .map_err(spec_error("is_hidden"))?
    } else {
        existing.expect("not creation").is_hidden
    };
    let hidden_net_name = if item.hidden_net_name.is_authored() || creation {
        let value = PIN_HIDDEN_NET_NAME
            .definition(&item.hidden_net_name)
            .map_err(spec_error("hidden_net_name"))?;
        let stored = existing.map(|view| view.hidden_net_name);
        if !value.is_empty() && stored != Some(value.as_str()) {
            return Err(elaboration(
                context,
                format!(
                    "hidden_net_name '{value}' cannot be elaborated: the SchLib binary pin \
                     body carries no hidden-net field and the pin-owned Parameter storage \
                     form is preserved-only (authoring unproven, increment-C restriction); \
                     only the stored value ({stored:?}) or the empty default is \
                     satisfiable"
                ),
            ));
        }
        value
    } else {
        existing
            .map(|view| view.hidden_net_name.to_owned())
            .unwrap_or_default()
    };
    let swap_group = if item.swap_group.is_authored() || creation {
        SWAP_GROUP
            .definition(&item.swap_group)
            .map_err(spec_error("swap_group"))?
    } else {
        existing
            .map(|view| view.swap_group.to_owned())
            .unwrap_or_default()
    };
    // Memo §5 rule 3: an unauthored prefix leaves the stored combined string
    // byte-verbatim (`None`); an authored prefix addresses that half and
    // re-encodes with the stored suffix preserved through the split. On New
    // both halves default to empty.
    let part_swap_group = if item.part_swap_group.is_authored() {
        Some(
            PIN_PART_SWAP
                .definition(&item.part_swap_group)
                .map_err(spec_error("part_swap_group"))?,
        )
    } else {
        None
    };
    let pair_swap_group = if item.pair_swap_group.is_authored() || creation {
        PIN_PAIR_SWAP
            .definition(&item.pair_swap_group)
            .map_err(spec_error("pair_swap_group"))?
    } else {
        existing
            .map(|view| view.pair_swap_group.to_owned())
            .unwrap_or_default()
    };
    Ok(SchPinInit {
        owner_part_id,
        owner_part_display_mode: None,
        designator: item.designator.node.clone(),
        name,
        electrical,
        length,
        location,
        orientation,
        is_hidden,
        hidden_net_name,
        swap_group,
        part_swap_group,
        part_pin_swap_suffix: None,
        pair_swap_group,
    })
}

// ── Graphic elaboration ─────────────────────────────────────────────────────

fn graphic_kind_name(kind: GraphicKind) -> &'static str {
    match kind {
        GraphicKind::Line => "Line",
        GraphicKind::Rectangle => "Rectangle",
        GraphicKind::Arc => "Arc",
        GraphicKind::EllipticalArc => "EllipticalArc",
        GraphicKind::Ellipse => "Ellipse",
        GraphicKind::Polyline => "Polyline",
        GraphicKind::Polygon => "Polygon",
        GraphicKind::Bezier => "Bezier",
        GraphicKind::Pie => "Pie",
        GraphicKind::RoundRectangle => "RoundRectangle",
        GraphicKind::Label => "Label",
        GraphicKind::TextFrame => "TextFrame",
        GraphicKind::Image => "Image",
    }
}

macro_rules! for_each_graphic_field {
    ($fields:expr, $apply:ident) => {
        for_each_graphic_field!(
            @impl $fields, $apply,
            from, to, center, at, radius, secondary_radius, corner_x_radius,
            corner_y_radius, start_angle, end_angle, vertices, color,
            area_color, text_color, line_width, line_style, start_line_shape,
            end_line_shape, line_shape_size, is_solid, transparent,
            show_border, text, font_id, file_name, source, orientation,
            justification, alignment, is_mirrored, url, word_wrap,
            clip_to_rect, text_margin, keep_aspect, embed_image
        )
    };
    (@impl $fields:expr, $apply:ident, $($field:ident),*) => {{
        // Exhaustive destructure (no `..`): the one field list above drives
        // BOTH the bindings and the per-field applications, so a field added
        // to spec-lang's `graphic_fields!` fails this build until it is
        // handled here — an authored value can never be silently invisible
        // to elaboration.
        let altium_spec_lang::intent::GraphicFields { $($field),* } = $fields;
        $($apply!($field);)*
    }};
}

/// Rejects authored fields the kind has no policy for (the compile-time
/// `UnknownProperty` rule, re-checked for hand-built intents) and `inherit`
/// on any native graphic field.
fn check_graphic_fields(
    context: &str,
    kind: GraphicKind,
    fields: &altium_spec_lang::intent::GraphicFields,
) -> Result<(), SchLibPatchError> {
    let policies = kind.field_policies();
    macro_rules! check {
        ($field:ident) => {
            if matches!($field, FieldIntent::Inherit(_)) {
                return Err(inherit_error(
                    context,
                    concat!("graphic.", stringify!($field)),
                ));
            }
            if $field.is_authored() && policies.$field.is_none() {
                return Err(elaboration(
                    context,
                    format!(
                        "property '{}' does not apply to {:?} graphics",
                        stringify!($field),
                        kind
                    ),
                ));
            }
        };
    }
    for_each_graphic_field!(fields, check);
    Ok(())
}

/// The destination values of one projected graphic, spread into per-field
/// options so a single resolution macro serves all thirteen kinds.
#[derive(Default)]
struct DestGraphic {
    from: Option<CoordPoint>,
    to: Option<CoordPoint>,
    center: Option<CoordPoint>,
    at: Option<CoordPoint>,
    radius: Option<Coord>,
    secondary_radius: Option<Coord>,
    corner_x_radius: Option<Coord>,
    corner_y_radius: Option<Coord>,
    start_angle: Option<f64>,
    end_angle: Option<Option<f64>>,
    vertices: Option<Vec<CoordPoint>>,
    color: Option<Color>,
    area_color: Option<Color>,
    text_color: Option<Color>,
    line_width: Option<PenWidth>,
    line_style: Option<LineStyle>,
    start_line_shape: Option<LineShape>,
    end_line_shape: Option<LineShape>,
    line_shape_size: Option<PenWidth>,
    is_solid: Option<bool>,
    transparent: Option<bool>,
    show_border: Option<bool>,
    text: Option<String>,
    stored_font_id: Option<i32>,
    file_name: Option<String>,
    orientation: Option<RotationBy90>,
    justification: Option<TextJustification>,
    alignment: Option<HorizontalAlign>,
    is_mirrored: Option<bool>,
    url: Option<String>,
    word_wrap: Option<bool>,
    clip_to_rect: Option<bool>,
    text_margin: Option<Coord>,
    keep_aspect: Option<bool>,
    embed_image: Option<bool>,
}

fn dest_graphic(detail: &SchGraphicDetail<'_>) -> DestGraphic {
    let mut dest = DestGraphic::default();
    match detail {
        SchGraphicDetail::Line {
            from,
            to,
            line_width,
            line_style,
            color,
        } => {
            dest.from = Some(*from);
            dest.to = Some(*to);
            dest.line_width = Some(*line_width);
            dest.line_style = Some(*line_style);
            dest.color = Some(*color);
        }
        SchGraphicDetail::Rectangle {
            from,
            to,
            line_width,
            line_style,
            color,
            area_color,
            is_solid,
            transparent,
        } => {
            dest.from = Some(*from);
            dest.to = Some(*to);
            dest.line_width = Some(*line_width);
            dest.line_style = Some(*line_style);
            dest.color = Some(*color);
            dest.area_color = Some(*area_color);
            dest.is_solid = Some(*is_solid);
            dest.transparent = Some(*transparent);
        }
        SchGraphicDetail::RoundRectangle {
            from,
            to,
            corner_x_radius,
            corner_y_radius,
            line_width,
            color,
            area_color,
            is_solid,
        } => {
            dest.from = Some(*from);
            dest.to = Some(*to);
            dest.corner_x_radius = Some(*corner_x_radius);
            dest.corner_y_radius = Some(*corner_y_radius);
            dest.line_width = Some(*line_width);
            dest.color = Some(*color);
            dest.area_color = Some(*area_color);
            dest.is_solid = Some(*is_solid);
        }
        SchGraphicDetail::Arc {
            center,
            radius,
            line_width,
            start_angle,
            end_angle,
            color,
        } => {
            dest.center = Some(*center);
            dest.radius = Some(*radius);
            dest.line_width = Some(*line_width);
            dest.start_angle = Some(*start_angle);
            dest.end_angle = Some(*end_angle);
            dest.color = Some(*color);
        }
        SchGraphicDetail::EllipticalArc {
            center,
            radius,
            secondary_radius,
            line_width,
            start_angle,
            end_angle,
            color,
        } => {
            dest.center = Some(*center);
            dest.radius = Some(*radius);
            dest.secondary_radius = Some(*secondary_radius);
            dest.line_width = Some(*line_width);
            dest.start_angle = Some(*start_angle);
            dest.end_angle = Some(*end_angle);
            dest.color = Some(*color);
        }
        SchGraphicDetail::Ellipse {
            center,
            radius,
            secondary_radius,
            line_width,
            color,
            area_color,
            is_solid,
            transparent,
        } => {
            dest.center = Some(*center);
            dest.radius = Some(*radius);
            dest.secondary_radius = Some(*secondary_radius);
            dest.line_width = Some(*line_width);
            dest.color = Some(*color);
            dest.area_color = Some(*area_color);
            dest.is_solid = Some(*is_solid);
            dest.transparent = Some(*transparent);
        }
        SchGraphicDetail::Pie {
            center,
            radius,
            line_width,
            start_angle,
            end_angle,
            color,
            area_color,
            is_solid,
        } => {
            dest.center = Some(*center);
            dest.radius = Some(*radius);
            dest.line_width = Some(*line_width);
            dest.start_angle = Some(*start_angle);
            dest.end_angle = Some(*end_angle);
            dest.color = Some(*color);
            dest.area_color = Some(*area_color);
            dest.is_solid = Some(*is_solid);
        }
        SchGraphicDetail::Polyline {
            vertices,
            line_width,
            line_style,
            start_line_shape,
            end_line_shape,
            line_shape_size,
            color,
        } => {
            dest.vertices = Some(vertices.to_vec());
            dest.line_width = Some(*line_width);
            dest.line_style = Some(*line_style);
            dest.start_line_shape = Some(*start_line_shape);
            dest.end_line_shape = Some(*end_line_shape);
            dest.line_shape_size = Some(*line_shape_size);
            dest.color = Some(*color);
        }
        SchGraphicDetail::Polygon {
            vertices,
            line_width,
            color,
            area_color,
            is_solid,
            transparent,
        } => {
            dest.vertices = Some(vertices.to_vec());
            dest.line_width = Some(*line_width);
            dest.color = Some(*color);
            dest.area_color = Some(*area_color);
            dest.is_solid = Some(*is_solid);
            dest.transparent = Some(*transparent);
        }
        SchGraphicDetail::Bezier {
            vertices,
            line_width,
            color,
        } => {
            dest.vertices = Some(vertices.to_vec());
            dest.line_width = Some(*line_width);
            dest.color = Some(*color);
        }
        SchGraphicDetail::Image {
            from,
            to,
            orientation,
            line_width,
            color,
            is_solid,
            keep_aspect,
            embed_image,
            file_name,
        } => {
            dest.from = Some(*from);
            dest.to = Some(*to);
            dest.orientation = Some(*orientation);
            dest.line_width = Some(*line_width);
            dest.color = Some(*color);
            dest.is_solid = Some(*is_solid);
            dest.keep_aspect = Some(*keep_aspect);
            dest.embed_image = Some(*embed_image);
            dest.file_name = Some((*file_name).to_owned());
        }
        SchGraphicDetail::Label {
            at,
            orientation,
            justification,
            color,
            font_id,
            text,
            is_mirrored,
            url,
        } => {
            dest.at = Some(*at);
            dest.orientation = Some(*orientation);
            dest.justification = Some(*justification);
            dest.color = Some(*color);
            dest.stored_font_id = Some(*font_id);
            dest.text = Some((*text).to_owned());
            dest.is_mirrored = Some(*is_mirrored);
            dest.url = Some((*url).to_owned());
        }
        SchGraphicDetail::TextFrame {
            from,
            to,
            line_width,
            color,
            area_color,
            text_color,
            font_id,
            is_solid,
            show_border,
            alignment,
            word_wrap,
            clip_to_rect,
            text,
            text_margin,
            transparent,
        } => {
            dest.from = Some(*from);
            dest.to = Some(*to);
            dest.line_width = Some(*line_width);
            dest.color = Some(*color);
            dest.area_color = Some(*area_color);
            dest.text_color = Some(*text_color);
            dest.stored_font_id = Some(*font_id);
            dest.is_solid = Some(*is_solid);
            dest.show_border = Some(*show_border);
            dest.alignment = Some(*alignment);
            dest.word_wrap = Some(*word_wrap);
            dest.clip_to_rect = Some(*clip_to_rect);
            dest.text = Some((*text).to_owned());
            dest.text_margin = Some(*text_margin);
            dest.transparent = Some(*transparent);
        }
        SchGraphicDetail::Symbol | SchGraphicDetail::Hyperlink | SchGraphicDetail::Other { .. } => {
        }
    }
    dest
}

fn elaborate_graphic_init(
    context: &str,
    item: &GraphicIntent,
    existing: Option<&SchGraphicView<'_>>,
    owner_part_id: i32,
) -> Result<SchGraphicInit, SchLibPatchError> {
    let kind = item.kind.node;
    check_graphic_fields(context, kind, &item.fields)?;
    let policies = kind.field_policies();
    let fields = &item.fields;
    let dest = existing.map(|view| dest_graphic(&view.detail));
    let spec_error = |field: &'static str| {
        let context = format!("{context} {field}");
        move |source| SchLibPatchError::Spec { context, source }
    };
    macro_rules! rv {
        ($field:ident) => {{
            if fields.$field.is_authored() || dest.is_none() {
                policies
                    .$field
                    .as_ref()
                    .expect("applicable field has a policy (checked)")
                    .definition(&fields.$field)
                    .map_err(spec_error(stringify!($field)))?
            } else {
                dest.as_ref()
                    .and_then(|dest| dest.$field.clone())
                    .expect("destination carries every applicable field of its kind")
            }
        }};
    }
    // `font_id: None` means "unaddressed": the engine resolves the native
    // default font on New and preserves the stored id verbatim (unvalidated)
    // on Replace — never re-validating a value this elaboration did not
    // author (B optional-field semantics).
    let font_id: Option<i32> = if fields.font_id.is_authored() {
        let reference = policies
            .font_id
            .as_ref()
            .expect("font policy checked applicable")
            .definition(&fields.font_id)
            .map_err(spec_error("font_id"))?;
        Some(i32::try_from(reference.0).map_err(|_| {
            elaboration(
                context,
                format!("font_id {} exceeds the representable range", reference.0),
            )
        })?)
    } else {
        None
    };
    let shape = match kind {
        GraphicKind::Line => SchGraphicShape::Line {
            from: rv!(from),
            to: rv!(to),
            line_width: rv!(line_width),
            line_style: rv!(line_style),
            color: rv!(color),
        },
        GraphicKind::Rectangle => SchGraphicShape::Rectangle {
            from: rv!(from),
            to: rv!(to),
            line_width: rv!(line_width),
            line_style: rv!(line_style),
            color: rv!(color),
            area_color: rv!(area_color),
            is_solid: rv!(is_solid),
            transparent: rv!(transparent),
        },
        GraphicKind::RoundRectangle => SchGraphicShape::RoundRectangle {
            from: rv!(from),
            to: rv!(to),
            corner_x_radius: rv!(corner_x_radius),
            corner_y_radius: rv!(corner_y_radius),
            line_width: rv!(line_width),
            color: rv!(color),
            area_color: rv!(area_color),
            is_solid: rv!(is_solid),
        },
        GraphicKind::Arc => SchGraphicShape::Arc {
            center: rv!(center),
            radius: rv!(radius),
            line_width: rv!(line_width),
            start_angle: rv!(start_angle),
            end_angle: rv!(end_angle),
            color: rv!(color),
        },
        GraphicKind::EllipticalArc => SchGraphicShape::EllipticalArc {
            center: rv!(center),
            radius: rv!(radius),
            secondary_radius: rv!(secondary_radius),
            line_width: rv!(line_width),
            start_angle: rv!(start_angle),
            end_angle: rv!(end_angle),
            color: rv!(color),
        },
        GraphicKind::Ellipse => SchGraphicShape::Ellipse {
            center: rv!(center),
            radius: rv!(radius),
            secondary_radius: rv!(secondary_radius),
            line_width: rv!(line_width),
            color: rv!(color),
            area_color: rv!(area_color),
            is_solid: rv!(is_solid),
            transparent: rv!(transparent),
        },
        GraphicKind::Pie => SchGraphicShape::Pie {
            center: rv!(center),
            radius: rv!(radius),
            line_width: rv!(line_width),
            start_angle: rv!(start_angle),
            end_angle: rv!(end_angle),
            color: rv!(color),
            area_color: rv!(area_color),
            is_solid: rv!(is_solid),
        },
        GraphicKind::Polyline => SchGraphicShape::Polyline {
            vertices: rv!(vertices),
            line_width: rv!(line_width),
            line_style: rv!(line_style),
            start_line_shape: rv!(start_line_shape),
            end_line_shape: rv!(end_line_shape),
            line_shape_size: rv!(line_shape_size),
            color: rv!(color),
        },
        GraphicKind::Polygon => SchGraphicShape::Polygon {
            vertices: rv!(vertices),
            line_width: rv!(line_width),
            color: rv!(color),
            area_color: rv!(area_color),
            is_solid: rv!(is_solid),
            transparent: rv!(transparent),
        },
        GraphicKind::Bezier => SchGraphicShape::Bezier {
            vertices: rv!(vertices),
            line_width: rv!(line_width),
            color: rv!(color),
        },
        GraphicKind::Image => {
            let embed_image = rv!(embed_image);
            let effective_source = match &fields.source {
                FieldIntent::Set(value) => value.node.as_ref(),
                _ => None,
            };
            item.validate_effective_asset_source(effective_source, embed_image)
                .map_err(spec_error("source"))?;
            SchGraphicShape::Image {
                from: rv!(from),
                to: rv!(to),
                orientation: rv!(orientation),
                line_width: rv!(line_width),
                color: rv!(color),
                is_solid: rv!(is_solid),
                keep_aspect: rv!(keep_aspect),
                embed_image,
                file_name: rv!(file_name),
            }
        }
        GraphicKind::Label => SchGraphicShape::Label {
            at: rv!(at),
            orientation: rv!(orientation),
            justification: rv!(justification),
            color: rv!(color),
            font_id,
            text: rv!(text),
            is_mirrored: rv!(is_mirrored),
            url: rv!(url),
        },
        GraphicKind::TextFrame => SchGraphicShape::TextFrame {
            from: rv!(from),
            to: rv!(to),
            line_width: rv!(line_width),
            color: rv!(color),
            area_color: rv!(area_color),
            text_color: rv!(text_color),
            font_id,
            is_solid: rv!(is_solid),
            show_border: rv!(show_border),
            alignment: rv!(alignment),
            word_wrap: rv!(word_wrap),
            clip_to_rect: rv!(clip_to_rect),
            text: rv!(text),
            text_margin: rv!(text_margin),
            transparent: rv!(transparent),
        },
    };
    Ok(SchGraphicInit {
        owner_part_id,
        owner_part_display_mode: None,
        shape,
    })
}

/// The byte-preserving baseline for a positional graphic Replace: destination
/// values with the unaddressed optionals (`display mode`, `font_id`) `None`.
fn preserving_graphic_init(view: &SchGraphicView<'_>) -> Result<SchGraphicInit, AltiumFormatError> {
    let mut init = SchGraphicInit::from_view(view)?;
    init.owner_part_display_mode = None;
    match &mut init.shape {
        SchGraphicShape::Label { font_id, .. } | SchGraphicShape::TextFrame { font_id, .. } => {
            *font_id = None;
        }
        _ => {}
    }
    Ok(init)
}

fn box_geometry_of(shape: &SchGraphicShape) -> Option<BoxGeometry> {
    match shape {
        SchGraphicShape::Rectangle { from, to, .. }
        | SchGraphicShape::RoundRectangle { from, to, .. }
        | SchGraphicShape::TextFrame { from, to, .. }
        | SchGraphicShape::Image { from, to, .. } => Some(BoxGeometry {
            from: *from,
            to: *to,
        }),
        _ => None,
    }
}

fn set_shape_box(
    context: &str,
    shape: &mut SchGraphicShape,
    geometry: BoxGeometry,
) -> Result<(), SchLibPatchError> {
    match shape {
        SchGraphicShape::Rectangle { from, to, .. }
        | SchGraphicShape::RoundRectangle { from, to, .. }
        | SchGraphicShape::TextFrame { from, to, .. }
        | SchGraphicShape::Image { from, to, .. } => {
            *from = geometry.from;
            *to = geometry.to;
            Ok(())
        }
        other => Err(elaboration(
            context,
            format!(
                "auto-size produced geometry for a {} graphic, which has no bound box",
                other.kind_name()
            ),
        )),
    }
}

// ── Group blocking checks ───────────────────────────────────────────────────

/// The unrepresentable-member matrix over ONE group's filtered projection:
/// complete scope always blocks; partial scope blocks only when the authored
/// prefix would address the member (tail preservation is fine).
fn check_graphic_group_members(
    desc: &str,
    full: &[SchGraphicView<'_>],
    filtered: &[usize],
    mode: CollectionMode,
    authored_len: usize,
) -> Result<(), SchLibPatchError> {
    for (position, full_index) in filtered.iter().enumerate() {
        let view = &full[*full_index];
        if view.detail.is_intent_representable() {
            continue;
        }
        let addressed = position < authored_len;
        if mode == CollectionMode::Complete || addressed {
            return Err(elaboration(
                format!("{desc} Graphics"),
                format!(
                    "group member #{position} (graphics()[{full_index}], kind {}) is \
                     intent-unrepresentable; a complete scope over a group containing such \
                     members is blocked, and a partial scope may not address one (an \
                     unaddressed tail is preserved verbatim)",
                    view.detail.kind_name()
                ),
            ));
        }
    }
    Ok(())
}

// ── Composition of per-group plans into full-projection batches ─────────────

enum Slot<I> {
    Keep,
    Replace(I),
    Removed,
}

struct Composer<I> {
    slots: Vec<Slot<I>>,
    appended: Vec<I>,
    changed: bool,
    removals: bool,
}

impl<I> Composer<I> {
    fn new(full_len: usize) -> Self {
        Self {
            slots: (0..full_len).map(|_| Slot::Keep).collect(),
            appended: Vec::new(),
            changed: false,
            removals: false,
        }
    }

    /// Folds one group plan in; `filtered` maps the plan's original indices
    /// (positions in the group's filtered projection) onto full-projection
    /// indices.
    fn fold(&mut self, plan: crate::collection::CollectionPlan<I>, filtered: &[usize]) {
        for index in plan.removed() {
            self.slots[filtered[index.get()]] = Slot::Removed;
            self.changed = true;
            self.removals = true;
        }
        for item in plan.into_items() {
            match item {
                PlannedItem::Keep { .. } => {}
                PlannedItem::Replace { index, after } => {
                    self.slots[filtered[index.get()]] = Slot::Replace(after);
                    self.changed = true;
                }
                PlannedItem::New { after } => {
                    self.appended.push(after);
                    self.changed = true;
                }
            }
        }
    }

    fn finalize<E>(
        self,
        keep: fn(usize) -> E,
        replace: fn(usize, I) -> E,
        new: fn(I) -> E,
    ) -> Option<(Vec<E>, bool)> {
        if !self.changed {
            return None;
        }
        let mut edits = Vec::new();
        for (index, slot) in self.slots.into_iter().enumerate() {
            match slot {
                Slot::Keep => edits.push(keep(index)),
                Slot::Replace(init) => edits.push(replace(index, init)),
                Slot::Removed => {}
            }
        }
        edits.extend(self.appended.into_iter().map(new));
        Some((edits, self.removals))
    }
}

// ── The main positional elaboration ────────────────────────────────────────

pub(super) struct PositionalChildEdits {
    pub pins: Option<Vec<SchPinEdit>>,
    pub graphics: Option<Vec<SchGraphicEdit>>,
    pub removals: bool,
}

pub(super) fn elaborate_positional_children(
    owner: &str,
    component: &SchLibComponentSnapshot,
    intent: &ComponentIntent,
    destination_effective: i32,
    resulting_effective: i32,
    created: bool,
) -> Result<PositionalChildEdits, SchLibPatchError> {
    let pin_scopes = pin_scopes(
        owner,
        intent,
        destination_effective,
        resulting_effective,
        created,
    )?;
    let graphic_scopes = graphic_scopes(
        owner,
        intent,
        destination_effective,
        resulting_effective,
        created,
    )?;
    let fanouts = part_swap_fanouts(owner, intent)?;
    if pin_scopes.is_empty() && graphic_scopes.is_empty() && fanouts.is_empty() {
        return Ok(PositionalChildEdits {
            pins: None,
            graphics: None,
            removals: false,
        });
    }

    let full_pins = component.pins();
    let full_graphics = component.graphics();
    let mut pin_composer: Composer<SchPinInit> = Composer::new(full_pins.len());
    let mut graphic_composer: Composer<SchGraphicInit> = Composer::new(full_graphics.len());

    // Owner units pair each layout owner's pin and graphic scopes so one
    // `evaluate_layout` run sees the placements and auto-size constraints
    // together. The component unit runs first; its evaluated geometry is the
    // inherited map for part units (part pins may anchor on component
    // graphics).
    let mut owners: Vec<LayoutOwner> = Vec::new();
    for scope in &pin_scopes {
        if !owners.contains(&scope.layout_owner) {
            owners.push(scope.layout_owner);
        }
    }
    for scope in &graphic_scopes {
        if !owners.contains(&scope.layout_owner) {
            owners.push(scope.layout_owner);
        }
    }
    owners.sort_by_key(|layout_owner| match layout_owner {
        LayoutOwner::Component => 0,
        LayoutOwner::Part(number) => i64::from(number.get()) + 1,
    });

    let mut inherited_geometry: BTreeMap<GraphicSourceRef, BoxGeometry> = BTreeMap::new();
    for layout_owner in owners {
        let pin_scope = pin_scopes
            .iter()
            .find(|scope| scope.layout_owner == layout_owner);
        let graphic_scope = graphic_scopes
            .iter()
            .find(|scope| scope.layout_owner == layout_owner);
        let desc = pin_scope
            .map(|scope| scope.desc.clone())
            .or_else(|| graphic_scope.map(|scope| scope.desc.clone()))
            .expect("unit exists because a scope references it");

        // ── Pins of this owner ──
        let mut pin_inits: Vec<SchPinInit> = Vec::new();
        let mut pin_baselines: Vec<Option<SchPinInit>> = Vec::new();
        let mut pin_filtered: Vec<usize> = Vec::new();
        if let Some(scope) = pin_scope {
            pin_filtered = full_pins
                .iter()
                .enumerate()
                .filter(|(_, view)| group_member(view.owner_group, scope.target))
                .map(|(index, _)| index)
                .collect();
            for (position, item) in scope.collection.items.iter().enumerate() {
                let existing = pin_filtered.get(position).map(|index| &full_pins[*index]);
                let context = format!("{} pin #{position}", scope.desc);
                let init = elaborate_pin_init(&context, item, existing, scope.target)?;
                let baseline = existing
                    .map(|view| preserving_pin_init(view))
                    .transpose()
                    .map_err(format_error(format!(
                        "projecting the destination state of {context}"
                    )))?;
                pin_inits.push(init);
                pin_baselines.push(baseline);
            }
        }

        // ── Graphics of this owner ──
        let mut graphic_inits: Vec<SchGraphicInit> = Vec::new();
        let mut graphic_baselines: Vec<Option<SchGraphicInit>> = Vec::new();
        let mut graphic_filtered: Vec<usize> = Vec::new();
        if let Some(scope) = graphic_scope {
            graphic_filtered = full_graphics
                .iter()
                .enumerate()
                .filter(|(_, view)| group_member(view.owner_group, scope.target))
                .map(|(index, _)| index)
                .collect();
            check_graphic_group_members(
                &scope.desc,
                &full_graphics,
                &graphic_filtered,
                scope.collection.scope.mode,
                scope.collection.items.len(),
            )?;
            for (position, item) in scope.collection.items.iter().enumerate() {
                let existing = graphic_filtered
                    .get(position)
                    .map(|index| &full_graphics[*index]);
                let context = format!("{} graphics entry #{position}", scope.desc);
                if let Some(view) = existing {
                    let destination_kind = view.detail.kind_name();
                    let authored_kind = graphic_kind_name(item.kind.node);
                    if destination_kind != authored_kind {
                        return Err(elaboration(
                            &context,
                            format!(
                                "authored kind {authored_kind} does not match the destination \
                                 kind {destination_kind} at this position; cross-kind \
                                 positional replacement has no preserved-field mapping — \
                                 use a complete scope that removes and re-authors the entry"
                            ),
                        ));
                    }
                }
                let init = elaborate_graphic_init(&context, item, existing, scope.target)?;
                let baseline = existing
                    .map(|view| preserving_graphic_init(view))
                    .transpose()
                    .map_err(format_error(format!(
                        "projecting the destination state of {context}"
                    )))?;
                graphic_inits.push(init);
                graphic_baselines.push(baseline);
            }
        }

        // ── Layout evaluation over this owner's authored slots ──
        let placements: Vec<_> = pin_scope
            .map(|scope| {
                scope
                    .collection
                    .items
                    .iter()
                    .map(|item| item.placement.clone())
                    .collect()
            })
            .unwrap_or_default();
        let auto_sizes: Vec<_> = graphic_scope
            .map(|scope| {
                scope
                    .collection
                    .items
                    .iter()
                    .map(|item| item.auto_size.clone())
                    .collect()
            })
            .unwrap_or_default();
        let has_layout =
            placements.iter().any(Option::is_some) || auto_sizes.iter().any(Option::is_some);
        if has_layout {
            // `pin_inits` is empty exactly when this owner manages no pins
            // (auto-size-only layouts); the flag vector mirrors its length.
            let orientation_authored: Vec<bool> = pin_scope
                .map(|scope| {
                    scope
                        .collection
                        .items
                        .iter()
                        .map(|item| item.orientation.is_authored())
                        .collect()
                })
                .unwrap_or_default();
            let effective_pins: Vec<EffectivePinGeometry> = pin_inits
                .iter()
                .zip(orientation_authored)
                .map(|(init, orientation_is_authored)| EffectivePinGeometry {
                    length: init.length,
                    location: init.location,
                    orientation: init.orientation,
                    orientation_is_authored,
                })
                .collect();
            let effective_graphics: Vec<Option<BoxGeometry>> = graphic_inits
                .iter()
                .map(|init| box_geometry_of(&init.shape))
                .collect();
            let evaluated = evaluate_layout(
                layout_owner,
                &placements,
                &auto_sizes,
                &effective_pins,
                &effective_graphics,
                &inherited_geometry,
            )
            .map_err(|source| SchLibPatchError::Spec {
                context: format!("{desc} layout"),
                source,
            })?;
            for (position, geometry) in evaluated.graphics.iter().enumerate() {
                if let Some(geometry) = geometry {
                    let context = format!("{desc} graphics entry #{position}");
                    set_shape_box(&context, &mut graphic_inits[position].shape, *geometry)?;
                }
            }
            for (position, placement) in evaluated.pins.iter().enumerate() {
                if let Some(placement) = placement {
                    pin_inits[position].location = placement.location;
                    pin_inits[position].orientation = placement.orientation;
                }
            }
            if layout_owner == LayoutOwner::Component {
                inherited_geometry = evaluated.geometry.clone();
            }
        } else if layout_owner == LayoutOwner::Component {
            // No layout authored, but part pins may still anchor on
            // component graphics: publish the component-authored boxes.
            for (position, init) in graphic_inits.iter().enumerate() {
                if let Some(geometry) = box_geometry_of(&init.shape) {
                    inherited_geometry.insert(
                        GraphicSourceRef {
                            owner: LayoutOwner::Component,
                            index: position,
                        },
                        geometry,
                    );
                }
            }
        }

        // ── Plan the pins group ──
        if let Some(scope) = pin_scope {
            let request = build_request(
                scope.collection.scope.mode,
                pin_filtered.len(),
                pin_inits,
                pin_baselines,
            );
            let plan = plan_collection(
                pin_filtered.len(),
                &scope.collection.scope,
                request,
                CollectionContext {
                    owner: scope.desc.clone(),
                    kind: CollectionKind::Pins,
                },
            )
            .map_err(collection_error)?;
            pin_composer.fold(plan, &pin_filtered);
        }

        // ── Plan the graphics group ──
        if let Some(scope) = graphic_scope {
            let request = build_request(
                scope.collection.scope.mode,
                graphic_filtered.len(),
                graphic_inits,
                graphic_baselines,
            );
            let plan = plan_collection(
                graphic_filtered.len(),
                &scope.collection.scope,
                request,
                CollectionContext {
                    owner: scope.desc.clone(),
                    kind: CollectionKind::Graphics,
                },
            )
            .map_err(collection_error)?;
            graphic_composer.fold(plan, &graphic_filtered);
        }
    }

    // ── Part swap-group fan-out over the FINAL pin sequence (memo §5) ──
    apply_part_swap_fanouts(owner, component, &full_pins, &fanouts, &mut pin_composer)?;

    let (pins, pin_removals) =
        match pin_composer.finalize(SchPinEdit::Keep, SchPinEdit::Replace, SchPinEdit::New) {
            Some((edits, removals)) => (Some(edits), removals),
            None => (None, false),
        };
    let (graphics, graphic_removals) = match graphic_composer.finalize(
        SchGraphicEdit::Keep,
        SchGraphicEdit::Replace,
        SchGraphicEdit::New,
    ) {
        Some((edits, removals)) => (Some(edits), removals),
        None => (None, false),
    };
    Ok(PositionalChildEdits {
        pins,
        graphics,
        removals: pin_removals || graphic_removals,
    })
}

/// Builds the group's planner request: partial = changed prefix slots as
/// `Replace` plus overflow appends; complete = position-aligned final list.
fn build_request<I: PartialEq>(
    mode: CollectionMode,
    filtered_len: usize,
    inits: Vec<I>,
    baselines: Vec<Option<I>>,
) -> CollectionRequest<I> {
    match mode {
        CollectionMode::Partial => {
            let mut edits = Vec::new();
            for (position, (init, baseline)) in
                inits.into_iter().zip(baselines.into_iter()).enumerate()
            {
                if position < filtered_len {
                    if baseline.as_ref() != Some(&init) {
                        edits.push(IndexedEdit::Replace {
                            index: position,
                            after: init,
                        });
                    }
                } else {
                    edits.push(IndexedEdit::Insert {
                        before: filtered_len,
                        after: init,
                    });
                }
            }
            CollectionRequest::Partial(edits)
        }
        CollectionMode::Complete => {
            let mut entries = Vec::new();
            for (position, (init, baseline)) in
                inits.into_iter().zip(baselines.into_iter()).enumerate()
            {
                if position < filtered_len {
                    if baseline.as_ref() == Some(&init) {
                        entries.push(FinalItem::Keep { index: position });
                    } else {
                        entries.push(FinalItem::Replace {
                            index: position,
                            after: init,
                        });
                    }
                } else {
                    entries.push(FinalItem::New { after: init });
                }
            }
            CollectionRequest::Complete(entries)
        }
    }
}

// ── Part swap fan-out ───────────────────────────────────────────────────────

struct PartSwapFanout {
    part: i32,
    target: String,
    desc: String,
}

fn part_swap_fanouts(
    owner: &str,
    intent: &ComponentIntent,
) -> Result<Vec<PartSwapFanout>, SchLibPatchError> {
    let mut fanouts = Vec::new();
    for item in &intent.parts.items {
        if !item.part_swap_group.is_authored() {
            continue;
        }
        let number = i32::try_from(item.number.node.get()).expect("checked by resolve_parts");
        let desc = format!("component '{owner}' part {number}");
        if matches!(item.part_swap_group, FieldIntent::Inherit(_)) {
            return Err(inherit_error(&desc, "part.part_swap_group"));
        }
        let target = PART_SWAP
            .definition(&item.part_swap_group)
            .map_err(|source| SchLibPatchError::Spec {
                context: format!("{desc} part_swap_group"),
                source,
            })?;
        fanouts.push(PartSwapFanout {
            part: number,
            target,
            desc,
        });
    }
    fanouts.sort_by_key(|fanout| fanout.part);
    Ok(fanouts)
}

fn apply_part_swap_fanouts(
    owner: &str,
    component: &SchLibComponentSnapshot,
    full_pins: &[SchPinView<'_>],
    fanouts: &[PartSwapFanout],
    composer: &mut Composer<SchPinInit>,
) -> Result<(), SchLibPatchError> {
    if fanouts.is_empty() {
        return Ok(());
    }
    let parts = component.parts().map_err(format_error(format!(
        "projecting parts of component '{owner}' for swap fan-out"
    )))?;
    for fanout in fanouts {
        let view = parts
            .iter()
            .find(|part| part.number == fanout.part)
            .ok_or_else(|| {
                elaboration(
                    &fanout.desc,
                    format!(
                        "part {} is outside the resulting effective part count",
                        fanout.part
                    ),
                )
            })?;
        // Memo §5 rule 2: a fan-out against a Mixed projection would
        // overwrite a distinction the file holds — hard block.
        if view.swap_group == SchPartSwapGroup::Mixed {
            return Err(elaboration(
                &fanout.desc,
                "part_swap_group cannot fan out: the part's pins currently carry MIXED \
                 SwapIdPart prefixes (the parts() projection is Mixed); resolve the \
                 per-pin values explicitly before authoring a part-level group",
            ));
        }
        for (index, slot) in composer.slots.iter_mut().enumerate() {
            let pin = &full_pins[index];
            // Owner-0 (shared) pins are excluded from fan-out natively;
            // membership is exact part equality on the raw owner.
            if !matches!(pin.owner_group, SchOwnerGroup::Part(n) if n == fanout.part) {
                continue;
            }
            match slot {
                Slot::Removed => {}
                Slot::Keep => {
                    if pin.part_swap_group != fanout.target {
                        let mut init = preserving_pin_init(pin).map_err(format_error(format!(
                            "projecting pins()[{index}] of component '{owner}' for swap \
                                 fan-out"
                        )))?;
                        init.part_swap_group = Some(fanout.target.clone());
                        *slot = Slot::Replace(init);
                        composer.changed = true;
                    }
                }
                Slot::Replace(init) => match &init.part_swap_group {
                    Some(authored) if *authored != fanout.target => {
                        return Err(elaboration(
                            &fanout.desc,
                            format!(
                                "pin (pins()[{index}], designator '{}') authors \
                                 part_swap_group '{authored}' while its part fans out \
                                 '{}'; the two writes contradict",
                                init.designator, fanout.target
                            ),
                        ));
                    }
                    Some(_) => {}
                    None => {
                        // Preserve byte-verbatim when the stored prefix
                        // already equals the target; re-encode otherwise.
                        if pin.part_swap_group != fanout.target {
                            init.part_swap_group = Some(fanout.target.clone());
                        }
                    }
                },
            }
        }
        for init in &mut composer.appended {
            if init.owner_part_id != fanout.part {
                continue;
            }
            match &init.part_swap_group {
                Some(authored) if *authored != fanout.target => {
                    return Err(elaboration(
                        &fanout.desc,
                        format!(
                            "appended pin (designator '{}') authors part_swap_group \
                             '{authored}' while its part fans out '{}'; the two writes \
                             contradict",
                            init.designator, fanout.target
                        ),
                    ));
                }
                Some(_) => {}
                None => {
                    if !fanout.target.is_empty() {
                        init.part_swap_group = Some(fanout.target.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_spec_lang::diagnostic::{Span, Spanned};
    use altium_spec_lang::intent::{CollectionScope, PartIntent, PartNumber};

    fn spanned<T>(value: T) -> Spanned<T> {
        Spanned::new(value, Span::new(0, 1))
    }

    /// Synthetic graphic views: `SchGraphicView` has public fields, so the
    /// block matrix over intent-unrepresentable members (which no public
    /// MUTATION path can create — creating Symbol/Hyperlink/owner-0/
    /// out-of-range graphics is a mutation-boundary hard error) is testable
    /// against the pure group checks directly.
    fn view(
        owner_group: SchOwnerGroup,
        detail: SchGraphicDetail<'static>,
    ) -> SchGraphicView<'static> {
        SchGraphicView {
            owner_group,
            owner_part_display_mode: 0,
            unique_id: None,
            detail,
        }
    }
    fn rectangle_detail() -> SchGraphicDetail<'static> {
        SchGraphicDetail::Rectangle {
            from: CoordPoint::zero(),
            to: CoordPoint::zero(),
            line_width: PenWidth::Small,
            line_style: LineStyle::Solid,
            color: Color::BLACK,
            area_color: Color::BLACK,
            is_solid: false,
            transparent: false,
        }
    }

    fn filtered_of(views: &[SchGraphicView<'_>], target: i32) -> Vec<usize> {
        views
            .iter()
            .enumerate()
            .filter(|(_, view)| group_member(view.owner_group, target))
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn unrepresentable_member_matrix_blocks_complete_and_partial_addressing() {
        // Part(1) group: [Rectangle, Symbol, Rectangle] at full indices
        // 0/2/3; a Hyperlink lives in Part(2) and an Other in OutOfRange(7).
        let views = vec![
            view(SchOwnerGroup::Part(1), rectangle_detail()),
            view(SchOwnerGroup::Part(2), SchGraphicDetail::Hyperlink),
            view(SchOwnerGroup::Part(1), SchGraphicDetail::Symbol),
            view(SchOwnerGroup::Part(1), rectangle_detail()),
            view(
                SchOwnerGroup::OutOfRange(7),
                SchGraphicDetail::Other { kind: "Wild" },
            ),
        ];
        let filtered = filtered_of(&views, 1);
        assert_eq!(
            filtered,
            [0, 2, 3],
            "the filtered→full mapping is the member index list"
        );

        // Complete scope always blocks, whatever the authored length.
        for authored_len in [0usize, 1, 2, 5] {
            let error = check_graphic_group_members(
                "component 'X' part 1",
                &views,
                &filtered,
                CollectionMode::Complete,
                authored_len,
            )
            .unwrap_err();
            let rendered = error.to_string();
            assert!(rendered.contains("Symbol"), "{rendered}");
            assert!(rendered.contains("graphics()[2]"), "{rendered}");
        }
        // Partial scope blocks only when the authored prefix ADDRESSES the
        // member (position 1); a shorter prefix preserves the tail.
        for authored_len in [2usize, 3, 9] {
            assert!(
                check_graphic_group_members(
                    "component 'X' part 1",
                    &views,
                    &filtered,
                    CollectionMode::Partial,
                    authored_len,
                )
                .is_err(),
                "partial prefix of {authored_len} addresses the Symbol"
            );
        }
        for authored_len in [0usize, 1] {
            check_graphic_group_members(
                "component 'X' part 1",
                &views,
                &filtered,
                CollectionMode::Partial,
                authored_len,
            )
            .unwrap();
        }

        // The unaddressable owner classes (SharedPins graphics, OutOfRange,
        // PartIndependent) can never be members of a targetable group, so a
        // scope over Part(1) or Part(2) never sees them: preservation, not
        // blocking, is structural.
        assert!(!group_member(SchOwnerGroup::SharedPins, 1));
        assert!(!group_member(SchOwnerGroup::PartIndependent, 1));
        assert!(!group_member(SchOwnerGroup::OutOfRange(7), 1));
        assert!(group_member(SchOwnerGroup::SharedPins, 0));
        // Raw-id matching lets a PRE-growth projection (OutOfRange(3) under
        // the old count) select the same members Part(3) selects after
        // growth.
        assert!(group_member(SchOwnerGroup::OutOfRange(3), 3));
        assert!(group_member(SchOwnerGroup::Part(3), 3));
        assert!(!group_member(SchOwnerGroup::Part(2), 3));
    }

    fn component_intent_with_parts(
        scope: CollectionScope,
        numbers: &[i32],
        part_count: FieldIntent<PartCount>,
    ) -> ComponentIntent {
        ComponentIntent {
            name: spanned("X".to_owned()),
            annotation: None,
            designator: FieldIntent::Absent,
            description: FieldIntent::Absent,
            component_kind: FieldIntent::Absent,
            part_count,
            show_hidden_pins: FieldIntent::Absent,
            pins: Collection::default(),
            parameters: Collection::default(),
            aliases: Collection::default(),
            footprints: Collection::default(),
            graphics: Collection::default(),
            parts: Collection {
                scope,
                items: numbers
                    .iter()
                    .map(|number| PartIntent {
                        number: spanned(PartNumber::new(*number).unwrap()),
                        part_swap_group: FieldIntent::Absent,
                        pins: Collection::default(),
                        graphics: Collection::default(),
                    })
                    .collect(),
            },
        }
    }
    fn scope(mode: CollectionMode, delete: CollectionDelete) -> CollectionScope {
        CollectionScope {
            mode,
            delete,
            span: Some(Span::new(0, 1)),
        }
    }
    fn count(value: i32) -> FieldIntent<PartCount> {
        FieldIntent::Set(spanned(PartCount::new(value).unwrap()))
    }

    #[test]
    fn parts_structure_resolution_matrix() {
        let partial = |numbers: &[i32], part_count| {
            component_intent_with_parts(CollectionScope::default(), numbers, part_count)
        };
        let complete = |numbers: &[i32], delete, part_count| {
            component_intent_with_parts(
                scope(CollectionMode::Complete, delete),
                numbers,
                part_count,
            )
        };

        // Partial: implied = max(destination, highest authored).
        let resolved =
            resolve_parts_structure("X", 1, &partial(&[3], FieldIntent::Absent)).unwrap();
        assert_eq!(resolved.resulting_effective, 3);
        assert_eq!(resolved.growth, 2);
        assert!(resolved.removed_slots.is_empty());
        // Explicit count must accommodate; lowering under partial blocks.
        assert!(resolve_parts_structure("X", 2, &partial(&[], count(1))).is_err());
        let resolved = resolve_parts_structure("X", 2, &partial(&[], count(5))).unwrap();
        assert_eq!(resolved.resulting_effective, 5);
        assert_eq!(resolved.growth, 3);
        // Duplicate part numbers are key collisions.
        assert!(resolve_parts_structure("X", 2, &partial(&[2, 2], FieldIntent::Absent)).is_err());

        // Complete: contiguity 1..=n re-checked; count must agree; shrink
        // needs delete: allow; at least one slot remains.
        let resolved = resolve_parts_structure(
            "X",
            3,
            &complete(&[1, 2], CollectionDelete::Allow, FieldIntent::Absent),
        )
        .unwrap();
        assert_eq!(resolved.resulting_effective, 2);
        assert_eq!(resolved.growth, 0);
        assert_eq!(resolved.removed_slots, [3]);
        let error = resolve_parts_structure(
            "X",
            3,
            &complete(&[1, 2], CollectionDelete::Deny, FieldIntent::Absent),
        )
        .unwrap_err();
        assert!(error.to_string().contains("delete: allow"), "{error}");
        let error = resolve_parts_structure(
            "X",
            1,
            &complete(&[1, 3], CollectionDelete::Deny, FieldIntent::Absent),
        )
        .unwrap_err();
        assert!(error.to_string().contains("contiguous"), "{error}");
        let error =
            resolve_parts_structure("X", 2, &complete(&[1, 2], CollectionDelete::Deny, count(3)))
                .unwrap_err();
        assert!(error.to_string().contains("disagrees"), "{error}");
        let error = resolve_parts_structure(
            "X",
            2,
            &complete(&[], CollectionDelete::Allow, FieldIntent::Absent),
        )
        .unwrap_err();
        assert!(error.to_string().contains("at least one"), "{error}");
        // Growth through a complete scope.
        let resolved = resolve_parts_structure(
            "X",
            1,
            &complete(&[1, 2, 3], CollectionDelete::Deny, count(3)),
        )
        .unwrap();
        assert_eq!(resolved.resulting_effective, 3);
        assert_eq!(resolved.growth, 2);
        assert!(resolved.removed_slots.is_empty());
    }
}
