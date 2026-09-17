//! Shared mutation-boundary validation helpers for SchLib editing
//! (resolutions memo `docs/worklogs/next-child-contracts-2026-09-16.md`).
//!
//! These helpers run ONLY when an editing API writes a value. They are never
//! wired into capture/parse paths: the AD26 importer enforces none of these
//! rules, so wild loaded values (out-of-range owner parts, dangling font ids,
//! orphan storage entries, style tables) must keep loading and round-tripping
//! verbatim (capture-forbidden boundary, resolutions §1/§4/§8).
//!
//! Where AD26 silently coerces (font ids to 1, current part to a no-op), the
//! hard errors here are OUR deliberate fail-fast deviation, never AD26
//! behavior.

use crate::{AltiumFormatError, Result};
use altium_format_types::sch::SchFont;

/// Native default font entry for new text-bearing children (resolutions §8a
/// rule 3, mirroring AD26 `SchDataFontManager.GetFontID` find-or-create for
/// Times New Roman, size 10, rotation 0, no styles).
const DEFAULT_FONT_NAME: &str = "Times New Roman";
const DEFAULT_FONT_SIZE: i32 = 10;

fn invalid(key: &str, detail: impl std::fmt::Display) -> AltiumFormatError {
    AltiumFormatError::InvalidParamValue {
        key: key.to_owned(),
        detail: detail.to_string(),
    }
}

/// Checks that a font id our APIs are about to write references an existing
/// font-table entry (resolutions §8a rule 2). Never replicates AD26's silent
/// coerce-to-1.
pub(crate) fn check_font_id_exists(fonts: &[SchFont], font_id: i32) -> Result<()> {
    if fonts.iter().any(|font| font.id == font_id) {
        return Ok(());
    }
    Err(invalid(
        "FontId",
        format!(
            "font id {font_id} does not reference any of the {} font-table entries",
            fonts.len()
        ),
    ))
}

/// Finds the native default font entry (Times New Roman 10, rotation 0, no
/// styles; name compared case-insensitively) or appends it, returning its id
/// (resolutions §8a rule 3). Font-table appends are an ordinary format-level
/// mutation; the table must be the destination file's table.
pub(crate) fn find_or_create_default_font(fonts: &mut Vec<SchFont>) -> Result<i32> {
    if let Some(font) = fonts.iter().find(|font| {
        font.name.eq_ignore_ascii_case(DEFAULT_FONT_NAME)
            && font.size == DEFAULT_FONT_SIZE
            && font.rotation == 0
            && !font.bold
            && !font.italic
            && !font.underline
            && !font.strikeout
    }) {
        return Ok(font.id);
    }
    // The serialized table is positional with 1-based sequential ids; refuse
    // to append behind a table that violates that (mutation-boundary check —
    // capture of such state stays permitted elsewhere).
    for (position, font) in fonts.iter().enumerate() {
        let expected = i32::try_from(position + 1)
            .map_err(|_| invalid("FontIdCount", "font table length exceeds native id range"))?;
        if font.id != expected {
            return Err(invalid(
                "FontId",
                format!(
                    "font table entry at position {position} carries id {} (expected {expected}); \
                     cannot append the default font behind a non-sequential table",
                    font.id
                ),
            ));
        }
    }
    let id = i32::try_from(fonts.len() + 1)
        .map_err(|_| invalid("FontIdCount", "font table length exceeds native id range"))?;
    fonts.push(SchFont {
        id,
        name: DEFAULT_FONT_NAME.to_owned(),
        size: DEFAULT_FONT_SIZE,
        rotation: 0,
        bold: false,
        italic: false,
        underline: false,
        strikeout: false,
    });
    Ok(id)
}

/// The meaning class a written `owner_part_id` is checked against
/// (resolutions §1/§2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WrittenOwnerPartClass {
    /// Binary/text pins: 0 (the natively defined shared-pin wildcard) or a
    /// numbered part.
    Pin,
    /// Drawing graphics: numbered parts only — 0 is NOT a wildcard for
    /// non-pins and out-of-range owners are preserved-only content that our
    /// APIs must never create.
    Graphic,
    /// Root text children (Designator/Parameter): −1 only.
    RootText,
}

