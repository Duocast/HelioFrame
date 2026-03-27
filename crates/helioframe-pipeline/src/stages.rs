#[derive(Debug, Clone)]
pub struct PipelineStage {
    pub name: &'static str,
    pub description: &'static str,
}

pub use helioframe_core::{SceneBoundary, TemporalWindow};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShotDetectionArtifact {
    pub threshold: f64,
    pub frame_count: usize,
    pub boundaries: Vec<SceneBoundary>,
}
