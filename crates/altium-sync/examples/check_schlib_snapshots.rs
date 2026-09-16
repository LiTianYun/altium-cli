//! Real-file probe for complete typed snapshots and explicit scalar patches.
//! Writes only temporary documents. This is not a user-facing saved-plan command.
use altium_format::SchLib;
use altium_format_types::ComponentKind;
use altium_spec_lang::{
    diagnostic::{Span, Spanned},
    intent::FieldIntent,
};
use altium_sync::schlib::{ComponentPropertiesIntent, SchLibPatch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut checked = 0;
    let mut failures = 0;
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let result = check(&path).map_err(|error| {
            std::io::Error::other(format!("checking snapshot '{}': {error}", path.display()))
        })?;
        if result["native_typed_noop_equal"] != true
            || result["native_typed_patch_equal"] != true
            || result["replan_empty"] != true
        {
            failures += 1;
        }
        println!("{}", serde_json::to_string(&result)?);
        checked += 1;
    }
    if checked == 0 {
        return Err("usage: check_schlib_snapshots <library.SchLib> ...".into());
    }
    if failures != 0 {
        return Err(format!(
            "{failures} libraries changed complete typed state on native serialization"
        )
        .into());
    }
    Ok(())
}

fn check(path: &std::path::Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut doc = SchLib::open(path)?;
    let input = doc.snapshot()?;
    let materialized = input.materialize()?;
    if materialized.snapshot()? != input {
        return Err("typed materialization changed captured state".into());
    }
    let temp = tempfile::tempdir()?;
    let noop_path = temp.path().join("noop.SchLib");
    materialized.save(&noop_path)?;
    let noop_equal = SchLib::open(&noop_path)?.snapshot()? == input;

    let mut fields = Vec::new();
    for index in 0..input.component_count() {
        let component = input.component(index)?;
        fields.push((
            index,
            ComponentPropertiesIntent {
                description: authored(Some(format!("snapshot validation component {index}"))),
                designator: authored(Some(format!("X{index}?"))),
                component_kind: authored(if index % 2 == 0 {
                    ComponentKind::Jumper
                } else {
                    ComponentKind::StandardNoBom
                }),
                show_hidden_pins: authored(!component.show_hidden_pins()),
            },
        ));
    }
    let patch = SchLibPatch::component_properties(&input, fields.clone())?;
    let expected = patch.resulting_snapshot()?;
    patch.apply_in_memory(&mut doc)?;
    if doc.snapshot()? != expected {
        return Err("in-memory patch differs from its reviewed candidate".into());
    }
    let edited_path = temp.path().join("edited.SchLib");
    doc.save(&edited_path)?;
    let reopened = SchLib::open(&edited_path)?.snapshot()?;
    let replan_empty = SchLibPatch::component_properties(&reopened, fields)?.is_empty();
    Ok(serde_json::json!({
        "file": path,
        "components": input.component_count(),
        "replacements": patch.replacements().len(),
        "typed_materialize_equal": true,
        "in_memory_patch_equal": true,
        "native_typed_noop_equal": noop_equal,
        "native_typed_patch_equal": reopened == expected,
        "replan_empty": replan_empty,
        "properties": ["description", "designator", "component_kind", "show_hidden_pins"],
    }))
}

fn authored<T>(value: T) -> FieldIntent<T> {
    FieldIntent::Set(Spanned::new(value, Span::new(0, 0)))
}
