//! Library-level namespace transitions (step-2 increment D1; contract
//! authority `docs/worklogs/next-child-contracts-2026-09-16.md`, cited as
//! "memo §N", incl. its errata section).
//!
//! # Model
//!
//! [`SchLibSnapshot::with_library_edit`] is the DECLARED-transition variant of
//! component replacement: one [`SchLibEdit`] batch may change the library
//! reference namespace — the component set, the global alias redirections, the
//! image-asset store, and the font table — and the resulting snapshot's
//! `ReferenceContext` is RECOMPUTED from the planned final library state,
//! never hand-patched. Every component snapshot taken from the result binds to
//! the recomputed context. The strict path is unchanged:
//! `SchLibSnapshot::replace_components` still hard-errors on any UNDECLARED
//! context difference, so a namespace change is only ever expressible through
//! this explicit, typed batch.
//!
//! Determinism: the final library state (and therefore the recomputed
//! context) is a pure function of the input snapshot and the edit, except for
//! the fresh `UniqueId`s handed to NEW components — those never enter the
//! `ReferenceContext`, so the same edit applied twice to the same input always
//! yields identical contexts (and identical full state when the edit creates
//! no components). An empty edit yields a context-and-state-identical
//! snapshot.
//!
//! # Application order (composition contract)
//!
//! Within one edit: font appends, then image-asset ops, then component slot
//! resolution, then alias ops (removes before adds), then the final
//! namespace/reference validation. Consequences:
//!
//! - A NEW blank component may rely on a font appended by the SAME edit (its
//!   records reference font id 1, validated against the post-append table).
//! - Component CONTENT edits (`with_children` batches) always run against a
//!   snapshot's captured context, so a child that needs a namespace addition
//!   (the memo-§8a default-font append; an image record referencing a new
//!   asset) is authored in TWO steps: first a library edit declaring the
//!   namespace transition, then ordinary component edits taken from the
//!   RESULT snapshot, whose recomputed context carries the extension. This
//!   resolves the recorded increment-B limitation: fonts stay inside
//!   `ReferenceContext`, and the declared append is the supported way to
//!   extend the table (memo errata item 3 — increment D adopted the
//!   "declared font-table append that recomputes the context" option).
//!
//! # Component slots
//!
//! `SchLibEdit::components` is the FINAL ordered component sequence (`None`
//! leaves the component set untouched). `Keep`/`ReplaceWith` indices address
//! the immutable input snapshot; referencing one input slot twice is a
//! conflict; omission is a removal and requires the explicit
//! `allow_component_removal` authorization. `ReplaceWith` keeps the strict
//! rules: the replacement must be captured in the INPUT's context, renaming is
//! rejected, and its alias list must be untouched (alias changes go through
//! [`SchAliasOp`]). New components come from a context-independent factory:
//!
//! - [`SchNewComponent::CloneOf`] deep-copies an INPUT component under a new
//!   name: fresh component (root) `UniqueId`, cleared alias state (alias names
//!   are global; a clone never inherits them), everything else — child
//!   records with their own unique ids, description, part counts, models —
//!   byte-identical to the source.
//! - [`SchNewComponent::Blank`] reuses the `new_blank_ad26` component shape
//!   (native part count 2, Designator/Comment field objects). Its records
//!   reference font id 1, which must exist in the (post-append) font table.
//!
//! Removing a component whose alias set is non-empty is a hard error unless
//! the SAME edit removes (or retargets) every one of its aliases — dangling
//! `Redirection` entries are never silently dropped. Storage keys need no op
//! surface: the save path derives SectionKeys entries from the final header
//! names (including names beyond the 31-character CFB storage limit;
//! generated keys are collision-checked case-insensitively and independently
//! of header order).
//!
//! # Aliases
//!
//! [`SchAliasOp`] addresses components by `lib_ref` (Add targets must be final
//! components; Remove may also target a component being removed by the same
//! edit, releasing its alias — including when a NEW component of the same
//! edit re-takes the removed component's name: the new slot cannot carry the
//! alias yet, so a Remove naming that `lib_ref` unambiguously releases the
//! REMOVED component's alias). The three native representations are kept
//! synchronized for every alias-touched component: the header index entry's
//! alias vector (op order: removals splice, additions append), the root
//! record's `AliasList` parameter (recomputed as the comma-join of the final
//! vector; AD26 stores a sorted set and comma-joins it — our append-order
//! join is a recorded deviation that preserves loaded order instead of
//! re-sorting), and the global `Redirection` entries (preserved verbatim when
//! the ordered header pair sequence is unchanged, otherwise rebuilt
//! deterministically in header order — exactly the parse order).
//!
//! Namespace rules: component and alias storage names live in ONE
//! case-insensitive CFB directory namespace, so uniqueness is checked on the
//! storage-folded form (illegal characters sanitized, per-character simple
//! uppercase — the container's own comparison; `COMPONENT_1` collides with
//! `Component_1`): final `lib_ref`s are unique; alias names are unique and
//! collide with no other component's `lib_ref`; an alias EXACTLY equal to its
//! OWN component's name (the self-alias storage-sharing case) is allowed,
//! while a case-variant of its own component's name is a collision (the save
//! path shares storage on exact equality only). These are mutation-boundary
//! policies: wild loaded state still captures verbatim. Added alias names
//! must be non-empty and comma-free (the native `AliasList` comma-join cannot
//! represent a comma; loaded state is still preserved verbatim).
//!
//! # Image assets and fonts
//!
//! [`SchImageAssetEdit`] ops apply IN ORDER through the existing
//! `SchLib::insert_image_resource` / `replace_image_resource` /
//! `remove_image_resource` operations (indices address the asset list as left
//! by the preceding op; serialization constraints validated as today), plus
//! the memo §8c reference rule, enforced PER PAIRING TARGET: each embedded
//! image record's `file_name` is resolved against the INPUT asset list to its
//! concrete pairing target (first exact match, then first case-insensitive —
//! the native pairing order), and that specific asset must survive the edit
//! with its name intact. Removing or renaming the pairing target is a hard
//! error even when a same-named or case-variant sibling would still satisfy
//! name resolution (the record would silently rebind to a different asset
//! body); same-name payload replacement, removing an orphan asset (the wild
//! SMotlaq class), and preserving already-dangling references all stay legal.
//! An image record referencing an asset INSERTED by a library edit is
//! authored through the two-step composition above (insert first, then
//! `with_children` on the result snapshot).
//!
//! [`SchFontAppend`] appends one font-table entry per op (every native axis;
//! the id is assigned positionally). Appending an entry axis-identical to an
//! existing one is rejected — reference the existing id instead — and the
//! table must be 1-based sequential (never silently renumbered).
//!
//! # Weight
//!
//! The header weight keeps the input's stored convention: the canonical
//! cardinality delta (records + aliases per component, plus one root per
//! component) between the final and input states is applied to the stored
//! value — the same policy `replace_components` uses, extended with the
//! component-root term. It is never recanonicalized.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{ReferenceContext, SchLibComponentSnapshot, SchLibSnapshot, validate_snapshot_library};
use crate::sch_records::SchRecord;
use crate::schlib::validation::{
    DEFAULT_FONT_NAME, DEFAULT_FONT_SIZE, check_font_id_exists, check_sequential_font_table,
    resolve_image_asset_reference,
};
use crate::schlib::{
    SchLibAlias, SchLibComponent, SchLibComponentIndex, blank_ad26_component, cfb_storage_fold,
    sanitize_cfb_name,
};
use crate::util::generate_unique_id;
use crate::{AltiumFormatError, Result, ResultExt};
use altium_format_types::sch::SchFont;

