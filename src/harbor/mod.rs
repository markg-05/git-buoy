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
pub(crate) use mapping::to_harbor_with_activity;
pub use model::{
    CargoCounts, CargoItem, CargoKind, Clearance, Condition, Convoy, Dock, DockEvent, DockKind,
    DockTransition, DockTransitionKind, EventKind, Harbor, Inspection, InspectionStatus,
    LandingStatus, ReviewStatus, Vessel, VesselActivity,
};
