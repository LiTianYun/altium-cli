//! Crate-private collection assembly against immutable input positions.
//!
//! This helper does not authenticate an input snapshot. Its caller must bind the
//! aggregate plan to the input snapshot/hash and verify that precondition before
//! materialization. Matching collection lengths alone never proves input identity.
//!
//! # Alignment classes and the partial-scope shape rule
//!
//! Every collection kind has a fixed alignment class
//! (`docs/proposals/snapshot-intent.md`, keyed/positional contract):
//!
//! - **Keyed** (components, parts, parameters, aliases, footprints): entries
//!   align by exact parent-scoped natural keys. The caller resolves keys to
//!   original indices before planning, so a sparse `Replace` index set is the
//!   legitimate product of key matching (and of no-op thinning).
//! - **Positional** (pins, graphics, maps): entries align by index within the
//!   typed owner collection. A partial positional request with N authored
//!   entries describes indices `0..N-1`; the planner cannot distinguish that
//!   contiguous-prefix provenance from a key-matched or no-op-thinned sparse
//!   `Replace` set, so the authored-prefix rule is enforced where index
//!   provenance is known — at the elaboration boundary — not here.
//!
//! What the planner enforces uniformly for BOTH classes
//! ([`reject_partial_interior_insert`], the single named enforcement point):
//! a partial collection updates original slots and appends. Interior insertion
//! (`Insert` with `before != input_len` — the sparse placement form the request
//! shape can express) is a hard error; removal is inexpressible by construction
//! (`IndexedEdit` has no removal variant, because the ratified contract gives a
//! partial removal no legal authorization: `delete: allow` requires
//! `mode: complete`, where removal is expressed by omission).
//!
//! Class-specific rule: a **complete positional** collection pairs authored
//! item N with original destination N for preserved fields, so `Keep`/`Replace`
//! entries must sit at their own original position; re-pairing (`Keep(0)` at
//! final position 1) is a hard error. Keyed complete collections reorder
//! freely — surviving keyed entries take source order.
use std::collections::{BTreeMap, BTreeSet};

use altium_spec_lang::ast::{CollectionDelete, CollectionKind, CollectionMode};
use altium_spec_lang::intent::CollectionScope;

/// The alignment class of a collection kind. Total: every kind has exactly one
/// class under the snapshot-intent contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionClass {
    Keyed,
    Positional,
}

