use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityObjective {
    TemporalConsistency,
    StructuralFidelity,
    PerceptualDetail,
    ArtifactSuppression,
    Throughput,
}

/// Categories of high-frequency content eligible for selective detail refinement.
///
/// Rather than applying refinement uniformly (which risks temporal sparkle on
/// smooth regions), the refiner targets only patches dominated by these
/// content categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetailCategory {
    /// Rendered text, signage, subtitles, UI overlays.
    Text,
    /// Hair strands, fur, fine filaments.
    Hair,
    /// Woven patterns, clothing folds, stitching.
    Fabric,
    /// Leaves, grass blades, bark, canopy edges.
    Foliage,
    /// Building facades, window grids, brickwork, roof tiles.
    Architecture,
}

impl DetailCategory {
    /// The full set of categories recommended for studio-quality refinement.
    pub fn studio_defaults() -> Vec<DetailCategory> {
        vec![
            DetailCategory::Text,
            DetailCategory::Hair,
            DetailCategory::Fabric,
            DetailCategory::Foliage,
            DetailCategory::Architecture,
        ]
    }
}

/// Thresholds that guard against temporal sparkle introduced by the detail
/// refinement pass.  If a refined window exceeds these limits the refiner
/// should fall back to the pre-refinement frames for that window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SparkleGuardThresholds {
    /// Maximum allowed frame-to-frame high-frequency energy variance.
    /// Values above this indicate the refiner is hallucinating flickering
    /// detail that was not present in the source.
    pub max_hf_flicker: f64,
    /// Maximum allowed per-patch temporal gradient magnitude.  Catches
    /// localised shimmer on edges and textures.
    pub max_patch_shimmer: f64,
}

impl Default for SparkleGuardThresholds {
    fn default() -> Self {
        Self {
            max_hf_flicker: 0.12,
            max_patch_shimmer: 0.08,
        }
    }
}

/// Policy controlling the second-stage detail refinement pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailRefinementPolicy {
    /// Whether to run the refinement stage at all.
    pub enabled: bool,
    /// Content categories eligible for selective refinement.  Patches that
    /// do not match any of these categories are left untouched.
    pub categories: Vec<DetailCategory>,
    /// Minimum high-frequency energy (0.0–1.0) a patch must exhibit before
    /// the refiner considers it a candidate.  Keeps the refiner away from
    /// smooth gradients and sky regions.
    pub hf_energy_threshold: f64,
    /// Temporal sparkle guard — if refined output exceeds these limits the
    /// window is rolled back to pre-refinement frames.
    pub sparkle_guard: SparkleGuardThresholds,
    /// Number of refinement diffusion steps (fewer than the main restore
    /// pass; typically 4–8).
    pub refinement_steps: usize,
    /// Strength multiplier applied to the refinement model.  Lower values
    /// produce subtler enhancement; higher values recover more detail but
    /// increase sparkle risk.
    pub refinement_strength: f64,
}

impl Default for DetailRefinementPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            categories: DetailCategory::studio_defaults(),
            hf_energy_threshold: 0.25,
            sparkle_guard: SparkleGuardThresholds::default(),
            refinement_steps: 6,
            refinement_strength: 0.4,
        }
    }
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
    pub detail_refinement: DetailRefinementPolicy,
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
            detail_refinement: DetailRefinementPolicy::default(),
            temporal_qc: TemporalQcPolicy::default(),
        }
    }
}
