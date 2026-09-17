//! Complete typed state for SchLib planning. No native record internals escape
//! this module; source scope and authorization belong to the planner.

mod children;
mod fields;
mod parts;
mod views;

pub use children::{
    SchChildBatch, SchGraphicEdit, SchGraphicInit, SchGraphicShape, SchParameterEdit,
    SchParameterInit, SchPinEdit, SchPinInit,
};
pub use parts::{SchCurrentPartIdAdjustment, SchPartsShrink, SchRemovedPartChild};
pub use views::{
    SchFontEntryView, SchFootprintMapView, SchFootprintMapsView, SchGraphicDetail, SchGraphicView,
    SchOwnerGroup, SchParameterView, SchPartSwapGroup, SchPartView, SchPinPadMapView, SchPinView,
};

use super::{SchLib, SchLibComponent, SchLibComponentIndex};
use crate::{AltiumFormatError, Result, ResultExt};
use std::sync::Arc;

/// The existing native namespace in which component references were captured.
/// This is derived from typed document state, not a generated identity token.
/// Image contents are hashed once per library capture, never cloned per component.
#[derive(Debug, Clone, PartialEq)]
struct ReferenceContext {
    library_id: String,
    minor_version: i32,
    fonts: Vec<altium_format_types::sch::SchFont>,
    display_settings: altium_format_types::SchDisplaySettings,
    aliases: Vec<super::SchLibAlias>,
    image_assets: Vec<(String, [u8; 32])>,
}

impl ReferenceContext {
    fn capture(library: &SchLib) -> Self {
        Self {
            library_id: library.header.unique_id.clone(),
            minor_version: library.header.minor_version,
            fonts: library.header.fonts.clone(),
            display_settings: library.header.display_settings.clone(),
            aliases: library.aliases.clone(),
            image_assets: library
                .embedded_images
                .iter()
                .map(|image| {
                    (
                        image.file_name.clone(),
                        *blake3::hash(&image.data).as_bytes(),
                    )
                })
                .collect(),
        }
    }
}

/// An immutable copy of all parsed library state, including ordered image assets.
///
/// Equality compares the complete typed state, including native ownership and
/// separate Data/Additional collections. It is not semantic normalization or
/// persistent identity. Capturing does not serialize or reconstruct API records.
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibSnapshot {
    library: SchLib,
    reference_context: Arc<ReferenceContext>,
}

/// A complete component aggregate and its library index entry.
///
/// Native fields and both child collections remain private and are retained even
/// when the source language has no corresponding authoring operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SchLibComponentSnapshot {
    component: SchLibComponent,
    index: SchLibComponentIndex,
    reference_context: Arc<ReferenceContext>,
}

impl SchLib {
    /// Captures complete typed state after checking library invariants and the
    /// full Data/Additional ownership graph.
    pub fn snapshot(&self) -> Result<SchLibSnapshot> {
        validate_snapshot_library(self).context("capturing SchLib snapshot")?;
        Ok(SchLibSnapshot {
            library: self.clone(),
            reference_context: Arc::new(ReferenceContext::capture(self)),
        })
    }
}

fn validate_snapshot_library(library: &SchLib) -> Result<()> {
    library
        .validate_invariants()
        .context("validating snapshot library invariants")?;
    for (index, component) in library.components.iter().enumerate() {
        super::ownership::validate_component_ownership(component)
            .with_context(|| format!("validating snapshot component[{index}] ownership"))?;
    }
    Ok(())
}

impl SchLibSnapshot {
    pub fn component_count(&self) -> usize {
        self.library.components.len()
    }

    /// Returns a complete aggregate by position in this immutable snapshot.
    pub fn component(&self, index: usize) -> Result<SchLibComponentSnapshot> {
        let component = self.library.components.get(index).ok_or_else(|| {
            AltiumFormatError::InvalidParamValue {
                key: "component index".to_owned(),
                detail: format!(
                    "SchLib snapshot index {index} exceeds component count {}",
                    self.component_count()
                ),
            }
        })?;
        Ok(SchLibComponentSnapshot {
            component: component.clone(),
            index: self.library.header.components[index].clone(),
            reference_context: Arc::clone(&self.reference_context),
        })
    }

