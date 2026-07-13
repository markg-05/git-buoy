//! The harbor scene model.
//!
//! Pure data describing what should be on screen, plus the mapping from a
//! [`crate::git::RepoSnapshot`] to that data. Nothing here knows about git2
//! or ratatui, which keeps the state-to-scene logic testable on its own.

mod animation;
mod mapping;
mod model;

pub use animation::Animation;
pub use mapping::to_harbor;
pub use model::{Condition, Dock, DockKind, Harbor, Vessel};
