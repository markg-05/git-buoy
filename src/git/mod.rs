//! Repository state collection.
//!
//! Everything Git-specific stays behind this module. The rest of the
//! application only sees [`RepoSnapshot`] and its plain data types.

mod collector;
mod snapshot;

pub use collector::{collect, discover_root};
pub use snapshot::{
    BranchInfo, ChangeCounts, ChangeFile, ChangeKind, HeadState, Operation, RepoSnapshot,
    SyncState, TipAction, TipInfo, Workspace,
};
