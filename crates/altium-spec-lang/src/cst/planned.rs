//! Persistable structured source edits (NEXT.md step-3 saved-plan contract:
//! "for existing source save structured edits whose locators are bound to
//! the source hash").
//!
//! A [`PlannedSpecEdit`] is the serializable, versioned mirror of one
//! [`SpecEdit`]. Its locator is a [`PlannedLocator`]: the child-*node* index
//! path of the target (the [`SourceId`] path without the process-local
//! snapshot counter, which is not persistable) plus the node's expected kind
//! and exact source text. The binding to a specific tree is external — the
//! saved plan records the source file's content hash and decode runs only
//! against a hash-verified text — so the path is deterministic; the recorded
//! kind and text are defense in depth that reject a stale or tampered plan
//! hard instead of retargeting a different node.
//!
//! Validated fragments serialize as their source text and are re-validated
//! on decode through the same probe-parsing constructors the in-memory API
//! uses ([`PropertyKey::new`], [`ExprSource::parse`], and the intent-block
//! single-declaration parse), so a tampered plan cannot smuggle an
//! unparseable fragment. The decode→[`apply_edits`] round trip preserves
//! CRLF/LF style, comments, authored expressions, and annotation IDs exactly
//! as the in-memory edits do, because decode reconstructs semantically
//! identical [`SpecEdit`] values and the byte-preserving application layer
//! is shared.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::access::{BindingMetadata, BlockKind, SourceId, SpecTree};
use super::edit::{
    ExprSource, IntentBlock, PropertyKey, SpecEdit, StructuredEditError, apply_edits,
};

/// Exact supported serialization version of [`PlannedSpecEdit`]. Bumped on
/// any change to edit semantics, locator form, or fragment validation; a
/// consumer holding a different version must reject the payload with a
/// regenerate message.
pub const SPEC_EDIT_FORMAT_VERSION: u32 = 1;

/// The expected node kind a locator must resolve to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlannedNodeKind {
    /// The source-file root (insertion parents only).
    Root,
    /// A typed declaration block.
    Block(BlockKind),
}

/// A rebindable locator: node path + expected kind + expected exact text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedLocator {
    /// Child-node index path from the root (tokens/trivia excluded), i.e.
    /// the [`SourceId`] path without its process-local snapshot counter.
    pub node_path: Vec<u32>,
    pub expected_kind: PlannedNodeKind,
    /// The exact source text of the target node at planning time (empty for
    /// the root). Re-validated on rebinding.
    pub expected_text: String,
}

impl PlannedLocator {
    /// Human-readable path for error context.
    pub fn path_display(&self) -> String {
        if self.node_path.is_empty() {
            "root".to_owned()
        } else {
            self.node_path
                .iter()
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>()
                .join("/")
        }
    }
}

/// Tri-state trailing-comment replacement of a block-replacement fragment
/// (an explicit encoding of `Option<Option<String>>`, which JSON cannot
/// represent losslessly through nested nulls).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlannedTrailingComment {
    /// The replacement does not address the trailing comment.
    Untouched,
    /// The replacement removes any trailing comment.
    Clear,
    /// The replacement sets the trailing comment to exactly this text.
    Set(String),
}

/// Serialized state of one validated intent-block fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedIntentBlock {
    pub text: String,
    /// Whether this insertion is the replacement half of a
    /// delete-then-insert pair (the merge machinery's in-place block
    /// replacement form).
    pub replacement: bool,
    /// A name-only replacement's new name source.
    pub replacement_name: Option<String>,
    pub trailing_comment: PlannedTrailingComment,
}

/// Serialized mirror of [`BindingMetadata`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedBinding {
    pub id: Option<String>,
    pub stable: Option<bool>,
    pub group: Option<String>,
    pub source_id: Option<String>,
}

impl PlannedBinding {
    fn from_metadata(binding: &BindingMetadata) -> Self {
        Self {
            id: binding.id.clone(),
            stable: binding.stable,
            group: binding.group.clone(),
            source_id: binding.source_id.clone(),
        }
    }

    fn to_metadata(&self) -> BindingMetadata {
        BindingMetadata {
            id: self.id.clone(),
            stable: self.stable,
            group: self.group.clone(),
            source_id: self.source_id.clone(),
        }
    }
}

/// One persistable structured edit (the serializable mirror of [`SpecEdit`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlannedSpecEdit {
    InsertBlock {
        parent: PlannedLocator,
        block: PlannedIntentBlock,
    },
    DeleteBlock {
        target: PlannedLocator,
    },
    SetProperty {
        target: PlannedLocator,
        key: String,
        value: String,
    },
    RemoveProperty {
        target: PlannedLocator,
        key: String,
    },
    SetAnnotation {
        target: PlannedLocator,
        binding: PlannedBinding,
    },
}

