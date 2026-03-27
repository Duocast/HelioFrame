#[derive(Debug, Clone, Copy)]
pub enum QualityObjective {
    TemporalConsistency,
    StructuralFidelity,
    PerceptualDetail,
    ArtifactSuppression,
    Throughput,
}

#[derive(Debug, Clone)]
pub struct QualityPolicy {
    pub prioritize: Vec<QualityObjective>,
    pub reject_if_temporal_regresses: bool,
    pub require_detail_refinement: bool,
}

impl Default for QualityPolicy {
    fn default() -> Self {
        Self {
            prioritize: vec![
                QualityObjective::TemporalConsistency,
                QualityObjective::StructuralFidelity,
                QualityObjective::PerceptualDetail,
                QualityObjective::ArtifactSuppression,
            ],
            reject_if_temporal_regresses: true,
            require_detail_refinement: true,
        }
    }
}
