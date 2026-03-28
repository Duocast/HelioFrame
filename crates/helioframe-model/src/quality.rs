use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityObjective {
    TemporalConsistency,
    StructuralFidelity,
    PerceptualDetail,
    ArtifactSuppression,
    Throughput,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TemporalQcThresholds {
    pub max_flicker_score: f64,
    pub max_ghosting_score: f64,
    pub max_instability_score: f64,
}

impl Default for TemporalQcThresholds {
    fn default() -> Self {
        Self {
            max_flicker_score: 0.55,
            max_ghosting_score: 0.52,
            max_instability_score: 0.56,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum RerunPolicy {
    Disabled,
    FailedWindows { max_attempts: usize },
}

impl Default for RerunPolicy {
    fn default() -> Self {
        Self::FailedWindows { max_attempts: 1 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalQcPolicy {
    pub enabled: bool,
    pub reject_if_unstable: bool,
    pub thresholds: TemporalQcThresholds,
    pub rerun_policy: Option<RerunPolicy>,
}

impl Default for TemporalQcPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            reject_if_unstable: true,
            thresholds: TemporalQcThresholds::default(),
            rerun_policy: Some(RerunPolicy::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityPolicy {
    pub prioritize: Vec<QualityObjective>,
    pub reject_if_temporal_regresses: bool,
    pub require_detail_refinement: bool,
    pub temporal_qc: TemporalQcPolicy,
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
            temporal_qc: TemporalQcPolicy::default(),
        }
    }
}
