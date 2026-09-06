//! Immutable input capture for imported Altium image bundles.
//!
//! Only the complete image body is opaque. This module does not decode pixels,
//! inspect native document streams, or grant resource mutation permissions.
use std::path::{Path, PathBuf};

use altium_spec_lang::intent::ImageAssetSource;

#[derive(Debug, thiserror::Error)]
pub enum ImageAssetError {
    #[error(
        "image asset '{asset}' has no declaring source; compile a captured source graph or supply an explicit source origin"
    )]
    MissingOrigin { asset: String },
    #[error("image asset '{asset}' requires an absolute declaring source, got '{origin}'")]
    RelativeOrigin { asset: String, origin: PathBuf },
    #[error("image asset '{path}' must name a nonempty .altium-image bundle")]
    InvalidPath { path: PathBuf },
    #[error("{operation} image asset '{path}': {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("image asset '{requested}' now resolves to '{actual}', expected '{expected}'; replan")]
    ResolutionChanged {
        requested: PathBuf,
        expected: PathBuf,
        actual: PathBuf,
    },
    #[error("image asset '{path}' changed since capture; replan")]
    ContentChanged { path: PathBuf },
}

/// Exact imported bytes and the path resolution reviewed with them.
///
/// The saved-plan layer must include these bytes in its payload and bind the
/// paths/hash into its manifest. Verifying this one input is not a transaction
/// or a substitute for verifying the complete source/document input set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImageAsset {
    requested_path: PathBuf,
    canonical_path: PathBuf,
    content_hash: [u8; 32],
    bytes: Vec<u8>,
}

impl CapturedImageAsset {
    pub fn capture(source: &ImageAssetSource) -> Result<Self, ImageAssetError> {
        let origin =
            source
                .declaring_source
                .as_ref()
                .ok_or_else(|| ImageAssetError::MissingOrigin {
                    asset: source.path.clone(),
                })?;
        if !origin.is_absolute() {
            return Err(ImageAssetError::RelativeOrigin {
                asset: source.path.clone(),
                origin: origin.clone(),
            });
        }
        let path = Path::new(&source.path);
        if path.extension().and_then(|extension| extension.to_str()) != Some("altium-image") {
            return Err(ImageAssetError::InvalidPath {
                path: path.to_owned(),
            });
        }
        let parent = origin
            .parent()
            .ok_or_else(|| ImageAssetError::RelativeOrigin {
                asset: source.path.clone(),
                origin: origin.clone(),
            })?;
        let requested_path = parent.join(path);
        let canonical_path = canonicalize(&requested_path)?;
        let bytes = read(&canonical_path)?;
        let content_hash = *blake3::hash(&bytes).as_bytes();
        let captured = Self {
            requested_path,
            canonical_path,
            content_hash,
            bytes,
        };
        // Catch path retargeting/content changes during the capture itself.
        captured.verify_unchanged()?;
        Ok(captured)
    }

    pub fn requested_path(&self) -> &Path {
        &self.requested_path
    }
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
    pub fn content_hash(&self) -> &[u8; 32] {
        &self.content_hash
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Check the input precondition without replacing the reviewed bytes.
    pub fn verify_unchanged(&self) -> Result<(), ImageAssetError> {
        let actual = canonicalize(&self.requested_path)?;
        if actual != self.canonical_path {
            return Err(ImageAssetError::ResolutionChanged {
                requested: self.requested_path.clone(),
                expected: self.canonical_path.clone(),
                actual,
            });
        }
        let bytes = read(&actual)?;
        if blake3::hash(&bytes).as_bytes() != &self.content_hash {
            return Err(ImageAssetError::ContentChanged { path: actual });
        }
        Ok(())
    }
}

fn canonicalize(path: &Path) -> Result<PathBuf, ImageAssetError> {
    std::fs::canonicalize(path).map_err(|source| ImageAssetError::Io {
        operation: "resolving",
        path: path.to_owned(),
        source,
    })
}

fn read(path: &Path) -> Result<Vec<u8>, ImageAssetError> {
    std::fs::read(path).map_err(|source| ImageAssetError::Io {
        operation: "reading",
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(dir: &Path) -> ImageAssetSource {
        ImageAssetSource {
            path: "logo.altium-image".into(),
            declaring_source: Some(dir.join("library.schlib-spec")),
        }
    }

    #[test]
    fn capture_preserves_bytes_and_detects_changes_without_refreshing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("logo.altium-image");
        let original = b"complete encoded image bundle, not decoded pixels";
        std::fs::write(&path, original).unwrap();
        let captured = CapturedImageAsset::capture(&source(temp.path())).unwrap();
        assert_eq!(captured.bytes(), original);
        captured.verify_unchanged().unwrap();
        std::fs::write(&path, b"edited bundle").unwrap();
        assert!(matches!(
            captured.verify_unchanged(),
            Err(ImageAssetError::ContentChanged { .. })
        ));
        assert_eq!(
            captured.bytes(),
            original,
            "verification must not refresh reviewed bytes"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            captured.verify_unchanged(),
            Err(ImageAssetError::Io { .. })
        ));
        assert_eq!(captured.bytes(), original);
    }

    #[test]
    fn requires_explicit_origin_and_bundle_extension() {
        let mut input = ImageAssetSource {
            path: "logo.altium-image".into(),
            declaring_source: None,
        };
        assert!(matches!(
            CapturedImageAsset::capture(&input),
            Err(ImageAssetError::MissingOrigin { .. })
        ));
        input.declaring_source = Some("relative/library.schlib-spec".into());
        assert!(matches!(
            CapturedImageAsset::capture(&input),
            Err(ImageAssetError::RelativeOrigin { .. })
        ));
        let temp = tempfile::tempdir().unwrap();
        input.declaring_source = Some(temp.path().join("library.schlib-spec"));
        input.path = "logo.png".into();
        assert!(matches!(
            CapturedImageAsset::capture(&input),
            Err(ImageAssetError::InvalidPath { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn retargeted_symlink_invalidates_even_identical_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.altium-image");
        let second = temp.path().join("second.altium-image");
        let alias = temp.path().join("logo.altium-image");
        std::fs::write(&first, b"identical bytes").unwrap();
        std::fs::write(&second, b"identical bytes").unwrap();
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let captured = CapturedImageAsset::capture(&source(temp.path())).unwrap();
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        assert!(matches!(
            captured.verify_unchanged(),
            Err(ImageAssetError::ResolutionChanged { .. })
        ));
    }
}
