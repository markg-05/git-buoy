//! Optional remote-hosting observation.
//!
//! The core Git collector never calls a network service. Hosting integrations
//! live in this separate layer and return plain snapshots that the application
//! can combine with the local harbor scene.

mod github;
mod model;

pub use github::collect_github;
pub use model::{
    Check, CheckState, HostingSnapshot, HostingSurvey, MergeState, PullRequest, Release,
    ReviewState,
};
