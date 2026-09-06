//! Explicit real-file validation of image bundle capture and native save/reopen.
//! This is not the user-facing dump/planner workflow or a whole-document diff.
use std::path::PathBuf;

use altium_format::SchLib;
use altium_spec_lang::intent::ImageAssetSource;
use altium_sync::image_asset::CapturedImageAsset;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err("usage: check_image_assets <library.SchLib> ...".into());
    }
    for path in paths {
        let result = check(&path).map_err(|error| {
            std::io::Error::other(format!(
                "checking image assets in '{}': {error}",
                path.display()
            ))
        })?;
        println!("{}", serde_json::to_string(&result)?);
    }
    Ok(())
}

fn check(path: &std::path::Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut doc = SchLib::open(path)?;
    let original: Vec<_> = doc
        .image_resources()
        .map(|resource| (resource.name.to_owned(), resource.payload.to_owned()))
        .collect();
    let temp = tempfile::tempdir()?;
    // Canonicalize the explicit declaring source directory just as captured
    // language compilation does. Nothing is written into the user's library.
    let origin = temp.path().canonicalize()?.join("export.schlib-spec");
    for (index, (name, bytes)) in original.iter().enumerate() {
        let asset_name = format!("image-{index}.altium-image");
        std::fs::write(temp.path().join(&asset_name), bytes)?;
        let captured = CapturedImageAsset::capture(&ImageAssetSource {
            path: asset_name,
            declaring_source: Some(origin.clone()),
        })?;
        if captured.bytes() != bytes {
            return Err(format!("resource #{index} '{name}': export/capture changed body").into());
        }
        doc.replace_image_resource(index, name.clone(), captured.bytes().to_owned())?;
        captured.verify_unchanged()?;
    }
    doc.validate_invariants()?;
    let saved_path = temp.path().join("saved.SchLib");
    doc.save(&saved_path)?;
    let reopened = SchLib::open(&saved_path)?;
    reopened.validate_invariants()?;
    let actual: Vec<_> = reopened
        .image_resources()
        .map(|resource| (resource.name.to_owned(), resource.payload.to_owned()))
        .collect();
    if original != actual {
        return Err("ordered image names/bodies changed after native save/reopen".into());
    }
    Ok(serde_json::json!({
        "file": path,
        "resources": original.len(),
        "payload_bytes": original.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
        "exact_image_bundle_roundtrip": true,
    }))
}