/// The fixed keyed/positional classification of every collection kind.
pub(crate) fn collection_class(kind: CollectionKind) -> CollectionClass {
    match kind {
        CollectionKind::Components
        | CollectionKind::Parts
        | CollectionKind::Parameters
        | CollectionKind::Aliases
        | CollectionKind::Footprints => CollectionClass::Keyed,
        CollectionKind::Pins | CollectionKind::Graphics | CollectionKind::Maps => {
            CollectionClass::Positional
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectionContext {
    pub owner: String,
    pub kind: CollectionKind,
}

impl CollectionContext {
    /// The alignment class the planner learned for this collection.
    pub fn class(&self) -> CollectionClass {
        collection_class(self.kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionErrorKind {
    ModeMismatch,
    OutOfRange,
    RepeatedMutation,
    DeletionDenied,
    InputLengthChanged,
    /// A partial request placed an entry anywhere except an original slot or
    /// the append boundary (both classes; see the module docs).
    PartialInteriorInsert,
    /// A complete positional request re-paired an original entry with a
    /// different final position (positional class only).
    PositionalCompleteMisalignment,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("owner '{owner}', {collection:?} collection, original index {index:?}: {detail}")]
pub(crate) struct CollectionError {
    pub kind: CollectionErrorKind,
    pub owner: String,
    pub collection: CollectionKind,
    pub index: Option<usize>,
    pub detail: String,
}

impl CollectionContext {
    fn error(
        &self,
        kind: CollectionErrorKind,
        index: Option<usize>,
        detail: impl Into<String>,
    ) -> CollectionError {
        CollectionError {
            kind,
            owner: self.owner.clone(),
            collection: self.kind,
            index,
            detail: detail.into(),
        }
    }
}

/// A position validated against this plan's immutable input collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OriginalIndex(usize);
impl OriginalIndex {
    pub fn get(self) -> usize {
        self.0
    }
}

/// One edit of a partial request. Partial scope has no removal form: the
/// ratified contract permits removal only under `mode: complete` (by
/// omission), so no `IndexedEdit` constructor can express one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexedEdit<T> {
    Replace {
        index: usize,
        after: T,
    },
    /// Only `before == input_len` (the append boundary) is legal; interior
    /// boundaries are rejected for both alignment classes (module docs).
    /// Multiple appends retain request order.
    Insert {
        before: usize,
        after: T,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FinalItem<T> {
    Keep { index: usize },
    Replace { index: usize, after: T },
    New { after: T },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CollectionRequest<T> {
    Partial(Vec<IndexedEdit<T>>),
    Complete(Vec<FinalItem<T>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlannedItem<T> {
    Keep { index: OriginalIndex },
    Replace { index: OriginalIndex, after: T },
    New { after: T },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectionPlan<T> {
    input_len: usize,
    context: CollectionContext,
    items: Vec<PlannedItem<T>>,
    removed: Vec<OriginalIndex>,
}
impl<T> CollectionPlan<T> {
    pub fn items(&self) -> &[PlannedItem<T>] {
        &self.items
    }
    pub fn removed(&self) -> &[OriginalIndex] {
        &self.removed
    }
    /// Consume validated slots so an aggregate patch can move replacement values
    /// without cloning every complete original component.
    pub fn into_items(self) -> impl ExactSizeIterator<Item = PlannedItem<T>> {
        self.items.into_iter()
    }
}
impl<T: Clone> CollectionPlan<T> {
    /// Resolve every retained/replaced address against the original input, never
    /// a vector already changed by earlier operations. The aggregate executor
    /// must verify its snapshot/hash precondition before calling this helper.
    pub fn materialize(&self, input: &[T]) -> Result<Vec<T>, CollectionError> {
        if input.len() != self.input_len {
            return Err(self.context.error(
                CollectionErrorKind::InputLengthChanged,
                None,
                format!(
                    "input count changed from {} to {}; replan",
                    self.input_len,
                    input.len()
                ),
            ));
        }
        Ok(self
            .items
            .iter()
            .map(|item| match item {
                PlannedItem::Keep { index } => input[index.get()].clone(),
                PlannedItem::Replace { after, .. } | PlannedItem::New { after } => after.clone(),
            })
            .collect())
    }
}

/// THE named enforcement point for the partial-scope shape rule (both
/// alignment classes): partial = updates in original slots + appends
/// (`docs/proposals/snapshot-intent.md`, partial collection contract).
/// Interior insertion is the sparse placement the request shape can express;
/// it (like any sparse selection) requires a complete collection.
fn reject_partial_interior_insert(
    context: &CollectionContext,
    before: usize,
    input_len: usize,
) -> Result<(), CollectionError> {
    if before == input_len {
        return Ok(());
    }
    Err(context.error(
        CollectionErrorKind::PartialInteriorInsert,
        Some(before),
        format!(
            "partial scope over a {:?} collection updates original slots and appends; \
             interior insertion before original index {before} (append boundary is \
             {input_len}) is sparse placement and requires a complete collection",
            context.class(),
        ),
    ))
}

pub(crate) fn plan_collection<T>(
    input_len: usize,
    scope: &CollectionScope,
    request: CollectionRequest<T>,
    context: CollectionContext,
) -> Result<CollectionPlan<T>, CollectionError> {
    let request_mode = match &request {
        CollectionRequest::Partial(_) => CollectionMode::Partial,
        CollectionRequest::Complete(_) => CollectionMode::Complete,
    };
    if scope.mode != request_mode {
        return Err(context.error(
            CollectionErrorKind::ModeMismatch,
            None,
            format!(
                "{:?} request conflicts with authored {:?} scope",
                request_mode, scope.mode
            ),
        ));
    }
    let index = |index: usize| -> Result<OriginalIndex, CollectionError> {
        if index >= input_len {
            Err(context.error(
                CollectionErrorKind::OutOfRange,
                Some(index),
                format!("no original item at this index (input count {input_len})"),
            ))
        } else {
            Ok(OriginalIndex(index))
        }
    };
    let (items, removed) = match request {
        CollectionRequest::Complete(entries) => {
            let mut used = BTreeSet::new();
            let mut items = Vec::with_capacity(entries.len());
            for (position, entry) in entries.into_iter().enumerate() {
                let original = match &entry {
                    FinalItem::Keep { index: value } | FinalItem::Replace { index: value, .. } => {
                        Some(index(*value)?)
                    }
                    FinalItem::New { .. } => None,
                };
                if let Some(original) = original {
                    if !used.insert(original) {
                        return Err(context.error(
                            CollectionErrorKind::RepeatedMutation,
                            Some(original.get()),
                            "original slot is used more than once in the complete final order",
                        ));
                    }
                    // A complete positional collection pairs authored item N
                    // with original destination N for preserved fields;
                    // re-pairing an original with a different final position
                    // has no preserved-field meaning (module docs).
                    if context.class() == CollectionClass::Positional && original.get() != position
                    {
                        return Err(context.error(
                            CollectionErrorKind::PositionalCompleteMisalignment,
                            Some(original.get()),
                            format!(
                                "complete positional item {position} references original \
                                 destination {}; positional item N uses original destination \
                                 N for preserved fields, so re-pairing requires authoring \
                                 the entry completely instead",
                                original.get(),
                            ),
                        ));
                    }
                }
                items.push(match entry {
                    FinalItem::Keep { index } => PlannedItem::Keep {
                        index: OriginalIndex(index),
                    },
                    FinalItem::Replace { index, after } => PlannedItem::Replace {
                        index: OriginalIndex(index),
                        after,
                    },
                    FinalItem::New { after } => PlannedItem::New { after },
                });
            }
            let removed = (0..input_len)
                .map(OriginalIndex)
                .filter(|index| !used.contains(index))
                .collect();
            (items, removed)
        }
        CollectionRequest::Partial(edits) => {
            let mut replacements: BTreeMap<OriginalIndex, T> = BTreeMap::new();
            let mut appended: Vec<T> = Vec::new();
            for edit in edits {
                match edit {
                    IndexedEdit::Insert { before, after } => {
                        if before > input_len {
                            return Err(context.error(
                                CollectionErrorKind::OutOfRange,
                                Some(before),
                                format!("insertion boundary exceeds input count {input_len}"),
                            ));
                        }
                        reject_partial_interior_insert(&context, before, input_len)?;
                        appended.push(after);
                    }
                    IndexedEdit::Replace {
                        index: value,
                        after,
                    } => {
                        let original = index(value)?;
                        if replacements.insert(original, after).is_some() {
                            return Err(context.error(
                                CollectionErrorKind::RepeatedMutation,
                                Some(original.get()),
                                "original slot has overlapping or repeated replacement",
                            ));
                        }
                    }
                }
            }
            let mut items = Vec::with_capacity(input_len + appended.len());
            for value in 0..input_len {
                let original = OriginalIndex(value);
                match replacements.remove(&original) {
                    Some(after) => items.push(PlannedItem::Replace {
                        index: original,
                        after,
                    }),
                    None => items.push(PlannedItem::Keep { index: original }),
                }
            }
            items.extend(appended.into_iter().map(|after| PlannedItem::New { after }));
            (items, Vec::new())
        }
    };
    if scope.delete != CollectionDelete::Allow {
        if let Some(first) = removed.first() {
            return Err(context.error(
                CollectionErrorKind::DeletionDenied,
                Some(first.get()),
                "removing an original entry requires explicit collection delete: allow",
            ));
        }
    }
    Ok(CollectionPlan {
        input_len,
        context,
        items,
        removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(mode: CollectionMode, delete: CollectionDelete) -> CollectionScope {
        CollectionScope {
            mode,
            delete,
            span: None,
        }
    }
    fn context_of(kind: CollectionKind) -> CollectionContext {
        CollectionContext {
            owner: "library/component R/part 1".into(),
            kind,
        }
    }
    fn context() -> CollectionContext {
        // Graphics: a positional collection.
        context_of(CollectionKind::Graphics)
    }
    fn plan(
        input_len: usize,
        mode: CollectionMode,
        delete: CollectionDelete,
        request: CollectionRequest<&'static str>,
    ) -> Result<CollectionPlan<&'static str>, CollectionError> {
        plan_collection(input_len, &scope(mode, delete), request, context())
    }
    fn plan_in(
        kind: CollectionKind,
        input_len: usize,
        mode: CollectionMode,
        delete: CollectionDelete,
        request: CollectionRequest<&'static str>,
    ) -> Result<CollectionPlan<&'static str>, CollectionError> {
        plan_collection(input_len, &scope(mode, delete), request, context_of(kind))
    }

    #[test]
    fn every_collection_kind_has_exactly_one_alignment_class() {
        for (kind, expected) in [
            (CollectionKind::Components, CollectionClass::Keyed),
            (CollectionKind::Parts, CollectionClass::Keyed),
            (CollectionKind::Parameters, CollectionClass::Keyed),
            (CollectionKind::Aliases, CollectionClass::Keyed),
            (CollectionKind::Footprints, CollectionClass::Keyed),
            (CollectionKind::Pins, CollectionClass::Positional),
            (CollectionKind::Graphics, CollectionClass::Positional),
            (CollectionKind::Maps, CollectionClass::Positional),
        ] {
            assert_eq!(collection_class(kind), expected, "{kind:?}");
            assert_eq!(context_of(kind).class(), expected, "{kind:?}");
        }
    }

    #[test]
    fn partial_updates_original_slots_and_appends_in_request_order() {
        let input = ["A", "B", "C", "D"];
        let result = plan(
            4,
            CollectionMode::Partial,
            CollectionDelete::Deny,
            CollectionRequest::Partial(vec![
                IndexedEdit::Replace {
                    index: 3,
                    after: "D changed",
                },
                IndexedEdit::Insert {
                    before: 4,
                    after: "first append",
                },
                // Sparse Replace sets are legitimate for both classes: key
                // matching and no-op thinning both produce them.
                IndexedEdit::Replace {
                    index: 1,
                    after: "B changed",
                },
                IndexedEdit::Insert {
                    before: 4,
                    after: "second append",
                },
            ]),
        )
        .unwrap();
        let expected = vec![
            "A",
            "B changed",
            "C",
            "D changed",
            "first append",
            "second append",
        ];
        assert_eq!(result.materialize(&input).unwrap(), expected);
        assert_eq!(
            result.materialize(&input).unwrap(),
            expected,
            "plan is repeatable against unchanged input"
        );
        assert_eq!(input, ["A", "B", "C", "D"]);
        assert!(result.removed().is_empty());
        assert!(
            matches!(&result.items()[3], PlannedItem::Replace { index, .. } if index.get() == 3)
        );
        let untouched = plan(
            4,
            CollectionMode::Partial,
            CollectionDelete::Allow,
            CollectionRequest::Partial(vec![]),
        )
        .unwrap();
        assert_eq!(
            untouched.materialize(&input).unwrap(),
            input,
            "allow alone never authorizes omission-driven deletion in partial scope"
        );
    }

    #[test]
    fn partial_interior_insert_is_rejected_for_both_classes() {
        for kind in [
            CollectionKind::Parameters, // keyed
            CollectionKind::Graphics,   // positional
        ] {
            for before in [0usize, 1, 3] {
                let error = plan_in(
                    kind,
                    4,
                    CollectionMode::Partial,
                    CollectionDelete::Allow,
                    CollectionRequest::Partial(vec![IndexedEdit::Insert {
                        before,
                        after: "sparse",
                    }]),
                )
                .unwrap_err();
                assert_eq!(
                    error.kind,
                    CollectionErrorKind::PartialInteriorInsert,
                    "{kind:?} before {before}"
                );
                assert_eq!(error.index, Some(before));
                assert!(
                    error.to_string().contains("requires a complete collection"),
                    "{error}"
                );
            }
            // The append boundary stays legal for both classes.
            let appended = plan_in(
                kind,
                4,
                CollectionMode::Partial,
                CollectionDelete::Deny,
                CollectionRequest::Partial(vec![IndexedEdit::Insert {
                    before: 4,
                    after: "tail",
                }]),
            )
            .unwrap();
            assert_eq!(
                appended.materialize(&["A", "B", "C", "D"]).unwrap(),
                ["A", "B", "C", "D", "tail"]
            );
        }
    }

    #[test]
    fn complete_keyed_reorders_freely_but_positional_pairs_by_position() {
        // Keyed complete: final order and membership from the request.
        let result = plan_in(
            CollectionKind::Parameters,
            4,
            CollectionMode::Complete,
            CollectionDelete::Allow,
            CollectionRequest::Complete(vec![
                FinalItem::Keep { index: 3 },
                FinalItem::New { after: "new" },
                FinalItem::Replace {
                    index: 1,
                    after: "B changed",
                },
                FinalItem::Keep { index: 0 },
            ]),
        )
        .unwrap();
        assert_eq!(
            result.materialize(&["A", "B", "C", "D"]).unwrap(),
            ["D", "new", "B changed", "A"]
        );
        assert_eq!(result.removed(), &[OriginalIndex(2)]);

        // Positional complete: Keep/Replace must sit at their own original
        // position; substitution-in-place and appends are legal.
        let aligned = plan(
            3,
            CollectionMode::Complete,
            CollectionDelete::Allow,
            CollectionRequest::Complete(vec![
                FinalItem::Keep { index: 0 },
                FinalItem::New {
                    after: "replaces 1",
                },
                FinalItem::Keep { index: 2 },
                FinalItem::New { after: "appended" },
            ]),
        )
        .unwrap();
        assert_eq!(
            aligned.materialize(&["A", "B", "C"]).unwrap(),
            ["A", "replaces 1", "C", "appended"]
        );
        assert_eq!(aligned.removed(), &[OriginalIndex(1)]);
        for misaligned in [
            CollectionRequest::Complete(vec![
                FinalItem::New { after: "front" },
                FinalItem::Keep { index: 0 },
            ]),
            CollectionRequest::Complete(vec![
                FinalItem::Keep { index: 1 },
                FinalItem::Keep { index: 0 },
            ]),
            CollectionRequest::Complete(vec![
                FinalItem::Keep { index: 0 },
                FinalItem::Replace {
                    index: 2,
                    after: "moved",
                },
            ]),
        ] {
            let error = plan(
                3,
                CollectionMode::Complete,
                CollectionDelete::Allow,
                misaligned,
            )
            .unwrap_err();
            assert_eq!(
                error.kind,
                CollectionErrorKind::PositionalCompleteMisalignment,
                "{error:?}"
            );
            assert!(
                error
                    .to_string()
                    .contains("uses original destination N for preserved fields"),
                "{error}"
            );
        }
    }

    #[test]
    fn complete_keyed_reorder_of_equal_values_keeps_distinct_original_slots() {
        let reorder = plan_in(
            CollectionKind::Parameters,
            2,
            CollectionMode::Complete,
            CollectionDelete::Deny,
            CollectionRequest::Complete(vec![
                FinalItem::Keep { index: 1 },
                FinalItem::Keep { index: 0 },
            ]),
        )
        .unwrap();
        assert_eq!(
            reorder.items(),
            &[
                PlannedItem::Keep {
                    index: OriginalIndex(1)
                },
                PlannedItem::Keep {
                    index: OriginalIndex(0)
                }
            ]
        );
        assert_eq!(
            reorder.materialize(&["same", "same"]).unwrap(),
            ["same", "same"]
        );
    }

    #[test]
    fn complete_omission_needs_deletion_permission_and_partial_cannot_remove() {
        let error = plan(
            2,
            CollectionMode::Complete,
            CollectionDelete::Deny,
            CollectionRequest::Complete(vec![FinalItem::Keep { index: 0 }]),
        )
        .unwrap_err();
        assert_eq!(error.kind, CollectionErrorKind::DeletionDenied);
        assert_eq!(error.index, Some(1));
        assert_eq!(error.owner, context().owner);
        assert_eq!(error.collection, CollectionKind::Graphics);
        // Partial removal is inexpressible by construction: IndexedEdit has no
        // removal variant, so the only partial outcomes are updates and
        // appends — asserted by materializing a full-coverage partial plan.
        let partial = plan(
            2,
            CollectionMode::Partial,
            CollectionDelete::Deny,
            CollectionRequest::Partial(vec![IndexedEdit::Replace {
                index: 1,
                after: "updated",
            }]),
        )
        .unwrap();
        assert_eq!(partial.materialize(&["A", "B"]).unwrap(), ["A", "updated"]);
        assert!(partial.removed().is_empty());
        let complete = plan(
            2,
            CollectionMode::Complete,
            CollectionDelete::Deny,
            CollectionRequest::Complete(vec![
                FinalItem::Keep { index: 0 },
                FinalItem::Replace {
                    index: 1,
                    after: "updated",
                },
                FinalItem::New { after: "new" },
            ]),
        )
        .unwrap();
        assert_eq!(
            complete.materialize(&["A", "B"]).unwrap(),
            ["A", "updated", "new"]
        );
    }

    #[test]
    fn repeated_original_slots_and_overlapping_mutations_are_rejected() {
        let error = plan(
            1,
            CollectionMode::Partial,
            CollectionDelete::Allow,
            CollectionRequest::Partial(vec![
                IndexedEdit::Replace {
                    index: 0,
                    after: "x",
                },
                IndexedEdit::Replace {
                    index: 0,
                    after: "y",
                },
            ]),
        )
        .unwrap_err();
        assert_eq!(error.kind, CollectionErrorKind::RepeatedMutation);
        assert_eq!(error.index, Some(0));
        for first in [
            FinalItem::Keep { index: 0 },
            FinalItem::Replace {
                index: 0,
                after: "x",
            },
        ] {
            for second in [
                FinalItem::Keep { index: 0 },
                FinalItem::Replace {
                    index: 0,
                    after: "y",
                },
            ] {
                let error = plan_in(
                    CollectionKind::Parameters,
                    1,
                    CollectionMode::Complete,
                    CollectionDelete::Allow,
                    CollectionRequest::Complete(vec![first.clone(), second]),
                )
                .unwrap_err();
                assert_eq!(error.kind, CollectionErrorKind::RepeatedMutation);
                assert_eq!(error.index, Some(0));
            }
        }
    }

    #[test]
    fn invalid_positions_modes_and_changed_input_length_are_contextual_errors() {
        for bad in [2, usize::MAX] {
            for (mode, request) in [
                (
                    CollectionMode::Partial,
                    CollectionRequest::Partial(vec![IndexedEdit::Replace {
                        index: bad,
                        after: "x",
                    }]),
                ),
                (
                    CollectionMode::Complete,
                    CollectionRequest::Complete(vec![FinalItem::Keep { index: bad }]),
                ),
                (
                    CollectionMode::Complete,
                    CollectionRequest::Complete(vec![FinalItem::Replace {
                        index: bad,
                        after: "x",
                    }]),
                ),
            ] {
                let error = plan(2, mode, CollectionDelete::Allow, request).unwrap_err();
                assert_eq!(error.kind, CollectionErrorKind::OutOfRange);
                assert_eq!(error.index, Some(bad));
                assert!(error.to_string().contains("library/component R/part 1"));
            }
        }
        assert_eq!(
            plan(
                2,
                CollectionMode::Partial,
                CollectionDelete::Allow,
                CollectionRequest::Partial(vec![IndexedEdit::Insert {
                    before: 3,
                    after: "x"
                }])
            )
            .unwrap_err()
            .kind,
            CollectionErrorKind::OutOfRange
        );
        for (mode, request) in [
            (CollectionMode::Partial, CollectionRequest::Complete(vec![])),
            (CollectionMode::Complete, CollectionRequest::Partial(vec![])),
        ] {
            assert_eq!(
                plan(0, mode, CollectionDelete::Allow, request)
                    .unwrap_err()
                    .kind,
                CollectionErrorKind::ModeMismatch
            );
        }
        let result = plan(
            1,
            CollectionMode::Partial,
            CollectionDelete::Deny,
            CollectionRequest::Partial(vec![]),
        )
        .unwrap();
        assert_eq!(
            result.materialize(&[]).unwrap_err().kind,
            CollectionErrorKind::InputLengthChanged
        );
        assert_eq!(
            result.materialize(&["A", "B"]).unwrap_err().kind,
            CollectionErrorKind::InputLengthChanged
        );
    }

    #[test]
    fn empty_collections_allow_explicit_creation_and_authorized_empty_final_order() {
        for (mode, request) in [
            (
                CollectionMode::Partial,
                CollectionRequest::Partial(vec![IndexedEdit::Insert {
                    before: 0,
                    after: "new",
                }]),
            ),
            (
                CollectionMode::Complete,
                CollectionRequest::Complete(vec![FinalItem::New { after: "new" }]),
            ),
        ] {
            let result = plan(0, mode, CollectionDelete::Deny, request).unwrap();
            assert_eq!(result.materialize(&[]).unwrap(), ["new"]);
            assert!(result.removed().is_empty());
        }
        let removed = plan(
            2,
            CollectionMode::Complete,
            CollectionDelete::Allow,
            CollectionRequest::Complete(vec![]),
        )
        .unwrap();
        assert!(removed.materialize(&["A", "B"]).unwrap().is_empty());
        assert_eq!(removed.removed(), &[OriginalIndex(0), OriginalIndex(1)]);
    }
}
