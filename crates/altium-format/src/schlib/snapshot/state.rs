//! Versioned serialization contract for complete SchLib component state
//! (step-3 increment A; design authority `/tmp/step3/design.md` §2, ratified
//! Q1: the versioned payload is a deliberate public API by design).
//!
//! [`ComponentStatePayload`] is a format-owned, fully typed JSON document
//! carrying EVERY field of the typed component model: the root component
//! record, both Data/Additional record sequences with their native owner
//! addresses, all sidecar-backed typed pin fields, and the library index
//! entry including aliases. No raw byte blobs, no opaque escape hatches —
//! embedded image BODIES live in the library store, never in component
//! state, so this payload contains none.
//!
//! Downstream crates hold the payload as data (serde round-trip only); its
//! fields stay private, so only `altium-format` can materialize or validate
//! it. Installation goes through the decoded-state edit forms
//! [`super::SchLibComponentEdit::ReplaceWithState`] and
//! [`super::SchNewComponent::FromState`], which flow through
//! `SchLibSnapshot::with_library_edit` WITHOUT fresh minting, through the
//! shared final-namespace/ownership validation and `ReferenceContext`
//! recompute, and through a state-install validation pass
//! (`validate_state_install`) re-establishing the reference and
//! consistency checks other forms get at authoring time. Decoded payloads
//! cannot carry the process-local `ReferenceContext`, so `ReplaceWith`'s
//! captured-context drift rejection has no state-form equivalent; in the
//! saved-plan pipeline the manifest whole-file hash plus before/after
//! typed equality at apply (step-3 increment B) subsume it.
//!
//! Versioning: any change to projection order, `assemble_records` policy,
//! owner-group mapping, count-floor normalization, or any serialized field
//! bumps [`SCHLIB_STATE_FORMAT_VERSION`]; decode rejects every other version
//! with an explicit regenerate message. Unknown fields are version skew and
//! are rejected (`deny_unknown_fields` on the payload and every nested
//! record type).

use serde::{Deserialize, Serialize};

use super::SchLibComponentSnapshot;
use crate::sch_records::SchLibComponent;
use crate::schlib::SchLibComponentIndex;

/// Exact supported version of the component-state payload. Bumped whenever
/// payload structure, projection order, or load-normalization semantics
/// change (design memo §S2); older plans then fail decode with a regenerate
/// message.
pub const SCHLIB_STATE_FORMAT_VERSION: u32 = 1;

/// Complete typed state of one SchLib component, in the versioned public
/// serialization form (see the module documentation). Fields are private:
/// downstream crates round-trip the payload but cannot construct or pick it
/// apart; equality is complete typed equality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentStatePayload {
    #[serde(deserialize_with = "deserialize_state_format_version")]
    state_format_version: u32,
    component: SchLibComponent,
    index: SchLibComponentIndex,
}

fn deserialize_state_format_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let found = u32::deserialize(deserializer)?;
    if found != SCHLIB_STATE_FORMAT_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported SchLib component state format version {found} \
             (supported: {SCHLIB_STATE_FORMAT_VERSION}); regenerate the plan \
             with the current tool"
        )));
    }
    Ok(found)
}

impl ComponentStatePayload {
    /// The payload's format version (always [`SCHLIB_STATE_FORMAT_VERSION`]
    /// for a payload this build encoded or decoded).
    pub fn state_format_version(&self) -> u32 {
        self.state_format_version
    }

    /// The component's library reference recorded in the index entry.
    pub fn lib_reference(&self) -> &str {
        &self.index.lib_ref
    }

    /// The alias names recorded in the index entry.
    pub fn aliases(&self) -> impl ExactSizeIterator<Item = &str> {
        self.index.aliases.iter().map(String::as_str)
    }

    /// Report/inspection projection of the payload: a read-only component
    /// view exposing the SAME typed projections the planner used
    /// (`parameters()`, `pins()`, `graphics()`, `parts()`,
    /// `footprint_maps()`, `aliases()`, and the scalar field projections),
    /// bound to an EMPTY captured reference namespace. Part of the ratified
    /// versioned payload contract (step-3 increment B): saved-plan reports
    /// derive exact before/after detail from decoded payloads through these
    /// views instead of re-opening any document.
    ///
    /// The captured-namespace projections (`captured_fonts`,
    /// `captured_image_asset_names`, `captured_display_styles_present`) are
    /// empty on this view — they describe a library, and this view has none.
    /// The view can never install: every install path compares captured
    /// reference contexts (or, for decoded payloads, goes through the
    /// dedicated `ReplaceWithState`/`FromState` edit forms), so passing this
    /// view to `replace_component` is rejected as context drift.
    pub fn component_view(&self) -> SchLibComponentSnapshot {
        SchLibComponentSnapshot {
            component: self.component.clone(),
            index: self.index.clone(),
            reference_context: std::sync::Arc::new(super::ReferenceContext {
                library_id: String::new(),
                minor_version: 0,
                fonts: Vec::new(),
                display_settings: Default::default(),
                aliases: Vec::new(),
                image_assets: Vec::new(),
            }),
        }
    }

