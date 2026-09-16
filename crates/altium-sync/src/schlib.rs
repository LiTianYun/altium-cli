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
use altium_format_types::ComponentKind;
use altium_spec_lang::intent::{
    COMPONENT_DESIGNATOR, COMPONENT_KIND, FieldIntent, FieldPolicy, OPTIONAL_DESCRIPTION,
};

/// Explicit scope for the supported library-definition scalar properties.
/// Child collections and part-count edits are intentionally separate inputs to
/// the full planner; this type never implies permission to change them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentPropertiesIntent {
    pub description: FieldIntent<Option<String>>,
    pub designator: FieldIntent<Option<String>>,
    pub component_kind: FieldIntent<ComponentKind>,
    pub show_hidden_pins: FieldIntent<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum SchLibPatchError {
    #[error("{context}: {source}")]
    Format {
        context: String,
        #[source]
        source: altium_format::AltiumFormatError,
    },
    #[error("component #{index} {field}: {source}")]
    Intent {
        index: usize,
        field: &'static str,
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
        Self::component_properties(
            input,
            fields.into_iter().map(|(index, description)| {
                (
                    index,
                    ComponentPropertiesIntent {
                        description,
                        ..Default::default()
                    },
                )
            }),
        )
    }

    /// Resolve only explicitly supplied component properties. Absent values
    /// preserve the existing complete aggregate; definition inherit/reset use
    /// declared defaults. This is not a full SchLibIntent or instance planner.
    pub fn component_properties(
        input: &SchLibSnapshot,
        fields: impl IntoIterator<Item = (usize, ComponentPropertiesIntent)>,
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
            let mut after = before.clone();
            macro_rules! property {
                ($field:ident, $policy:expr, $setter:ident) => {
                    if intent.$field.is_authored() {
                        let value = $policy.definition(&intent.$field).map_err(|source| {
                            SchLibPatchError::Intent {
                                index,
                                field: stringify!($field),
                                source,
                            }
                        })?;
                        after =
                            after
                                .$setter(value)
                                .map_err(|source| SchLibPatchError::Format {
                                    context: format!(
                                        "elaborating {} of component #{index} '{}'",
                                        stringify!($field),
                                        before.lib_reference()
                                    ),
                                    source,
                                })?;
                    }
                };
            }
            property!(description, OPTIONAL_DESCRIPTION, with_description);
            property!(designator, COMPONENT_DESIGNATOR, with_designator);
            property!(component_kind, COMPONENT_KIND, with_component_kind);
            property!(
                show_hidden_pins,
                FieldPolicy {
                    name: "component.show_hidden_pins",
                    default: Some(|| false),
                    clear: None,
                },
                with_show_hidden_pins
            );
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
                        detail: "component-property patch cannot create a component".into(),
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
    fn component_properties_survive_native_save_and_replan_without_changes() {
        let mut doc = SchLib::new_blank_ad26().unwrap();
        let input = doc.snapshot().unwrap();
        let properties = ComponentPropertiesIntent {
            description: set("complete property edit"),
            designator: set("R?"),
            component_kind: FieldIntent::Set(Spanned::new(ComponentKind::Jumper, Span::new(0, 1))),
            show_hidden_pins: FieldIntent::Set(Spanned::new(true, Span::new(0, 1))),
        };
        let patch = SchLibPatch::component_properties(&input, [(0, properties.clone())]).unwrap();
        let expected = patch.resulting_snapshot().unwrap();
        let component = expected.component(0).unwrap();
        assert_eq!(component.component_kind(), ComponentKind::Jumper);
        assert_eq!(component.designator().unwrap(), Some("R?"));
        assert!(component.show_hidden_pins());
        patch.apply_in_memory(&mut doc).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("properties.SchLib");
        doc.save(&path).unwrap();
        let reopened = SchLib::open(&path).unwrap().snapshot().unwrap();
        assert_eq!(reopened, expected);
        assert!(
            SchLibPatch::component_properties(&reopened, [(0, properties)])
                .unwrap()
                .is_empty()
        );
        assert!(
            SchLibPatch::component_properties(
                &reopened,
                [(0, ComponentPropertiesIntent::default())]
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn clear_and_reset_only_change_explicit_component_properties() {
        let doc = SchLib::new_blank_ad26().unwrap();
        let original = doc.snapshot().unwrap();
        let authored = SchLibPatch::component_properties(
            &original,
            [(
                0,
                ComponentPropertiesIntent {
                    description: set("retained"),
                    designator: set("X?"),
                    show_hidden_pins: FieldIntent::Set(Spanned::new(true, Span::new(0, 1))),
                    ..Default::default()
                },
            )],
        )
        .unwrap()
        .resulting_snapshot()
        .unwrap();
        let result = SchLibPatch::component_properties(
            &authored,
            [(
                0,
                ComponentPropertiesIntent {
                    designator: FieldIntent::Clear(Span::new(0, 1)),
                    show_hidden_pins: FieldIntent::Reset(Span::new(0, 1)),
                    ..Default::default()
                },
            )],
        )
        .unwrap()
        .resulting_snapshot()
        .unwrap();
        let component = result.component(0).unwrap();
        assert_eq!(component.designator().unwrap(), None);
        assert_eq!(component.description(), Some("retained"));
        assert!(!component.show_hidden_pins());
        let error = SchLibPatch::component_properties(
            &result,
            [(
                0,
                ComponentPropertiesIntent {
                    show_hidden_pins: FieldIntent::Clear(Span::new(0, 1)),
                    ..Default::default()
                },
            )],
        )
        .unwrap_err();
        assert!(error.to_string().contains("show_hidden_pins"), "{error}");
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
