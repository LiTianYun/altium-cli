use super::*;
use crate::{
    diagnostic::{Span, Spanned},
    import::capture_imports,
    parser::parse_spec,
};
use altium_format_types::{Coord, PinElectricalType};

fn compile(source: &str) -> Result<SchLibIntent, crate::SpecError> {
    compile_schlib(&parse_spec(source).expect("valid syntax"))
}
fn pin_for(value: &str) -> PinIntent {
    compile(&format!("component R {{ pin 1 {{ {value} }} }}"))
        .unwrap()
        .components
        .items
        .remove(0)
        .pins
        .items
        .remove(0)
}
#[test]
fn preserves_all_five_operations_and_source_spans() {
    let absent = pin_for("");
    assert_eq!(absent.name, FieldIntent::Absent);
    assert_eq!(absent.location, FieldIntent::Absent);
    let set = pin_for("name: \"\"");
    assert!(
        matches!(set.name, FieldIntent::Set(Spanned { node, span }) if node.is_empty() && span.end > span.start)
    );
    assert!(matches!(
        pin_for("name: inherit").name,
        FieldIntent::Inherit(_)
    ));
    assert!(matches!(pin_for("name: reset").name, FieldIntent::Reset(_)));
    assert!(matches!(pin_for("name: null").name, FieldIntent::Clear(_)));
}
#[test]
fn resolves_definition_and_instance_context_without_erasing_authorship() {
    let base = Coord::from_mils_f64(40.0);
    let target = Coord::from_mils_f64(60.0);
    let absent = FieldIntent::Absent;
    assert_eq!(
        PIN_LENGTH.definition(&absent).unwrap(),
        Coord::from_mils_f64(25.0)
    );
    assert!(!absent.is_authored());
    assert_eq!(
        PIN_LENGTH
            .instance(&absent, Some(&target), Some(&base))
            .unwrap(),
        target
    );
    assert_eq!(
        PIN_LENGTH.instance(&absent, None, Some(&base)).unwrap(),
        base
    );
    let span = Span::new(10, 17);
    let inherit = FieldIntent::Inherit(span);
    let reset = FieldIntent::Reset(span);
    assert_eq!(
        PIN_LENGTH
            .instance(&inherit, Some(&target), Some(&base))
            .unwrap(),
        base
    );
    assert_eq!(
        PIN_LENGTH
            .instance(&reset, Some(&target), Some(&base))
            .unwrap(),
        Coord::from_mils_f64(25.0)
    );
    assert!(PIN_LENGTH.instance(&inherit, Some(&target), None).is_err());
    assert!(PIN_LENGTH.instance(&absent, None, None).is_err());
    assert_eq!(
        PIN_ELECTRICAL.definition(&FieldIntent::Absent).unwrap(),
        PinElectricalType::Passive
    );
}
#[test]
fn clear_optional_string_is_distinct_from_empty_set() {
    let result =
        compile("component A { description: null } component B { description: \"\" }").unwrap();
    let a = &result.components.items[0].description;
    let b = &result.components.items[1].description;
    assert!(matches!(a, FieldIntent::Clear(_)));
    assert_eq!(OPTIONAL_DESCRIPTION.definition(a).unwrap(), None);
    assert_eq!(
        OPTIONAL_DESCRIPTION.definition(b).unwrap(),
        Some(String::new())
    );
}
#[test]
fn evaluated_null_has_same_clearability_as_literal() {
    let result =
        compile("let empty = null\ncomponent A { description: empty, pin 1 { name: empty } }")
            .unwrap();
    assert!(matches!(
        result.components.items[0].description,
        FieldIntent::Clear(_)
    ));
    for field in ["length", "electrical", "at", "orientation", "is_hidden"] {
        for expr in ["null", "empty"] {
            assert!(
                compile(&format!(
                    "let empty = null\ncomponent A {{ pin 1 {{ {field}: {expr} }} }}"
                ))
                .is_err(),
                "{field}: {expr}"
            );
        }
    }
    assert!(compile("component A { part_count: null }").is_err());
}
#[test]
fn rejects_duplicate_keys_synonyms_and_unknown_fields() {
    for source in [
        "component A { description: \"a\", description: \"b\" }",
        "component A { parameter P { text: \"a\", value: \"b\" } }",
        "component A { pin 1 { at: (0mil, 0mil), location: (1mil, 1mil) } }",
        "component A { misspelled: true }",
        "component A { pin 1 { is_hidden: \"false\" } }",
        "component A { part 1 { ignored: true } }",
        "swap_group G { ignored: true } component A {}",
        "component A { pin 1 -> nc }",
    ] {
        assert!(compile(source).is_err(), "must reject {source}");
    }
}
#[test]
fn scope_defaults_empty_complete_and_required_parts() {
    let result = compile("collection components { mode: complete, delete: allow } component A { collection aliases { mode: complete, delete: allow } }").unwrap();
    assert_eq!(
        result.components.scope.mode,
        crate::ast::CollectionMode::Complete
    );
    let a = &result.components.items[0];
    assert_eq!(a.pins.scope, CollectionScope::default());
    assert!(a.aliases.items.is_empty());
    assert_eq!(a.aliases.scope.delete, crate::ast::CollectionDelete::Allow);
    assert!(compile("component A { collection parts { mode: complete } }").is_err());
    assert!(
        compile("component A { collection parts { mode: complete } part 1 {} part 3 {} }").is_err()
    );
    assert!(
        compile("component A { collection parts { mode: complete } part_count: 3 part 1 {} }")
            .is_err()
    );
    assert!(compile("component A { part_count: 2 part 3 {} }").is_err());
    assert!(compile("component A { part 0 {} }").is_err());
}
#[test]
fn duplicate_keyed_children_block_but_pin_designators_are_not_keys() {
    assert!(compile("component A {} component A {}").is_err());
    assert!(compile("component A { alias X alias X }").is_err());
    assert!(compile("component A { parameter X {} parameter X {} }").is_err());
    assert!(compile("component A { part 1 {} part 1 {} }").is_err());
    assert!(compile("component A { footprint X footprint X }").is_err());
    assert_eq!(
        compile("component A { pin 1 {} pin 1 {} }")
            .unwrap()
            .components
            .items[0]
            .pins
            .items
            .len(),
        2
    );
}
#[test]
fn typed_graphics_preserve_mixed_source_order_and_reject_inapplicable_fields() {
    let result = compile("component A { line { from: (0mil, 0mil), to: (1mil, 1mil) } label { text: \"x\", at: (2mil, 3mil) } rectangle {} }").unwrap();
    let kinds: Vec<_> = result.components.items[0]
        .graphics
        .items
        .iter()
        .map(|g| g.kind.node)
        .collect();
    assert_eq!(
        kinds,
        [
            GraphicKind::Line,
            GraphicKind::Label,
            GraphicKind::Rectangle
        ]
    );
    assert!(compile("component A { line { radius: 1mil } }").is_err());
    assert!(compile("component A { label { font_id: 0 } }").is_err());
    assert!(compile("component A { rectangle { from: reset } }").is_err());
}
#[test]
fn scopes_swap_bindings_and_map_paths_are_resolved_strictly() {
    let result = compile("swap_group G {}\ncomponent A { let len = 40mil\np = pin 1 { length: len, swap_group: $G } footprint X { pin \"1\": pad \"3\" } part 1 { part_swap_group: reset, pin 2 { part_swap_group: null } } }").unwrap();
    let component = &result.components.items[0];
    assert!(
        matches!(&component.pins.items[0].swap_group, FieldIntent::Set(value) if value.node == "G")
    );
    assert_eq!(component.footprints.items[0].maps.items[0].pad.node, "3");
    assert!(matches!(
        component.parts.items[0].part_swap_group,
        FieldIntent::Reset(_)
    ));
    assert!(compile("component A { pin 1 { swap_group: $missing } }").is_err());
}
#[test]
fn captured_transitive_import_values_do_not_schedule_imported_components() {
    let dir = tempfile::tempdir().unwrap();
    let leaf = dir.path().join("leaf.schlib-spec");
    let middle = dir.path().join("middle.schlib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(&leaf, "let size = 40mil\ncomponent Imported {}").unwrap();
    std::fs::write(
        &middle,
        "import \"leaf.schlib-spec\" as leaf\nlet size = leaf.size + 5mil\ncomponent Middle {}",
    )
    .unwrap();
    std::fs::write(&root, "import \"middle.schlib-spec\" as symbols\ncomponent Root { pin 1 { length: symbols.size } }").unwrap();
    let captured = capture_imports(&root).unwrap();
    // Compilation consumes captured sources, not freshly read files.
    std::fs::write(&leaf, "this is now invalid").unwrap();
    let compiled = compile_captured_schlib(&captured).unwrap();
    assert_eq!(compiled.root.components.items.len(), 1);
    assert_eq!(compiled.root.components.items[0].name.node, "Root");
    assert_eq!(compiled.imported_libraries.len(), 2);
    assert!(
        matches!(&compiled.root.components.items[0].pins.items[0].length, FieldIntent::Set(value) if value.node == Coord::from_mils_f64(45.0))
    );
    assert!(captured.verify_unchanged().is_err());
}
#[test]
fn imported_footprint_member_paths_validate_every_segment() {
    let dir = tempfile::tempdir().unwrap();
    let footprint = dir.path().join("footprints.pcblib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(&footprint, "footprint PKG { pad 1 {} terminal = pad 2 {} }").unwrap();
    std::fs::write(&root, "import \"footprints.pcblib-spec\" as fp\ncomponent A { p = pin 1 {} footprint $fp.PKG { $p: $fp.PKG.terminal } }").unwrap();
    let compiled = compile_captured_schlib(&capture_imports(&root).unwrap()).unwrap();
    assert_eq!(
        compiled.root.components.items[0].footprints.items[0]
            .model_name
            .node,
        "PKG"
    );
    assert_eq!(
        compiled.root.components.items[0].footprints.items[0]
            .maps
            .items[0]
            .pad
            .node,
        "2"
    );
    std::fs::write(
        &root,
        "import \"footprints.pcblib-spec\" as fp\ncomponent A { footprint $fp.MISSING }",
    )
    .unwrap();
    assert!(compile_captured_schlib(&capture_imports(&root).unwrap()).is_err());
}

#[test]
fn imported_strings_are_values_and_component_refs_are_not_footprints() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library.schlib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(
        &library,
        "let description = \"imported text\"\ncomponent Base {}",
    )
    .unwrap();
    std::fs::write(&root, "import \"library.schlib-spec\" as symbols\ncomponent A { description: symbols.description }").unwrap();
    let result = compile_captured_schlib(&capture_imports(&root).unwrap()).unwrap();
    assert!(
        matches!(&result.root.components.items[0].description, FieldIntent::Set(value) if value.node.as_deref() == Some("imported text"))
    );
    std::fs::write(
        &root,
        "import \"library.schlib-spec\" as symbols\ncomponent A { footprint $symbols.Base }",
    )
    .unwrap();
    assert!(compile_captured_schlib(&capture_imports(&root).unwrap()).is_err());
}
#[test]
fn invalid_import_domain_and_imported_diagnostic_location() {
    let dir = tempfile::tempdir().unwrap();
    let wrong_root = dir.path().join("wrong.schdoc-spec");
    std::fs::write(&wrong_root, "").unwrap();
    assert!(compile_captured_schlib(&capture_imports(&wrong_root).unwrap()).is_err());
    let library = dir.path().join("library.schlib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(&library, "component Base { pin 1 { length: null } }").unwrap();
    std::fs::write(
        &root,
        "import \"library.schlib-spec\" as symbols\ncomponent A {}",
    )
    .unwrap();
    let error = compile_captured_schlib(&capture_imports(&root).unwrap()).unwrap_err();
    assert!(
        error.span.is_none(),
        "import span must not point into root source"
    );
    assert!(
        error.message.contains("library.schlib-spec:1:"),
        "{}",
        error.message
    );
}
#[test]
fn local_self_reference_is_an_error_and_spreads_preserve_source_scope() {
    assert!(compile("let x = x\ncomponent A { description: x }").is_err());
    let result =
        compile("let fields = { name: \"A\", length: 40mil }\ncomponent R { pin 1 { ...fields } }")
            .unwrap();
    assert!(
        matches!(&result.components.items[0].pins.items[0].name, FieldIntent::Set(value) if value.node == "A")
    );
    assert!(
        compile("let fields = { name: \"A\", name: \"B\" }\ncomponent R { pin 1 { ...fields } }")
            .is_err()
    );
}
#[test]
fn explicit_set_clear_and_missing_default_resolution() {
    let span = Span::new(1, 2);
    let set = FieldIntent::Set(Spanned::new("chosen".to_string(), span));
    let clear = FieldIntent::Clear(span);
    assert_eq!(
        PIN_NAME
            .instance(&set, Some(&"target".to_string()), Some(&"base".to_string()))
            .unwrap(),
        "chosen"
    );
    assert_eq!(PIN_NAME.instance(&clear, None, None).unwrap(), "");
    let required: FieldPolicy<String> = FieldPolicy {
        name: "required",
        default: None,
        clear: None,
    };
    assert!(required.definition(&FieldIntent::Absent).is_err());
    assert!(required.definition(&FieldIntent::Reset(span)).is_err());
    assert!(required.definition(&FieldIntent::Clear(span)).is_err());
}
#[test]
fn spread_overlay_order_preserves_explicit_overrides_and_rejects_direct_typos() {
    for (body, wanted) in [
        ("...{name: \"base\"}, name: \"override\"", "override"),
        ("name: \"first\", ...{name: \"later\"}", "later"),
        ("...{name: \"a\"}, ...{name: \"b\"}", "b"),
    ] {
        let pin = pin_for(body);
        assert!(matches!(&pin.name, FieldIntent::Set(value) if value.node == wanted));
    }
    let parameter =
        compile("component A { parameter P { ...{text: \"base\"}, value: \"override\" } }")
            .unwrap()
            .components
            .items
            .remove(0)
            .parameters
            .items
            .remove(0);
    assert!(matches!(parameter.text, FieldIntent::Set(value) if value.node == "override"));
    assert!(compile("component A { parameter P { ...{text: \"a\", value: \"b\"} } }").is_err());
    assert!(
        compile("component A { pin 1 { name: \"a\", ...{name: \"b\"}, name: \"c\" } }").is_err()
    );
}
#[test]
fn forged_footprint_object_is_not_a_definition_reference() {
    assert!(
        compile("let fake = {model_name: \"nonexistent\"}\ncomponent A { footprint $fake }")
            .is_err()
    );
}
#[test]
fn captured_swap_bindings_match_standalone_and_wrong_domain_declarations_fail() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(
        &root,
        "swap_group G {}\nlet g = $G\ncomponent A { pin 1 { swap_group: $g } }",
    )
    .unwrap();
    assert!(compile_captured_schlib(&capture_imports(&root).unwrap()).is_ok());
    let pcb = dir.path().join("wrong.pcblib-spec");
    std::fs::write(&pcb, "component Invalid {}").unwrap();
    std::fs::write(&root, "import \"wrong.pcblib-spec\" as pcb\ncomponent A {}").unwrap();
    assert!(compile_captured_schlib(&capture_imports(&root).unwrap()).is_err());
}

#[test]
fn imported_pcb_file_rejects_schlib_collection_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root.schlib-spec");
    let pcb = dir.path().join("wrong.pcblib-spec");
    std::fs::write(
        &pcb,
        "collection components { mode: complete, delete: allow } footprint F {} ",
    )
    .unwrap();
    std::fs::write(
        &root,
        "import \"wrong.pcblib-spec\" as pcb\ncomponent A { footprint $pcb.F }",
    )
    .unwrap();
    assert!(compile_captured_schlib(&capture_imports(&root).unwrap()).is_err());
}
#[test]
fn nested_spreads_retain_synonym_provenance_and_last_overlay_order() {
    for (value, expected) in [
        ("{...{text: \"base\"}, value: \"override\"}", "override"),
        (
            "{...{text: \"base\"}, ...{value: \"override\"}, ...{text: \"later\"}}",
            "later",
        ),
    ] {
        let source =
            format!("let defaults = {value}\ncomponent A {{ parameter P {{ ...defaults }} }}");
        let compiled = compile(&source).unwrap();
        assert!(
            matches!(&compiled.components.items[0].parameters.items[0].text, FieldIntent::Set(value) if value.node == expected)
        );
    }
    assert!(compile("let defaults = {text: \"a\", value: \"b\"}\ncomponent A { parameter P { ...defaults, text: \"override\" } }").is_err());
    let cleared = compile("let defaults = { ...{text: \"base\"}, value: null }\ncomponent A { parameter P { ...defaults } }").unwrap();
    assert!(matches!(
        cleared.components.items[0].parameters.items[0].text,
        FieldIntent::Clear(_)
    ));
    // Generic expression objects retain distinct keys; aliases are meaningful
    // only when lowering into a registered typed field owner.
    assert!(
        compile(
            "let values = {text: \"a\", value: \"b\"}\ncomponent A { description: values.text }"
        )
        .is_ok()
    );
}
#[test]
fn imported_footprint_retains_captured_source_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let footprint = dir.path().join("footprints.pcblib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(&footprint, "footprint PKG { pad 1 {} }").unwrap();
    std::fs::write(
        &root,
        "import \"footprints.pcblib-spec\" as fp\ncomponent A { footprint $fp.PKG }",
    )
    .unwrap();
    let compiled = compile_captured_schlib(&capture_imports(&root).unwrap()).unwrap();
    let reference = compiled.root.components.items[0].footprints.items[0]
        .source
        .as_ref()
        .unwrap();
    assert_eq!(reference.source, footprint.canonicalize().unwrap());
    assert_eq!(reference.name, "PKG");
}
#[test]
fn imported_object_groups_with_identical_offsets_remain_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.schlib-spec");
    let b = dir.path().join("b.schlib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(&a, "let fields = {text: \"a\"}").unwrap();
    std::fs::write(&b, "let fields = {value: \"b\"}").unwrap();
    std::fs::write(&root, "import \"a.schlib-spec\" as a\nimport \"b.schlib-spec\" as b\nlet merged = {...a.fields, ...b.fields}\ncomponent A { parameter P {...merged} }").unwrap();
    let compiled = compile_captured_schlib(&capture_imports(&root).unwrap()).unwrap();
    assert!(
        matches!(&compiled.root.components.items[0].parameters.items[0].text, FieldIntent::Set(value) if value.node == "b")
    );
}
#[test]
fn imported_synonym_error_points_to_consuming_root_spread() {
    let dir = tempfile::tempdir().unwrap();
    let imported = dir.path().join("imported.schlib-spec");
    let root = dir.path().join("root.schlib-spec");
    std::fs::write(&imported, "let fields = {text: \"a\", value: \"b\"}").unwrap();
    let source = "import \"imported.schlib-spec\" as imported\ncomponent A { parameter P { ...imported.fields } }";
    std::fs::write(&root, source).unwrap();
    let error = compile_captured_schlib(&capture_imports(&root).unwrap()).unwrap_err();
    let start = source.find("imported.fields").unwrap() as u32;
    assert_eq!(
        error.span,
        Some(Span::new(start, start + "imported.fields".len() as u32))
    );
}

