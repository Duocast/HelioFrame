pub mod backend;
pub mod onnx;
pub mod plan;
pub mod quality;
pub mod traits;
pub mod worker;

pub use backend::{BackendCapabilities, BackendRegistry};
pub use plan::{ExecutionHints, InferencePlan};
pub use quality::{
    DetailCategory, DetailRefinementPolicy, QualityObjective, QualityPolicy, RerunPolicy,
    SparkleGuardThresholds, TemporalQcPolicy, TemporalQcThresholds,
};
pub use traits::InferenceBackend;
pub use worker::{python_exe, WorkerAdapter, WorkerLaunchConfig, WorkerOutputCallback, WorkerRunResult};