    /// Enumerates aggregates in the same order used by `component`.
    pub fn components(&self) -> impl ExactSizeIterator<Item = SchLibComponentSnapshot> + '_ {
        self.library
            .components
            .iter()
            .zip(&self.library.header.components)
            .map(|(component, index)| SchLibComponentSnapshot {
                component: component.clone(),
                index: index.clone(),
                reference_context: Arc::clone(&self.reference_context),
            })
    }

    /// Creates a validated candidate with one aggregate replaced. The input
    /// remains unchanged on success or failure. Renaming is not supported here.
    ///
    /// This does not grant source scope permissions or relocate font/resource
    /// references between libraries. A differing captured reference namespace is
    /// rejected; explicit relocation requires a later import contract. Future
    /// structural editing also requires stronger ownership/reference validation.
    pub fn replace_component(
        &self,
        index: usize,
        replacement: SchLibComponentSnapshot,
    ) -> Result<Self> {
        self.replace_components([(index, replacement)])
    }

    /// Replaces distinct immutable-input slots with one clone and one final
    /// invariant check. Duplicate indices are conflicting edits, even when their
    /// replacement values are equal.
    pub fn replace_components(
        &self,
        replacements: impl IntoIterator<Item = (usize, SchLibComponentSnapshot)>,
    ) -> Result<Self> {
        validate_snapshot_library(&self.library)
            .context("validating original SchLib snapshot before component replacement")?;
        let replacements: Vec<_> = replacements.into_iter().collect();
        let mut seen = std::collections::HashSet::new();
        let mut weight_delta = 0i64;
        for (index, replacement) in &replacements {
            if !seen.insert(*index) {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "component index".to_owned(),
                    detail: format!(
                        "replacing SchLib snapshot component[{index}]: duplicate replacement index"
                    ),
                });
            }
            let existing = self.library.header.components.get(*index).ok_or_else(|| {
                AltiumFormatError::InvalidParamValue {
                    key: "component index".to_owned(),
                    detail: format!("replacing SchLib snapshot component[{index}]: index exceeds component count {}", self.component_count()),
                }
            })?;
            if replacement.reference_context != self.reference_context {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "component reference context".to_owned(),
                    detail: format!(
                        "replacing SchLib component[{index}] '{}': captured library/font/style/image/alias namespace differs; explicit reference relocation is required",
                        existing.lib_ref
                    ),
                });
            }
            if existing.lib_ref != replacement.lib_reference() {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "component reference".to_owned(),
                    detail: format!(
                        "replacing SchLib component[{index}] '{}' with '{}': renaming is unsupported",
                        existing.lib_ref,
                        replacement.lib_reference()
                    ),
                });
            }
            super::ownership::validate_component_ownership(&replacement.component)
                .with_context(|| format!("validating replacement component[{index}] ownership"))?;
            // Retain the input's accepted native exporter weight convention.
            // Only actual Data/alias cardinality changes affect the count; scalar
            // edits must not recanonicalize an otherwise valid stored header.
            let old_count = self.library.components[*index]
                .records
                .len()
                .checked_add(existing.aliases.len());
            let new_count = replacement
                .component
                .records
                .len()
                .checked_add(replacement.index.aliases.len());
            let counts = old_count
                .and_then(|old| i64::try_from(old).ok())
                .zip(new_count.and_then(|new| i64::try_from(new).ok()));
            let Some((old_count, new_count)) = counts else {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "weight".to_owned(),
                    detail: format!(
                        "replacing SchLib component[{index}]: record/alias count exceeds supported range"
                    ),
                });
            };
            weight_delta = weight_delta
                .checked_add(new_count - old_count)
                .ok_or_else(|| AltiumFormatError::InvalidParamValue {
                    key: "weight".to_owned(),
                    detail: "replacing SchLib components: cardinality delta overflow".to_owned(),
                })?;
        }
        let weight = i64::from(self.library.header.weight)
            .checked_add(weight_delta)
            .and_then(|weight| i32::try_from(weight).ok())
            .ok_or_else(|| AltiumFormatError::InvalidParamValue {
                key: "weight".to_owned(),
                detail: "replacing SchLib components: resulting native header weight overflow"
                    .to_owned(),
            })?;
        let mut candidate = self.library.clone();
        candidate.header.weight = weight;
        for (index, replacement) in replacements {
            candidate.components[index] = replacement.component;
            candidate.header.components[index] = replacement.index;
        }
        validate_snapshot_library(&candidate).context("replacing SchLib snapshot components")?;
        Ok(Self {
            library: candidate,
            reference_context: Arc::clone(&self.reference_context),
        })
    }

    /// Materializes exactly the captured typed state, without using the lossy
    /// semantic Component API. Validation covers current native invariants.
    pub fn materialize(&self) -> Result<SchLib> {
        validate_snapshot_library(&self.library).context("materializing SchLib snapshot")?;
        Ok(self.library.clone())
    }
}

