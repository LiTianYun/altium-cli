//! Native scalar projection and immutable edits. Source intent is resolved by
//! the sync layer before calling these methods.

use super::SchLibComponentSnapshot;
use crate::param_collection::ParameterCollection;
use crate::sch_records::{SchDesignator, SchRecord};
use crate::schlib::ownership::{
    RecordAddress, RecordItem, RecordOwner, RecordStream, assemble_records,
};
use crate::{AltiumFormatError, Result, ResultExt};
use altium_format_types::common::ComponentKind;

impl SchLibComponentSnapshot {
    /// Resolves the three native compatibility fields with AD26's import
    /// precedence (FileFormatV5.ImportComponent).
    pub fn component_kind(&self) -> ComponentKind {
        let root = &self.component.component;
        if root.component_kind_version3 == ComponentKind::Jumper {
            ComponentKind::Jumper
        } else if matches!(
            root.component_kind_version2,
            ComponentKind::StandardNoBom | ComponentKind::Jumper
        ) {
            root.component_kind_version2
        } else {
            root.component_kind
        }
    }

    /// Updates only the native compatibility representation of component kind.
    /// An already matching effective kind retains its existing representation.
    pub fn with_component_kind(&self, kind: ComponentKind) -> Result<Self> {
        let mut candidate = self.clone();
        if self.component_kind() != kind {
            let root = &mut candidate.component.component;
            (
                root.component_kind,
                root.component_kind_version2,
                root.component_kind_version3,
            ) = match kind {
                ComponentKind::Jumper => (ComponentKind::Standard, ComponentKind::Standard, kind),
                ComponentKind::StandardNoBom => {
                    (ComponentKind::Standard, kind, ComponentKind::Standard)
                }
                _ => (kind, ComponentKind::Standard, ComponentKind::Standard),
            };
        }
        Ok(candidate)
    }

    pub fn show_hidden_pins(&self) -> bool {
        self.component.component.show_hidden_pins
    }

    pub fn with_show_hidden_pins(&self, value: bool) -> Result<Self> {
        let mut candidate = self.clone();
        candidate.component.component.show_hidden_pins = value;
        Ok(candidate)
    }

    /// The number of native active parts, excluding reserved native part zero.
    /// On success the result is always ≥ 1: the load path upgrades legacy
    /// native `PartCount < 2` (including wire-absent) to 2 as a documented
    /// format upgrade (resolutions §3 rule 1), so every file-loaded snapshot
    /// satisfies this contract. Programmatically constructed state that
    /// bypassed the load upgrade can still carry a smaller native count; that
    /// remains a hard error here — never a silent clamp. No structural part
    /// edit is performed.
    pub fn effective_part_count(&self) -> Result<i32> {
        let count = self.component.component.part_count;
        if count < 2 {
            return Err(self.scalar_error(
                "part_count",
                format!(
                    "native PartCount {count} has no effective parts; loaded documents are \
                     upgraded to ≥ 2 at load, so this state bypassed the documented upgrade"
                ),
            ));
        }
        Ok(count - 1)
    }

    /// Returns the sole Designator record's text. Empty text remains an explicit
    /// value; absence means no record. Multiple records are ambiguous even when
    /// their texts agree, including duplicates split across Data and Additional.
    pub fn designator(&self) -> Result<Option<&str>> {
        Ok(self.designator_position()?.map(|(additional, index)| {
            let records = if additional {
                &self.component.additional_records
            } else {
                &self.component.records
            };
            let SchRecord::Designator(record) = &records[index] else {
                unreachable!("position selected from Designator records")
            };
            record.text.as_str()
        }))
    }

