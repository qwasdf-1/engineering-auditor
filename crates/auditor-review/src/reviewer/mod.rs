//! The engineering reviewer's view of a scanned repository: an orientation
//! pack given up front, and retrieval tools for the rest. Both read only the
//! report, the repository map, and hash-checked mapped source, and every row
//! they render cites an evidence ID the host issued.

mod document;
mod modules;
mod orientation;
mod retrieval;
mod retrieval_render;
mod view;

pub use orientation::{OrientationPack, MINIMUM_ORIENTATION_BYTES, TARGET_ORIENTATION_BYTES};
pub use retrieval::{
    retrieval_tool_specs, RetrievalBudget, RetrievalHost, RetrievalOutcome, RetrievalRecord,
    RETRIEVAL_TOOL_NAMES,
};
pub use view::RepositoryView;