impl SchLibComponentSnapshot {
    pub fn lib_reference(&self) -> &str {
        &self.index.lib_ref
    }

    /// Empty native descriptions project to absence.
    pub fn description(&self) -> Option<&str> {
        (!self.index.description.is_empty()).then_some(self.index.description.as_str())
    }

    /// Changes only the description in the root record and library index.
    /// `None` and an empty string both use the native empty representation.
    pub fn with_description(&self, description: Option<String>) -> Result<Self> {
        let description = description.unwrap_or_default();
        let mut candidate = self.clone();
        candidate.component.component.component_description = description.clone();
        candidate.index.description = description;
        Ok(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param_collection::ParameterCollection;
    use crate::sch_records::{
        SchImplementation, SchImplementationList, SchLabel, SchLine, SchRecord, SchSymbol,
    };
    use crate::schlib::{SchLibAlias, SchLibEmbeddedImage, compute_weight};
    use altium_format_types::sch::SchDisplayStyle;

    fn rich_library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.header.fonts[0].bold = true;
        library
            .header
            .display_settings
            .styles
            .push(SchDisplayStyle {
                id: 1,
                gradient_depth: Some(37),
                ..Default::default()
            });
        let aggregate = &mut library.components[0];
        aggregate.component.vault_guid = "preserved-vault".to_owned();
        aggregate.component.custom_display_mode_names =
            vec!["normal".to_owned(), "alternate".to_owned()];
        aggregate.component.pins_moveable = true;
        let mut symbol = SchSymbol::from_params(&mut ParameterCollection::new()).unwrap();
        symbol.base.selection_memory = 3;
        symbol.base.style_id = 1;
        aggregate.records.push(SchRecord::Symbol(symbol));
        let mut hyperlink = SchLabel::from_params(&mut ParameterCollection::new()).unwrap();
        hyperlink.text = "source link".to_owned();
        hyperlink.url = "https://example.invalid/component".to_owned();
        aggregate.records.push(SchRecord::Hyperlink(hyperlink));
        let list_index = aggregate.records.len() + 1;
        aggregate.records.push(SchRecord::ImplementationList(
            SchImplementationList::from_params(&mut ParameterCollection::new()).unwrap(),
        ));
        let mut implementation =
            SchImplementation::from_params(&mut ParameterCollection::new()).unwrap();
        implementation.base.owner_index = list_index as i32;
        implementation.model_type = "SIM".to_owned();
        implementation.model_name = "retained-model".to_owned();
        aggregate
            .records
            .push(SchRecord::Implementation(implementation));
        let mut pin = crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
        pin.defined_functions = vec!["CLOCK".to_owned(), "GPIO".to_owned()];
        pin.selected_functions = vec!["GPIO".to_owned()];
        aggregate.records.push(SchRecord::Pin(pin));
        let mut line = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
        line.base.index_in_sheet = 17;
        aggregate
            .additional_records
            .push(SchRecord::Line(line.clone()));
        line.base.owner_index_additional_list = true;
        line.base.owner_index = 0;
        aggregate.additional_records.push(SchRecord::Line(line));
        let name = aggregate.component.lib_reference.clone();
        library.header.components[0].aliases.push(name.clone());
        library.aliases.push(SchLibAlias {
            alias_name: name.clone(),
            canonical_name: name,
        });
        library.embedded_images = vec![
            SchLibEmbeddedImage {
                file_name: "same".to_owned(),
                data: vec![1, 2, 3],
            },
            SchLibEmbeddedImage {
                file_name: "same".to_owned(),
                data: vec![4, 5],
            },
        ];
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        library
    }

    #[test]
    fn complete_snapshot_retains_all_typed_state_and_stream_partition() {
        let mut original = rich_library();
        let snapshot = original.snapshot().unwrap();
        assert_eq!(snapshot.component_count(), 1);
        assert_eq!(snapshot.components().count(), 1);
        assert_eq!(
            snapshot.component(0).unwrap(),
            snapshot.components().next().unwrap()
        );
        assert_eq!(snapshot.materialize().unwrap(), original);
        assert_eq!(
            snapshot.materialize().unwrap().snapshot().unwrap(),
            snapshot
        );
        assert!(snapshot.component(1).is_err());
        original.components[0].additional_records.clear();
        assert_ne!(
            snapshot.materialize().unwrap(),
            original,
            "captured Additional records are independent of the live document"
        );
    }

    #[test]
    fn description_replacement_preserves_every_other_typed_field() {
        let original = rich_library();
        let snapshot = original.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        let replacement = component
            .with_description(Some("new description".to_owned()))
            .unwrap();
        let updated = snapshot.replace_component(0, replacement).unwrap();
        let mut expected = original.clone();
        expected.components[0].component.component_description = "new description".to_owned();
        expected.header.components[0].description = "new description".to_owned();
        assert_eq!(updated.materialize().unwrap(), expected);
        assert_eq!(snapshot.materialize().unwrap(), original);
        assert_eq!(
            updated.component(0).unwrap().description(),
            Some("new description")
        );
        assert_eq!(
            component.with_description(None).unwrap(),
            component.with_description(Some(String::new())).unwrap()
        );
        assert!(snapshot.replace_component(1, component.clone()).is_err());
        let mut renamed = component;
        renamed.index.lib_ref = "different".to_owned();
        assert!(snapshot.replace_component(0, renamed).is_err());
        assert_eq!(snapshot.materialize().unwrap(), original);
    }

    #[test]
    fn batch_replacement_resolves_original_slots_and_rejects_conflicts_atomically() {
        let mut original = rich_library();
        let mut second = original.components[0].clone();
        second.component.lib_reference = "second".to_owned();
        let mut second_index = original.header.components[0].clone();
        second_index.lib_ref = "second".to_owned();
        second_index.aliases.clear();
        original.components.push(second);
        original.header.components.push(second_index);
        original.header.weight = compute_weight(&original.header, &original.components);
        let snapshot = original.snapshot().unwrap();
        let first = snapshot
            .component(0)
            .unwrap()
            .with_description(Some("first update".to_owned()))
            .unwrap();
        let second = snapshot
            .component(1)
            .unwrap()
            .with_description(Some("second update".to_owned()))
            .unwrap();
        let result = snapshot
            .replace_components([(1, second.clone()), (0, first.clone())])
            .unwrap();
        assert_eq!(
            result.component(0).unwrap().description(),
            Some("first update")
        );
        assert_eq!(
            result.component(1).unwrap().description(),
            Some("second update")
        );
        assert!(
            snapshot
                .replace_components([(0, first.clone()), (0, first.clone())])
                .is_err()
        );
        assert!(
            snapshot
                .replace_components([(0, first.clone()), (2, second.clone())])
                .is_err()
        );
        assert!(
            snapshot
                .replace_components([(0, first), (1, snapshot.component(0).unwrap())])
                .is_err()
        );
        // The first valid replacement must not take effect when final native
        // invariant validation rejects a later aggregate.
        let mut invalid = second;
        invalid.index.part_count += 1;
        assert!(snapshot.replace_components([(1, invalid)]).is_err());
        assert_eq!(snapshot.materialize().unwrap(), original);
    }

    #[test]
    fn all_snapshot_boundaries_reject_invalid_complete_ownership() {
        let original = rich_library().snapshot().unwrap();
        for case in 0..5 {
            let mut invalid = original.clone();
            let component = &mut invalid.library.components[0];
            let expected = match case {
                0 | 1 | 4 => {
                    let SchRecord::Line(line) = &mut component.additional_records[1] else {
                        unreachable!()
                    };
                    line.base.owner_index = match case {
                        0 => 1,
                        1 => 99,
                        _ => -1,
                    };
                    match case {
                        0 => "cycle",
                        1 => "exceeds stream record count",
                        _ => "negative owner index",
                    }
                }
                2 => {
                    let implementation = component
                        .records
                        .iter_mut()
                        .find_map(|record| match record {
                            SchRecord::Implementation(implementation) => Some(implementation),
                            _ => None,
                        })
                        .unwrap();
                    implementation.base.owner_index = 0;
                    "required parent kind ImplementationList"
                }
                _ => {
                    let pin = component
                        .records
                        .iter_mut()
                        .find_map(|record| match record {
                            SchRecord::Pin(pin) => Some(pin),
                            _ => None,
                        })
                        .unwrap();
                    pin.owner_index = 0;
                    pin.owner_index_additional_list = true;
                    "Data records selecting Additional ownership are unsupported"
                }
            };
            // These invalid graphs escaped the old Data-only invariant check.
            invalid.library.validate_invariants().unwrap();
            for error in [
                invalid.library.snapshot().unwrap_err(),
                invalid.materialize().unwrap_err(),
                invalid.replace_components([]).unwrap_err(),
            ] {
                let message = error.to_string();
                assert!(message.contains(expected), "case {case}: {message}");
                assert!(message.contains("component[0]"), "case {case}: {message}");
                assert!(
                    message.contains(original.component(0).unwrap().lib_reference()),
                    "case {case}: {message}"
                );
            }
            let mut replacement = original.component(0).unwrap();
            replacement.component = invalid.library.components[0].clone();
            let error = original
                .replace_component(0, replacement)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "candidate case {case}: {error}");
            assert_eq!(
                original.materialize().unwrap().snapshot().unwrap(),
                original
            );
        }
    }