#[derive(Debug, Error)]
pub enum PlannedEditError {
    #[error("edit #{index}: source id {id} does not belong to the encoded tree")]
    ForeignSourceId { index: usize, id: String },
    #[error("edit #{index}: locator {path} does not resolve to a node in this source; replan")]
    UnresolvedLocator { index: usize, path: String },
    #[error(
        "edit #{index}: locator {path} resolves to a {found} node, expected {expected}; replan"
    )]
    LocatorKindMismatch {
        index: usize,
        path: String,
        expected: String,
        found: String,
    },
    #[error(
        "edit #{index}: locator {path} resolves to different source text than recorded; \
         the source no longer matches the plan — replan"
    )]
    LocatorTextMismatch { index: usize, path: String },
    #[error("edit #{index}: {source}")]
    Fragment {
        index: usize,
        #[source]
        source: StructuredEditError,
    },
}

fn kind_display(kind: PlannedNodeKind) -> String {
    match kind {
        PlannedNodeKind::Root => "root".to_owned(),
        PlannedNodeKind::Block(block) => format!("{block:?}"),
    }
}

/// Encodes a locator for one live [`SourceId`] of `tree`.
fn encode_locator(
    tree: &SpecTree,
    id: &SourceId,
    index: usize,
) -> Result<PlannedLocator, PlannedEditError> {
    if id.is_root() {
        return Ok(PlannedLocator {
            node_path: Vec::new(),
            expected_kind: PlannedNodeKind::Root,
            expected_text: String::new(),
        });
    }
    let block = tree
        .block(id)
        .ok_or_else(|| PlannedEditError::ForeignSourceId {
            index,
            id: id.to_string(),
        })?;
    Ok(PlannedLocator {
        node_path: id.segments().to_vec(),
        expected_kind: PlannedNodeKind::Block(block.kind()),
        expected_text: block.text().to_owned(),
    })
}

/// Rebinds a locator against `tree`, verifying kind and exact text.
fn decode_locator(
    tree: &SpecTree,
    locator: &PlannedLocator,
    index: usize,
) -> Result<SourceId, PlannedEditError> {
    let id = tree.source_id_at(&locator.node_path).ok_or_else(|| {
        PlannedEditError::UnresolvedLocator {
            index,
            path: locator.path_display(),
        }
    })?;
    match locator.expected_kind {
        PlannedNodeKind::Root => {
            if !locator.node_path.is_empty() {
                return Err(PlannedEditError::LocatorKindMismatch {
                    index,
                    path: locator.path_display(),
                    expected: "root".to_owned(),
                    found: "non-root path".to_owned(),
                });
            }
        }
        PlannedNodeKind::Block(expected) => {
            let Some(block) = tree.block(&id) else {
                return Err(PlannedEditError::LocatorKindMismatch {
                    index,
                    path: locator.path_display(),
                    expected: kind_display(locator.expected_kind),
                    found: "non-block node".to_owned(),
                });
            };
            if block.kind() != expected {
                return Err(PlannedEditError::LocatorKindMismatch {
                    index,
                    path: locator.path_display(),
                    expected: kind_display(locator.expected_kind),
                    found: format!("{:?}", block.kind()),
                });
            }
            if block.text() != locator.expected_text {
                return Err(PlannedEditError::LocatorTextMismatch {
                    index,
                    path: locator.path_display(),
                });
            }
        }
    }
    Ok(id)
}

/// Encodes typed edits produced against `tree` into their persistable form.
pub fn encode_spec_edits(
    tree: &SpecTree,
    edits: &[SpecEdit],
) -> Result<Vec<PlannedSpecEdit>, PlannedEditError> {
    edits
        .iter()
        .enumerate()
        .map(|(index, edit)| {
            Ok(match edit {
                SpecEdit::InsertBlock { parent, block } => {
                    let (text, replacement, replacement_name, trailing) = block.planned_parts();
                    PlannedSpecEdit::InsertBlock {
                        parent: encode_locator(tree, parent, index)?,
                        block: PlannedIntentBlock {
                            text: text.to_owned(),
                            replacement,
                            replacement_name: replacement_name.map(str::to_owned),
                            trailing_comment: match trailing {
                                None => PlannedTrailingComment::Untouched,
                                Some(None) => PlannedTrailingComment::Clear,
                                Some(Some(comment)) => {
                                    PlannedTrailingComment::Set(comment.to_owned())
                                }
                            },
                        },
                    }
                }
                SpecEdit::DeleteBlock { id } => PlannedSpecEdit::DeleteBlock {
                    target: encode_locator(tree, id, index)?,
                },
                SpecEdit::SetProperty { id, key, value } => PlannedSpecEdit::SetProperty {
                    target: encode_locator(tree, id, index)?,
                    key: key.as_str().to_owned(),
                    value: value.as_str().to_owned(),
                },
                SpecEdit::RemoveProperty { id, key } => PlannedSpecEdit::RemoveProperty {
                    target: encode_locator(tree, id, index)?,
                    key: key.as_str().to_owned(),
                },
                SpecEdit::SetAnnotation { id, binding } => PlannedSpecEdit::SetAnnotation {
                    target: encode_locator(tree, id, index)?,
                    binding: PlannedBinding::from_metadata(binding),
                },
            })
        })
        .collect()
}

