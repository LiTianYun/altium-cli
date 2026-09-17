//! Effective-part-slot grow/shrink on a component snapshot (step-2 increment
//! C; contract authority `docs/worklogs/next-child-contracts-2026-09-16.md`,
//! cited as "memo §N").
//!
//! # Model
//!
//! Effective parts are the numbered slots `1..=effective_part_count()`; the
//! native count is always effective + 1 (native slot 0 is the reserved/common
//! slot, memo §1) and is maintained on BOTH the component record and the
//! write-only FileHeader index entry. Part ops never touch the component's
//! `display_mode` / `display_mode_count` / custom display-mode names, and
//! growth never reclassifies existing children (memo §2: native
//! `AddPart`/`DeletePart` precedent).
//!
//! - [`SchLibComponentSnapshot::with_parts_appended`] appends empty trailing
//!   slots. No child is created, moved, or reclassified, so every A2 child
//!   projection of the result is index-identical to the input's.
//! - [`SchLibComponentSnapshot::with_parts_removed`] removes TRAILING slots
//!   only (the contiguous `1..n` prefix is always preserved and at least one
//!   effective part must remain). Children owned by the removed slots — plus
//!   their ownership descendants, which would otherwise become retained
//!   orphans — are deleted ONLY under the explicit
//!   `allow_descendant_deletion` authorization; without it the error lists
//!   every record the removal would delete. Deletion removes by raw
//!   `owner_part_id` match (the selection half of the native `DeletePart`
//!   walk; see the recorded-deviations section for the halves we do NOT
//!   replicate), so wild root text carrying a removed slot number is deleted
//!   (and listed) too, while owner-0 pins, −1 root text, and out-of-range
//!   owners are never touched. `current_part_id` follows memo §3 rule 4
//!   exactly (per removed slot, decrement by 1 only when > 1) and the
//!   adjustment is RETURNED as a typed reported change, never applied
//!   silently.
//!
//! Two `with_children` collection invariants are mirrored on this door so
//! they hold library-wide (they cannot be bypassed by routing a membership
//! change through a part removal; neither is curable by the deletion
//! authorization):
//!
//! - **Additional-pin freeze** (increment C, `children.rs`
//!   `resolve_pin_edits`): while ANY pin member lives in the Additional
//!   stream, a shrink whose deletion set contains a pin hard-errors —
//!   sidecars serialize from final Data pin encounter order only and
//!   Additional pin sidecars are unproven, so pin membership may not change
//!   through any door. Without Additional pin members, shrink pin deletion
//!   remains supported (and round-trip tested).
//! - **Single-current footprint invariant** (increment D2, `children.rs`
//!   "is_current"): a shrink may not break a coherent input's single-current
//!   PCBLIB state by deleting a (wild, part-owned) current Implementation
//!   while other members remain; pre-existing incoherent states pass through
//!   because the shrink writes no current value.
//!
//! A stored `current_part_id` outside `[1, native_count)` blocks BOTH ops
//! (memo §3 rule 3: wild loaded values are preserved verbatim and must be
//! explicitly resolved before any parts mutation — never silently clamped).
//! Growth is additionally bounded so the resulting native count fits the
//! signed-16 range: the binary pin body stores `owner_part_id` as i16, so a
//! part slot beyond `i16::MAX` could never carry a serializable pin owner
//! (`serialize_binary_pin` hard-errors on overflow instead of truncating).
//!
//! # Recorded deviations from native `AddPart`/`DeletePart`
//!
//! (`EngineObjects/SchComponent.cs:1575-1620`; each is our deliberate
//! policy, never attributed to AD26.)
//!
//! - Native `DeletePart` also DECREMENTS the `owner_part_id` of every
//!   retained first-level object whose owner exceeds the removed slot — the
//!   contiguous-renumbering half of the native walk. Under the trailing-only
//!   restriction the only retained records with a higher owner are
//!   out-of-range wilds, and those are preserved BYTE-VERBATIM per memo §1
//!   ("preserved verbatim through any patch") instead of being renumbered;
//!   the memo rule wins over the raw native walk (tested: an owner-4 record
//!   survives a shrink to native 3 unchanged, still out of range, where
//!   native would have decremented it to 3).
//! - Native `AddPart` ends with `SetCurrentPartID(GetPartCount() - 1)`,
//!   moving the current part to the newly added slot; `with_parts_appended`
//!   leaves `current_part_id` untouched (batch APIs have no interactive part
//!   cursor — asserted in tests).
//! - Whether native `DeletePart` reaches wild ROOT TEXT (Designator /
//!   Parameter records) depends on their membership in
//!   `SchDataConsts.SpatialObjectSet`, which the decompile hides in an
//!   opaque token blob — unverified. Their deletion here therefore cites the
//!   memo-§1 raw-`owner_part_id`-match rule, not a native precedent; it is
//!   flag-authorized and individually listed either way.
//!
//! # Composition with [`super::SchChildBatch`]
//!
//! Part ops and child edits compose instead of sharing one call:
//! `with_parts_appended` leaves every child untouched, so a following
//! `with_children` resolves against projections identical to the ones the
//! caller took from the ORIGINAL input snapshot (this is the "grow + create
//! children in the new part" flow). `with_parts_removed` deletes children, so
//! its outcome snapshot is the immutable input for any subsequent batch: the
//! report lists every deletion, and the caller re-projects before addressing
//! children again. A deletion-bearing part op is never silently fused with a
//! child batch resolved against pre-deletion indices.