/// One declared library-level namespace transition batch. Every field
/// defaults to "untouched"; see the module documentation for the complete
/// contract and application order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchLibEdit {
    /// Final ordered component sequence, or `None` to leave the component set
    /// untouched.
    pub components: Option<Vec<SchLibComponentEdit>>,
    /// Alias set changes, addressed by component `lib_ref`. Removals apply
    /// before additions (so a remove+add pair retargets an alias).
    pub alias_ops: Vec<SchAliasOp>,
    /// Ordered image-asset store ops (each op's index addresses the store as
    /// left by the preceding op).
    pub image_assets: Vec<SchImageAssetEdit>,
    /// Font-table appends (ids are assigned positionally).
    pub font_appends: Vec<SchFontAppend>,
    /// Explicit authorization for removing components omitted from the final
    /// sequence (their complete aggregates are deleted).
    pub allow_component_removal: bool,
}

/// One item of the final component sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum SchLibComponentEdit {
    /// The input component at `component(i)`, preserved verbatim.
    Keep(usize),
    /// The input slot `i` replaced by an edited aggregate captured in the
    /// INPUT's reference context (renaming and alias changes rejected).
    ReplaceWith(usize, SchLibComponentSnapshot),
    /// A newly created component (context-independent factory).
    New(SchNewComponent),
}

/// Context-independent sources for a new component.
#[derive(Debug, Clone, PartialEq)]
pub enum SchNewComponent {
    /// Deep copy of INPUT component `source` under a new unique name, with a
    /// fresh component `UniqueId` and cleared alias state.
    CloneOf {
        source: usize,
        lib_reference: String,
    },
    /// The minimal blank AD26 component shape under the given name.
    Blank { lib_reference: String },
}

/// One alias set change. `component` is a `lib_ref`: for `Add` it must name a
/// component of the FINAL sequence; for `Remove` it may also name a component
/// the same edit removes (releasing the alias so the removal cannot dangle).
#[derive(Debug, Clone, PartialEq)]
pub enum SchAliasOp {
    Add { component: String, alias: String },
    Remove { component: String, alias: String },
}

/// One image-asset store op, wrapping the corresponding `SchLib` operation.
#[derive(Debug, Clone, PartialEq)]
pub enum SchImageAssetEdit {
    Insert {
        index: usize,
        name: String,
        payload: Vec<u8>,
    },
    Replace {
        index: usize,
        name: String,
        payload: Vec<u8>,
    },
    Remove {
        index: usize,
    },
}

/// One font-table entry to append: every native axis except the id, which is
/// assigned positionally (1-based) by the edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchFontAppend {
    pub name: String,
    pub size: i32,
    pub rotation: i32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

impl SchFontAppend {
    /// Times New Roman 10, rotation 0, no styles — the native default entry
    /// new text-bearing children resolve (memo §8a rule 3; the axes are the
    /// shared `validation` constants, so the creation-time resolution and
    /// this append surface can never drift apart).
    pub fn native_default() -> Self {
        Self {
            name: DEFAULT_FONT_NAME.to_owned(),
            size: DEFAULT_FONT_SIZE,
            rotation: 0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
        }
    }

    /// Axis equality against an existing entry (name compared
    /// case-insensitively, mirroring the native find-or-create match).
    fn matches(&self, font: &SchFont) -> bool {
        font.name.eq_ignore_ascii_case(&self.name)
            && font.size == self.size
            && font.rotation == self.rotation
            && font.bold == self.bold
            && font.italic == self.italic
            && font.underline == self.underline
            && font.strikeout == self.strikeout
    }
}

fn invalid(key: &str, detail: impl std::fmt::Display) -> AltiumFormatError {
    AltiumFormatError::InvalidParamValue {
        key: key.to_owned(),
        detail: detail.to_string(),
    }
}

/// One resolved final component slot.
struct FinalSlot {
    component: SchLibComponent,
    index: SchLibComponentIndex,
    alias_touched: bool,
}

/// The storage-namespace identity of a component/alias name: illegal CFB
/// characters sanitized, then case-folded the way the CFB directory namespace
/// compares entries. Two names with equal keys cannot coexist as storages, so
/// the final-namespace uniqueness checks compare THESE, not raw strings.
fn storage_namespace_key(name: &str) -> String {
    cfb_storage_fold(&sanitize_cfb_name(name))
}

/// Formats the case/sanitize note appended to a namespace-collision error
/// when the colliding names are not byte-identical.
fn namespace_collision_note(existing: &str, incoming: &str) -> String {
    if existing == incoming {
        String::new()
    } else {
        format!(
            " ('{existing}' and '{incoming}' collide because component and alias storage \
             names share one case-insensitive CFB namespace)"
        )
    }
}

/// Canonical cardinality total used for the weight DELTA only (records +
/// aliases per component, plus one root per component); the stored weight
/// itself keeps the input file's convention.
fn canonical_total(
    components: &[SchLibComponent],
    indices: &[SchLibComponentIndex],
) -> Result<i64> {
    let mut total = i64::try_from(components.len()).map_err(|_| {
        invalid(
            "WEIGHT",
            "component count exceeds supported cardinality range",
        )
    })?;
    for (component, index) in components.iter().zip(indices) {
        let slot = component
            .records
            .len()
            .checked_add(index.aliases.len())
            .and_then(|count| i64::try_from(count).ok())
            .ok_or_else(|| {
                invalid(
                    "WEIGHT",
                    format!(
                        "component '{}' record/alias count exceeds supported range",
                        index.lib_ref
                    ),
                )
            })?;
        total = total
            .checked_add(slot)
            .ok_or_else(|| invalid("WEIGHT", "cardinality total overflow"))?;
    }
    Ok(total)
}

impl SchLibSnapshot {
    /// Applies one declared library-level namespace transition; see the
    /// module documentation for the complete contract. The input snapshot is
    /// never modified; errors leave no partial state. The result's
    /// `ReferenceContext` is recomputed from the final library state, and all
    /// component snapshots taken from the result bind to it.
    pub fn with_library_edit(&self, edit: SchLibEdit) -> Result<Self> {
        self.with_library_edit_inner(edit)
            .context("applying SchLib library edit")
    }