/// Decodes persisted edits against a freshly parsed `tree` whose text the
/// caller has already hash-verified: rebinds every locator (kind and exact
/// text re-checked) and re-validates every fragment through the probe
/// parsers. The returned edits feed the byte-preserving [`apply_edits`].
pub fn decode_spec_edits(
    tree: &SpecTree,
    planned: &[PlannedSpecEdit],
) -> Result<Vec<SpecEdit>, PlannedEditError> {
    let fragment = |index: usize| {
        move |source: StructuredEditError| PlannedEditError::Fragment { index, source }
    };
    planned
        .iter()
        .enumerate()
        .map(|(index, edit)| {
            Ok(match edit {
                PlannedSpecEdit::InsertBlock { parent, block } => SpecEdit::InsertBlock {
                    parent: decode_locator(tree, parent, index)?,
                    block: IntentBlock::from_planned_parts(
                        block.text.clone(),
                        block.replacement,
                        block.replacement_name.clone(),
                        match &block.trailing_comment {
                            PlannedTrailingComment::Untouched => None,
                            PlannedTrailingComment::Clear => Some(None),
                            PlannedTrailingComment::Set(comment) => Some(Some(comment.clone())),
                        },
                    )
                    .map_err(fragment(index))?,
                },
                PlannedSpecEdit::DeleteBlock { target } => SpecEdit::DeleteBlock {
                    id: decode_locator(tree, target, index)?,
                },
                PlannedSpecEdit::SetProperty { target, key, value } => SpecEdit::SetProperty {
                    id: decode_locator(tree, target, index)?,
                    key: PropertyKey::new(key.clone()).map_err(fragment(index))?,
                    value: ExprSource::parse(value.clone()).map_err(fragment(index))?,
                },
                PlannedSpecEdit::RemoveProperty { target, key } => SpecEdit::RemoveProperty {
                    id: decode_locator(tree, target, index)?,
                    key: PropertyKey::new(key.clone()).map_err(fragment(index))?,
                },
                PlannedSpecEdit::SetAnnotation { target, binding } => SpecEdit::SetAnnotation {
                    id: decode_locator(tree, target, index)?,
                    binding: binding.to_metadata(),
                },
            })
        })
        .collect()
}