use std::collections::HashSet;

use super::SchLibComponentSnapshot;
use super::children::{Identity, MODEL_TYPE_PCBLIB, identity_of, renumber_touched_owners};
use crate::sch_records::{SchRecord, record_type_for};
use crate::schlib::ownership::{
    RecordAddress, RecordItem, RecordStream, assemble_records, owner_of,
};
use crate::{AltiumFormatError, Result, ResultExt};
use altium_format_types::SchRecordType;

/// Reported `current_part_id` change produced by a trailing shrink (memo §3
/// rule 4: the native `DeletePart` decrement, surfaced — never silent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchCurrentPartIdAdjustment {
    pub before: i32,
    pub after: i32,
}

/// One record deleted by a trailing part removal, identified in the input
/// snapshot's child encounter order (Data stream records, then Additional
/// stream records; the component root is not a child and never appears).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchRemovedPartChild {
    /// Position in the input's child encounter order.
    pub child_index: usize,
    /// Typed kind of the deleted record.
    pub kind: SchRecordType,
    /// The record's raw stored `owner_part_id`. For ownership descendants
    /// this may differ from the removed slot that doomed their owner.
    pub owner_part_id: i32,
}

/// Outcome of [`SchLibComponentSnapshot::with_parts_removed`]: the edited
/// snapshot plus the typed report of everything the removal changed beyond
/// the count fields.
#[derive(Debug, Clone, PartialEq)]
pub struct SchPartsShrink {
    pub component: SchLibComponentSnapshot,
    /// The removed trailing slot numbers, ascending.
    pub removed_parts: Vec<i32>,
    /// Every deleted child, in input child encounter order.
    pub deleted_children: Vec<SchRemovedPartChild>,
    /// The memo §3 rule 4 `current_part_id` adjustment, when one occurred.
    pub current_part_id_adjustment: Option<SchCurrentPartIdAdjustment>,
}

impl SchLibComponentSnapshot {
    /// Appends `additional_parts` empty effective part slots. See the module
    /// documentation for the complete contract.
    pub fn with_parts_appended(&self, additional_parts: i32) -> Result<Self> {
        self.with_parts_appended_inner(additional_parts)
            .with_context(|| {
                format!(
                    "appending {additional_parts} part slot(s) to component '{}'",
                    self.lib_reference()
                )
            })
    }

