//! Git Buoy: a living terminal harbor for one Git repository.
//!
//! The crate is layered so each concern can change or be tested alone:
//!
//! - [`git`] reads repository state into a plain [`git::RepoSnapshot`].
//! - [`harbor`] maps a snapshot to the pure scene model, with no Git or
//!   terminal types involved.

pub mod git;
pub mod harbor;
