#[derive(Debug, Clone)]
pub struct PipelineStage {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneBoundary {
    pub frame_index: usize,
    pub timestamp_seconds: f64,
    pub score: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemporalWindow {
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShotDetectionArtifact {
    pub threshold: f64,
    pub frame_count: usize,
    pub boundaries: Vec<SceneBoundary>,
    pub windows: Vec<TemporalWindow>,
}
