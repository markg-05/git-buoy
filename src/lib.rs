//! Git Buoy: a living terminal harbor for one Git repository.
//!
//! The crate is layered so each concern can change or be tested alone.
//! [`git`] reads repository state into a plain [`git::RepoSnapshot`], keeping
//! every libgit2 detail behind that boundary.

pub mod git;