    fn with_parts_appended_inner(&self, additional_parts: i32) -> Result<Self> {
        if additional_parts < 1 {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "PartCount".to_owned(),
                detail: format!("cannot append {additional_parts} part slots; at least 1 required"),
            });
        }
        // Guarantees native count ≥ 2 (legacy programmatic state hard-errors).
        self.effective_part_count()?;
        self.check_resolvable_current_part_id()?;
        let native = self.component.component.part_count;
        // Bounded by the signed-16 binary pin owner_part_id field: a slot
        // beyond i16::MAX could never carry a serializable pin owner
        // (serialize_binary_pin hard-errors instead of truncating).
        let new_native = native
            .checked_add(additional_parts)
            .filter(|count| *count <= i32::from(i16::MAX))
            .ok_or_else(|| AltiumFormatError::InvalidParamValue {
                key: "PartCount".to_owned(),
                detail: format!(
                    "native part count {native} + {additional_parts} exceeds the signed-16 \
                     range that bounds serializable binary pin owner_part_id values (max \
                     native count {})",
                    i16::MAX
                ),
            })?;
        let mut candidate = self.clone();
        candidate.component.component.part_count = new_native;
        candidate.index.part_count = new_native;
        Ok(candidate)
    }

    /// Removes the given TRAILING effective part slots, deleting their
    /// descendants only under `allow_descendant_deletion`. See the module
    /// documentation for the complete contract; the `current_part_id`
    /// adjustment and every deletion are returned in the typed report.
    pub fn with_parts_removed(
        &self,
        removed_parts: &[i32],
        allow_descendant_deletion: bool,
    ) -> Result<SchPartsShrink> {
        self.with_parts_removed_inner(removed_parts, allow_descendant_deletion)
            .with_context(|| {
                format!(
                    "removing part slots {removed_parts:?} from component '{}'",
                    self.lib_reference()
                )
            })
    }

    fn with_parts_removed_inner(
        &self,
        removed_parts: &[i32],
        allow_descendant_deletion: bool,
    ) -> Result<SchPartsShrink> {
        let effective = self.effective_part_count()?;
        self.check_resolvable_current_part_id()?;
        if removed_parts.is_empty() {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "PartCount".to_owned(),
                detail: "no part slots named for removal".to_owned(),
            });
        }
        let mut removed: Vec<i32> = removed_parts.to_vec();
        removed.sort_unstable();
        removed.dedup();
        let slots_removed = i32::try_from(removed.len()).ok().filter(|k| *k < effective);
        let Some(slots_removed) = slots_removed else {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "PartCount".to_owned(),
                detail: format!(
                    "removing {} distinct slot(s) from {effective} effective part(s) would \
                     leave no effective part; at least one must remain",
                    removed.len()
                ),
            });
        };
        let expected_trailing: Vec<i32> = (effective - slots_removed + 1..=effective).collect();
        if removed != expected_trailing {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "PartCount".to_owned(),
                detail: format!(
                    "only trailing part slots can be removed (the contiguous 1..n prefix is \
                     preserved): removing {} slot(s) from {effective} effective part(s) must \
                     name exactly {expected_trailing:?}, got {removed:?}",
                    removed.len()
                ),
            });
        }
        let removed_set: HashSet<i32> = removed.iter().copied().collect();

        // Deletion set: raw owner_part_id ∈ removed slots (native DeletePart
        // precedent), transitively closed over record ownership so no retained
        // orphan can survive. Encounter order (Data then Additional) makes a
        // single in-order pass per stream complete: validated graphs guarantee
        // same-stream owners precede their children and Additional-to-Data
        // owners were fully decided in the Data pass.
        let component = &self.component;
        let mut marked: HashSet<RecordAddress> = HashSet::new();
        let mut deleted_children = Vec::new();
        let data_len = component.records.len();
        for (address, record) in all_child_records(component) {
            let owner_part_id =
                raw_owner_part_id(record).context("classifying children for part removal")?;
            let part_owned = removed_set.contains(&owner_part_id);
            let owner_deleted = owner_of(component, address, record)
                .context("resolving ownership for part removal")?
                .is_some_and(|owner| marked.contains(&owner));
            if part_owned || owner_deleted {
                marked.insert(address);
                deleted_children.push(SchRemovedPartChild {
                    child_index: match address.stream {
                        RecordStream::Data => address.index,
                        RecordStream::Additional => data_len + address.index,
                    },
                    kind: record_type_for(record),
                    owner_part_id,
                });
            }
        }
        // Increment-C Additional-pin freeze, mirrored from `with_children`
        // (`children.rs`, "Additional-stream pins freeze the collection"):
        // the nine pin sidecar streams are serialized from final DATA pin
        // encounter order only and Additional pin sidecars are unproven, so
        // while ANY pin member lives in the Additional stream pin membership
        // may not change through ANY door. A shrink whose deletion set
        // contains a pin therefore hard-errors here regardless of
        // descendant-deletion authorization — the same membership change is
        // rejected by `resolve_pin_edits` on the batch door.
        let additional_pins_present = component
            .additional_records
            .iter()
            .any(|record| matches!(record, SchRecord::Pin(_)));
        if additional_pins_present {
            if let Some(doomed) = deleted_children
                .iter()
                .find(|child| child.kind == SchRecordType::Pin)
            {
                return Err(AltiumFormatError::InvalidParamValue {
                    key: "pins".to_owned(),
                    detail: format!(
                        "removing part slots {removed:?} would delete pin child[{}] \
                         (owner_part_id {}) while pin members live in the Additional stream; \
                         the pin collection is frozen to membership-preserving edits \
                         (Additional pin sidecars are unproven), so part removal deleting \
                         pins is unsupported here regardless of deletion authorization",
                        doomed.child_index, doomed.owner_part_id
                    ),
                });
            }
        }
        // D2 single-current invariant, mirrored from `with_children`
        // (`children.rs`, "is_current"): a shrink may not BREAK a coherent
        // input's single-current PCBLIB state — a wild part-owned current
        // Implementation would otherwise be deleted through this door without
        // the batch door's guard. Membership mirrors `footprint_maps()`:
        // PCBLIB implementations in encounter order. Pre-existing incoherent
        // (wild) states pass through: the shrink writes no current value.
        let mut input_members = 0usize;
        let mut input_currents = 0usize;
        let mut final_members = 0usize;
        let mut final_currents = 0usize;
        for (address, record) in all_child_records(component) {
            let SchRecord::Implementation(implementation) = record else {
                continue;
            };
            if !implementation
                .model_type
                .eq_ignore_ascii_case(MODEL_TYPE_PCBLIB)
            {
                continue;
            }
            input_members += 1;
            input_currents += usize::from(implementation.is_current);
            if !marked.contains(&address) {
                final_members += 1;
                final_currents += usize::from(implementation.is_current);
            }
        }
        let input_coherent = input_members == 0 || input_currents == 1;
        if input_coherent && final_members > 0 && final_currents != 1 {
            return Err(AltiumFormatError::InvalidParamValue {
                key: "footprints".to_owned(),
                detail: format!(
                    "removing part slots {removed:?} would leave {final_currents} current \
                     footprint(s) across {final_members} retained PCBLIB implementation(s); \
                     a coherent single-current input may never be broken (native model: \
                     exactly one current implementation per model type) — resolve the \
                     doomed current footprint through the footprints collection first"
                ),
            });
        }
        if !deleted_children.is_empty() && !allow_descendant_deletion {
            let listing: Vec<String> = deleted_children
                .iter()
                .map(|child| {
                    format!(
                        "child[{}] {} (owner_part_id {})",
                        child.child_index, child.kind, child.owner_part_id
                    )
                })
                .collect();
            return Err(AltiumFormatError::InvalidParamValue {
                key: "PartCount".to_owned(),
                detail: format!(
                    "removing part slots {removed:?} would delete {} descendant record(s): \
                     [{}]; explicit allow_descendant_deletion authorization required",
                    deleted_children.len(),
                    listing.join(", ")
                ),
            });
        }

        // Retained arrangement through the immutable-slot assembly (owners
        // remapped, retained orphans impossible by construction of the
        // closure), then the memo §6 touched-owner renumbering.
        let retained = |stream: RecordStream, count: usize| -> Vec<RecordItem> {
            (0..count)
                .map(|index| RecordAddress { stream, index })
                .filter(|address| !marked.contains(address))
                .map(RecordItem::Original)
                .collect()
        };
        let data_items = retained(RecordStream::Data, component.records.len());
        let additional_items =
            retained(RecordStream::Additional, component.additional_records.len());
        let data_ids: Vec<Identity> = data_items
            .iter()
            .map(|item| match item {
                RecordItem::Original(address) => identity_of(*address),
                RecordItem::New { .. } => unreachable!("part removal creates no records"),
            })
            .collect();
        let additional_ids: Vec<Identity> = additional_items
            .iter()
            .map(|item| match item {
                RecordItem::Original(address) => identity_of(*address),
                RecordItem::New { .. } => unreachable!("part removal creates no records"),
            })
            .collect();
        let assembled = assemble_records(component, &data_items, &additional_items)
            .context("assembling part removal")?;

        let mut candidate = self.clone();
        candidate.component.records = assembled.data;
        candidate.component.additional_records = assembled.additional;
        renumber_touched_owners(
            component,
            &mut candidate.component,
            &data_ids,
            &additional_ids,
        )
        .context("renumbering IndexInSheet after part removal")?;

        let new_native = self.component.component.part_count - slots_removed;
        candidate.component.component.part_count = new_native;
        candidate.index.part_count = new_native;

        // Memo §3 rule 4: per removed slot, decrement by 1 only when > 1 —
        // the native DeletePart precedent, reported, never a clamp.
        let before = self.component.component.current_part_id;
        let mut current = before;
        for _ in 0..slots_removed {
            if current > 1 {
                current -= 1;
            }
        }
        let current_part_id_adjustment = (current != before).then(|| {
            candidate.component.component.current_part_id = current;
            SchCurrentPartIdAdjustment {
                before,
                after: current,
            }
        });

        Ok(SchPartsShrink {
            component: candidate,
            removed_parts: removed,
            deleted_children,
            current_part_id_adjustment,
        })
    }

    /// Memo §3 rule 3: a stored `current_part_id` outside `[1, native_count)`
    /// is preserved verbatim at capture, and any parts mutation on such a
    /// component hard-errors until the caller resolves it explicitly — never
    /// a silent clamp.
    fn check_resolvable_current_part_id(&self) -> Result<()> {
        let native = self.component.component.part_count;
        let current = self.component.component.current_part_id;
        if current >= 1 && current < native {
            return Ok(());
        }
        Err(AltiumFormatError::InvalidParamValue {
            key: "CurrentPartId".to_owned(),
            detail: format!(
                "stored current_part_id {current} is outside [1, {native}); wild loaded \
                 values are preserved verbatim and must be explicitly resolved before any \
                 parts mutation (never silently clamped)"
            ),
        })
    }
}