    pub(in crate::schlib) fn into_parts(self) -> (SchLibComponent, SchLibComponentIndex) {
        (self.component, self.index)
    }

    pub(in crate::schlib) fn index(&self) -> &SchLibComponentIndex {
        &self.index
    }

    pub(in crate::schlib) fn component(&self) -> &SchLibComponent {
        &self.component
    }
}

impl SchLibComponentSnapshot {
    /// Encodes the complete captured typed state — root record, both record
    /// sequences with owner addresses, all sidecar-backed pin fields, and the
    /// index entry including aliases — into the versioned payload form.
    /// Encoding never loses fields: `decode(encode(c))` installed through
    /// [`super::SchLibComponentEdit::ReplaceWithState`] is a typed no-op.
    pub fn encode_state(&self) -> ComponentStatePayload {
        ComponentStatePayload {
            state_format_version: SCHLIB_STATE_FORMAT_VERSION,
            component: self.component.clone(),
            index: self.index.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        SchAliasOp, SchChildBatch, SchFootprintEdit, SchFootprintInit, SchGraphicEdit,
        SchGraphicInit, SchGraphicShape, SchLibComponentEdit, SchLibEdit, SchLibSnapshot,
        SchNewComponent, SchParameterEdit, SchParameterInit, SchPinEdit, SchPinInit,
        SchPinPadMapInit,
    };
    use super::*;
    use crate::param_collection::ParameterCollection;
    use crate::sch_records::{
        SchImplementation, SchImplementationList, SchLabel, SchLine, SchRecord,
    };
    use crate::schlib::{SchLib, SchLibAlias, compute_weight};
    use altium_format_types::{
        Color, Coord, CoordPoint, LineStyle, PenWidth, PinElectricalType, RotationBy90,
    };

    /// A programmatic "collection zoo" snapshot: every collection class the
    /// step-2 semantics ship — multi-part slots, a shared (part-0) pin and a
    /// numbered-part pin, a root-owned hidden parameter, Additional-stream
    /// records with owner addresses, a PCBLIB footprint chain with a
    /// pin-pad map next to a non-PCBLIB sibling implementation, an embedded
    /// image graphic paired to a store asset, aliases, and wild preserved
    /// content (an out-of-range owner_part_id and a stale AllPinCount
    /// cache).
    fn zoo_snapshot() -> SchLibSnapshot {
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            aggregate.component.vault_guid = "preserved-vault".to_owned();
            aggregate.component.all_pin_count = 17; // stale cache, preserved verbatim
            // Non-PCBLIB implementation sibling under an ImplementationList.
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
            // Shared pin (owner_part_id 0) with sidecar-backed typed fields.
            let mut shared_pin =
                crate::sch_records::parse_text_pin(&mut ParameterCollection::new()).unwrap();
            // Binary pin bodies always carry the four symbol fields; the
            // canonical loaded form has the presence flags set.
            shared_pin.symbol_inner_edge_present = true;
            shared_pin.symbol_outer_edge_present = true;
            shared_pin.symbol_inside_present = true;
            shared_pin.symbol_outside_present = true;
            shared_pin.owner_part_id = 0;
            shared_pin.designator = "SH1".to_owned();
            shared_pin.defined_functions = vec!["CLOCK".to_owned(), "GPIO".to_owned()];
            shared_pin.selected_functions = vec!["GPIO".to_owned()];
            shared_pin.pin_package_length = "12mil".to_owned();
            aggregate.records.push(SchRecord::Pin(shared_pin));
            // Wild out-of-range owner_part_id, preserved verbatim.
            let mut wild_line = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            wild_line.base.owner_part_id = 99;
            wild_line.base.index_in_sheet = 7;
            aggregate.records.push(SchRecord::Line(wild_line));
            // Alias on the component (all three native representations).
            let name = aggregate.component.lib_reference.clone();
            aggregate.component.alias_list = "ZOO_ALIAS".to_owned();
            library.header.components[0]
                .aliases
                .push("ZOO_ALIAS".to_owned());
            library.aliases.push(SchLibAlias {
                alias_name: "ZOO_ALIAS".to_owned(),
                canonical_name: name,
            });
        }
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        let input = library.snapshot().unwrap();

        // Image asset + a second component through declared transitions.
        let with_asset = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::New(SchNewComponent::CloneOf {
                        source: 0,
                        lib_reference: "ZOO_SECOND".to_owned(),
                    }),
                ]),
                image_assets: vec![super::super::SchImageAssetEdit::Insert {
                    index: 0,
                    name: "logo.png".to_owned(),
                    payload: vec![1, 2, 3, 4],
                }],
                ..Default::default()
            })
            .unwrap();

        // Component 0: grow to two parts, then add a numbered-part pin, an
        // embedded image graphic, and a PCBLIB footprint with a pin-pad map.
        let component = with_asset.component(0).unwrap();
        let multi_part = component.with_parts_appended(1).unwrap();
        let pin_count = multi_part.pins().len();
        let graphic_count = multi_part.graphics().len();
        let footprint_count = multi_part.footprint_maps().unwrap().footprints.len();
        let parameter_count = multi_part.parameters().unwrap().len();
        let edited = multi_part
            .with_children(SchChildBatch {
                parameters: Some(
                    (0..parameter_count)
                        .map(SchParameterEdit::Keep)
                        .chain([SchParameterEdit::New(SchParameterInit {
                            name: "ZOO_PARAM".to_owned(),
                            text: "zoo parameter".to_owned(),
                            is_hidden: true,
                        })])
                        .collect(),
                ),
                graphics: Some(
                    // The zoo's Additional-stream lines project as graphics;
                    // a created record must land in the Data stream, so the
                    // New entry precedes every kept member.
                    std::iter::once(SchGraphicEdit::New(SchGraphicInit {
                        owner_part_id: 1,
                        owner_part_display_mode: None,
                        shape: SchGraphicShape::Image {
                            from: CoordPoint::zero(),
                            to: CoordPoint {
                                x: Coord::from_mils(100).unwrap(),
                                y: Coord::from_mils(50).unwrap(),
                            },
                            orientation: RotationBy90::Rotate0,
                            line_width: PenWidth::Small,
                            color: Color::BLACK,
                            is_solid: false,
                            keep_aspect: true,
                            embed_image: true,
                            file_name: "logo.png".to_owned(),
                        },
                    }))
                    .chain((0..graphic_count).map(SchGraphicEdit::Keep))
                    .collect(),
                ),
                pins: Some(
                    (0..pin_count)
                        .map(SchPinEdit::Keep)
                        .chain([SchPinEdit::New(SchPinInit {
                            owner_part_id: 2,
                            owner_part_display_mode: Some(0),
                            designator: "P2".to_owned(),
                            name: "PART2".to_owned(),
                            electrical: PinElectricalType::Passive,
                            length: Coord::from_mils(300).unwrap(),
                            location: CoordPoint::zero(),
                            orientation: RotationBy90::Rotate0,
                            is_hidden: false,
                            hidden_net_name: String::new(),
                            swap_group: String::new(),
                            part_swap_group: Some(String::new()),
                            part_pin_swap_suffix: Some(String::new()),
                            pair_swap_group: String::new(),
                        })])
                        .collect(),
                ),
                footprints: Some(
                    (0..footprint_count)
                        .map(SchFootprintEdit::Keep)
                        .chain([SchFootprintEdit::New(SchFootprintInit {
                            model_name: "ZOO_FP".to_owned(),
                            description: "zoo footprint".to_owned(),
                            is_current: true,
                            pin_pad_maps: Some(vec![SchPinPadMapInit {
                                pin: "SH1".to_owned(),
                                pads: vec!["1".to_owned(), "2".to_owned()],
                            }]),
                        })])
                        .collect(),
                ),
                allow_deletion: false,
            })
            .unwrap();
        let result = with_asset.replace_component(0, edited).unwrap();
        // Additional-stream records (with an owner address into that stream)
        // land on the clone via direct construction: the structural batch API
        // deliberately refuses to create records in the Additional stream.
        let mut library = result.materialize().unwrap();
        {
            let aggregate = &mut library.components[1];
            let mut root_line = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            root_line.base.index_in_sheet = 17;
            aggregate
                .additional_records
                .push(SchRecord::Line(root_line));
            let mut owned_line = SchLine::from_params(&mut ParameterCollection::new()).unwrap();
            owned_line.base.owner_index_additional_list = true;
            owned_line.base.owner_index = 0;
            aggregate
                .additional_records
                .push(SchRecord::Line(owned_line));
        }
        library.header.weight = compute_weight(&library.header, &library.components);
        let result = library.snapshot().unwrap();
        // The zoo must survive native save/reopen so its wild classes are
        // real captured state, not construction artifacts.
        let file = tempfile::NamedTempFile::new().unwrap();
        result.materialize().unwrap().save(file.path()).unwrap();
        let reopened = SchLib::open(file.path()).unwrap().snapshot().unwrap();
        assert_eq!(reopened, result);
        reopened
    }

    fn json_of(payload: &ComponentStatePayload) -> serde_json::Value {
        serde_json::to_value(payload).unwrap()
    }

    #[test]
    fn payload_roundtrip_is_typed_noop_across_collection_zoo() {
        let input = zoo_snapshot();
        for index in 0..input.component_count() {
            let payload = input.component(index).unwrap().encode_state();
            assert_eq!(payload.state_format_version(), SCHLIB_STATE_FORMAT_VERSION);
            let json = serde_json::to_string(&payload).unwrap();
            let decoded: ComponentStatePayload = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, payload, "component[{index}] payload equality");
            let slots = (0..input.component_count())
                .map(|slot| {
                    if slot == index {
                        SchLibComponentEdit::ReplaceWithState(slot, decoded.clone())
                    } else {
                        SchLibComponentEdit::Keep(slot)
                    }
                })
                .collect();
            let result = input
                .with_library_edit(SchLibEdit {
                    components: Some(slots),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                result, input,
                "component[{index}] decode(encode) install must be a typed no-op"
            );
        }
    }

    #[test]
    fn from_state_reinstalls_removed_component_exactly() {
        let input = zoo_snapshot();
        let last = input.component_count() - 1;
        let payload = input.component(last).unwrap().encode_state();
        let json = serde_json::to_string(&payload).unwrap();
        let decoded: ComponentStatePayload = serde_json::from_str(&json).unwrap();
        let aliases: Vec<String> = payload.aliases().map(str::to_owned).collect();
        let removed = input
            .with_library_edit(SchLibEdit {
                components: Some((0..last).map(SchLibComponentEdit::Keep).collect()),
                alias_ops: aliases
                    .iter()
                    .map(|alias| SchAliasOp::Remove {
                        component: payload.lib_reference().to_owned(),
                        alias: alias.clone(),
                    })
                    .collect(),
                allow_component_removal: true,
                ..Default::default()
            })
            .unwrap();
        let restored = removed
            .with_library_edit(SchLibEdit {
                components: Some(
                    (0..last)
                        .map(SchLibComponentEdit::Keep)
                        .chain([SchLibComponentEdit::New(SchNewComponent::FromState(
                            decoded,
                        ))])
                        .collect(),
                ),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            restored, input,
            "FromState must reinstall the complete captured state without minting"
        );
    }

    #[test]
    fn component_view_projects_like_a_live_snapshot_but_cannot_install() {
        let input = zoo_snapshot();
        for index in 0..input.component_count() {
            let live = input.component(index).unwrap();
            let payload = live.encode_state();
            let json = serde_json::to_string(&payload).unwrap();
            let decoded: ComponentStatePayload = serde_json::from_str(&json).unwrap();
            let view = decoded.component_view();
            // The same typed projections the planner uses agree with the
            // live snapshot's, field for field.
            assert_eq!(view.lib_reference(), live.lib_reference());
            assert_eq!(view.description(), live.description());
            assert_eq!(view.designator().unwrap(), live.designator().unwrap());
            assert_eq!(view.component_kind(), live.component_kind());
            assert_eq!(view.show_hidden_pins(), live.show_hidden_pins());
            assert_eq!(
                view.effective_part_count().unwrap(),
                live.effective_part_count().unwrap()
            );
            assert_eq!(view.parts().unwrap(), live.parts().unwrap());
            assert_eq!(view.pins(), live.pins());
            assert_eq!(view.parameters().unwrap(), live.parameters().unwrap());
            assert_eq!(view.graphics(), live.graphics());
            assert_eq!(
                view.footprint_maps().unwrap(),
                live.footprint_maps().unwrap()
            );
            assert_eq!(
                view.aliases().collect::<Vec<_>>(),
                live.aliases().collect::<Vec<_>>()
            );
            // The captured-namespace projections are empty on this view.
            assert_eq!(view.captured_fonts().len(), 0);
            assert_eq!(view.captured_image_asset_names().len(), 0);
            assert!(!view.captured_display_styles_present());
        }
        // The view can never install: context drift is rejected.
        let view = input.component(0).unwrap().encode_state().component_view();
        let error = input.replace_component(0, view).unwrap_err().to_string();
        assert!(error.contains("namespace differs"), "{error}");
    }

    #[test]
    fn decode_rejects_foreign_versions_with_regenerate_message() {
        let input = zoo_snapshot();
        let payload = input.component(0).unwrap().encode_state();
        let mut json = json_of(&payload);
        json["state_format_version"] = serde_json::json!(SCHLIB_STATE_FORMAT_VERSION + 1);
        let error = serde_json::from_value::<ComponentStatePayload>(json)
            .unwrap_err()
            .to_string();
        assert!(error.contains("regenerate"), "{error}");
        assert!(
            error.contains(&format!("version {}", SCHLIB_STATE_FORMAT_VERSION + 1)),
            "{error}"
        );
    }

    #[test]
    fn decode_rejects_unknown_fields_at_every_level() {
        let input = zoo_snapshot();
        let payload = input.component(0).unwrap().encode_state();
        let mut top = json_of(&payload);
        top["surprise"] = serde_json::json!(1);
        let error = serde_json::from_value::<ComponentStatePayload>(top)
            .unwrap_err()
            .to_string();
        assert!(error.contains("surprise"), "{error}");
        let mut nested = json_of(&payload);
        nested["component"]["component"]["surprise"] = serde_json::json!(true);
        let error = serde_json::from_value::<ComponentStatePayload>(nested)
            .unwrap_err()
            .to_string();
        assert!(error.contains("surprise"), "{error}");
        let mut index = json_of(&payload);
        index["index"]["surprise"] = serde_json::json!("x");
        assert!(serde_json::from_value::<ComponentStatePayload>(index).is_err());
    }

    #[test]
    fn state_installs_reject_renames_alias_changes_and_broken_ownership() {
        let input = zoo_snapshot();
        let payload = input.component(0).unwrap().encode_state();

        // Installing component 0's state into slot 1 is a rename.
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::ReplaceWithState(1, payload.clone()),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("renaming is unsupported"), "{error}");

        // A decoded state smuggling an alias change is rejected.
        let mut smuggled = json_of(&payload);
        smuggled["index"]["aliases"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("SMUGGLED"));
        let smuggled: ComponentStatePayload = serde_json::from_value(smuggled).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWithState(0, smuggled),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("SchAliasOp"), "{error}");

        // A tampered payload with a broken ownership graph fails the shared
        // final validation, exactly like every other edit form.
        let mut broken = json_of(&input.component(1).unwrap().encode_state());
        broken["component"]["additional_records"][1]["Line"]["base"]["owner_index"] =
            serde_json::json!(99);
        let broken: ComponentStatePayload = serde_json::from_value(broken).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::ReplaceWithState(1, broken),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds"), "{error}");

        // A decoded state smuggling an index/root description desync (a
        // pair no snapshot-editing API can author) is rejected.
        let mut desync = json_of(&payload);
        desync["index"]["description"] = serde_json::json!("SMUGGLED-INDEX-DESCRIPTION");
        let desync: ComponentStatePayload = serde_json::from_value(desync).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWithState(0, desync),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("index description"), "{error}");

        // An empty-name FromState install is rejected before namespace checks.
        let mut unnamed = json_of(&payload);
        unnamed["index"]["lib_ref"] = serde_json::json!("");
        unnamed["component"]["component"]["lib_reference"] = serde_json::json!("");
        let unnamed: ComponentStatePayload = serde_json::from_value(unnamed).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                    SchLibComponentEdit::New(SchNewComponent::FromState(unnamed)),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not be empty"), "{error}");
    }

    #[test]
    fn state_installs_reject_fresh_dangling_references() {
        let input = zoo_snapshot();
        let payload = input.component(0).unwrap().encode_state();
        let replace_first = |payload: ComponentStatePayload| {
            input.with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::ReplaceWithState(0, payload),
                    SchLibComponentEdit::Keep(1),
                ]),
                ..Default::default()
            })
        };

        // A fresh dangling font reference (the input component never carried
        // it, and the table has no such id) is rejected.
        let mut font_tamper = json_of(&payload);
        let mut patched = false;
        for record in font_tamper["component"]["records"].as_array_mut().unwrap() {
            let inner = record.as_object_mut().unwrap().values_mut().next().unwrap();
            if inner.get("font_id").is_some() {
                inner["font_id"] = serde_json::json!(99);
                patched = true;
                break;
            }
        }
        assert!(patched, "zoo component 0 must carry a font-bearing record");
        let font_tamper: ComponentStatePayload = serde_json::from_value(font_tamper).unwrap();
        let error = replace_first(font_tamper).unwrap_err().to_string();
        assert!(error.contains("font id 99"), "{error}");

        // A fresh embedded-image reference resolving against neither asset
        // store is rejected.
        let mut image_tamper = json_of(&payload);
        let mut patched = false;
        for record in image_tamper["component"]["records"].as_array_mut().unwrap() {
            if let Some(image) = record.get_mut("Image")
                && image["embed_image"] == serde_json::json!(true)
            {
                image["file_name"] = serde_json::json!("missing.png");
                patched = true;
                break;
            }
        }
        assert!(
            patched,
            "zoo component 0 must carry an embedded image record"
        );
        let image_tamper: ComponentStatePayload = serde_json::from_value(image_tamper).unwrap();
        let error = replace_first(image_tamper).unwrap_err().to_string();
        assert!(error.contains("missing.png"), "{error}");

        // A FromState alias containing a comma (unrepresentable in the
        // native AliasList join, rejected by SchAliasOp for every other
        // alias source) is rejected.
        let mut comma = json_of(&payload);
        comma["index"]["lib_ref"] = serde_json::json!("ZOO_THIRD");
        comma["component"]["component"]["lib_reference"] = serde_json::json!("ZOO_THIRD");
        comma["index"]["aliases"] = serde_json::json!(["BAD,NAME"]);
        comma["component"]["component"]["alias_list"] = serde_json::json!("BAD,NAME");
        let comma: ComponentStatePayload = serde_json::from_value(comma).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::Keep(1),
                    SchLibComponentEdit::New(SchNewComponent::FromState(comma)),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("comma"), "{error}");
    }

    #[test]
    fn wild_dangling_references_round_trip_but_do_not_relocate() {
        // Wild classes loaded files can carry: a dangling font reference, a
        // dangling embedded-image reference, and an index/root description
        // desync. They must keep round-tripping through the state payload in
        // place (capture-forbidden boundary), while FromState — with no
        // input slot to inherit wild content from — stays strict.
        let mut library = SchLib::new_blank_ad26().unwrap();
        {
            let aggregate = &mut library.components[0];
            let SchRecord::Designator(designator) = &mut aggregate.records[0] else {
                panic!("blank component record 0 is the designator");
            };
            designator.font_id = 9; // dangling: the table only has id 1
            let mut wild_image =
                crate::sch_records::SchImage::from_params(&mut ParameterCollection::new()).unwrap();
            wild_image.embed_image = true;
            wild_image.file_name = "gone.bmp".to_owned();
            aggregate.records.push(SchRecord::Image(wild_image));
        }
        library.header.components[0].description = "wild index-only description".to_owned();
        library.header.weight = compute_weight(&library.header, &library.components);
        library.validate_invariants().unwrap();
        // The wild classes survive native save/reopen: they are loadable
        // state, not construction artifacts.
        let file = tempfile::NamedTempFile::new().unwrap();
        library.save(file.path()).unwrap();
        let input = SchLib::open(file.path()).unwrap().snapshot().unwrap();

        let payload = input.component(0).unwrap().encode_state();
        let json = serde_json::to_string(&payload).unwrap();
        let decoded: ComponentStatePayload = serde_json::from_str(&json).unwrap();
        let result = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![SchLibComponentEdit::ReplaceWithState(
                    0,
                    decoded.clone(),
                )]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            result, input,
            "wild dangling/desynced state must round-trip in place"
        );

        // The same wild content cannot relocate in as a NEW component.
        let mut twin = json_of(&decoded);
        twin["index"]["lib_ref"] = serde_json::json!("WILD_TWIN");
        twin["component"]["component"]["lib_reference"] = serde_json::json!("WILD_TWIN");
        twin["index"]["description"] = serde_json::json!(""); // sync the pair
        let twin: ComponentStatePayload = serde_json::from_value(twin).unwrap();
        let error = input
            .with_library_edit(SchLibEdit {
                components: Some(vec![
                    SchLibComponentEdit::Keep(0),
                    SchLibComponentEdit::New(SchNewComponent::FromState(twin)),
                ]),
                ..Default::default()
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("font id 9"), "{error}");
    }
}