#[test]
fn compiled_anchor_intent_waits_for_effective_target_values() {
    let intent = compile("component R { body = rectangle { from: (0mil, 0mil), to: (100mil, 100mil) } pin 1 { on: $body.left } }").unwrap();
    let component = &intent.components.items[0];
    let pin = &component.pins.items[0];
    assert_eq!(pin.length, FieldIntent::Absent);
    assert_eq!(pin.location, FieldIntent::Absent);
    assert_eq!(pin.orientation, FieldIntent::Absent);
    let placement = pin.placement.as_ref().unwrap();
    assert_eq!(
        placement.graphic.node,
        GraphicSourceRef {
            owner: LayoutOwner::Component,
            index: 0
        }
    );
    assert_eq!(placement.edge.node, AnchorEdge::Left);
    let point = |x, y| {
        altium_format_types::CoordPoint::new(Coord::from_mils_f64(x), Coord::from_mils_f64(y))
    };
    let geometry = Some(BoxGeometry {
        from: point(0.0, 0.0),
        to: point(100.0, 100.0),
    });
    for length in [25.0, 60.0] {
        let evaluated = evaluate_layout(
            LayoutOwner::Component,
            &[pin.placement.clone()],
            &[None],
            &[EffectivePinGeometry {
                length: Coord::from_mils_f64(length),
                location: point(400.0, 300.0),
                orientation: altium_format_types::RotationBy90::Rotate90,
                orientation_is_authored: false,
            }],
            &[geometry],
            &Default::default(),
        )
        .unwrap();
        assert_eq!(evaluated.pins[0].unwrap().location, point(-length, 50.0));
        assert_eq!(evaluated.graphics[0], None);
    }
}
#[test]
fn retained_layout_preserves_owner_scopes_and_unbound_absence() {
    let intent = compile("component R { body = rectangle { padding: reset, min_width: 80mil } rectangle {} pin 1 { on: $body.left } part 1 { local = rectangle {} pin 2 { on: $local.right } pin 3 { on: $body.top } } }").unwrap();
    let component = &intent.components.items[0];
    assert!(matches!(
        component.graphics.items[0]
            .auto_size
            .as_ref()
            .unwrap()
            .padding,
        FieldIntent::Reset(_)
    ));
    assert_eq!(component.graphics.items[0].fields.from, FieldIntent::Absent);
    assert_eq!(component.graphics.items[0].fields.to, FieldIntent::Absent);
    assert_eq!(
        component.graphics.items[1].auto_size, None,
        "unbound empty graphic is an ordinary partial-scope graphic"
    );
    let part = &component.parts.items[0];
    assert_eq!(
        part.pins.items[0].placement.as_ref().unwrap().graphic.node,
        GraphicSourceRef {
            owner: LayoutOwner::Part(PartNumber::new(1).unwrap()),
            index: 0
        }
    );
    assert_eq!(
        part.pins.items[1].placement.as_ref().unwrap().graphic.node,
        GraphicSourceRef {
            owner: LayoutOwner::Component,
            index: 0
        }
    );
}
#[test]
fn anchored_field_compilation_keeps_spread_provenance_and_strict_unknowns() {
    let source = "let defaults = {name: \"A\", length: 40mil}\ncomponent R { body = rectangle {} pin 1 {...defaults, on: $body.left, name: \"B\", gap: reset} }";
    let compiled = compile(source).unwrap();
    let pin = &compiled.components.items[0].pins.items[0];
    assert!(matches!(&pin.name, FieldIntent::Set(value) if value.node == "B"));
    assert!(matches!(
        pin.placement.as_ref().unwrap().gap,
        FieldIntent::Reset(_)
    ));
    for source in [
        "component R { body = rectangle {} pin 1 {on: $body.left, location: (0mil,0mil)} }",
        "component R { body = rectangle {} pin 1 {on: $body.left, typo: true} }",
        "component R { body = rectangle {} pin 1 {on: $body.left, gap: null} }",
        "component R { body = rectangle {} p=pin 1 {on: $body.left, after:$p} }",
        "component R { body = rectangle {} part 1 { body = line {} pin 1 {on:$body.left} } }",
    ] {
        assert!(compile(source).is_err(), "must reject {source}");
    }
}
#[test]
fn bare_numeric_pin_lengths_remain_mils() {
    for value in ["25", "25.0", "25mil"] {
        let pin = pin_for(&format!("length: {value}"));
        assert!(
            matches!(pin.length, FieldIntent::Set(value) if value.node == Coord::from_mils_f64(25.0))
        );
    }
}
#[test]
fn layout_binding_namespace_rejects_owner_local_collisions() {
    for source in [
        "component R { body = rectangle {} body = pin 1 {on:$body.left} }",
        "component R { let body=1\nbody=rectangle {} }",
        "component R { swap_group body {} body=rectangle {} }",
        "component R { part 1 { body=rectangle {} body=pin 1 {} } }",
        "component R { part 1 { let body=1\nbody=rectangle {} } }",
    ] {
        assert!(
            compile(source).is_err(),
            "duplicate binding must reject {source}"
        );
    }
    assert!(
        compile(
            "component R { body=rectangle {} part 1 { body=rectangle {} pin 1 {on:$body.left} } }"
        )
        .is_ok(),
        "nested owner may intentionally shadow parent"
    );
}
#[test]
fn inherited_component_graphic_shadows_older_root_value_binding() {
    let compiled =
        compile("let body=1\ncomponent R {body=rectangle{} part 1 {pin 1 {on:$body.left}}}")
            .unwrap();
    let reference = compiled.components.items[0].parts.items[0].pins.items[0]
        .placement
        .as_ref()
        .unwrap()
        .graphic
        .node;
    assert_eq!(
        reference,
        GraphicSourceRef {
            owner: LayoutOwner::Component,
            index: 0
        }
    );
}
