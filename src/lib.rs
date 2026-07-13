//! Git Buoy: a living terminal harbor for one Git repository.
//!
//! The crate is layered so each concern can change or be tested alone:
//!
//! - [`git`] reads repository state into a plain [`git::RepoSnapshot`].
//! - [`harbor`] maps a snapshot to the pure scene model, with no Git or
//!   terminal types involved.
//! - [`ui`] renders the scene with ratatui.
//! - [`app`] holds the state machine connecting them: mode, selection,
//!   animation clock, and message handling.

pub mod app;
pub mod git;
pub mod harbor;
pub mod hosting;
pub mod ui;
