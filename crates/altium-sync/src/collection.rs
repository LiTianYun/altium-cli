//! Crate-private collection assembly against immutable input positions.
//!
//! This helper does not authenticate an input snapshot. Its caller must bind the
//! aggregate plan to the input snapshot/hash and verify that precondition before
//! materialization. Matching collection lengths alone never proves input identity.
use std::collections::{BTreeMap, BTreeSet};

use altium_spec_lang::ast::{CollectionDelete, CollectionKind, CollectionMode};
use altium_spec_lang::intent::CollectionScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectionContext {
    pub owner: String,
    pub kind: CollectionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollectionErrorKind {
    ModeMismatch,
    OutOfRange,
    RepeatedMutation,
    DeletionDenied,
    InputLengthChanged,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexedEdit<T> {
    Replace {
        index: usize,
        after: T,
    },
    Remove {
        index: usize,
    },
    /// `before == input_len` appends. Multiple inserts at the same original
    /// boundary retain request order, even when that original item is removed.
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
            for entry in entries {
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
            // None is an explicit removal; absence from this map preserves input.
            let mut mutations: BTreeMap<OriginalIndex, Option<T>> = BTreeMap::new();
            let mut insertions: BTreeMap<usize, Vec<T>> = BTreeMap::new();
            for edit in edits {
                let (original, replacement) = match edit {
                    IndexedEdit::Insert { before, after } => {
                        if before > input_len {
                            return Err(context.error(
                                CollectionErrorKind::OutOfRange,
                                Some(before),
                                format!("insertion boundary exceeds input count {input_len}"),
                            ));
                        }
                        insertions.entry(before).or_default().push(after);
                        continue;
                    }
                    IndexedEdit::Replace {
                        index: value,
                        after,
                    } => (index(value)?, Some(after)),
                    IndexedEdit::Remove { index: value } => (index(value)?, None),
                };
                if mutations.insert(original, replacement).is_some() {
                    return Err(context.error(
                        CollectionErrorKind::RepeatedMutation,
                        Some(original.get()),
                        "original slot has overlapping or repeated replacement/removal",
                    ));
                }
            }
            let mut items = Vec::new();
            let mut removed = Vec::new();
            for value in 0..input_len {
                if let Some(inserted) = insertions.remove(&value) {
                    items.extend(inserted.into_iter().map(|after| PlannedItem::New { after }));
                }
                let original = OriginalIndex(value);
                match mutations.remove(&original) {
                    Some(Some(after)) => items.push(PlannedItem::Replace {
                        index: original,
                        after,
                    }),
                    Some(None) => removed.push(original),
                    None => items.push(PlannedItem::Keep { index: original }),
                }
            }
            if let Some(inserted) = insertions.remove(&input_len) {
                items.extend(inserted.into_iter().map(|after| PlannedItem::New { after }));
            }
            (items, removed)
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
    fn context() -> CollectionContext {
        CollectionContext {
            owner: "library/component R/part 1".into(),
            kind: CollectionKind::Graphics,
        }
    }
    fn plan(
        input_len: usize,
        mode: CollectionMode,
        delete: CollectionDelete,
        request: CollectionRequest<&'static str>,
    ) -> Result<CollectionPlan<&'static str>, CollectionError> {
        plan_collection(input_len, &scope(mode, delete), request, context())
    }

    #[test]
    fn partial_multi_edits_use_immutable_original_indices_and_keep_omissions() {
        let input = ["A", "B", "C", "D"];
        let result = plan(
            4,
            CollectionMode::Partial,
            CollectionDelete::Allow,
            CollectionRequest::Partial(vec![
                IndexedEdit::Insert {
                    before: 0,
                    after: "head",
                },
                IndexedEdit::Remove { index: 1 },
                IndexedEdit::Replace {
                    index: 3,
                    after: "D changed",
                },
                IndexedEdit::Insert {
                    before: 1,
                    after: "first",
                },
                IndexedEdit::Insert {
                    before: 1,
                    after: "second",
                },
                IndexedEdit::Insert {
                    before: 4,
                    after: "tail",
                },
            ]),
        )
        .unwrap();
        let expected = vec!["head", "A", "first", "second", "C", "D changed", "tail"];
        assert_eq!(result.materialize(&input).unwrap(), expected);
        assert_eq!(
            result.materialize(&input).unwrap(),
            expected,
            "plan is repeatable against unchanged input"
        );
        assert_eq!(input, ["A", "B", "C", "D"]);
        assert_eq!(result.removed(), &[OriginalIndex(1)]);
        assert!(
            matches!(&result.items()[5], PlannedItem::Replace { index, .. } if index.get() == 3)
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
    fn complete_sets_explicit_final_order_and_length_without_similarity() {
        let result = plan(
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
        // Equal-valued items are still distinct original positions.
        let reorder = plan(
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
    fn omissions_and_explicit_removals_need_deletion_permission() {
        for (mode, request) in [
            (
                CollectionMode::Complete,
                CollectionRequest::Complete(vec![FinalItem::Keep { index: 0 }]),
            ),
            (
                CollectionMode::Partial,
                CollectionRequest::Partial(vec![IndexedEdit::Remove { index: 1 }]),
            ),
        ] {
            let error = plan(2, mode, CollectionDelete::Deny, request).unwrap_err();
            assert_eq!(error.kind, CollectionErrorKind::DeletionDenied);
            assert_eq!(error.index, Some(1));
            assert_eq!(error.owner, context().owner);
            assert_eq!(error.collection, CollectionKind::Graphics);
        }
        let complete = plan(
            2,
            CollectionMode::Complete,
            CollectionDelete::Deny,
            CollectionRequest::Complete(vec![
                FinalItem::New { after: "new" },
                FinalItem::Replace {
                    index: 1,
                    after: "updated",
                },
                FinalItem::Keep { index: 0 },
            ]),
        )
        .unwrap();
        assert_eq!(
            complete.materialize(&["A", "B"]).unwrap(),
            ["new", "updated", "A"]
        );
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
    }

    #[test]
    fn repeated_original_slots_and_overlapping_mutations_are_rejected() {
        for first in [
            IndexedEdit::Remove { index: 0 },
            IndexedEdit::Replace {
                index: 0,
                after: "x",
            },
        ] {
            for second in [
                IndexedEdit::Remove { index: 0 },
                IndexedEdit::Replace {
                    index: 0,
                    after: "y",
                },
            ] {
                let error = plan(
                    1,
                    CollectionMode::Partial,
                    CollectionDelete::Allow,
                    CollectionRequest::Partial(vec![first.clone(), second]),
                )
                .unwrap_err();
                assert_eq!(error.kind, CollectionErrorKind::RepeatedMutation);
                assert_eq!(error.index, Some(0));
            }
        }
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
                let error = plan(
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
                    CollectionMode::Partial,
                    CollectionRequest::Partial(vec![IndexedEdit::Remove { index: bad }]),
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