    #[test]
    fn snapshot_rejects_invalid_native_state_at_capture_and_materialize() {
        let mut library = rich_library();
        library.header.components[0].part_count += 1;
        assert!(library.snapshot().is_err());
        let reference_context = Arc::new(ReferenceContext::capture(&library));
        assert!(
            SchLibSnapshot {
                library,
                reference_context
            }
            .materialize()
            .is_err()
        );
    }

    #[test]
    fn snapshot_rejects_foreign_reference_namespaces_before_replacement() {
        let original = rich_library();
        let input = original.snapshot().unwrap();
        // An independently captured copy with the same complete namespace is
        // compatible; Arc pointer identity must never become an execution key.
        let compatible = original.clone().snapshot().unwrap().component(0).unwrap();
        assert_eq!(input.replace_component(0, compatible).unwrap(), input);

        for change in 0..4 {
            let mut foreign = original.clone();
            match change {
                0 => foreign.header.unique_id.push_str("different"),
                1 => foreign.header.fonts[0].name.push_str("different"),
                2 => foreign.embedded_images[0].data.push(77),
                3 => foreign.header.display_settings.system_font = Some(999),
                _ => unreachable!(),
            }
            let replacement = foreign
                .snapshot()
                .unwrap()
                .component(0)
                .unwrap()
                .with_description(Some("foreign".into()))
                .unwrap();
            let error = input
                .replace_component(0, replacement)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("namespace differs"),
                "case {change}: {error}"
            );
            assert_eq!(input.materialize().unwrap(), original);
        }
    }

    #[test]
    fn native_component_export_preserves_stored_pin_count_cache() {
        let mut library = rich_library();
        library.components[0].component.display_mode_count = 2;
        for mode in [0, 1] {
            let mut pin =
                crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
            pin.owner_part_display_mode = mode;
            library.components[0].records.push(SchRecord::Pin(pin));
        }
        for count in [0, 17] {
            library.components[0].component.all_pin_count = count;
            let data = crate::schlib::serialize_component_data(&library.components[0]).unwrap();
            let blocks = crate::block_stream::parse_blocks(&data).unwrap();
            let mut params = ParameterCollection::from_bytes(&blocks[0].data).unwrap();
            let actual = params
                .remove_with_default::<i32>(
                    altium_format_types::constants::component::ALL_PIN_COUNT,
                    0,
                )
                .unwrap();
            assert_eq!(
                actual, count,
                "native export must preserve the data object's stored AllPinCount, including zero"
            );
        }
    }

    #[test]
    fn description_snapshot_survives_native_save_reopen() {
        let temp = tempfile::tempdir().unwrap();
        let before_path = temp.path().join("before.SchLib");
        let after_path = temp.path().join("after.SchLib");
        // Establish the native writer's canonical baseline before comparison.
        // This separate test does not claim all programmatic rich state is
        // already supported by every native serializer/normalization path.
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.components[0].component.display_mode_count = 2;
        for mode in [0, 1] {
            let mut pin =
                crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
            pin.owner_part_display_mode = mode;
            library.components[0].records.push(SchRecord::Pin(pin));
        }
        library.header.weight = compute_weight(&library.header, &library.components);
        library.save(&before_path).unwrap();
        let before = SchLib::open(&before_path).unwrap().snapshot().unwrap();
        assert_eq!(before.library.components[0].component.all_pin_count, 0);
        let component = before
            .component(0)
            .unwrap()
            .with_description(Some("snapshot edit".to_owned()))
            .unwrap();
        let after = before.replace_component(0, component).unwrap();
        after.materialize().unwrap().save(&after_path).unwrap();
        let reopened = SchLib::open(&after_path).unwrap().snapshot().unwrap();
        assert_eq!(reopened, after);
    }
}
