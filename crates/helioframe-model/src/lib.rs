pub mod backend;
pub mod plan;
pub mod quality;
pub mod traits;
pub mod worker;

pub use backend::{BackendCapabilities, BackendRegistry};
pub use plan::{ExecutionHints, InferencePlan};
pub use traits::InferenceBackend;
pub use worker::{WorkerAdapter, WorkerLaunchConfig, WorkerRunResult};