/// Child records in encounter order (Data then Additional; the component root
/// is not a child).
fn all_child_records(
    component: &crate::sch_records::SchLibComponent,
) -> impl Iterator<Item = (RecordAddress, &SchRecord)> {
    let data = component.records.iter().enumerate().map(|(index, record)| {
        (
            RecordAddress {
                stream: RecordStream::Data,
                index,
            },
            record,
        )
    });
    let additional = component
        .additional_records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            (
                RecordAddress {
                    stream: RecordStream::Additional,
                    index,
                },
                record,
            )
        });
    data.chain(additional)
}

/// The record's raw stored `owner_part_id`. Pin and Component — the only
/// `SchRecord` variants without a `base()` — are matched explicitly, so the
/// error arm is unreachable today; it is threaded as a hard error (never a
/// fabricated default) in case a future baseless variant appears.
fn raw_owner_part_id(record: &SchRecord) -> Result<i32> {
    match record {
        SchRecord::Pin(pin) => Ok(pin.owner_part_id),
        SchRecord::Component(component) => Ok(component.owner_part_id),
        other => other.base().map(|base| base.owner_part_id).ok_or_else(|| {
            AltiumFormatError::InvalidParamValue {
                key: "OwnerPartId".to_owned(),
                detail: format!(
                    "{} record carries no base owner_part_id; the part-removal deletion \
                     scan cannot classify it",
                    record_type_for(other)
                ),
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::super::SchOwnerGroup;
    use super::*;
    use crate::param_collection::ParameterCollection;
    use crate::sch_records::{SchLine, SchParameter, SchRectangle, parse_text_pin};
    use crate::schlib::{SchLib, compute_weight};

    fn rect(owner_part: i32, unique_id: &str) -> SchRecord {
        let mut record = SchRectangle::from_params(&mut ParameterCollection::new()).unwrap();
        record.base.owner_part_id = owner_part;
        record.unique_id = unique_id.to_owned();
        SchRecord::Rectangle(record)
    }

    fn line(owner_index: i32, owner_part: i32) -> SchRecord {
        let mut record = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
        record.base.owner_index = owner_index;
        record.base.owner_part_id = owner_part;
        SchRecord::Line(record)
    }

    fn pin(owner_part: i32, designator: &str) -> SchRecord {
        let mut record = parse_text_pin(&mut ParameterCollection::new()).unwrap();
        record.owner_part_id = owner_part;
        record.designator = designator.to_owned();
        SchRecord::Pin(record)
    }

    /// Native PARTCOUNT 4 (effective 3), current part 3. Data children:
    /// part-1 rectangle+pin, part-2 line, part-3 rectangle owning a PART-1
    /// line (ownership-closure case), part-3 pin with a PinMiscData sidecar,
    /// shared pin (owner 0), out-of-range line (owner 4 == native count), and
    /// a wild root parameter carrying owner 3. One Additional line carries
    /// owner_part 3 (deleted through the Additional pass).
    fn parts_library() -> SchLib {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            aggregate.component.part_count = 4;
            aggregate.component.current_part_id = 3;
            let records = &mut aggregate.records;
            records.push(rect(1, "RECT0001")); // index 2
            records.push(pin(1, "1")); // index 3
            records.push(line(0, 2)); // index 4
            records.push(rect(3, "RECT0003")); // index 5, native address 6
            records.push(line(6, 1)); // index 6, owned by RECT0003, part 1
            let SchRecord::Pin(mut part3_pin) = pin(3, "3") else {
                unreachable!()
            };
            part3_pin.swap_id_pair = "PAIR".to_owned();
            records.push(SchRecord::Pin(part3_pin)); // index 7
            records.push(pin(0, "9")); // index 8, shared
            records.push(line(0, 4)); // index 9, out-of-range owner
            let mut wild = SchParameter::from_params(&mut ParameterCollection::new()).unwrap();
            wild.base.owner_part_id = 3;
            wild.name = "Wild".to_owned();
            records.push(SchRecord::Parameter(wild)); // index 10
            aggregate.additional_records.push(line(0, 3));
        }
        library.header.components[0].part_count = 4;
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        SchLib::open(file.path()).unwrap()
    }

    #[test]
    fn growth_appends_slots_without_reclassifying_children_or_touching_display_state() {
        let library = parts_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();
        assert_eq!(component.effective_part_count().unwrap(), 3);

        let grown = component.with_parts_appended(2).unwrap();
        assert_eq!(grown.effective_part_count().unwrap(), 5);
        assert_eq!(grown.component.component.part_count, 6);
        assert_eq!(grown.index.part_count, 6);
        // Children, display state, and current part are untouched.
        assert_eq!(grown.component.records, component.component.records);
        assert_eq!(
            grown.component.additional_records,
            component.component.additional_records
        );
        assert_eq!(grown.component.component.current_part_id, 3);
        assert_eq!(
            grown.component.component.display_mode_count,
            component.component.component.display_mode_count
        );
        assert_eq!(
            grown.component.component.custom_display_mode_names,
            component.component.component.custom_display_mode_names
        );
        // The previously out-of-range owner 4 is a numbered part under the
        // grown count — a raw-classification consequence, not a record edit.
        assert_eq!(grown.graphics()[4].owner_group, SchOwnerGroup::Part(4));

        // Native save/reopen through the snapshot path.
        let replaced = snapshot.replace_component(0, grown).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), replaced);
        assert_eq!(reopened.components[0].component.part_count, 6);
        assert_eq!(reopened.header.components[0].part_count, 6);

        for invalid in [0, -1] {
            let error = component
                .with_parts_appended(invalid)
                .unwrap_err()
                .to_string();
            assert!(error.contains("at least 1"), "{error}");
        }

        // Growth is bounded by the signed-16 binary pin owner_part_id field:
        // both the exact-overflow and the i32-overflow shapes hard-error.
        for excessive in [i32::from(i16::MAX), i32::MAX] {
            let error = component
                .with_parts_appended(excessive)
                .unwrap_err()
                .to_string();
            assert!(error.contains("signed-16"), "{error}");
        }
        component
            .with_parts_appended(i32::from(i16::MAX) - 4)
            .expect("native 4 + 32763 = 32767 is the exact bound");
    }

    #[test]
    fn trailing_shrink_deletes_descendants_with_authorization_and_reports_current_part_id() {
        let library = parts_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();

        let shrink = component.with_parts_removed(&[3], true).unwrap();
        assert_eq!(shrink.removed_parts, [3]);
        // Deleted: part-3 rectangle, its part-1 line descendant (ownership
        // closure), the part-3 pin, the wild root parameter carrying owner 3,
        // and the Additional line (child_index = data_len + 0 = 11).
        assert_eq!(
            shrink
                .deleted_children
                .iter()
                .map(|child| (child.child_index, child.kind, child.owner_part_id))
                .collect::<Vec<_>>(),
            [
                (5, SchRecordType::Rectangle, 3),
                (6, SchRecordType::Line, 1),
                (7, SchRecordType::Pin, 3),
                (10, SchRecordType::Parameter, 3),
                (11, SchRecordType::Line, 3),
            ]
        );
        assert_eq!(
            shrink.current_part_id_adjustment,
            Some(SchCurrentPartIdAdjustment {
                before: 3,
                after: 2
            })
        );
        let edited = &shrink.component;
        assert_eq!(edited.component.component.current_part_id, 2);
        assert_eq!(edited.component.component.part_count, 3);
        assert_eq!(edited.index.part_count, 3);
        assert!(edited.component.additional_records.is_empty());
        // Preserved children: shared pin, part-1/2 children, and the
        // out-of-range owner (4 ≥ new native 3 → still preserved-only).
        assert_eq!(
            edited
                .pins()
                .iter()
                .map(|view| (view.designator.to_owned(), view.owner_group))
                .collect::<Vec<_>>(),
            [
                ("1".to_owned(), SchOwnerGroup::Part(1)),
                ("9".to_owned(), SchOwnerGroup::SharedPins),
            ]
        );
        assert_eq!(
            edited.graphics()[2].owner_group,
            SchOwnerGroup::OutOfRange(4)
        );
        // Display state untouched.
        assert_eq!(
            edited.component.component.display_mode_count,
            component.component.component.display_mode_count
        );
        // Memo §6: the deletion touched the root's child sequence, so its
        // in-objectList children are renumbered to final sibling positions.
        // Final Data: Designator(-1), Comment(-1), rect1(0), pin(no field),
        // line2(2), shared pin(no field), out-of-range line(4).
        let index_in_sheet: Vec<Option<i32>> = edited
            .component
            .records
            .iter()
            .map(|record| record.base().map(|base| base.index_in_sheet))
            .collect();
        assert_eq!(
            index_in_sheet,
            [Some(-1), Some(-1), Some(0), None, Some(2), None, Some(4)]
        );

        // Native save/reopen (also proves the PinMiscData sidecar dropped its
        // deleted pin's entry consistently) and the weight convention.
        let replaced = snapshot.replace_component(0, shrink.component).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        replaced.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), replaced);
        assert_eq!(
            i64::from(reopened.header.weight),
            i64::from(library.header.weight) - 4,
            "four Data records deleted (the Additional record is outside the weight)"
        );

        // Removing two trailing slots decrements current part per slot.
        let shrink = component.with_parts_removed(&[2, 3], true).unwrap();
        assert_eq!(shrink.removed_parts, [2, 3]);
        assert_eq!(
            shrink.current_part_id_adjustment,
            Some(SchCurrentPartIdAdjustment {
                before: 3,
                after: 1
            })
        );
        assert_eq!(shrink.component.effective_part_count().unwrap(), 1);
        assert!(
            shrink
                .deleted_children
                .iter()
                .any(|child| child.kind == SchRecordType::Line && child.owner_part_id == 2)
        );

        // current_part_id 1 never decrements (native DeletePart precedent).
        let mut at_one = component.clone();
        at_one.component.component.current_part_id = 1;
        let shrink = at_one.with_parts_removed(&[3], true).unwrap();
        assert_eq!(shrink.current_part_id_adjustment, None);
        assert_eq!(shrink.component.component.component.current_part_id, 1);
    }

    #[test]
    fn shrink_rejections_cover_authorization_trailing_minimum_and_wild_current_part() {
        let library = parts_library();
        let snapshot = library.snapshot().unwrap();
        let component = snapshot.component(0).unwrap();

        // Unauthorized descendant deletion lists every doomed record.
        let error = component
            .with_parts_removed(&[3], false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("allow_descendant_deletion"), "{error}");
        assert!(
            error.contains("child[5] Rectangle (owner_part_id 3)"),
            "{error}"
        );
        assert!(error.contains("child[6] Line (owner_part_id 1)"), "{error}");
        assert!(error.contains("child[7] Pin (owner_part_id 3)"), "{error}");
        assert!(
            error.contains("child[10] Parameter (owner_part_id 3)"),
            "{error}"
        );
        assert!(
            error.contains("child[11] Line (owner_part_id 3)"),
            "{error}"
        );
        assert_eq!(snapshot.component(0).unwrap(), component);

        // Non-trailing removals are rejected; the prefix is inviolable.
        for non_trailing in [&[2][..], &[1, 3][..], &[4][..], &[1][..]] {
            let error = component
                .with_parts_removed(non_trailing, true)
                .unwrap_err()
                .to_string();
            assert!(error.contains("trailing"), "{non_trailing:?}: {error}");
        }

        // At least one effective part must remain; empty lists are refused.
        let error = component
            .with_parts_removed(&[1, 2, 3], true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("at least one"), "{error}");
        let error = component
            .with_parts_removed(&[], true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no part slots"), "{error}");

        // A wild stored current_part_id blocks both ops until resolved.
        let mut wild = component.clone();
        wild.component.component.current_part_id = 9;
        for error in [
            wild.with_parts_removed(&[3], true).unwrap_err().to_string(),
            wild.with_parts_appended(1).unwrap_err().to_string(),
        ] {
            assert!(error.contains("explicitly resolved"), "{error}");
            assert!(error.contains("current_part_id 9"), "{error}");
        }
    }

    /// The two `with_children` collection invariants mirrored on the shrink
    /// door (module docs): the increment-C Additional-pin freeze and the D2
    /// single-current footprint invariant hold even when the membership
    /// change arrives as a part removal with full deletion authorization.
    #[test]
    fn shrink_mirrors_additional_pin_freeze_and_current_footprint_coherence() {
        use super::super::{SchChildBatch, SchFootprintEdit, SchFootprintInit};

        // Native PARTCOUNT 3 (effective 2): a Data pin owned by doomed slot 2
        // plus a retained part-1 pin living in the Additional stream.
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            aggregate.component.part_count = 3;
            aggregate.records.push(pin(2, "2"));
            aggregate.additional_records.push(pin(1, "X"));
        }
        library.header.components[0].part_count = 3;
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let library = SchLib::open(file.path()).unwrap();
        let component = library.snapshot().unwrap().component(0).unwrap();

        // Deleting the Data pin while an Additional pin member exists is
        // frozen even under full authorization (the batch-door contract:
        // "such pins can only be kept untouched" / no membership change).
        let error = component
            .with_parts_removed(&[2], true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional stream"), "{error}");
        assert!(
            error.contains("regardless of deletion authorization"),
            "{error}"
        );

        // The same freeze covers a doomed ADDITIONAL pin (wild part-owned).
        let mut doomed_additional = component.clone();
        let SchRecord::Pin(record) = &mut doomed_additional.component.additional_records[0] else {
            unreachable!()
        };
        record.owner_part_id = 2;
        let error = doomed_additional
            .with_parts_removed(&[2], true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Additional stream"), "{error}");

        // Single-current invariant: two PCBLIB footprints (one current), then
        // the CURRENT implementation is made wild part-2-owned. Removing slot
        // 2 would delete it and leave one retained non-current member — the
        // coherent input may not be broken through the shrink door.
        let mut library = SchLib::new_blank_ad26().unwrap();
        library.components[0].component.part_count = 3;
        library.header.components[0].part_count = 3;
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let library = SchLib::open(file.path()).unwrap();
        let component = library
            .snapshot()
            .unwrap()
            .component(0)
            .unwrap()
            .with_children(SchChildBatch {
                parameters: None,
                graphics: None,
                pins: None,
                footprints: Some(vec![
                    SchFootprintEdit::New(SchFootprintInit {
                        model_name: "FP-A".to_owned(),
                        description: String::new(),
                        is_current: true,
                        pin_pad_maps: None,
                    }),
                    SchFootprintEdit::New(SchFootprintInit {
                        model_name: "FP-B".to_owned(),
                        description: String::new(),
                        is_current: false,
                        pin_pad_maps: None,
                    }),
                ]),
                allow_deletion: false,
            })
            .unwrap();
        let retarget_implementation = |snapshot: &SchLibComponentSnapshot, current: bool| {
            let mut wild = snapshot.clone();
            for record in &mut wild.component.records {
                if let SchRecord::Implementation(implementation) = record {
                    if implementation.is_current == current {
                        implementation.base.owner_part_id = 2;
                    }
                }
            }
            wild
        };
        let error = retarget_implementation(&component, true)
            .with_parts_removed(&[2], true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("0 current footprint(s)"), "{error}");
        assert!(error.contains("coherent single-current"), "{error}");

        // The inverse wild shape — a doomed NON-current implementation —
        // breaks no coherence and shrinks fine, reporting the deletion.
        let shrink = retarget_implementation(&component, false)
            .with_parts_removed(&[2], true)
            .unwrap();
        assert!(
            shrink.deleted_children.iter().any(
                |child| child.kind == SchRecordType::Implementation && child.owner_part_id == 2
            ),
            "{:?}",
            shrink.deleted_children
        );
        assert_eq!(shrink.component.effective_part_count().unwrap(), 1);
    }
}