    /// Edits the existing record in place, preserving all non-text attributes.
    /// Missing records are appended to Data with typed defaults. Removing an
    /// existing record uses immutable-slot ownership remapping across both streams
    /// and rejects retained dependents.
    pub fn with_designator(&self, value: Option<String>) -> Result<Self> {
        let position = self.designator_position()?;
        if let (Some((additional, index)), None) = (position, &value) {
            let retained = |stream, count| {
                (0..count)
                    .filter(|slot| {
                        !(additional == (stream == RecordStream::Additional) && *slot == index)
                    })
                    .map(|index| RecordItem::Original(RecordAddress { stream, index }))
                    .collect::<Vec<_>>()
            };
            let data = retained(RecordStream::Data, self.component.records.len());
            let additional = retained(
                RecordStream::Additional,
                self.component.additional_records.len(),
            );
            let records =
                assemble_records(&self.component, &data, &additional).with_context(|| {
                    format!(
                        "removing Designator from component '{}'",
                        self.lib_reference()
                    )
                })?;
            let mut candidate = self.clone();
            candidate.component.records = records.data;
            candidate.component.additional_records = records.additional;
            return Ok(candidate);
        }
        let mut candidate = self.clone();
        match (position, value) {
            (Some((additional, index)), Some(text)) => {
                let records = if additional {
                    &mut candidate.component.additional_records
                } else {
                    &mut candidate.component.records
                };
                let SchRecord::Designator(record) = &mut records[index] else {
                    unreachable!("position selected from Designator records")
                };
                record.text = text;
            }
            (None, Some(text)) => {
                let mut record = SchDesignator::from_params(&mut ParameterCollection::new())
                    .with_context(|| {
                        format!(
                            "creating Designator for component '{}'",
                            self.lib_reference()
                        )
                    })?;
                crate::schlib::validation::check_font_id_exists(
                    &self.reference_context.fonts,
                    record.font_id,
                )
                .with_context(|| {
                    format!(
                        "creating Designator for component '{}': default font must exist \
                         in the captured font table",
                        self.lib_reference()
                    )
                })?;
                record.text = text;
                record.unique_id = crate::util::generate_unique_id();
                let mut data: Vec<_> = (0..self.component.records.len())
                    .map(|index| {
                        RecordItem::Original(RecordAddress {
                            stream: RecordStream::Data,
                            index,
                        })
                    })
                    .collect();
                data.push(RecordItem::New {
                    id: 0,
                    record: SchRecord::Designator(record),
                    owner: RecordOwner::Root,
                });
                let additional: Vec<_> = (0..self.component.additional_records.len())
                    .map(|index| {
                        RecordItem::Original(RecordAddress {
                            stream: RecordStream::Additional,
                            index,
                        })
                    })
                    .collect();
                let records =
                    assemble_records(&self.component, &data, &additional).with_context(|| {
                        format!(
                            "inserting Designator into component '{}'",
                            self.lib_reference()
                        )
                    })?;
                candidate.component.records = records.data;
                candidate.component.additional_records = records.additional;
            }
            (_, None) => {}
        }
        Ok(candidate)
    }

    fn designator_position(&self) -> Result<Option<(bool, usize)>> {
        let mut found = None;
        for (additional, records) in [
            (false, &self.component.records),
            (true, &self.component.additional_records),
        ] {
            for (index, record) in records.iter().enumerate() {
                if matches!(record, SchRecord::Designator(_)) {
                    if found.is_some() {
                        return Err(self.scalar_error(
                            "designator",
                            "multiple Designator records are ambiguous",
                        ));
                    }
                    found = Some((additional, index));
                }
            }
        }
        Ok(found)
    }