/// Convenience for tests and callers proving the round-trip law:
/// `apply_edits(tree, decode(encode(edits)))` byte-equals
/// `apply_edits(tree, edits)`.
pub fn apply_planned_edits(
    tree: &SpecTree,
    planned: &[PlannedSpecEdit],
) -> Result<SpecTree, StructuredEditError> {
    let edits = decode_spec_edits(tree, planned)
        .map_err(|error| StructuredEditError::InvalidPlannedFragment(error.to_string()))?;
    apply_edits(tree, &edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::merge::plan_merge_dump;

    fn roundtrip(existing: &str, fresh: &str) -> (String, String) {
        let existing_tree = SpecTree::parse(existing.to_owned()).unwrap();
        let fresh_tree = SpecTree::parse(fresh.to_owned()).unwrap();
        let edits = plan_merge_dump(&existing_tree, &fresh_tree).unwrap();
        let direct = apply_edits(&existing_tree, &edits)
            .unwrap()
            .source()
            .to_owned();

        // Serialize, decode against a FRESH parse of the same text (a new
        // snapshot, as saved-plan apply does), and re-apply.
        let planned = encode_spec_edits(&existing_tree, &edits).unwrap();
        let json = serde_json::to_string(&planned).unwrap();
        let decoded: Vec<PlannedSpecEdit> = serde_json::from_str(&json).unwrap();
        let reparsed = SpecTree::parse(existing.to_owned()).unwrap();
        let replayed = apply_planned_edits(&reparsed, &decoded)
            .unwrap()
            .source()
            .to_owned();
        (direct, replayed)
    }

    #[test]
    fn planned_edits_replay_byte_identical_to_direct_merge_with_crlf_and_comments() {
        let existing = "// header comment\r\n#[annotation(id = \"KEEP0001\")]\r\ncomponent R {\r\n    description : \"old\" // keep me\r\n    value: \"10k\"\r\n    pin 1 { electrical: passive }\r\n}\r\n";
        let fresh = "#[annotation(id = \"FRESH999\")]\ncomponent R {\n    description: \"new\"\n    value: \"10k\"\n    pin 1 { electrical: input }\n}\ncomponent ADDED {\n    value: \"1n\"\n}\n";
        let (direct, replayed) = roundtrip(existing, fresh);
        assert_eq!(direct, replayed);
        assert!(direct.contains("// header comment\r\n"));
        assert!(direct.contains("description : \"new\" // keep me"));
        assert!(direct.contains("KEEP0001"));
        assert!(direct.contains("component ADDED"));
    }

    #[test]
    fn planned_edits_replay_deletions_and_annotation_changes() {
        let existing = "component GONE { value: \"x\" }\n#[annotation(id = \"OLD00001\", source_id = \"U1\")]\ncomponent KEEP { a: 1, b: 2 }\n";
        let fresh =
            "#[annotation(id = \"NEW00001\", source_id = \"U2\")]\ncomponent KEEP { a: 3 }\n";
        let (direct, replayed) = roundtrip(existing, fresh);
        assert_eq!(direct, replayed);
        assert!(!direct.contains("GONE"));
        assert!(direct.contains("a: 3"));
        assert!(!direct.contains("b: 2"));
    }

    #[test]
    fn stale_or_tampered_locators_are_rejected_hard() {
        let existing =
            SpecTree::parse("component A { x: 1 }\ncomponent B { x: 2 }\n".to_owned()).unwrap();
        let fresh =
            SpecTree::parse("component A { x: 9 }\ncomponent B { x: 2 }\n".to_owned()).unwrap();
        let edits = plan_merge_dump(&existing, &fresh).unwrap();
        let planned = encode_spec_edits(&existing, &edits).unwrap();
        assert!(!planned.is_empty());

        // One changed byte in the source: the locator text no longer matches.
        let drifted =
            SpecTree::parse("component A { x: 7 }\ncomponent B { x: 2 }\n".to_owned()).unwrap();
        let error = decode_spec_edits(&drifted, &planned).unwrap_err();
        assert!(
            matches!(error, PlannedEditError::LocatorTextMismatch { .. }),
            "{error}"
        );

        // A path off the end of the tree.
        let mut off = planned.clone();
        if let PlannedSpecEdit::SetProperty { target, .. } = &mut off[0] {
            target.node_path = vec![99];
        } else {
            panic!("expected a SetProperty edit, got {:?}", off[0]);
        }
        let error = decode_spec_edits(&existing, &off).unwrap_err();
        assert!(
            matches!(error, PlannedEditError::UnresolvedLocator { .. }),
            "{error}"
        );

        // A kind mismatch: point the block locator at the root.
        let mut wrong_kind = planned.clone();
        if let PlannedSpecEdit::SetProperty { target, .. } = &mut wrong_kind[0] {
            target.node_path = Vec::new();
        }
        let error = decode_spec_edits(&existing, &wrong_kind).unwrap_err();
        assert!(
            matches!(error, PlannedEditError::LocatorKindMismatch { .. }),
            "{error}"
        );
    }

    #[test]
    fn tampered_fragments_cannot_smuggle_unparseable_or_multi_block_text() {
        let tree = SpecTree::parse("component A {}\n".to_owned()).unwrap();
        for text in [
            "component X {",
            "component X {}\ncomponent Y {}",
            "} component Z {",
        ] {
            let planned = vec![PlannedSpecEdit::InsertBlock {
                parent: PlannedLocator {
                    node_path: Vec::new(),
                    expected_kind: PlannedNodeKind::Root,
                    expected_text: String::new(),
                },
                block: PlannedIntentBlock {
                    text: text.to_owned(),
                    replacement: false,
                    replacement_name: None,
                    trailing_comment: PlannedTrailingComment::Untouched,
                },
            }];
            let error = decode_spec_edits(&tree, &planned).unwrap_err();
            assert!(matches!(error, PlannedEditError::Fragment { .. }), "{text}");
        }

        // Nested fragments (component children) remain decodable.
        let planned = vec![PlannedSpecEdit::InsertBlock {
            parent: PlannedLocator {
                node_path: tree.top_level_blocks()[0].id().segments().to_vec(),
                expected_kind: PlannedNodeKind::Block(BlockKind::Component),
                expected_text: "component A {}".to_owned(),
            },
            block: PlannedIntentBlock {
                text: "pin 1 { electrical: passive }".to_owned(),
                replacement: false,
                replacement_name: None,
                trailing_comment: PlannedTrailingComment::Untouched,
            },
        }];
        let edited = apply_planned_edits(&tree, &planned).unwrap();
        assert!(edited.source().contains("pin 1 { electrical: passive }"));
    }
}