    fn with_library_edit_inner(&self, edit: SchLibEdit) -> Result<Self> {
        validate_snapshot_library(&self.library)
            .context("validating original SchLib snapshot before library edit")?;
        let mut working = self.library.clone();

        // 1. Font appends (before component resolution, so new components may
        //    rely on appended entries).
        if !edit.font_appends.is_empty() {
            check_sequential_font_table(&working.header.fonts)
                .context("appending font-table entries")?;
        }
        for (position, append) in edit.font_appends.into_iter().enumerate() {
            let context = || format!("font append #{position} '{}'", append.name);
            if append.name.is_empty() {
                return Err(invalid("FontName", "font name must not be empty"))
                    .with_context(context);
            }
            if let Some(existing) = working
                .header
                .fonts
                .iter()
                .find(|font| append.matches(font))
            {
                return Err(invalid(
                    "FontId",
                    format!(
                        "an axis-identical font-table entry already exists as id {}; \
                         reference the existing entry instead of appending a duplicate",
                        existing.id
                    ),
                ))
                .with_context(context);
            }
            let id = i32::try_from(working.header.fonts.len() + 1)
                .map_err(|_| invalid("FontIdCount", "font table length exceeds native id range"))
                .with_context(context)?;
            working.header.fonts.push(SchFont {
                id,
                name: append.name,
                size: append.size,
                rotation: append.rotation,
                bold: append.bold,
                italic: append.italic,
                underline: append.underline,
                strikeout: append.strikeout,
            });
        }

        // 2. Image-asset ops, in order, through the existing SchLib
        //    operations (serialization constraints validated there). Each
        //    INPUT asset's position is tracked through the op sequence so the
        //    §8c guard (step 9) can check the concrete pairing target of every
        //    embedded reference, not just name resolvability — duplicate and
        //    case-variant asset names are distinct assets.
        let mut input_asset_positions: Vec<Option<usize>> =
            (0..self.library.embedded_images.len()).map(Some).collect();
        for (position, op) in edit.image_assets.into_iter().enumerate() {
            let context = || format!("image asset op #{position}");
            match op {
                SchImageAssetEdit::Insert {
                    index,
                    name,
                    payload,
                } => {
                    working
                        .insert_image_resource(index, name, payload)
                        .with_context(context)?;
                    for current in input_asset_positions.iter_mut().flatten() {
                        if *current >= index {
                            *current += 1;
                        }
                    }
                }
                SchImageAssetEdit::Replace {
                    index,
                    name,
                    payload,
                } => {
                    // In-place: positions are unchanged; a rename of a pairing
                    // target is caught by the final-state check in step 9.
                    working
                        .replace_image_resource(index, name, payload)
                        .with_context(context)?;
                }
                SchImageAssetEdit::Remove { index } => {
                    working.remove_image_resource(index).with_context(context)?;
                    for slot in &mut input_asset_positions {
                        match slot {
                            Some(current) if *current == index => *slot = None,
                            Some(current) if *current > index => *current -= 1,
                            _ => {}
                        }
                    }
                }
            }
        }

        // 3. Component slot resolution against the immutable input.
        let input_components = &self.library.components;
        let input_indices = &self.library.header.components;
        let mut referenced: HashSet<usize> = HashSet::new();
        let mut slots: Vec<FinalSlot> = Vec::new();
        match edit.components {
            None => {
                for (component, index) in input_components.iter().zip(input_indices) {
                    referenced.insert(slots.len());
                    slots.push(FinalSlot {
                        component: component.clone(),
                        index: index.clone(),
                        alias_touched: false,
                    });
                }
            }
            Some(component_edits) => {
                for (position, component_edit) in component_edits.into_iter().enumerate() {
                    let slot = self
                        .resolve_component_edit(component_edit, &mut referenced, &working)
                        .with_context(|| format!("component slot #{position}"))?;
                    slots.push(slot);
                }
            }
        }
        let removed: Vec<usize> = (0..input_components.len())
            .filter(|index| !referenced.contains(index))
            .collect();
        if !removed.is_empty() && !edit.allow_component_removal {
            let listed: Vec<String> = removed
                .iter()
                .map(|&index| format!("component[{index}] '{}'", input_indices[index].lib_ref))
                .collect();
            return Err(invalid(
                "component slot",
                format!(
                    "the final sequence omits {} (deletion of their complete aggregates); \
                     removal requires allow_component_removal",
                    listed.join(", ")
                ),
            ));
        }

        // Final lib_ref uniqueness (needed before alias ops resolve names).
        // Uniqueness is checked on the storage-namespace key: the CFB
        // directory namespace is case-insensitive, so a case-variant name
        // could never be saved as a separate storage.
        let mut final_by_name: HashMap<String, usize> = HashMap::new();
        let mut final_by_key: HashMap<String, usize> = HashMap::new();
        for (position, slot) in slots.iter().enumerate() {
            if slot.index.lib_ref.is_empty() {
                return Err(invalid(
                    "lib_reference",
                    format!("component slot #{position} has an empty lib reference"),
                ));
            }
            if let Some(previous) =
                final_by_key.insert(storage_namespace_key(&slot.index.lib_ref), position)
            {
                let previous_name = &slots[previous].index.lib_ref;
                return Err(invalid(
                    "lib_reference",
                    format!(
                        "final component slots #{previous} and #{position} both use lib \
                         reference '{}'{}",
                        slot.index.lib_ref,
                        namespace_collision_note(previous_name, &slot.index.lib_ref)
                    ),
                ));
            }
            final_by_name.insert(slot.index.lib_ref.clone(), position);
        }

        // 4. Alias ops: removals first (so remove+add retargets), then adds.
        //    Removals may address a component this edit removes, releasing
        //    its alias so the removal cannot dangle.
        let mut removed_aliases: Vec<(usize, String, Vec<String>)> = removed
            .iter()
            .map(|&index| {
                (
                    index,
                    input_indices[index].lib_ref.clone(),
                    input_indices[index].aliases.clone(),
                )
            })
            .collect();
        let (removes, adds): (Vec<SchAliasOp>, Vec<SchAliasOp>) = edit
            .alias_ops
            .into_iter()
            .partition(|op| matches!(op, SchAliasOp::Remove { .. }));
        for op in removes {
            let SchAliasOp::Remove { component, alias } = op else {
                unreachable!("partitioned on Remove")
            };
            // Resolution order: the final component actually carrying the
            // alias, then a removed input component of that name. The two can
            // only coexist when a NEW component re-takes a removed name, and
            // a New slot cannot carry the alias yet (removals apply before
            // additions), so the resolution is unambiguous.
            let final_slot = final_by_name.get(&component).copied();
            if let Some(slot) = final_slot
                && let Some(position) = slots[slot]
                    .index
                    .aliases
                    .iter()
                    .position(|existing| *existing == alias)
            {
                slots[slot].index.aliases.remove(position);
                slots[slot].alias_touched = true;
                continue;
            }
            let removed_entry = removed_aliases
                .iter_mut()
                .find(|(_, lib_ref, _)| *lib_ref == component);
            if let Some(entry) = removed_entry {
                if let Some(position) = entry.2.iter().position(|existing| *existing == alias) {
                    entry.2.remove(position);
                    continue;
                }
                return Err(invalid(
                    "alias",
                    format!("alias '{alias}' is not present on removed component '{component}'"),
                ));
            }
            if final_slot.is_some() {
                return Err(invalid(
                    "alias",
                    format!("alias '{alias}' is not present on component '{component}'"),
                ));
            }
            return Err(invalid(
                "alias",
                format!(
                    "alias removal targets unknown component '{component}' (neither a \
                     final nor a removed component of this edit)"
                ),
            ));
        }
        for op in adds {
            let SchAliasOp::Add { component, alias } = op else {
                unreachable!("partitioned on Remove")
            };
            if alias.is_empty() {
                return Err(invalid("alias", "alias names must not be empty"));
            }
            if alias.contains(',') {
                return Err(invalid(
                    "alias",
                    format!(
                        "alias '{alias}' contains a comma, which the native AliasList \
                         comma-join cannot represent"
                    ),
                ));
            }
            let Some(&slot) = final_by_name.get(&component) else {
                return Err(invalid(
                    "alias",
                    format!(
                        "alias addition targets unknown component '{component}' (removed \
                         components cannot receive aliases)"
                    ),
                ));
            };
            if slots[slot].index.aliases.contains(&alias) {
                return Err(invalid(
                    "alias",
                    format!("alias '{alias}' is already present on component '{component}'"),
                ));
            }
            slots[slot].index.aliases.push(alias);
            slots[slot].alias_touched = true;
        }
        for (index, lib_ref, remaining) in &removed_aliases {
            if !remaining.is_empty() {
                return Err(invalid(
                    "alias",
                    format!(
                        "removing component[{index}] '{lib_ref}' would dangle its alias \
                         redirection(s) {remaining:?}; remove or retarget them in the same edit"
                    ),
                ));
            }
        }

        // 5. Namespace uniqueness across the final component/alias name set,
        //    keyed by the storage-namespace form (the CFB directory namespace
        //    is case-insensitive): alias names are globally unique and collide
        //    with no OTHER component's lib_ref; the self-alias (alias EXACTLY
        //    equal to its own component's name, sharing that component's
        //    storage) is legal, while a case-variant of the own name is a
        //    collision — the save path shares storage on exact equality only.
        let mut used_names: HashMap<String, (String, String)> = slots
            .iter()
            .map(|slot| {
                (
                    storage_namespace_key(&slot.index.lib_ref),
                    (
                        slot.index.lib_ref.clone(),
                        format!("lib reference of component '{}'", slot.index.lib_ref),
                    ),
                )
            })
            .collect();
        for slot in &slots {
            for alias in &slot.index.aliases {
                if *alias == slot.index.lib_ref {
                    continue; // self-alias: shares the component's storage
                }
                let use_description = format!("alias of component '{}'", slot.index.lib_ref);
                if let Some((previous_name, previous)) = used_names.insert(
                    storage_namespace_key(alias),
                    (alias.clone(), use_description),
                ) {
                    return Err(invalid(
                        "alias",
                        format!(
                            "name '{alias}' is already used as the {previous}{}",
                            namespace_collision_note(&previous_name, alias)
                        ),
                    ));
                }
            }
        }

        // 6. Root-record AliasList resync for alias-touched components.
        for slot in &mut slots {
            if slot.alias_touched {
                slot.component.component.alias_list = slot.index.aliases.join(",");
            }
        }

        // 7. Global alias redirections: preserved verbatim when the ordered
        //    header pair sequence is unchanged, otherwise rebuilt in header
        //    order (the parse order).
        let pair_sequence = |indices: &[SchLibComponentIndex]| -> Vec<(String, String)> {
            indices
                .iter()
                .flat_map(|index| {
                    index
                        .aliases
                        .iter()
                        .map(|alias| (alias.clone(), index.lib_ref.clone()))
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let final_indices: Vec<SchLibComponentIndex> =
            slots.iter().map(|slot| slot.index.clone()).collect();
        let final_pairs = pair_sequence(&final_indices);
        working.aliases = if final_pairs == pair_sequence(input_indices) {
            self.library.aliases.clone()
        } else {
            final_pairs
                .into_iter()
                .map(|(alias_name, canonical_name)| SchLibAlias {
                    alias_name,
                    canonical_name,
                })
                .collect()
        };

        // 8. Weight: the canonical cardinality delta applied to the stored
        //    value (input convention preserved, never recanonicalized).
        let final_components: Vec<SchLibComponent> =
            slots.into_iter().map(|slot| slot.component).collect();
        let delta = canonical_total(&final_components, &final_indices)?
            - canonical_total(input_components, input_indices)?;
        working.header.weight = i64::from(self.library.header.weight)
            .checked_add(delta)
            .and_then(|weight| i32::try_from(weight).ok())
            .ok_or_else(|| {
                invalid(
                    "WEIGHT",
                    "resulting native header weight overflows the stored field",
                )
            })?;
        working.components = final_components;
        working.header.components = final_indices;

        // 9. Embedded-image reference validation (memo §8c, per pairing
        //    target): each reference is resolved against the INPUT asset list
        //    to its concrete pairing target (first exact, then first
        //    case-insensitive — the native order), and that specific asset
        //    must survive the edit with its name intact. A same-named or
        //    case-variant sibling never substitutes for the target (the
        //    record would silently rebind to a different asset body); wild
        //    dangling references stay preserved.
        let input_assets: Vec<&str> = self
            .library
            .embedded_images
            .iter()
            .map(|image| image.file_name.as_str())
            .collect();
        for (component, index) in working.components.iter().zip(&working.header.components) {
            for record in component
                .records
                .iter()
                .chain(&component.additional_records)
            {
                let SchRecord::Image(image) = record else {
                    continue;
                };
                if !image.embed_image {
                    continue;
                }
                let Some(target) =
                    resolve_image_asset_reference(input_assets.iter().copied(), &image.file_name)
                else {
                    continue; // already dangling in the input: preserved, never blocks
                };
                let survives = input_asset_positions[target].is_some_and(|final_position| {
                    working.embedded_images[final_position].file_name == input_assets[target]
                });
                if !survives {
                    return Err(invalid(
                        "FileName",
                        format!(
                            "component '{}': an embedded image record references storage \
                             asset '{}' (pairing target at input position {target}), which \
                             this edit removes or renames; remove the referencing record in \
                             the same transition or keep the asset",
                            index.lib_ref, image.file_name
                        ),
                    ));
                }
            }
        }

        validate_snapshot_library(&working).context("validating SchLib library edit result")?;
        Ok(Self {
            reference_context: Arc::new(ReferenceContext::capture(&working)),
            library: working,
        })
    }

    fn resolve_component_edit(
        &self,
        component_edit: SchLibComponentEdit,
        referenced: &mut HashSet<usize>,
        working: &crate::schlib::SchLib,
    ) -> Result<FinalSlot> {
        let input_len = self.library.components.len();
        let mut claim_slot = |index: usize| -> Result<()> {
            if index >= input_len {
                return Err(invalid(
                    "component slot",
                    format!("input index {index} exceeds component count {input_len}"),
                ));
            }
            if !referenced.insert(index) {
                return Err(invalid(
                    "component slot",
                    format!("input component[{index}] is referenced more than once"),
                ));
            }
            Ok(())
        };
        match component_edit {
            SchLibComponentEdit::Keep(index) => {
                claim_slot(index)?;
                Ok(FinalSlot {
                    component: self.library.components[index].clone(),
                    index: self.library.header.components[index].clone(),
                    alias_touched: false,
                })
            }
            SchLibComponentEdit::ReplaceWith(index, replacement) => {
                claim_slot(index)?;
                let existing = &self.library.header.components[index];
                if replacement.reference_context != self.reference_context {
                    return Err(invalid(
                        "component reference context",
                        format!(
                            "replacing component[{index}] '{}': captured \
                             library/font/style/image/alias namespace differs from the \
                             input's; build replacements from this input snapshot (the \
                             declared transition recomputes the RESULT's context)",
                            existing.lib_ref
                        ),
                    ));
                }
                if existing.lib_ref != replacement.index.lib_ref {
                    return Err(invalid(
                        "component reference",
                        format!(
                            "replacing component[{index}] '{}' with '{}': renaming is \
                             unsupported (clone under the new name and remove the \
                             original instead)",
                            existing.lib_ref, replacement.index.lib_ref
                        ),
                    ));
                }
                if existing.aliases != replacement.index.aliases {
                    return Err(invalid(
                        "alias",
                        format!(
                            "replacing component[{index}] '{}': the replacement changes \
                             the alias list; alias changes go through SchAliasOp",
                            existing.lib_ref
                        ),
                    ));
                }
                Ok(FinalSlot {
                    component: replacement.component,
                    index: replacement.index,
                    alias_touched: false,
                })
            }
            SchLibComponentEdit::New(SchNewComponent::CloneOf {
                source,
                lib_reference,
            }) => {
                if lib_reference.is_empty() {
                    return Err(invalid(
                        "lib_reference",
                        "new component name must not be empty",
                    ));
                }
                let Some(source_component) = self.library.components.get(source) else {
                    return Err(invalid(
                        "component slot",
                        format!("clone source index {source} exceeds component count {input_len}"),
                    ));
                };
                let mut component = source_component.clone();
                component.component.lib_reference = lib_reference.clone();
                component.component.unique_id = generate_unique_id();
                component.component.alias_list = String::new();
                let mut index = self.library.header.components[source].clone();
                index.lib_ref = lib_reference;
                index.aliases = Vec::new();
                Ok(FinalSlot {
                    component,
                    index,
                    alias_touched: false,
                })
            }
            SchLibComponentEdit::New(SchNewComponent::Blank { lib_reference }) => {
                if lib_reference.is_empty() {
                    return Err(invalid(
                        "lib_reference",
                        "new component name must not be empty",
                    ));
                }
                let (component, index) = blank_ad26_component(&lib_reference)
                    .with_context(|| format!("creating blank component '{lib_reference}'"))?;
                check_font_id_exists(&working.header.fonts, 1).with_context(|| {
                    format!(
                        "creating blank component '{lib_reference}': its records reference \
                         font id 1 (append a font in the same edit for an empty table)"
                    )
                })?;
                Ok(FinalSlot {
                    component,
                    index,
                    alias_touched: false,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param_collection::ParameterCollection;
    use crate::sch_records::{SchImage, SchRectangle};
    use crate::schlib::{
        SchChildBatch, SchGraphicDetail, SchGraphicEdit, SchGraphicInit, SchGraphicShape, SchLib,
        SchLibEmbeddedImage, SchParameterEdit, SchParameterInit, compute_weight,
    };
    use altium_format_types::{Color, CoordPoint, PenWidth, RotationBy90};

    /// 42 characters — beyond the 31-character CFB storage-name limit, so the
    /// save path must route it through SectionKeys.
    const LONG_NAME: &str = "STEP2D_CLONE_WITH_A_VERY_LONG_STORAGE_NAME";

    /// Two components; component 0 carries a rectangle and an embedded image
    /// record referencing `logo.png`; the store holds that asset plus an
    /// unreferenced orphan (the wild SMotlaq class).
    fn library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        let mut second = library.components[0].clone();
        second.component.lib_reference = "Second".to_owned();
        second.component.unique_id = "SECOND01".to_owned();
        let mut second_index = library.header.components[0].clone();
        second_index.lib_ref = "Second".to_owned();
        library.components.push(second);
        library.header.components.push(second_index);

        let mut rectangle = SchRectangle::from_params(&mut ParameterCollection::new()).unwrap();
        rectangle.base.owner_part_id = 1;
        rectangle.unique_id = "RECT0001".to_owned();
        library.components[0]
            .records
            .push(crate::sch_records::SchRecord::Rectangle(rectangle));
        let mut image = SchImage::from_params(&mut ParameterCollection::new()).unwrap();
        image.base.owner_part_id = 1;
        image.embed_image = true;
        image.file_name = "logo.png".to_owned();
        image.unique_id = "IMG00001".to_owned();
        library.components[0]
            .records
            .push(crate::sch_records::SchRecord::Image(image));
        library.embedded_images = vec![
            SchLibEmbeddedImage {
                file_name: "logo.png".to_owned(),
                data: vec![1, 2, 3],
            },
            SchLibEmbeddedImage {
                file_name: "orphan.bin".to_owned(),
                data: vec![9],
            },
        ];
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        library
    }

    fn add(component: &str, alias: &str) -> SchAliasOp {
        SchAliasOp::Add {
            component: component.to_owned(),
            alias: alias.to_owned(),
        }
    }

    fn remove(component: &str, alias: &str) -> SchAliasOp {
        SchAliasOp::Remove {
            component: component.to_owned(),
            alias: alias.to_owned(),
        }
    }

    #[test]
    fn empty_and_all_keep_edits_are_context_and_state_identical() {
        let library = library();
        let input = library.snapshot().unwrap();
        let unchanged = input.with_library_edit(SchLibEdit::default()).unwrap();
        assert_eq!(unchanged, input);
        let all_keep = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all_keep, input);
        // A state-changing edit compares unequal (replan sees the difference).
        let edited = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![add("Second", "ALT")],
                ..Default::default()
            })
            .unwrap();
        assert_ne!(edited, input);
        assert_eq!(input.materialize().unwrap(), library, "input never mutated");
    }

    #[test]
    fn font_appends_recompute_context_and_unlock_child_creation() {
        // A library whose captured table lacks the native default entry: the
        // recorded increment-B limitation blocks parameter creation.
        let mut base = library();
        base.header.fonts[0].name = "Arial".to_owned();
        base.header.fonts[0].size = 8;
        base.header.weight = compute_weight(&base.header, &base.components);
        let input = base.snapshot().unwrap();
        let parameter_batch = SchChildBatch {
            parameters: Some(vec![
                SchParameterEdit::Keep(0),
                SchParameterEdit::New(SchParameterInit {
                    name: "Tolerance".to_owned(),
                    text: "1%".to_owned(),
                    is_hidden: false,
                }),
            ]),
            ..Default::default()
        };
        let error = input
            .component(0)
            .unwrap()
            .with_children(parameter_batch.clone())
            .unwrap_err()
            .to_string();
        assert!(error.contains("with_library_edit"), "{error}");

        // The declared append recomputes the context from the final state.
        let extended = input
            .with_library_edit(SchLibEdit {
                font_appends: vec![SchFontAppend::native_default()],
                ..Default::default()
            })
            .unwrap();
        assert_ne!(extended, input);
        let component = extended.component(0).unwrap();
        let fonts: Vec<_> = component.captured_fonts().collect();
        assert_eq!(fonts.len(), 2);
        assert_eq!(
            (fonts[1].id, fonts[1].name, fonts[1].size),
            (2, "Times New Roman", 10)
        );

        // End-to-end: the extended snapshot's components can create the
        // parameter, and the default resolves to the appended entry.
        let edited = component.with_children(parameter_batch).unwrap();
        let created = edited
            .component
            .records
            .iter()
            .find_map(|record| match record {
                crate::sch_records::SchRecord::Parameter(parameter)
                    if parameter.name == "Tolerance" =>
                {
                    Some(parameter)
                }
                _ => None,
            })
            .expect("created parameter");
        assert_eq!(created.font_id, 2, "resolves the appended default entry");
        let updated = extended.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        updated.materialize().unwrap().save(file.path()).unwrap();
        assert_eq!(
            SchLib::open(file.path()).unwrap().snapshot().unwrap(),
            updated
        );

        // The strict path still rejects UNDECLARED context drift.
        let drift = input
            .replace_component(0, extended.component(0).unwrap())
            .unwrap_err()
            .to_string();
        assert!(drift.contains("namespace differs"), "{drift}");

        // Rejections: axis-identical duplicates (case-insensitive name, in
        // the table or within one edit), empty names, non-sequential tables.
        let duplicate = extended
            .with_library_edit(SchLibEdit {
                font_appends: vec![SchFontAppend {
                    name: "times new roman".to_owned(),
                    ..SchFontAppend::native_default()
                }],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(duplicate.contains("already exists as id 2"), "{duplicate}");
        let twice = input
            .with_library_edit(SchLibEdit {
                font_appends: vec![
                    SchFontAppend::native_default(),
                    SchFontAppend::native_default(),
                ],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(twice.contains("already exists"), "{twice}");
        let empty_name = input
            .with_library_edit(SchLibEdit {
                font_appends: vec![SchFontAppend {
                    name: String::new(),
                    ..SchFontAppend::native_default()
                }],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(empty_name.contains("must not be empty"), "{empty_name}");
        let mut broken = library();
        broken.header.fonts[0].id = 5;
        let non_sequential = broken
            .snapshot()
            .unwrap()
            .with_library_edit(SchLibEdit {
                font_appends: vec![SchFontAppend::native_default()],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(
            non_sequential.contains("non-sequential"),
            "{non_sequential}"
        );
    }

    #[test]
    fn alias_ops_synchronize_all_three_representations_and_reject_conflicts() {
        let library = library();
        let input = library.snapshot().unwrap();
        let edited = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![
                    add("Component_1", "ALIAS_B"),
                    add("Component_1", "ALIAS_A"),
                    // The self-alias storage-sharing case.
                    add("Second", "Second"),
                ],
                ..Default::default()
            })
            .unwrap();
        let lib = edited.materialize().unwrap();
        assert_eq!(lib.header.components[0].aliases, vec!["ALIAS_B", "ALIAS_A"]);
        assert_eq!(lib.components[0].component.alias_list, "ALIAS_B,ALIAS_A");
        assert_eq!(lib.header.components[1].aliases, vec!["Second"]);
        assert_eq!(lib.components[1].component.alias_list, "Second");
        assert_eq!(
            lib.aliases,
            vec![
                SchLibAlias {
                    alias_name: "ALIAS_B".to_owned(),
                    canonical_name: "Component_1".to_owned(),
                },
                SchLibAlias {
                    alias_name: "ALIAS_A".to_owned(),
                    canonical_name: "Component_1".to_owned(),
                },
                SchLibAlias {
                    alias_name: "Second".to_owned(),
                    canonical_name: "Second".to_owned(),
                },
            ]
        );
        assert_eq!(lib.header.weight, library.header.weight + 3);
        assert_eq!(
            edited.component(0).unwrap().aliases().collect::<Vec<_>>(),
            vec!["ALIAS_B", "ALIAS_A"]
        );

        // Save/reopen typed equality, incl. the self-alias Redirection that
        // shares its component's storage.
        let file = tempfile::NamedTempFile::new().unwrap();
        lib.save(file.path()).unwrap();
        assert_eq!(
            SchLib::open(file.path()).unwrap().snapshot().unwrap(),
            edited
        );

        // Removal resyncs all three representations; determinism: the same
        // edit on the same input yields the identical snapshot.
        let removal = SchLibEdit {
            alias_ops: vec![remove("Component_1", "ALIAS_B")],
            ..Default::default()
        };
        let removed = edited.with_library_edit(removal.clone()).unwrap();
        assert_eq!(removed, edited.with_library_edit(removal).unwrap());
        let lib = removed.materialize().unwrap();
        assert_eq!(lib.header.components[0].aliases, vec!["ALIAS_A"]);
        assert_eq!(lib.components[0].component.alias_list, "ALIAS_A");
        assert_eq!(lib.aliases.len(), 2);
        assert_eq!(lib.header.weight, library.header.weight + 2);

        let reject = |ops: Vec<SchAliasOp>, needle: &str| {
            let error = edited
                .with_library_edit(SchLibEdit {
                    alias_ops: ops,
                    ..Default::default()
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains(needle), "{needle}: {error}");
        };
        reject(vec![add("Component_1", "ALIAS_A")], "already present");
        reject(vec![add("Second", "ALIAS_A")], "already used");
        reject(vec![add("Component_1", "Second")], "already used");
        reject(vec![add("Component_1", "A,B")], "contains a comma");
        reject(vec![add("Component_1", "")], "must not be empty");
        reject(vec![add("Nowhere", "Z")], "unknown component");
        reject(vec![remove("Component_1", "MISSING")], "not present");
        reject(vec![remove("Nowhere", "Z")], "unknown component");
    }

    #[test]
    fn clone_blank_and_removal_manage_namespace_storage_and_weight() {
        let library = library();
        let input = library.snapshot().unwrap();
        let clone_edit = SchLibEdit {
            components: Some(vec![
                SchLibComponentEdit::Keep(0),
                SchLibComponentEdit::Keep(1),
                SchLibComponentEdit::New(SchNewComponent::CloneOf {
                    source: 0,
                    lib_reference: LONG_NAME.to_owned(),
                }),
            ]),
            ..Default::default()
        };
        let cloned = input.with_library_edit(clone_edit.clone()).unwrap();
        assert_eq!(cloned.component_count(), 3);
        let lib = cloned.materialize().unwrap();
        // Deep copy minus name / component UniqueId / alias state.
        let mut expected = library.components[0].clone();
        expected.component.lib_reference = LONG_NAME.to_owned();
        expected.component.unique_id = lib.components[2].component.unique_id.clone();
        expected.component.alias_list = String::new();
        assert_eq!(lib.components[2], expected);
        assert_ne!(
            lib.components[2].component.unique_id,
            library.components[0].component.unique_id
        );
        assert_eq!(lib.header.components[2].lib_ref, LONG_NAME);
        assert!(lib.header.components[2].aliases.is_empty());
        let records = i32::try_from(library.components[0].records.len()).unwrap();
        assert_eq!(lib.header.weight, library.header.weight + records + 1);

        // >31-character storage name routes through SectionKeys on save.
        let file = tempfile::NamedTempFile::new().unwrap();
        lib.save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap().snapshot().unwrap();
        assert_eq!(reopened, cloned);
        assert_eq!(reopened.component(2).unwrap().lib_reference(), LONG_NAME);

        // Determinism: the recomputed context is identical across
        // applications (fresh UniqueIds never enter the context).
        let again = input.with_library_edit(clone_edit).unwrap();
        assert_eq!(again.reference_context, cloned.reference_context);

        // Removal needs the explicit authorization and then restores the
        // original snapshot exactly.
        let unauthorized = reopened
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(
            unauthorized.contains("allow_component_removal"),
            "{unauthorized}"
        );
        assert!(unauthorized.contains(LONG_NAME), "{unauthorized}");
        let restored = reopened
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                ]),
                allow_component_removal: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(restored, input);

        // Blank creation reuses the new_blank_ad26 shape and validates the
        // font its records reference — appended fonts apply first.
        let blanked = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                    SchLibComponentEdit::New(SchNewComponent::Blank {
                        lib_reference: "Fresh".to_owned(),
                    }),
                ]),
                ..Default::default()
            })
            .unwrap();
        let fresh = blanked.component(2).unwrap();
        assert_eq!(fresh.lib_reference(), "Fresh");
        assert_eq!(fresh.designator().unwrap(), Some("U?"));
        assert_eq!(fresh.effective_part_count().unwrap(), 1);
        assert_eq!(
            blanked.materialize().unwrap().components[2].records.len(),
            2
        );
        let mut fontless = library.clone();
        fontless.header.fonts.clear();
        let fontless_input = fontless.snapshot().unwrap();
        let blank_slot = || {
            vec![
                SchLibComponentEdit::Keep(0),
                SchLibComponentEdit::Keep(1),
                SchLibComponentEdit::New(SchNewComponent::Blank {
                    lib_reference: "Fresh".to_owned(),
                }),
            ]
        };
        let blocked = fontless_input
            .with_library_edit(SchLibEdit {
                components: Some(blank_slot()),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(blocked.contains("font id 1"), "{blocked}");
        fontless_input
            .with_library_edit(SchLibEdit {
                components: Some(blank_slot()),
                font_appends: vec![SchFontAppend::native_default()],
                ..Default::default()
            })
            .unwrap();

        // Removing a component with aliases must release or retarget them in
        // the same edit — never a silent drop of Redirection entries.
        let aliased = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![add("Second", "ALT_NAME")],
                ..Default::default()
            })
            .unwrap();
        let dangling = aliased
            .with_library_edit(SchLibEdit {
                components: Some(vec![SchLibComponentEdit::Keep(0)]),
                allow_component_removal: true,
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(dangling.contains("dangle"), "{dangling}");
        assert!(dangling.contains("ALT_NAME"), "{dangling}");
        let released = aliased
            .with_library_edit(SchLibEdit {
                components: Some(vec![SchLibComponentEdit::Keep(0)]),
                alias_ops: vec![remove("Second", "ALT_NAME")],
                allow_component_removal: true,
                ..Default::default()
            })
            .unwrap();
        assert!(released.materialize().unwrap().aliases.is_empty());
        let retargeted = aliased
            .with_library_edit(SchLibEdit {
                components: Some(vec![SchLibComponentEdit::Keep(0)]),
                alias_ops: vec![remove("Second", "ALT_NAME"), add("Component_1", "ALT_NAME")],
                allow_component_removal: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            retargeted
                .component(0)
                .unwrap()
                .aliases()
                .collect::<Vec<_>>(),
            vec!["ALT_NAME"]
        );

        // Slot misuse and namespace conflicts.
        let reject = |components: Vec<SchLibComponentEdit>, needle: &str| {
            let error = input
                .with_library_edit(SchLibEdit {
                    components: Some(components),
                    allow_component_removal: true,
                    ..Default::default()
                })
                .unwrap_err()
                .to_string();
            assert!(error.contains(needle), "{needle}: {error}");
        };
        reject(
            vec![SchLibComponentEdit::Keep(0), SchLibComponentEdit::Keep(0)],
            "referenced more than once",
        );
        reject(
            vec![SchLibComponentEdit::Keep(0), SchLibComponentEdit::Keep(5)],
            "exceeds component count",
        );
        reject(
            vec![
                SchLibComponentEdit::Keep(0),
                SchLibComponentEdit::Keep(1),
                SchLibComponentEdit::New(SchNewComponent::CloneOf {
                    source: 9,
                    lib_reference: "X".to_owned(),
                }),
            ],
            "clone source index 9",
        );
        reject(
            vec![
                SchLibComponentEdit::Keep(0),
                SchLibComponentEdit::Keep(1),
                SchLibComponentEdit::New(SchNewComponent::CloneOf {
                    source: 0,
                    lib_reference: "Second".to_owned(),
                }),
            ],
            "both use lib reference",
        );
        reject(
            vec![
                SchLibComponentEdit::Keep(0),
                SchLibComponentEdit::Keep(1),
                SchLibComponentEdit::New(SchNewComponent::Blank {
                    lib_reference: String::new(),
                }),
            ],
            "must not be empty",
        );
        // A clone under a name already used as an alias collides too.
        let clash = aliased
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                    SchLibComponentEdit::New(SchNewComponent::CloneOf {
                        source: 0,
                        lib_reference: "ALT_NAME".to_owned(),
                    }),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(clash.contains("already used"), "{clash}");

        // ReplaceWith keeps the strict rules: input context, no renames, no
        // alias changes outside SchAliasOp. A replacement carrying another
        // input slot's name is a rename and is rejected with the clone
        // guidance.
        let renamed = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWith(0, input.component(1).unwrap()),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(renamed.contains("renaming is unsupported"), "{renamed}");
        let mut foreign_library = library.clone();
        foreign_library.header.unique_id.push_str("-different");
        let foreign = foreign_library.snapshot().unwrap().component(0).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWith(0, foreign),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("namespace differs"), "{error}");
        let mut alias_smuggle = input.component(0).unwrap();
        alias_smuggle.index.aliases.push("SMUGGLED".to_owned());
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWith(0, alias_smuggle),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("SchAliasOp"), "{error}");
    }

    #[test]
    fn image_asset_ops_wrap_schlib_resources_with_reference_validation() {
        let library = library();
        let input = library.snapshot().unwrap();

        // Removing the unreferenced orphan asset is legal (the wild SMotlaq
        // class keeps loading; only the mutation is namespace-checked).
        let removed_orphan = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Remove { index: 1 }],
                ..Default::default()
            })
            .unwrap();
        assert_ne!(removed_orphan, input);
        let lib = removed_orphan.materialize().unwrap();
        assert_eq!(lib.embedded_images.len(), 1);
        let file = tempfile::NamedTempFile::new().unwrap();
        lib.save(file.path()).unwrap();
        assert_eq!(
            SchLib::open(file.path()).unwrap().snapshot().unwrap(),
            removed_orphan
        );

        // Removing or renaming the referenced asset is blocked (§8c rule 3).
        let blocked = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Remove { index: 0 }],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(blocked.contains("logo.png"), "{blocked}");
        assert!(blocked.contains("Component_1"), "{blocked}");
        let renamed = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Replace {
                    index: 0,
                    name: "other.png".to_owned(),
                    payload: vec![7],
                }],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(renamed.contains("logo.png"), "{renamed}");

        // Same-name payload replacement is legal and recomputes the context
        // (the asset hash changes).
        let repainted = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Replace {
                    index: 0,
                    name: "logo.png".to_owned(),
                    payload: vec![9, 9],
                }],
                ..Default::default()
            })
            .unwrap();
        assert_ne!(repainted.reference_context, input.reference_context);

        // Case-insensitive references block removal too (native pairing).
        let mut ci = library.clone();
        for record in &mut ci.components[0].records {
            if let SchRecord::Image(image) = record {
                image.file_name = "LOGO.PNG".to_owned();
            }
        }
        assert!(
            ci.snapshot()
                .unwrap()
                .with_library_edit(SchLibEdit {
                    image_assets: vec![SchImageAssetEdit::Remove { index: 0 }],
                    ..Default::default()
                })
                .is_err()
        );

        // Removing the referencing record in the SAME edit unblocks the
        // asset removal (validation runs over the final state).
        let component = input.component(0).unwrap();
        let keeps: Vec<SchGraphicEdit> = component
            .graphics()
            .iter()
            .enumerate()
            .filter(|(_, view)| !matches!(view.detail, SchGraphicDetail::Image { .. }))
            .map(|(index, _)| SchGraphicEdit::Keep(index))
            .collect();
        let without_image = component
            .with_children(SchChildBatch {
                graphics: Some(keeps),
                footprints: None,
                allow_deletion: true,
                ..Default::default()
            })
            .unwrap();
        let cleared = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWith(0, without_image),
                    SchLibComponentEdit::Keep(1),
                ]),
                image_assets: vec![SchImageAssetEdit::Remove { index: 0 }],
                ..Default::default()
            })
            .unwrap();
        let lib = cleared.materialize().unwrap();
        assert_eq!(lib.embedded_images.len(), 1);
        assert_eq!(lib.embedded_images[0].file_name, "orphan.bin");

        // §8c is a per-pairing-target rule: with duplicate asset names the
        // record's concrete target (the FIRST exact match) is protected, and
        // a same-named sibling is no substitute — removing the target would
        // silently rebind the record to a different asset body.
        let mut duplicated = library.clone();
        duplicated.embedded_images.push(SchLibEmbeddedImage {
            file_name: "logo.png".to_owned(),
            data: vec![7, 7],
        });
        let dup_input = duplicated.snapshot().unwrap();
        let remove_at = |index| SchLibEdit {
            image_assets: vec![SchImageAssetEdit::Remove { index }],
            ..Default::default()
        };
        let rebind = dup_input
            .with_library_edit(remove_at(0))
            .unwrap_err()
            .to_string();
        assert!(
            rebind.contains("pairing target at input position 0"),
            "{rebind}"
        );
        let kept = dup_input.with_library_edit(remove_at(2)).unwrap();
        assert_eq!(kept.materialize().unwrap().embedded_images.len(), 2);

        // Pairing-target tracking follows insert shifts: after inserting at
        // position 0 the referenced target sits at 1 and removing it is still
        // blocked, while removing the freshly inserted asset stays legal.
        let shifted = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![
                    SchImageAssetEdit::Insert {
                        index: 0,
                        name: "front.png".to_owned(),
                        payload: vec![1],
                    },
                    SchImageAssetEdit::Remove { index: 1 },
                ],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(shifted.contains("logo.png"), "{shifted}");
        input
            .with_library_edit(SchLibEdit {
                image_assets: vec![
                    SchImageAssetEdit::Insert {
                        index: 0,
                        name: "front.png".to_owned(),
                        payload: vec![1],
                    },
                    SchImageAssetEdit::Remove { index: 0 },
                ],
                ..Default::default()
            })
            .unwrap();

        // Wrapped-op errors carry the op position and the SchLib context.
        let bad = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Insert {
                    index: 9,
                    name: "late.png".to_owned(),
                    payload: vec![1],
                }],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(bad.contains("image asset op #0"), "{bad}");

        // A reference that was ALREADY dangling in the input stays preserved:
        // it never blocks unrelated asset ops.
        let mut wild = library.clone();
        for record in &mut wild.components[0].records {
            if let SchRecord::Image(image) = record {
                image.file_name = "never-present.png".to_owned();
            }
        }
        wild.snapshot()
            .unwrap()
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Remove { index: 1 }],
                ..Default::default()
            })
            .unwrap();
    }

    #[test]
    fn namespace_collisions_are_rejected_case_insensitively() {
        let input = library().snapshot().unwrap();

        // A clone under a case-variant of an existing lib_ref could never be
        // a separate CFB storage: rejected at the declared boundary, not at
        // save time.
        let clash = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                    SchLibComponentEdit::New(SchNewComponent::CloneOf {
                        source: 0,
                        lib_reference: "COMPONENT_1".to_owned(),
                    }),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(clash.contains("both use lib reference"), "{clash}");
        assert!(clash.contains("case-insensitive"), "{clash}");

        // A case-variant alias of ANOTHER component's lib_ref collides.
        let alias_clash = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![add("Component_1", "second")],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(alias_clash.contains("already used"), "{alias_clash}");
        assert!(alias_clash.contains("case-insensitive"), "{alias_clash}");

        // A case-variant of the OWN lib_ref is NOT a self-alias: the save
        // path shares storage on exact equality only, so this collides too.
        let self_variant = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![add("Second", "SECOND")],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(self_variant.contains("already used"), "{self_variant}");

        // Two case-variant alias additions within one edit collide with each
        // other.
        let dual = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![add("Component_1", "ALT"), add("Second", "alt")],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(dual.contains("already used"), "{dual}");

        // Names colliding only after illegal-character sanitize could never
        // be distinct storages either.
        let sanitized = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                    SchLibComponentEdit::New(SchNewComponent::Blank {
                        lib_reference: "A_B".to_owned(),
                    }),
                    SchLibComponentEdit::New(SchNewComponent::Blank {
                        lib_reference: "a?b".to_owned(),
                    }),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(sanitized.contains("both use lib reference"), "{sanitized}");
    }

    #[test]
    fn section_keys_disambiguate_truncation_collisions_in_any_header_order() {
        let input = library().snapshot().unwrap();
        // Storage-key torture set, deliberately ordered LONG BEFORE SHORT so
        // key generation cannot depend on header order:
        // - a 42-char name whose 31-char truncation equals a later 31-char
        //   component name;
        // - two 32-char case-variant names (distinct suffixes keep their full
        //   folds distinct) whose truncated keys collide case-insensitively.
        let long_a = "A".repeat(42);
        let short_a = "A".repeat(31);
        let long_b1 = format!("{}1", "B".repeat(31));
        let long_b2 = format!("{}2", "b".repeat(31));
        let clone_as = |names: &[&String]| SchLibEdit {
            components: Some(
                [SchLibComponentEdit::Keep(0), SchLibComponentEdit::Keep(1)]
                    .into_iter()
                    .chain(names.iter().map(|name| {
                        SchLibComponentEdit::New(SchNewComponent::CloneOf {
                            source: 0,
                            lib_reference: (*name).clone(),
                        })
                    }))
                    .collect(),
            ),
            ..Default::default()
        };
        let cloned = input
            .with_library_edit(clone_as(&[&long_a, &short_a, &long_b1, &long_b2]))
            .unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        cloned.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap().snapshot().unwrap();
        assert_eq!(reopened, cloned);
        for (slot, name) in [(2, &long_a), (3, &short_a), (4, &long_b1), (5, &long_b2)] {
            assert_eq!(reopened.component(slot).unwrap().lib_reference(), name);
        }

        // Full-name case variants (fold-equal whole names) stay rejected: two
        // 42-char names differing only by case are one storage identity.
        let case_variant_long = "a".repeat(42);
        let rejected = input
            .with_library_edit(clone_as(&[&long_a, &case_variant_long]))
            .unwrap_err()
            .to_string();
        assert!(rejected.contains("both use lib reference"), "{rejected}");
    }

    #[test]
    fn removed_component_aliases_release_when_a_new_component_retakes_the_name() {
        let input = library().snapshot().unwrap();
        let aliased = input
            .with_library_edit(SchLibEdit {
                alias_ops: vec![add("Component_1", "OLD_ALT")],
                ..Default::default()
            })
            .unwrap();
        // Content-reset-under-the-same-name flow in ONE edit: remove the
        // aliased original, create a NEW component (fresh UniqueId) under the
        // same lib_ref, and release the removed component's alias — the
        // Remove resolves to the REMOVED component because the new slot
        // cannot carry the alias yet.
        let reset_edit = |alias_ops| SchLibEdit {
            components: Some(vec![
                SchLibComponentEdit::Keep(1),
                SchLibComponentEdit::New(SchNewComponent::CloneOf {
                    source: 0,
                    lib_reference: "Component_1".to_owned(),
                }),
            ]),
            alias_ops,
            allow_component_removal: true,
            ..Default::default()
        };
        let dangling = aliased
            .with_library_edit(reset_edit(Vec::new()))
            .unwrap_err()
            .to_string();
        assert!(dangling.contains("dangle"), "{dangling}");
        assert!(dangling.contains("OLD_ALT"), "{dangling}");
        let reset = aliased
            .with_library_edit(reset_edit(vec![remove("Component_1", "OLD_ALT")]))
            .unwrap();
        let lib = reset.materialize().unwrap();
        assert_eq!(lib.header.components[1].lib_ref, "Component_1");
        assert!(lib.header.components[1].aliases.is_empty());
        assert_eq!(lib.components[1].component.alias_list, "");
        assert!(lib.aliases.is_empty());
        assert_ne!(
            lib.components[1].component.unique_id,
            aliased.materialize().unwrap().components[0]
                .component
                .unique_id,
            "the re-taken name carries a NEW component identity"
        );

        // The released alias may be retargeted within the same edit (removes
        // apply before adds).
        let retargeted = aliased
            .with_library_edit(reset_edit(vec![
                remove("Component_1", "OLD_ALT"),
                add("Second", "OLD_ALT"),
            ]))
            .unwrap();
        assert_eq!(
            retargeted
                .component(0)
                .unwrap()
                .aliases()
                .collect::<Vec<_>>(),
            vec!["OLD_ALT"]
        );

        // An alias present on NEITHER the final nor the removed component of
        // that name still fails loudly.
        let missing = aliased
            .with_library_edit(reset_edit(vec![remove("Component_1", "NOWHERE")]))
            .unwrap_err()
            .to_string();
        assert!(missing.contains("not present"), "{missing}");
    }

    #[test]
    fn image_asset_insert_unlocks_two_step_record_creation() {
        let input = library().snapshot().unwrap();
        let image_init = SchGraphicInit {
            owner_part_id: 1,
            owner_part_display_mode: None,
            shape: SchGraphicShape::Image {
                from: CoordPoint::zero(),
                to: CoordPoint::zero(),
                orientation: RotationBy90::Rotate0,
                line_width: PenWidth::Small,
                color: Color::BLACK,
                is_solid: false,
                keep_aspect: true,
                embed_image: true,
                file_name: "inserted.png".to_owned(),
            },
        };
        let component_batch = || SchChildBatch {
            graphics: Some(vec![
                SchGraphicEdit::Keep(0),
                SchGraphicEdit::Keep(1),
                SchGraphicEdit::New(image_init.clone()),
            ]),
            ..Default::default()
        };
        // Referencing the not-yet-inserted asset is blocked against the INPUT
        // context (memo §8c rule 2).
        let blocked = input
            .component(0)
            .unwrap()
            .with_children(component_batch())
            .unwrap_err()
            .to_string();
        assert!(blocked.contains("inserted.png"), "{blocked}");

        // Step 1: the declared insert recomputes the context from the final
        // state.
        let extended = input
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Insert {
                    index: 2,
                    name: "inserted.png".to_owned(),
                    payload: vec![42, 42],
                }],
                ..Default::default()
            })
            .unwrap();
        assert_ne!(extended.reference_context, input.reference_context);
        let lib = extended.materialize().unwrap();
        assert_eq!(lib.embedded_images.len(), 3);
        assert_eq!(lib.embedded_images[2].file_name, "inserted.png");
        assert_eq!(lib.embedded_images[2].data, vec![42, 42]);

        // Step 2: component edits taken from the RESULT snapshot resolve the
        // inserted asset; the composed state save/reopens to typed equality.
        let edited = extended
            .component(0)
            .unwrap()
            .with_children(component_batch())
            .unwrap();
        let updated = extended.replace_component(0, edited).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        updated.materialize().unwrap().save(file.path()).unwrap();
        assert_eq!(
            SchLib::open(file.path()).unwrap().snapshot().unwrap(),
            updated
        );

        // The inserted asset is now a referenced pairing target: a later edit
        // removing it is blocked.
        let later = updated
            .with_library_edit(SchLibEdit {
                image_assets: vec![SchImageAssetEdit::Remove { index: 2 }],
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(later.contains("inserted.png"), "{later}");
    }
}
