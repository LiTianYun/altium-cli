//! Concrete SchLib snapshot-bound component edits.
//!
//! This is the initial scalar elaboration path, not the full source planner.
//! Child collections, creation/deletion, inherited instances and native-reference
//! rebuilding remain separate requirements before shipping the SchLib workflow.
use crate::collection::{
    CollectionContext, CollectionRequest, IndexedEdit, PlannedItem, plan_collection,
};
use altium_format::{
    SchLib,
    api::{SchLibComponentSnapshot, SchLibSnapshot},
};
use altium_spec_lang::intent::{FieldIntent, OPTIONAL_DESCRIPTION};

#[derive(Debug, thiserror::Error)]
pub enum SchLibPatchError {
    #[error("{context}: {source}")]
    Format {
        context: String,
        #[source]
        source: altium_format::AltiumFormatError,
    },
    #[error("component #{index} description: {source}")]
    Intent {
        index: usize,
        #[source]
        source: altium_spec_lang::SpecError,
    },
    #[error("component #{index} is addressed more than once in the same patch")]
    Overlap { index: usize },
    #[error("SchLib typed input state changed since planning; replan")]
    StaleInput,
    #[error("{detail}")]
    Collection { detail: String },
}

/// One complete component after-state addressed in the immutable input snapshot.
/// Native snapshot fields remain private to altium-format.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentReplacement {
    index: usize,
    after: SchLibComponentSnapshot,
}

impl ComponentReplacement {
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn after(&self) -> &SchLibComponentSnapshot {
        &self.after
    }
}

/// A checked in-memory patch bound to the entire typed input, including fields
/// outside the edited API projection. File hashes and saved-plan serialization
/// must additionally bind physical input artifacts in the transaction layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibPatch {
    input: SchLibSnapshot,
    replacements: Vec<ComponentReplacement>,
}

impl SchLibPatch {
    /// Elaborate explicitly addressed description fields on library definitions.
    /// Omission preserves the existing target; explicit inherit/reset uses the
    /// declared definition default. This function does not interpret instance
    /// inheritance or grant scope to other fields or child collections.
    pub fn descriptions(
        input: &SchLibSnapshot,
        fields: impl IntoIterator<Item = (usize, FieldIntent<Option<String>>)>,
    ) -> Result<Self, SchLibPatchError> {
        let mut seen = std::collections::BTreeSet::new();
        let mut replacements = Vec::new();
        for (index, intent) in fields {
            if !seen.insert(index) {
                return Err(SchLibPatchError::Overlap { index });
            }
            let before = input
                .component(index)
                .map_err(|source| SchLibPatchError::Format {
                    context: format!("resolving input component #{index}"),
                    source,
                })?;
            if matches!(intent, FieldIntent::Absent) {
                continue;
            }
            let value = OPTIONAL_DESCRIPTION
                .definition(&intent)
                .map_err(|source| SchLibPatchError::Intent { index, source })?;
            let after =
                before
                    .with_description(value)
                    .map_err(|source| SchLibPatchError::Format {
                        context: format!(
                            "elaborating description of component #{index} '{}'",
                            before.lib_reference()
                        ),
                        source,
                    })?;
            if after != before {
                replacements.push(ComponentReplacement { index, after });
            }
        }
        // Reuse the collection planner's original-index assembly. This scalar
        // entry point grants partial replacement only, never collection deletion.
        let collection = plan_collection(
            input.component_count(),
            &altium_spec_lang::intent::CollectionScope::default(),
            CollectionRequest::Partial(
                replacements
                    .into_iter()
                    .map(|replacement| IndexedEdit::Replace {
                        index: replacement.index,
                        after: replacement.after,
                    })
                    .collect(),
            ),
            CollectionContext {
                owner: "SchLib".into(),
                kind: altium_spec_lang::ast::CollectionKind::Components,
            },
        )
        .map_err(|error| SchLibPatchError::Collection {
            detail: error.to_string(),
        })?;
        let mut replacements = Vec::new();
        for item in collection.into_items() {
            match item {
                PlannedItem::Keep { .. } => {} // Preserved by the complete input snapshot.
                PlannedItem::Replace { index, after } => replacements.push(ComponentReplacement {
                    index: index.get(),
                    after,
                }),
                PlannedItem::New { .. } => {
                    return Err(SchLibPatchError::Collection {
                        detail: "description-only patch cannot create a component".into(),
                    });
                }
            }
        }
        let patch = Self {
            input: input.clone(),
            replacements,
        };
        patch.resulting_snapshot()?;
        Ok(patch)
    }

