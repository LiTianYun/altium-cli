//! Structural assembly over immutable Data and Additional record addresses.
//!
//! Native Data address zero is the separate component root; Data child slots
//! use index + 1. Additional addresses are zero-based without a root. Insertion
//! IDs are local to one assembly. Original records never migrate between streams.
//! IndexInSheet is preserved; it is not an ownership address.

use std::collections::{HashMap, HashSet};

use super::{SchLibComponent, record_owner_index};
use crate::sch_records::SchRecord;
use crate::{AltiumFormatError, Result, ResultExt};
use altium_format_types::constants::record_structure::OWNER_INDEX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RecordStream {
    Data,
    Additional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct RecordAddress {
    pub stream: RecordStream,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordOwner {
    Root,
    Original(RecordAddress),
    New { stream: RecordStream, id: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum RecordItem {
    Original(RecordAddress),
    /// Explicit owner replaces only the native owner index and address-space flag.
    New {
        id: usize,
        record: SchRecord,
        owner: RecordOwner,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct AssembledRecords {
    pub data: Vec<SchRecord>,
    pub additional: Vec<SchRecord>,
}

// Convenience for existing callers that edit Data while retaining all Additional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DataOwner {
    Root,
    Original(usize),
    New(usize),
}
#[derive(Debug, Clone, PartialEq)]
pub(super) enum DataRecordItem {
    Original(usize),
    New {
        id: usize,
        record: SchRecord,
        owner: DataOwner,
    },
}

fn address(stream: RecordStream, index: usize) -> RecordAddress {
    RecordAddress { stream, index }
}

fn error(
    component: &SchLibComponent,
    node: Option<RecordAddress>,
    detail: impl std::fmt::Display,
) -> AltiumFormatError {
    AltiumFormatError::InvalidParamValue {
        key: OWNER_INDEX.to_owned(),
        detail: format!(
            "component '{}'{}: {detail}",
            component.component.lib_reference,
            node.map(|n| format!(" /{:?} record[{}]", n.stream, n.index))
                .unwrap_or_default()
        ),
    }
}

fn records_in<'a>(
    data: &'a [SchRecord],
    additional: &'a [SchRecord],
    stream: RecordStream,
) -> &'a [SchRecord] {
    match stream {
        RecordStream::Data => data,
        RecordStream::Additional => additional,
    }
}

fn record_at<'a>(
    data: &'a [SchRecord],
    additional: &'a [SchRecord],
    node: RecordAddress,
) -> Option<&'a SchRecord> {
    records_in(data, additional, node.stream).get(node.index)
}

fn native_address(component: &SchLibComponent, node: RecordAddress) -> Result<i32> {
    let index = match node.stream {
        RecordStream::Data => node.index.checked_add(1),
        RecordStream::Additional => Some(node.index),
    };
    index.and_then(|i| i32::try_from(i).ok()).ok_or_else(|| {
        error(
            component,
            Some(node),
            "record address exceeds native signed OwnerIndex range",
        )
    })
}

fn owner_of(
    component: &SchLibComponent,
    node: RecordAddress,
    record: &SchRecord,
) -> Result<Option<RecordAddress>> {
    let additional = match record {
        SchRecord::Pin(pin) => pin.owner_index_additional_list,
        SchRecord::Component(_) => {
            return Err(error(
                component,
                Some(node),
                "nested Component records are unsupported; the SchLib component root is separate",
            ));
        }
        other => {
            other
                .base()
                .ok_or_else(|| {
                    error(
                        component,
                        Some(node),
                        "record has no owner address-space field",
                    )
                })?
                .owner_index_additional_list
        }
    };
    let native = record_owner_index(record);
    let index = usize::try_from(native).map_err(|_| {
        error(
            component,
            Some(node),
            format!("negative owner index {native}"),
        )
    })?;
    Ok(if additional {
        Some(address(RecordStream::Additional, index))
    } else {
        index.checked_sub(1).map(|i| address(RecordStream::Data, i))
    })
}

/// Validates complete parsed ownership without rebuilding or changing records.
/// Used by snapshot boundaries as well as structural assembly.
pub(super) fn validate_component_ownership(component: &SchLibComponent) -> Result<()> {
    validate_graph(component, &component.records, &component.additional_records)
}

fn validate_graph(
    component: &SchLibComponent,
    data: &[SchRecord],
    additional: &[SchRecord],
) -> Result<()> {
    let mut edges = HashMap::new();
    let mut nodes = Vec::with_capacity(data.len().saturating_add(additional.len()));
    for stream in [RecordStream::Data, RecordStream::Additional] {
        for (index, record) in records_in(data, additional, stream).iter().enumerate() {
            let node = address(stream, index);
            native_address(component, node)?;
            let owner = owner_of(component, node, record)?;
            let parent = match owner {
                Some(owner) => Some(record_at(data, additional, owner).ok_or_else(|| {
                    error(
                        component,
                        Some(node),
                        format!(
                            "owner {:?} record[{}] exceeds stream record count",
                            owner.stream, owner.index
                        ),
                    )
                })?),
                None => None,
            };
            let required = match record {
                SchRecord::Implementation(_)
                    if !matches!(parent, Some(SchRecord::ImplementationList(_))) =>
                {
                    Some("ImplementationList")
                }
                SchRecord::ImplementationMap(_)
                    if !matches!(parent, Some(SchRecord::Implementation(_))) =>
                {
                    Some("Implementation")
                }
                SchRecord::MapDefiner(_)
                    if !matches!(parent, Some(SchRecord::ImplementationMap(_))) =>
                {
                    Some("ImplementationMap")
                }
                SchRecord::ParameterList(_)
                    if !matches!(parent, Some(SchRecord::Implementation(_))) =>
                {
                    Some("Implementation")
                }
                _ => None,
            };
            if let Some(kind) = required {
                return Err(error(
                    component,
                    Some(node),
                    format!("required parent kind {kind}; owner is {owner:?}"),
                ));
            }
            nodes.push(node);
            edges.insert(node, owner);
        }
    }
    // Iterative traversal avoids recursion depth limits for long native chains.
    let mut finished = HashSet::new();
    for &node in &nodes {
        let mut path = HashSet::new();
        let mut cursor = Some(node);
        while let Some(current) = cursor {
            if finished.contains(&current) {
                break;
            }
            if !path.insert(current) {
                return Err(error(
                    component,
                    Some(node),
                    format!(
                        "ownership cycle through {:?} record[{}]",
                        current.stream, current.index
                    ),
                ));
            }
            cursor = edges[&current];
        }
        finished.extend(path);
    }
    for node in nodes {
        if let Some(owner) = edges[&node] {
            if node.stream == RecordStream::Data && owner.stream == RecordStream::Additional {
                return Err(error(
                    component,
                    Some(node),
                    "Data records selecting Additional ownership are unsupported by the verified SchLib load path",
                ));
            }
            if node.stream == owner.stream && owner.index >= node.index {
                return Err(error(
                    component,
                    Some(node),
                    "owner refers to a self or forward record in the same stream",
                ));
            }
        }
    }
    Ok(())
}

/// Assembles complete typed records from immutable slots and local insertion IDs.
/// Deletion authorization belongs to the caller. Input and final graph validation
/// precede any caller mutation. Additional owners may refer to any Data record;
/// same-stream owners must precede children, matching incremental native loading.
pub(super) fn assemble_records(
    component: &SchLibComponent,
    data_items: &[RecordItem],
    additional_items: &[RecordItem],
) -> Result<AssembledRecords> {
    validate_graph(component, &component.records, &component.additional_records)
        .context("validating original SchLib ownership graph")?;
    let mut originals = HashMap::new();
    let mut new_records = HashMap::new();
    for (stream, items) in [
        (RecordStream::Data, data_items),
        (RecordStream::Additional, additional_items),
    ] {
        for (index, item) in items.iter().enumerate() {
            let node = address(stream, index);
            native_address(component, node)?;
            match item {
                RecordItem::Original(original) => {
                    if original.stream != stream {
                        return Err(error(
                            component,
                            Some(node),
                            "moving original records between Data and Additional is unsupported",
                        ));
                    }
                    if record_at(&component.records, &component.additional_records, *original)
                        .is_none()
                    {
                        return Err(error(
                            component,
                            Some(node),
                            format!(
                                "original slot {} exceeds input {:?} count",
                                original.index, stream
                            ),
                        ));
                    }
                    if originals.insert(*original, node).is_some() {
                        return Err(error(
                            component,
                            Some(node),
                            format!("duplicate original slot {}", original.index),
                        ));
                    }
                }
                RecordItem::New { id, record, .. } => {
                    if matches!(record, SchRecord::Component(_)) {
                        return Err(error(
                            component,
                            Some(node),
                            "nested Component records are unsupported",
                        ));
                    }
                    if new_records.insert((stream, *id), node).is_some() {
                        return Err(error(
                            component,
                            Some(node),
                            format!("duplicate new record ID {id}"),
                        ));
                    }
                }
            }
        }
    }
    let mut output = AssembledRecords {
        data: Vec::with_capacity(data_items.len()),
        additional: Vec::with_capacity(additional_items.len()),
    };
    for (stream, items) in [
        (RecordStream::Data, data_items),
        (RecordStream::Additional, additional_items),
    ] {
        for (index, item) in items.iter().enumerate() {
            let node = address(stream, index);
            let (mut record, owner) = match item {
                RecordItem::Original(original) => {
                    let record =
                        record_at(&component.records, &component.additional_records, *original)
                            .ok_or_else(|| {
                                error(
                                    component,
                                    Some(node),
                                    "original record disappeared during immutable assembly",
                                )
                            })?;
                    let owner = owner_of(component, *original, record)?
                        .map(RecordOwner::Original)
                        .unwrap_or(RecordOwner::Root);
                    (record.clone(), owner)
                }
                RecordItem::New { record, owner, .. } => (record.clone(), *owner),
            };
            let final_owner = match owner {
                RecordOwner::Root => None,
                RecordOwner::Original(original) => {
                    Some(*originals.get(&original).ok_or_else(|| {
                        error(
                            component,
                            Some(node),
                            format!(
                                "retained orphan: original parent {:?} slot {} is absent",
                                original.stream, original.index
                            ),
                        )
                    })?)
                }
                RecordOwner::New { stream, id } => {
                    Some(*new_records.get(&(stream, id)).ok_or_else(|| {
                        error(
                            component,
                            Some(node),
                            format!("new parent ID {id} in {stream:?} is absent"),
                        )
                    })?)
                }
            };
            let (native, additional) = match final_owner {
                None => (0, false),
                Some(owner) => (
                    native_address(component, owner)?,
                    owner.stream == RecordStream::Additional,
                ),
            };
            match &mut record {
                SchRecord::Pin(pin) => {
                    pin.owner_index = native;
                    pin.owner_index_additional_list = additional;
                }
                other => {
                    let base = other.base_mut().ok_or_else(|| {
                        error(component, Some(node), "record has no writable owner fields")
                    })?;
                    base.owner_index = native;
                    base.owner_index_additional_list = additional;
                }
            }
            match stream {
                RecordStream::Data => output.data.push(record),
                RecordStream::Additional => output.additional.push(record),
            }
        }
    }
    validate_graph(component, &output.data, &output.additional)
        .context("validating assembled SchLib ownership graph")?;
    Ok(output)
}

/// Data-edit convenience wrapper. The caller must assign BOTH returned arrays:
/// retained Additional records can reference remapped Data parents.
pub(super) fn assemble_data_records(
    component: &SchLibComponent,
    items: &[DataRecordItem],
) -> Result<AssembledRecords> {
    let data: Vec<_> = items
        .iter()
        .map(|item| match item {
            DataRecordItem::Original(index) => {
                RecordItem::Original(address(RecordStream::Data, *index))
            }
            DataRecordItem::New { id, record, owner } => RecordItem::New {
                id: *id,
                record: record.clone(),
                owner: match owner {
                    DataOwner::Root => RecordOwner::Root,
                    DataOwner::Original(index) => {
                        RecordOwner::Original(address(RecordStream::Data, *index))
                    }
                    DataOwner::New(id) => RecordOwner::New {
                        stream: RecordStream::Data,
                        id: *id,
                    },
                },
            },
        })
        .collect();
    let additional: Vec<_> = (0..component.additional_records.len())
        .map(|index| RecordItem::Original(address(RecordStream::Additional, index)))
        .collect();
    assemble_records(component, &data, &additional)
}

#[cfg(test)]
mod tests {
    use super::super::SchLib;
    use super::*;
    use crate::param_collection::ParameterCollection;
    use crate::sch_records::{
        SchImplementation, SchImplementationList, SchImplementationMap, SchLine, SchMapDefiner,
        SchParameterList,
    };

    fn line(owner: i32) -> SchRecord {
        let mut value = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
        value.base.owner_index = owner;
        value.base.index_in_sheet = 73;
        SchRecord::Line(value)
    }

    fn component(records: Vec<SchRecord>) -> SchLibComponent {
        let mut component = SchLib::new_blank_ad26().unwrap().components.remove(0);
        component.records = records;
        component.additional_records.clear();
        component
    }

    fn tree() -> Vec<SchRecord> {
        let list = SchImplementationList::from_params(&mut ParameterCollection::new()).unwrap();
        let mut implementation =
            SchImplementation::from_params(&mut ParameterCollection::new()).unwrap();
        implementation.base.owner_index = 1;
        implementation.model_name = "complete-model".to_owned();
        let mut map = SchImplementationMap::from_params(&mut ParameterCollection::new()).unwrap();
        map.base.owner_index = 2;
        let mut definer = SchMapDefiner::from_params(&mut ParameterCollection::new()).unwrap();
        definer.base.owner_index = 3;
        let mut parameters =
            SchParameterList::from_params(&mut ParameterCollection::new()).unwrap();
        parameters.base.owner_index = 2;
        vec![
            SchRecord::ImplementationList(list),
            SchRecord::Implementation(implementation),
            SchRecord::ImplementationMap(map),
            SchRecord::MapDefiner(definer),
            SchRecord::ParameterList(parameters),
        ]
    }

    fn owners(records: &[SchRecord]) -> Vec<i32> {
        records.iter().map(record_owner_index).collect()
    }

    #[test]
    fn moves_whole_implementation_subtree_and_preserves_complete_fields() {
        let mut records = tree();
        records.push(line(0));
        records.push(line(0));
        let input = component(records);
        let before = input.clone();
        let request = [5, 6, 0, 1, 2, 3, 4].map(DataRecordItem::Original);
        let output = assemble_data_records(&input, &request).unwrap().data;
        assert_eq!(owners(&output), [0, 0, 0, 3, 4, 5, 4]);
        for (slot, old) in [5, 6, 0, 1, 2, 3, 4].into_iter().enumerate() {
            let mut restored = output[slot].clone();
            restored.base_mut().unwrap().owner_index = record_owner_index(&input.records[old]);
            assert_eq!(restored, input.records[old]);
        }
        assert_eq!(input, before);
    }

    #[test]
    fn simultaneous_insert_delete_reorder_and_explicit_new_parent() {
        let input = component(vec![line(0), line(1), line(0), line(0)]);
        let request = vec![
            DataRecordItem::Original(3),
            DataRecordItem::New {
                id: 40,
                record: line(999),
                owner: DataOwner::Root,
            },
            DataRecordItem::New {
                id: 8,
                record: line(-1),
                owner: DataOwner::New(40),
            },
            DataRecordItem::Original(0),
            DataRecordItem::Original(1),
            DataRecordItem::New {
                id: 20,
                record: line(999),
                owner: DataOwner::Original(1),
            },
        ];
        let output = assemble_data_records(&input, &request).unwrap().data;
        assert_eq!(owners(&output), [0, 0, 2, 0, 4, 5]);
        assert!(
            output
                .iter()
                .all(|r| r.base().unwrap().index_in_sheet == 73)
        );
        assert_eq!(assemble_data_records(&input, &[]).unwrap().data, []);
    }

    #[test]
    fn rejects_orphans_repeated_slots_ids_and_illegal_reorders() {
        let input = component(vec![line(0), line(1)]);
        for (request, expected) in [
            (vec![DataRecordItem::Original(1)], "orphan"),
            (
                vec![DataRecordItem::Original(0), DataRecordItem::Original(0)],
                "duplicate original",
            ),
            (vec![DataRecordItem::Original(2)], "exceeds input"),
            (
                vec![DataRecordItem::Original(1), DataRecordItem::Original(0)],
                "forward",
            ),
            (
                vec![DataRecordItem::New {
                    id: 3,
                    record: line(0),
                    owner: DataOwner::New(9),
                }],
                "parent ID 9",
            ),
            (
                vec![DataRecordItem::New {
                    id: 3,
                    record: line(0),
                    owner: DataOwner::New(3),
                }],
                "cycle",
            ),
            (
                vec![
                    DataRecordItem::New {
                        id: 3,
                        record: line(0),
                        owner: DataOwner::Root,
                    },
                    DataRecordItem::New {
                        id: 3,
                        record: line(0),
                        owner: DataOwner::Root,
                    },
                ],
                "duplicate new",
            ),
        ] {
            let error = assemble_data_records(&input, &request)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
            assert!(error.contains("/Data record["), "{error}");
        }
    }

    #[test]
    fn validates_original_graph_and_required_parent_kinds_including_root() {
        for (records, expected) in [
            (vec![line(-1)], "negative"),
            (vec![line(2)], "exceeds stream"),
            (vec![line(1)], "cycle"),
            (vec![line(2), line(0)], "forward"),
        ] {
            let error = assemble_data_records(&component(records), &[])
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
        for mut record in tree().into_iter().skip(1) {
            record.base_mut().unwrap().owner_index = 0;
            let error = assemble_data_records(&component(vec![record.clone()]), &[])
                .unwrap_err()
                .to_string();
            assert!(error.contains("required parent kind"), "{error}");
            let input = component(vec![line(0)]);
            let request = [
                DataRecordItem::Original(0),
                DataRecordItem::New {
                    id: 0,
                    record,
                    owner: DataOwner::Original(0),
                },
            ];
            let error = assemble_data_records(&input, &request)
                .unwrap_err()
                .to_string();
            assert!(error.contains("required parent kind"), "{error}");
        }
    }

    fn original(stream: RecordStream, index: usize) -> RecordItem {
        RecordItem::Original(address(stream, index))
    }
    fn additional_line(owner: i32) -> SchRecord {
        let mut record = line(owner);
        record.base_mut().unwrap().owner_index_additional_list = true;
        record
    }

    #[test]
    fn pin_clone_preserves_sidecar_fields_and_remaps_additional_owners() {
        let mut pin = crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
        pin.owner_index = 0;
        pin.owner_index_additional_list = true;
        pin.owner_part_id = 3;
        pin.defined_functions = vec!["GPIO".to_owned(), "CLOCK".to_owned()];
        pin.selected_functions = vec!["CLOCK".to_owned()];
        pin.swap_id_pin = "swap-pin".to_owned();
        let mut input = component(vec![line(0)]);
        input.additional_records = vec![line(1), SchRecord::Pin(pin.clone())];
        let output = assemble_records(
            &input,
            &[original(RecordStream::Data, 0)],
            &[
                RecordItem::New {
                    id: 4,
                    record: line(0),
                    owner: RecordOwner::Root,
                },
                original(RecordStream::Additional, 0),
                original(RecordStream::Additional, 1),
            ],
        )
        .unwrap();
        pin.owner_index = 1;
        assert_eq!(output.additional[2], SchRecord::Pin(pin));
        let mut invalid = input.clone();
        invalid.records[0]
            .base_mut()
            .unwrap()
            .owner_index_additional_list = true;
        let error = assemble_records(&invalid, &[], &[])
            .unwrap_err()
            .to_string();
        assert!(error.contains("cycle"), "{error}");
    }

    #[test]
    fn cross_stream_insert_delete_reorder_and_wrapper_update_both_arrays() {
        let mut input = component(vec![line(0), line(0), line(0)]);
        input.additional_records = vec![line(3), additional_line(0), line(0)];
        let output = assemble_records(
            &input,
            &[
                original(RecordStream::Data, 2),
                original(RecordStream::Data, 0),
            ],
            &[
                original(RecordStream::Additional, 2),
                RecordItem::New {
                    id: 1,
                    record: line(999),
                    owner: RecordOwner::Original(address(RecordStream::Data, 2)),
                },
                original(RecordStream::Additional, 0),
                original(RecordStream::Additional, 1),
                RecordItem::New {
                    id: 2,
                    record: line(-1),
                    owner: RecordOwner::New {
                        stream: RecordStream::Additional,
                        id: 1,
                    },
                },
            ],
        )
        .unwrap();
        assert_eq!(owners(&output.data), [0, 0]);
        assert_eq!(owners(&output.additional), [0, 1, 1, 2, 1]);
        assert!(
            output.additional[3]
                .base()
                .unwrap()
                .owner_index_additional_list
        );
        assert!(
            output.additional[4]
                .base()
                .unwrap()
                .owner_index_additional_list
        );
        assert!(
            !output.additional[2]
                .base()
                .unwrap()
                .owner_index_additional_list
        );
        let wrapped = assemble_data_records(
            &input,
            &[DataRecordItem::Original(0), DataRecordItem::Original(2)],
        )
        .unwrap();
        assert_eq!(owners(&wrapped.additional), [2, 0, 0]);
        assert_eq!(wrapped.additional[1], input.additional_records[1]);
        assert_eq!(input.additional_records[0].base().unwrap().owner_index, 3);
    }

    #[test]
    fn split_graph_rejects_orphans_cycles_order_migration_and_nested_components() {
        let mut input = component(vec![line(0), line(0)]);
        input.additional_records = vec![line(2), additional_line(0)];
        for (data, additional, expected) in [
            (
                vec![original(RecordStream::Data, 0)],
                vec![original(RecordStream::Additional, 0)],
                "orphan",
            ),
            (
                vec![original(RecordStream::Data, 1)],
                vec![original(RecordStream::Additional, 1)],
                "orphan",
            ),
            (
                vec![original(RecordStream::Data, 1)],
                vec![
                    original(RecordStream::Additional, 1),
                    original(RecordStream::Additional, 0),
                ],
                "forward",
            ),
            (
                vec![original(RecordStream::Additional, 0)],
                vec![],
                "between Data and Additional",
            ),
        ] {
            let error = assemble_records(&input, &data, &additional)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
        let mut invalid = component(vec![line(0)]);
        invalid.additional_records = vec![additional_line(1), additional_line(0)];
        assert!(
            assemble_records(&invalid, &[], &[])
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
        invalid.additional_records = vec![additional_line(1), line(0)];
        assert!(
            assemble_records(&invalid, &[], &[])
                .unwrap_err()
                .to_string()
                .contains("forward")
        );
        invalid.additional_records = vec![line(0)];
        invalid.records[0] = additional_line(0);
        assert!(
            assemble_records(&invalid, &[], &[])
                .unwrap_err()
                .to_string()
                .contains("Data records selecting Additional")
        );
        let child = SchRecord::Component(input.component.clone());
        let error = assemble_records(
            &input,
            &[RecordItem::New {
                id: 0,
                record: child,
                owner: RecordOwner::Root,
            }],
            &[],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("nested Component"), "{error}");
    }

    #[test]
    fn additional_implementation_parent_kinds_and_zero_address_are_distinct_from_root() {
        let mut records = tree();
        for record in &mut records[1..] {
            let base = record.base_mut().unwrap();
            base.owner_index -= 1;
            base.owner_index_additional_list = true;
        }
        let mut input = component(vec![]);
        input.additional_records = records;
        let request: Vec<_> = (0..5)
            .map(|i| original(RecordStream::Additional, i))
            .collect();
        let output = assemble_records(&input, &[], &request).unwrap();
        assert_eq!(output.additional, input.additional_records);
        assert_eq!(owners(&output.additional), [0, 0, 1, 2, 1]);
        input.additional_records[1]
            .base_mut()
            .unwrap()
            .owner_index_additional_list = false;
        let error = assemble_records(&input, &[], &request)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("required parent kind ImplementationList"),
            "{error}"
        );
    }

    #[test]
    fn native_split_stream_save_reopen_preserves_remapped_owners_and_index_in_sheet() {
        let mut library = SchLib::new_blank_ad26().unwrap();
        let component = &mut library.components[0];
        component.records.push(line(0));
        component.records.push(line(0));
        let last = component.records.len() - 1;
        component.additional_records =
            vec![line(i32::try_from(last + 1).unwrap()), additional_line(0)];
        let mut data: Vec<_> = (0..component.records.len())
            .map(|i| original(RecordStream::Data, i))
            .collect();
        data.remove(last - 1);
        let additional = [
            RecordItem::New {
                id: 7,
                record: line(0),
                owner: RecordOwner::Root,
            },
            original(RecordStream::Additional, 0),
            original(RecordStream::Additional, 1),
        ];
        let output = assemble_records(component, &data, &additional).unwrap();
        component.records = output.data;
        component.additional_records = output.additional;
        let expected = component.clone();
        library.header.weight = super::super::compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        reopened.validate_invariants().unwrap();
        assert_eq!(
            reopened.components[0].additional_records,
            expected.additional_records
        );
        assert_eq!(reopened.components[0].records, expected.records);
        let second = tempfile::NamedTempFile::new().unwrap();
        reopened.save(second.path()).unwrap();
        crate::test_utils::assert_cfb_files_semantic_eq(file.path(), second.path());
    }
}