    fn scalar_error(&self, key: &str, detail: impl std::fmt::Display) -> AltiumFormatError {
        AltiumFormatError::InvalidParamValue {
            key: key.to_owned(),
            detail: format!("component '{}': {detail}", self.lib_reference()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sch_records::{SchLine, SchParameter};
    use crate::schlib::SchLib;

    fn library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.components[0].component.vault_guid = "outside-scope-vault".to_owned();
        library.header.fonts[0].italic = true;
        let SchRecord::Designator(designator) = &mut library.components[0].records[0] else {
            panic!("blank native fixture has Designator")
        };
        designator.not_allow_database_synchronize = true;
        designator.description = "preserve designator metadata".to_owned();
        designator.base.selection_memory = 3;
        library
    }

    #[test]
    fn scalar_fields_preserve_complete_outside_state_and_kind_compatibility() {
        let library = library();
        let snapshot = library.snapshot().unwrap();
        let original = snapshot.component(0).unwrap();
        let updated = original
            .with_show_hidden_pins(true)
            .unwrap()
            .with_component_kind(ComponentKind::Jumper)
            .unwrap()
            .with_designator(Some("X?".to_owned()))
            .unwrap();
        let mut expected = original.clone();
        expected.component.component.show_hidden_pins = true;
        expected.component.component.component_kind = ComponentKind::Standard;
        expected.component.component.component_kind_version2 = ComponentKind::Standard;
        expected.component.component.component_kind_version3 = ComponentKind::Jumper;
        let SchRecord::Designator(designator) = &mut expected.component.records[0] else {
            unreachable!()
        };
        designator.text = "X?".to_owned();
        assert_eq!(updated, expected);
        assert!(updated.show_hidden_pins());
        assert_eq!(updated.component_kind(), ComponentKind::Jumper);
        assert_eq!(updated.designator().unwrap(), Some("X?"));
        assert_eq!(original.effective_part_count().unwrap(), 1);
        assert_eq!(snapshot.materialize().unwrap(), library);
        let materialized = snapshot.replace_component(0, updated).unwrap();
        assert_eq!(
            materialized.library.header.weight,
            snapshot.library.header.weight
        );

        for kind in [
            ComponentKind::Standard,
            ComponentKind::Mechanical,
            ComponentKind::Graphical,
            ComponentKind::NetTieBom,
            ComponentKind::NetTieNoBom,
            ComponentKind::StandardNoBom,
            ComponentKind::Jumper,
        ] {
            let changed = expected.with_component_kind(kind).unwrap();
            assert_eq!(changed.component_kind(), kind);
        }
        let mut compatibility = original.clone();
        compatibility.component.component.component_kind = ComponentKind::Mechanical;
        compatibility.component.component.component_kind_version2 = ComponentKind::StandardNoBom;
        assert_eq!(compatibility.component_kind(), ComponentKind::StandardNoBom);
        assert_eq!(
            compatibility
                .with_component_kind(ComponentKind::StandardNoBom)
                .unwrap(),
            compatibility
        );
        compatibility.component.component.component_kind_version3 = ComponentKind::Jumper;
        assert_eq!(compatibility.component_kind(), ComponentKind::Jumper);
        // Native counts below 2 exist only for state that bypassed the
        // documented load upgrade; the accessor hard-errors instead of
        // reporting zero effective parts (contract: success is always ≥ 1).
        for legacy in [1, 0, -1] {
            compatibility.component.component.part_count = legacy;
            assert!(compatibility.effective_part_count().is_err());
        }
    }

    #[test]
    fn designator_absence_create_clear_and_weight_delta_preserve_input_convention() {
        let mut library = library();
        // Another accepted historical exporter formula, intentionally not the
        // canonical compute_weight value. Scalar edits must retain this offset.
        library.header.weight =
            crate::schlib::compute_weight(&library.header, &library.components) + 1;
        let snapshot = library.snapshot().unwrap();
        let original = snapshot.component(0).unwrap();
        let removed = snapshot
            .replace_component(0, original.with_designator(None).unwrap())
            .unwrap();
        assert_eq!(removed.component(0).unwrap().designator().unwrap(), None);
        assert_eq!(
            removed.library.header.weight,
            snapshot.library.header.weight - 1
        );
        assert_eq!(
            removed.component(0).unwrap().with_designator(None).unwrap(),
            removed.component(0).unwrap()
        );
        let restored = removed
            .replace_component(
                0,
                removed
                    .component(0)
                    .unwrap()
                    .with_designator(Some(String::new()))
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(
            restored.component(0).unwrap().designator().unwrap(),
            Some("")
        );
        assert_eq!(
            restored.library.header.weight,
            snapshot.library.header.weight
        );
        assert!(matches!(
            restored.library.components[0].records.last(),
            Some(SchRecord::Designator(_))
        ));
        assert_eq!(snapshot.materialize().unwrap(), library);
    }

    #[test]
    fn ambiguous_designators_and_unsafe_removals_fail_before_mutation() {
        let snapshot = library().snapshot().unwrap();
        let original = snapshot.component(0).unwrap();
        let mut duplicate = original.clone();
        duplicate
            .component
            .additional_records
            .push(duplicate.component.records[0].clone());
        assert!(duplicate.designator().is_err());
        assert!(duplicate.with_designator(Some("X?".to_owned())).is_err());
        assert!(duplicate.with_designator(None).is_err());
        let mut additional = original.clone();
        additional
            .component
            .additional_records
            .push(SchRecord::Line(
                SchLine::from_params(&mut ParameterCollection::new()).unwrap(),
            ));
        assert!(additional.with_designator(None).is_ok());
        let mut dependent = original.clone();
        let mut parameter = SchParameter::from_params(&mut ParameterCollection::new()).unwrap();
        parameter.base.owner_index = 1;
        dependent
            .component
            .records
            .push(SchRecord::Parameter(parameter));
        assert!(dependent.with_designator(None).is_err());
        assert_eq!(snapshot.component(0).unwrap(), original);
        let mut missing_font = original.with_designator(None).unwrap();
        std::sync::Arc::make_mut(&mut missing_font.reference_context)
            .fonts
            .clear();
        assert!(missing_font.with_designator(Some("X?".to_owned())).is_err());
    }

    #[test]
    fn designator_removal_remaps_retained_data_owners_and_additional_text_edits_preserve_state() {
        let mut library = library();
        let records = &mut library.components[0].records;
        let list_slot = records.len();
        records.push(SchRecord::ImplementationList(
            crate::sch_records::SchImplementationList::from_params(&mut ParameterCollection::new())
                .unwrap(),
        ));
        let mut implementation =
            crate::sch_records::SchImplementation::from_params(&mut ParameterCollection::new())
                .unwrap();
        implementation.base.owner_index = list_slot as i32 + 1;
        implementation.base.index_in_sheet = 777;
        records.push(SchRecord::Implementation(implementation));
        library.header.weight = crate::schlib::compute_weight(&library.header, &library.components);
        let snapshot = library.snapshot().unwrap();
        let cleared = snapshot
            .replace_component(
                0,
                snapshot
                    .component(0)
                    .unwrap()
                    .with_designator(None)
                    .unwrap(),
            )
            .unwrap();
        let Some(SchRecord::Implementation(implementation)) =
            cleared.library.components[0].records.last()
        else {
            panic!("retained implementation")
        };
        assert_eq!(implementation.base.owner_index, list_slot as i32);
        assert_eq!(implementation.base.index_in_sheet, 777);
        let mut additional = snapshot.component(0).unwrap();
        let designator = additional.component.records[0].clone();
        let retained: Vec<_> = (1..additional.component.records.len())
            .map(|index| {
                RecordItem::Original(RecordAddress {
                    stream: RecordStream::Data,
                    index,
                })
            })
            .collect();
        let records = assemble_records(&additional.component, &retained, &[]).unwrap();
        additional.component.records = records.data;
        additional.component.additional_records = records.additional;
        additional.component.additional_records.push(designator);
        let updated = additional.with_designator(Some("A?".to_owned())).unwrap();
        let mut expected = additional;
        let SchRecord::Designator(designator) = &mut expected.component.additional_records[0]
        else {
            unreachable!()
        };
        designator.text = "A?".to_owned();
        assert_eq!(updated, expected);
        assert!(updated.with_designator(None).is_ok());
    }

    #[test]
    fn native_scalar_edits_and_designator_creation_reopen_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scalar.SchLib");
        SchLib::new_blank_ad26().unwrap().save(&path).unwrap();
        let snapshot = SchLib::open(&path).unwrap().snapshot().unwrap();
        let component = snapshot
            .component(0)
            .unwrap()
            .with_component_kind(ComponentKind::StandardNoBom)
            .unwrap()
            .with_show_hidden_pins(true)
            .unwrap()
            .with_designator(Some("P?".to_owned()))
            .unwrap();
        let updated = snapshot.replace_component(0, component).unwrap();
        updated.materialize().unwrap().save(&path).unwrap();
        assert_eq!(SchLib::open(&path).unwrap().snapshot().unwrap(), updated);
        let cleared = updated
            .replace_component(
                0,
                updated.component(0).unwrap().with_designator(None).unwrap(),
            )
            .unwrap();
        cleared.materialize().unwrap().save(&path).unwrap();
        assert_eq!(SchLib::open(&path).unwrap().snapshot().unwrap(), cleared);
        let created = cleared
            .replace_component(
                0,
                cleared
                    .component(0)
                    .unwrap()
                    .with_designator(Some("J?".to_owned()))
                    .unwrap(),
            )
            .unwrap();
        created.materialize().unwrap().save(&path).unwrap();
        assert_eq!(SchLib::open(&path).unwrap().snapshot().unwrap(), created);
    }
    #[test]
    fn additional_designator_clear_remaps_chain_and_rejects_orphans_atomically() {
        let mut library = library();
        let component = &mut library.components[0];
        let designator = component.records.remove(0);
        let mut parent = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
        parent.base.index_in_sheet = 401;
        let mut child = parent.clone();
        child.base.owner_index = 1;
        child.base.owner_index_additional_list = true;
        child.base.index_in_sheet = 402;
        component.additional_records =
            vec![designator, SchRecord::Line(parent), SchRecord::Line(child)];
        library.header.weight = crate::schlib::compute_weight(&library.header, &library.components);
        let snapshot = library.snapshot().unwrap();
        let original = snapshot.component(0).unwrap();
        let cleared = original.with_designator(None).unwrap();
        let mut expected = original.clone();
        expected.component.additional_records.remove(0);
        expected.component.additional_records[1]
            .base_mut()
            .unwrap()
            .owner_index = 0;
        assert_eq!(cleared, expected);
        let candidate = snapshot
            .replace_component(0, cleared)
            .unwrap()
            .materialize()
            .unwrap();
        assert_eq!(candidate.header.weight, snapshot.library.header.weight);
        let file = tempfile::NamedTempFile::new().unwrap();
        candidate.save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        reopened.validate_invariants().unwrap();
        assert_eq!(
            reopened.components[0].additional_records,
            expected.component.additional_records
        );
        let mut orphan = original.clone();
        orphan.component.additional_records[2]
            .base_mut()
            .unwrap()
            .owner_index = 0;
        let before = orphan.clone();
        let error = orphan.with_designator(None).unwrap_err().to_string();
        assert!(error.contains("orphan"), "{error}");
        assert!(error.contains("Additional"), "{error}");
        assert_eq!(orphan, before);
        assert_eq!(snapshot.component(0).unwrap(), original);
    }

    #[test]
    fn clearing_last_additional_designator_saves_and_reopens_without_additional_stream() {
        let mut library = library();
        let component = &mut library.components[0];
        component
            .additional_records
            .push(component.records.remove(0));
        library.header.weight = crate::schlib::compute_weight(&library.header, &library.components);
        let original_file = tempfile::NamedTempFile::new().unwrap();
        library.save(original_file.path()).unwrap();
        let snapshot = SchLib::open(original_file.path())
            .unwrap()
            .snapshot()
            .unwrap();
        let updated = snapshot
            .component(0)
            .unwrap()
            .with_designator(None)
            .unwrap();
        assert!(updated.component.additional_records.is_empty());
        let candidate = snapshot
            .replace_component(0, updated)
            .unwrap()
            .materialize()
            .unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        candidate.save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        reopened.validate_invariants().unwrap();
        assert!(reopened.components[0].additional_records.is_empty());
        assert_eq!(
            reopened
                .snapshot()
                .unwrap()
                .component(0)
                .unwrap()
                .designator()
                .unwrap(),
            None
        );
        let second = tempfile::NamedTempFile::new().unwrap();
        reopened.save(second.path()).unwrap();
        crate::test_utils::assert_cfb_files_semantic_eq(file.path(), second.path());
    }
    #[test]
    fn designator_creation_cannot_bind_preexisting_invalid_additional_owner() {
        let mut library = library();
        let component = &mut library.components[0];
        component.records.remove(0);
        library.header.weight = crate::schlib::compute_weight(&library.header, &library.components);
        let snapshot = library.snapshot().unwrap();
        // Capture rejects this graph now. Mutate only the private test aggregate
        // to independently exercise the setter's own original-state preflight.
        let mut original = snapshot.component(0).unwrap();
        let mut dangling = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
        dangling.base.owner_index = i32::try_from(original.component.records.len() + 1).unwrap();
        original
            .component
            .additional_records
            .push(SchRecord::Line(dangling));
        let before = original.clone();
        let error = original
            .with_designator(Some("X?".to_owned()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("original SchLib ownership graph"), "{error}");
        assert!(error.contains("Additional"), "{error}");
        assert!(error.contains("exceeds stream"), "{error}");
        assert_eq!(original, before);
        assert!(
            snapshot
                .component(0)
                .unwrap()
                .component
                .additional_records
                .is_empty()
        );
        assert_eq!(original.designator().unwrap(), None);
    }
}
