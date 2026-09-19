//! # altium-sync
//!
//! The directional plan/apply layer of the Altium spec toolchain. It projects
//! authored intent (via [`altium_spec_lang`]) and typed Altium documents (via
//! [`altium_format`]) into snapshots, then produces self-contained typed patches.
//! Addresses are collection indices bound to exact input hashes. Apply executes
//! the reviewed patches; display heuristics cannot change execution targets.
//!
//! No persistent sync state or embedded binding metadata is required. Explicit
//! binary design artifacts, saved plans, and temporary recovery files are supported
//! workflow concepts, not a database of cross-artifact object identities.
//!
//! This crate is being populated as part of the clean-slate plan/apply rebuild
//! (see `NEXT.md`). It depends on both `altium-format` and `altium-spec-lang`;
//! neither depends on it.
//!
//! Planned modules (see `NEXT.md`):
//! - `snapshot` — per-domain projection and intent elaboration
//! - `plan` — versioned patches, input preconditions, and typed before/after state
//! - `planner` — directional diff and managed-scope validation
//! - `apply` — checked execution, staging, reopen validation, and journal recovery
//! - `render` — exact patch reporting and optional display-only pairing/grouping

pub mod address;
pub mod apply;
pub mod image_asset;
pub mod plan;
pub mod planner;
pub mod report;
pub mod schlib;

pub(crate) mod collection;
