//! Real-file probe for complete typed snapshots and explicit scalar patches.
//! Writes only temporary documents. This is not a user-facing saved-plan command.
use altium_format::SchLib;
use altium_spec_lang::{
    diagnostic::{Span, Spanned},
    intent::FieldIntent,
};
use altium_sync::schlib::SchLibPatch;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut checked = 0;
    let mut failures = 0;
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let result = check(&path).map_err(|error| {
            std::io::Error::other(format!("checking snapshot '{}': {error}", path.display()))
        })?;
        if result["native_typed_noop_equal"] != true || result["native_typed_patch_equal"] != true {
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

    let fields = (0..input.component_count()).map(|index| {
        (
            index,
            FieldIntent::Set(Spanned::new(
                Some(format!("snapshot validation component {index}")),
                Span::new(0, 0),
            )),
        )
    });
    let patch = SchLibPatch::descriptions(&input, fields)?;
    let expected = patch.resulting_snapshot()?;
    patch.apply_in_memory(&mut doc)?;
    if doc.snapshot()? != expected {
        return Err("in-memory patch differs from its reviewed candidate".into());
    }
    let edited_path = temp.path().join("edited.SchLib");
    doc.save(&edited_path)?;
    let reopened = SchLib::open(&edited_path)?.snapshot()?;
    Ok(serde_json::json!({
        "file": path,
        "components": input.component_count(),
        "replacements": patch.replacements().len(),
        "typed_materialize_equal": true,
        "in_memory_patch_equal": true,
        "native_typed_noop_equal": noop_equal,
        "native_typed_patch_equal": reopened == expected,
    }))
}