/// Range/meaning check for an `owner_part_id` value our APIs write
/// (resolutions §1 value domain, §2 creation rules). `native_part_count` is
/// the component's native count (effective + 1) and must itself be ≥ 2.
pub(crate) fn check_written_owner_part_id(
    class: WrittenOwnerPartClass,
    owner_part_id: i32,
    native_part_count: i32,
) -> Result<()> {
    if native_part_count < 2 {
        return Err(invalid(
            "OwnerPartId",
            format!(
                "native part count {native_part_count} has no writable part slots; \
                 loaded documents are upgraded to ≥ 2 at load"
            ),
        ));
    }
    let effective = native_part_count - 1;
    let accepted = match class {
        WrittenOwnerPartClass::Pin => {
            owner_part_id == 0 || (1..=effective).contains(&owner_part_id)
        }
        WrittenOwnerPartClass::Graphic => (1..=effective).contains(&owner_part_id),
        WrittenOwnerPartClass::RootText => owner_part_id == -1,
    };
    if accepted {
        // The binary pin body stores owner_part_id as a signed 16-bit field
        // (serialize_binary_pin hard-errors on overflow instead of
        // truncating): with a wild loaded PARTCOUNT beyond that range the
        // effective bound alone would admit unserializable ids, so refuse
        // them at the mutation boundary too.
        if matches!(class, WrittenOwnerPartClass::Pin) && i16::try_from(owner_part_id).is_err() {
            return Err(invalid(
                "OwnerPartId",
                format!(
                    "pin owner part id {owner_part_id} exceeds the signed-16 range of the \
                     binary pin body field and could never serialize"
                ),
            ));
        }
        return Ok(());
    }
    Err(invalid(
        "OwnerPartId",
        format!(
            "owner part id {owner_part_id} is not writable for {class:?} children \
             (native part count {native_part_count}, effective parts 1..={effective}); \
             out-of-range owners in loaded files are preserved, never created"
        ),
    ))
}

/// Bound check for a written `owner_part_display_mode` or component
/// `display_mode` (resolutions §4 rule 3): `0 ≤ mode < display_mode_count`.
/// `field` names the checked field in the error. The hard error (instead of
/// AD26's checked-setter silent ignore) is our deliberate deviation.
pub(crate) fn check_written_display_mode(
    field: &str,
    mode: i32,
    display_mode_count: i32,
) -> Result<()> {
    if (0..display_mode_count).contains(&mode) {
        return Ok(());
    }
    Err(invalid(
        field,
        format!(
            "{field} {mode} is outside the writable display-mode range 0..{display_mode_count}; \
             out-of-range modes in loaded files are preserved, never created"
        ),
    ))
}

/// Bound check for a written `custom_display_mode_names` index
/// (resolutions §4 rule 5): `index < display_mode_count`.
// Still dead after increment C by design: no C path writes display-mode-
// adjacent state (part grow/shrink leaves display_mode/display_mode_count/
// custom names untouched, and pin/graphic creation validates only
// owner_part_display_mode via `check_written_display_mode`). Consumed by the
// increment-D engine when component-level display-mode/custom-name edits land;
// until then it is exercised by unit tests only.
#[allow(dead_code)]
pub(crate) fn check_custom_display_mode_name_index(
    index: usize,
    display_mode_count: i32,
) -> Result<()> {
    if i32::try_from(index).is_ok_and(|index| index < display_mode_count) {
        return Ok(());
    }
    Err(invalid(
        "CustomDisplayModeName",
        format!(
            "custom display mode name index {index} is outside the writable range \
             0..{display_mode_count}"
        ),
    ))
}

/// Existence check for an embedded image's storage reference our APIs write
/// (resolutions §8c rule 2): an embedded image must reference an asset present
/// in the resulting library state, matched exactly first and then
/// case-insensitively (mirroring the native pairing order). External-path
/// images (`embed_image == false`) reference the filesystem, not `/Storage`.
/// Check-only: asset lifecycle ops belong to increment D.
pub(crate) fn check_embedded_image_reference<'a>(
    asset_names: impl IntoIterator<Item = &'a str> + Clone,
    file_name: &str,
    embed_image: bool,
) -> Result<()> {
    if !embed_image {
        return Ok(());
    }
    let exact = asset_names
        .clone()
        .into_iter()
        .any(|name| name == file_name);
    let case_insensitive = asset_names
        .into_iter()
        .any(|name| name.eq_ignore_ascii_case(file_name));
    if exact || case_insensitive {
        return Ok(());
    }
    Err(invalid(
        "FileName",
        format!(
            "embedded image references storage asset '{file_name}' which does not exist \
             in the resulting library state (dangling references in loaded files are \
             preserved, never created)"
        ),
    ))
}