    pub fn replacements(&self) -> &[ComponentReplacement] {
        &self.replacements
    }
    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty()
    }

    pub fn resulting_snapshot(&self) -> Result<SchLibSnapshot, SchLibPatchError> {
        self.input
            .replace_components(
                self.replacements
                    .iter()
                    .map(|replacement| (replacement.index, replacement.after.clone())),
            )
            .map_err(|source| SchLibPatchError::Format {
                context: "replacing original component slots in candidate".into(),
                source,
            })
    }

    /// Execute the captured replacements only after verifying complete typed
    /// before-state. Build and validate a separate document before swapping it in.
    /// This does not write files or implement a durable filesystem transaction.
    pub fn apply_in_memory(&self, target: &mut SchLib) -> Result<(), SchLibPatchError> {
        let actual = target
            .snapshot()
            .map_err(|source| SchLibPatchError::Format {
                context: "capturing target before applying component patch".into(),
                source,
            })?;
        if actual != self.input {
            return Err(SchLibPatchError::StaleInput);
        }
        let candidate = self.resulting_snapshot()?.materialize().map_err(|source| {
            SchLibPatchError::Format {
                context: "materializing complete SchLib patch result".into(),
                source,
            }
        })?;
        *target = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_spec_lang::diagnostic::{Span, Spanned};

    fn set(value: &str) -> FieldIntent<Option<String>> {
        FieldIntent::Set(Spanned::new(Some(value.into()), Span::new(0, 1)))
    }

    #[test]
    fn description_patch_materializes_reviewed_state_and_replans_empty() {
        let mut doc = SchLib::new_blank_ad26().unwrap();
        doc.insert_image_resource(0, "shared".into(), vec![1, 0, 255])
            .unwrap();
        let input = doc.snapshot().unwrap();
        let patch = SchLibPatch::descriptions(&input, [(0, set("reviewed description"))]).unwrap();
        assert_eq!(patch.replacements().len(), 1);
        let expected = patch.resulting_snapshot().unwrap();
        patch.apply_in_memory(&mut doc).unwrap();
        assert_eq!(doc.snapshot().unwrap(), expected);
        assert_eq!(doc.image_resources().next().unwrap().payload, &[1, 0, 255]);
        assert_eq!(
            expected.component(0).unwrap().description(),
            Some("reviewed description")
        );
        assert!(
            SchLibPatch::descriptions(&expected, [(0, set("reviewed description"))])
                .unwrap()
                .is_empty()
        );
        assert!(
            SchLibPatch::descriptions(&expected, [(0, FieldIntent::Absent)])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn edits_outside_description_still_invalidate_patch_atomically() {
        let mut doc = SchLib::new_blank_ad26().unwrap();
        let patch = SchLibPatch::descriptions(&doc.snapshot().unwrap(), [(0, set("new"))]).unwrap();
        doc.insert_image_resource(0, "later".into(), vec![4, 5])
            .unwrap();
        let changed = doc.snapshot().unwrap();
        assert!(matches!(
            patch.apply_in_memory(&mut doc),
            Err(SchLibPatchError::StaleInput)
        ));
        assert_eq!(
            doc.snapshot().unwrap(),
            changed,
            "rejected execution must not mutate target"
        );
    }

    #[test]
    fn conflicting_or_missing_input_slots_error_and_clear_is_explicit() {
        let doc = SchLib::new_blank_ad26().unwrap();
        let input = doc.snapshot().unwrap();
        assert!(matches!(
            SchLibPatch::descriptions(&input, [(0, set("a")), (0, set("b"))]),
            Err(SchLibPatchError::Overlap { index: 0 })
        ));
        assert!(SchLibPatch::descriptions(&input, [(usize::MAX, set("a"))]).is_err());
        let written = SchLibPatch::descriptions(&input, [(0, set("a"))])
            .unwrap()
            .resulting_snapshot()
            .unwrap();
        let cleared =
            SchLibPatch::descriptions(&written, [(0, FieldIntent::Clear(Span::new(0, 1)))])
                .unwrap()
                .resulting_snapshot()
                .unwrap();
        assert_eq!(cleared.component(0).unwrap().description(), None);
    }
}