/// Refuses authoring a pin `hidden_net_name` value the verified SchLib
/// serialization path cannot carry: the binary pin body has no hidden-net
/// field (`parse_binary_pin`/`serialize_binary_pin`), no pin sidecar stores
/// one, and the native storage form — a pin-owned Parameter record named
/// "HiddenNetName" — is preserved-only content in this model (no authoring
/// path is proven). Preserving the original value verbatim and writing the
/// empty wire default both pass; any other authored value is a hard error
/// until a verified serialization path exists. `original` is `None` for a
/// newly created pin.
pub(crate) fn check_pin_hidden_net_name_write(original: Option<&str>, written: &str) -> Result<()> {
    if written.is_empty() || original == Some(written) {
        return Ok(());
    }
    Err(invalid(
        "HiddenNetName",
        format!(
            "writing pin hidden_net_name '{written}' is unsupported: the SchLib binary pin \
             body carries no hidden-net field and the pin-owned Parameter storage form is \
             preserved-only (authoring unproven); only the loaded value or the empty default \
             may be written"
        ),
    ))
}

/// Refuses any style_id authoring (resolutions §8b): loaded style ids and
/// style tables are preserved verbatim, but writing one — a nonzero id on a
/// new child, or any change to an existing child's id — is a hard error until
/// the Delphi style-table implementation is reverse engineered. `original` is
/// `None` for a newly created child.
pub(crate) fn check_style_id_write(original: Option<i32>, written: i32) -> Result<()> {
    let unchanged = match original {
        Some(original) => original == written,
        None => written == 0,
    };
    if unchanged {
        return Ok(());
    }
    Err(invalid(
        "StyleId",
        format!(
            "writing style_id {written} is unsupported: style_id semantics unproven — \
             native (Delphi) style-table implementation not yet reverse engineered; \
             loaded style ids are preserved verbatim only"
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(id: i32, name: &str, size: i32) -> SchFont {
        SchFont {
            id,
            name: name.to_owned(),
            size,
            rotation: 0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
        }
    }

    #[test]
    fn font_id_membership_checks_table_entries_not_positions() {
        let fonts = vec![font(1, "Times New Roman", 10), font(2, "Arial", 8)];
        check_font_id_exists(&fonts, 1).unwrap();
        check_font_id_exists(&fonts, 2).unwrap();
        for dangling in [0, -1, 3, 1000] {
            let error = check_font_id_exists(&fonts, dangling)
                .unwrap_err()
                .to_string();
            assert!(error.contains(&dangling.to_string()), "{error}");
        }
        assert!(check_font_id_exists(&[], 1).is_err());
    }

    #[test]
    fn default_font_is_found_case_insensitively_or_appended_sequentially() {
        let mut fonts = vec![font(1, "Arial", 8), font(2, "times new roman", 10)];
        assert_eq!(find_or_create_default_font(&mut fonts).unwrap(), 2);
        assert_eq!(fonts.len(), 2);

        // Style differences and size differences do not match the default.
        let mut styled = vec![font(1, "Times New Roman", 12)];
        let mut bold = font(2, "Times New Roman", 10);
        bold.bold = true;
        styled.push(bold);
        assert_eq!(find_or_create_default_font(&mut styled).unwrap(), 3);
        assert_eq!(styled.len(), 3);
        assert_eq!(styled[2].name, "Times New Roman");
        assert_eq!(styled[2].size, 10);
        assert_eq!(styled[2].id, 3);

        // A non-sequential table is refused rather than silently renumbered.
        let mut broken = vec![font(5, "Arial", 8)];
        let error = find_or_create_default_font(&mut broken)
            .unwrap_err()
            .to_string();
        assert!(error.contains("non-sequential"), "{error}");
        assert_eq!(broken.len(), 1);
    }

    #[test]
    fn written_owner_part_ids_follow_native_meaning_classes() {
        use WrittenOwnerPartClass::*;
        // Native count 3 = effective parts 1..=2.
        check_written_owner_part_id(Pin, 0, 3).unwrap();
        check_written_owner_part_id(Pin, 1, 3).unwrap();
        check_written_owner_part_id(Pin, 2, 3).unwrap();
        assert!(check_written_owner_part_id(Pin, 3, 3).is_err());
        assert!(check_written_owner_part_id(Pin, -1, 3).is_err());

        check_written_owner_part_id(Graphic, 1, 3).unwrap();
        check_written_owner_part_id(Graphic, 2, 3).unwrap();
        assert!(check_written_owner_part_id(Graphic, 0, 3).is_err());
        assert!(check_written_owner_part_id(Graphic, 3, 3).is_err());
        assert!(check_written_owner_part_id(Graphic, -1, 3).is_err());

        check_written_owner_part_id(RootText, -1, 2).unwrap();
        assert!(check_written_owner_part_id(RootText, 0, 2).is_err());
        assert!(check_written_owner_part_id(RootText, 1, 2).is_err());

        // Un-upgraded native counts have no writable slots at all.
        for count in [1, 0, -1] {
            assert!(check_written_owner_part_id(Pin, 1, count).is_err());
        }

        // A wild native count beyond the signed-16 pin field admits ids the
        // binary body could never carry: those are refused for pins even
        // though they sit inside the effective range.
        let wild_count = 40_000;
        check_written_owner_part_id(Pin, i32::from(i16::MAX), wild_count).unwrap();
        let error = check_written_owner_part_id(Pin, i32::from(i16::MAX) + 1, wild_count)
            .unwrap_err()
            .to_string();
        assert!(error.contains("signed-16"), "{error}");
    }

    #[test]
    fn display_mode_bounds_and_name_indices_reject_out_of_range_writes() {
        check_written_display_mode("OwnerPartDisplayMode", 0, 1).unwrap();
        check_written_display_mode("OwnerPartDisplayMode", 7, 8).unwrap();
        for (mode, count) in [(1, 1), (-1, 1), (8, 8), (0, 0)] {
            let error = check_written_display_mode("OwnerPartDisplayMode", mode, count)
                .unwrap_err()
                .to_string();
            assert!(error.contains("OwnerPartDisplayMode"), "{error}");
        }
        check_custom_display_mode_name_index(0, 1).unwrap();
        check_custom_display_mode_name_index(7, 8).unwrap();
        assert!(check_custom_display_mode_name_index(1, 1).is_err());
        assert!(check_custom_display_mode_name_index(0, 0).is_err());
    }

    #[test]
    fn embedded_image_references_require_existing_assets_external_paths_do_not() {
        let names = ["logo.png", "Photo.BMP"];
        check_embedded_image_reference(names, "logo.png", true).unwrap();
        // Case-insensitive fallback mirrors the native pairing pass.
        check_embedded_image_reference(names, "photo.bmp", true).unwrap();
        let error = check_embedded_image_reference(names, "missing.png", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing.png"), "{error}");
        // External-path images have no storage requirement.
        check_embedded_image_reference(names, "C:/anywhere/else.png", false).unwrap();
        check_embedded_image_reference([], "external.png", false).unwrap();
    }

    #[test]
    fn pin_hidden_net_name_authoring_is_limited_to_preserve_or_empty() {
        // New pins may only carry the empty wire default.
        check_pin_hidden_net_name_write(None, "").unwrap();
        // Replace may preserve the loaded value verbatim or clear it.
        check_pin_hidden_net_name_write(Some("VCC"), "VCC").unwrap();
        check_pin_hidden_net_name_write(Some("VCC"), "").unwrap();
        for (original, written) in [(None, "VCC"), (Some("VCC"), "GND"), (Some(""), "VCC")] {
            let error = check_pin_hidden_net_name_write(original, written)
                .unwrap_err()
                .to_string();
            assert!(error.contains("unsupported"), "{error}");
            assert!(error.contains(written), "{error}");
        }
    }

    #[test]
    fn style_id_authoring_is_a_hard_error_preservation_is_not() {
        // Preserving a loaded value verbatim is fine, including wild ones.
        check_style_id_write(Some(0), 0).unwrap();
        check_style_id_write(Some(7), 7).unwrap();
        // New children carry the 0 default only.
        check_style_id_write(None, 0).unwrap();
        for (original, written) in [(None, 1), (Some(0), 1), (Some(7), 0), (Some(1), 2)] {
            let error = check_style_id_write(original, written)
                .unwrap_err()
                .to_string();
            assert!(error.contains("style_id semantics unproven"), "{error}");
            assert!(error.contains("Delphi"), "{error}");
        }
    }
}
