use crate::{
    AppConfig, BackendKind, HelioFrameError, HelioFrameResult, TemporalWindow, UpscalePreset,
    WindowTileManifest,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunProbeInfo {
    pub container: String,
    pub assumed_resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTiming {
    pub stage: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalQcSummary {
    pub total_windows: usize,
    pub unstable_windows: usize,
    pub rerun_scheduled_windows: usize,
    pub reject_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalQcWindowStatus {
    pub window_index: usize,
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub flicker_score: f64,
    pub ghosting_score: f64,
    pub instability_score: f64,
    pub unstable: bool,
    pub rerun_scheduled: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalQcManifest {
    pub summary: TemporalQcSummary,
    pub windows: Vec<TemporalQcWindowStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    pub input: String,
    pub output: String,
    pub preset: UpscalePreset,
    pub backend: BackendKind,
    pub probe: RunProbeInfo,
    pub windows: Vec<TemporalWindow>,
    pub window_tiles: Vec<WindowTileManifest>,
    pub temporal_qc: Option<TemporalQcManifest>,
    pub stage_timings: Vec<StageTiming>,
    #[serde(default)]
    pub completed_stages: Vec<String>,
    #[serde(default)]
    pub completed_windows: Vec<usize>,
}

impl RunManifest {
    pub fn new(run_id: String, config: &AppConfig, probe: RunProbeInfo) -> Self {
        Self {
            run_id,
            input: config.input.clone(),
            output: config.output.clone(),
            preset: config.preset,
            backend: config.backend,
            probe,
            windows: Vec::new(),
            window_tiles: Vec::new(),
            temporal_qc: None,
            stage_timings: vec![StageTiming {
                stage: "preset".into(),
                elapsed_ms: 0,
            }],
            completed_stages: Vec::new(),
            completed_windows: Vec::new(),
        }
    }

    pub fn set_windows(&mut self, windows: Vec<TemporalWindow>) {
        self.windows = windows;
    }

    pub fn set_window_tiles(&mut self, window_tiles: Vec<WindowTileManifest>) {
        self.window_tiles = window_tiles;
    }

    pub fn set_temporal_qc(&mut self, temporal_qc: TemporalQcManifest) {
        self.temporal_qc = Some(temporal_qc);
    }

    pub fn append_stage_timing(&mut self, stage: impl Into<String>, elapsed: Duration) {
        self.stage_timings.push(StageTiming {
            stage: stage.into(),
            elapsed_ms: elapsed.as_millis(),
        });
    }

    pub fn mark_stage_completed(&mut self, stage: &str) {
        let name = stage.to_string();
        if !self.completed_stages.contains(&name) {
            self.completed_stages.push(name);
        }
    }

    pub fn is_stage_completed(&self, stage: &str) -> bool {
        self.completed_stages.iter().any(|s| s == stage)
    }

    pub fn mark_window_completed(&mut self, window_index: usize) {
        if !self.completed_windows.contains(&window_index) {
            self.completed_windows.push(window_index);
        }
    }

    pub fn is_window_completed(&self, window_index: usize) -> bool {
        self.completed_windows.contains(&window_index)
    }

    pub fn pending_window_indices(&self) -> Vec<usize> {
        (0..self.windows.len())
            .filter(|idx| !self.completed_windows.contains(idx))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct RunLayout {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub input_artifacts_dir: PathBuf,
    pub intermediate_artifacts_dir: PathBuf,
    pub output_artifacts_dir: PathBuf,
    pub manifest_path: PathBuf,
}

impl RunLayout {
    pub fn create(base_dir: impl AsRef<Path>) -> HelioFrameResult<Self> {
        let run_id = create_run_id();
        let run_dir = base_dir
            .as_ref()
            .join(".helioframe")
            .join("runs")
            .join(&run_id);
        let artifacts_dir = run_dir.join("artifacts");
        let input_artifacts_dir = artifacts_dir.join("input");
        let intermediate_artifacts_dir = artifacts_dir.join("intermediate");
        let output_artifacts_dir = artifacts_dir.join("output");

        fs::create_dir_all(&input_artifacts_dir).map_err(|err| {
            HelioFrameError::RunManifest(format!(
                "failed to create run artifact layout at {}: {err}",
                run_dir.display()
            ))
        })?;
        fs::create_dir_all(&intermediate_artifacts_dir).map_err(|err| {
            HelioFrameError::RunManifest(format!(
                "failed to create intermediate artifacts dir at {}: {err}",
                intermediate_artifacts_dir.display()
            ))
        })?;
        fs::create_dir_all(&output_artifacts_dir).map_err(|err| {
            HelioFrameError::RunManifest(format!(
                "failed to create output artifacts dir at {}: {err}",
                output_artifacts_dir.display()
            ))
        })?;

        let manifest_path = run_dir.join("manifest.json");

        Ok(Self {
            run_id,
            run_dir,
            artifacts_dir,
            input_artifacts_dir,
            intermediate_artifacts_dir,
            output_artifacts_dir,
            manifest_path,
        })
    }

    pub fn from_existing(run_dir: impl AsRef<Path>) -> HelioFrameResult<Self> {
        let run_dir = run_dir.as_ref().to_path_buf();
        if !run_dir.exists() {
            return Err(HelioFrameError::RunManifest(format!(
                "run directory does not exist: {}",
                run_dir.display()
            )));
        }
        let manifest_path = run_dir.join("manifest.json");
        if !manifest_path.exists() {
            return Err(HelioFrameError::RunManifest(format!(
                "manifest not found in run directory: {}",
                manifest_path.display()
            )));
        }
        let run_id = run_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let artifacts_dir = run_dir.join("artifacts");
        Ok(Self {
            run_id,
            run_dir: run_dir.clone(),
            artifacts_dir: artifacts_dir.clone(),
            input_artifacts_dir: artifacts_dir.join("input"),
            intermediate_artifacts_dir: artifacts_dir.join("intermediate"),
            output_artifacts_dir: artifacts_dir.join("output"),
            manifest_path,
        })
    }

    pub fn load_manifest(&self) -> HelioFrameResult<RunManifest> {
        let raw = fs::read_to_string(&self.manifest_path).map_err(|err| {
            HelioFrameError::RunManifest(format!(
                "failed to read manifest from {}: {err}",
                self.manifest_path.display()
            ))
        })?;
        serde_json::from_str(&raw).map_err(|err| {
            HelioFrameError::RunManifest(format!("failed to parse manifest: {err}"))
        })
    }

    pub fn write_manifest(&self, manifest: &RunManifest) -> HelioFrameResult<()> {
        let json = serde_json::to_string_pretty(manifest).map_err(|err| {
            HelioFrameError::RunManifest(format!("failed to serialize run manifest: {err}"))
        })?;

        fs::write(&self.manifest_path, json).map_err(|err| {
            HelioFrameError::RunManifest(format!(
                "failed to write manifest to {}: {err}",
                self.manifest_path.display()
            ))
        })?;

        Ok(())
    }
}

fn create_run_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let pid = std::process::id();
    format!("run-{}-{}", now.as_millis(), pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_layout_creates_manifest_and_artifact_dirs() {
        let temp = std::env::temp_dir().join(format!(
            "helioframe-manifest-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        let layout = RunLayout::create(&temp).expect("run layout should be created");

        assert!(layout.run_dir.exists());
        assert!(layout.artifacts_dir.exists());
        assert!(layout.input_artifacts_dir.exists());
        assert!(layout.intermediate_artifacts_dir.exists());
        assert!(layout.output_artifacts_dir.exists());

        fs::remove_dir_all(temp).expect("temp directory cleanup should succeed");
    }

    fn make_test_config() -> AppConfig {
        AppConfig {
            input: "input.mp4".into(),
            output: "output.mp4".into(),
            backend: BackendKind::StcditStudio,
            preset: UpscalePreset::Studio,
            target_resolution: crate::Resolution::UHD_4K,
        }
    }

    fn make_test_manifest(run_id: &str) -> RunManifest {
        let config = make_test_config();
        let probe = RunProbeInfo {
            container: "mp4".into(),
            assumed_resolution: "1920x1080".into(),
        };
        RunManifest::new(run_id.to_string(), &config, probe)
    }

    #[test]
    fn mark_stage_completed_tracks_stages() {
        let mut manifest = make_test_manifest("test-run-1");
        assert!(!manifest.is_stage_completed("probe"));

        manifest.mark_stage_completed("probe");
        assert!(manifest.is_stage_completed("probe"));
        assert!(!manifest.is_stage_completed("decode"));

        // Duplicate marking is idempotent.
        manifest.mark_stage_completed("probe");
        assert_eq!(
            manifest
                .completed_stages
                .iter()
                .filter(|s| s.as_str() == "probe")
                .count(),
            1
        );
    }

    #[test]
    fn mark_window_completed_tracks_windows() {
        let mut manifest = make_test_manifest("test-run-2");
        assert!(!manifest.is_window_completed(0));

        manifest.mark_window_completed(0);
        manifest.mark_window_completed(2);
        assert!(manifest.is_window_completed(0));
        assert!(!manifest.is_window_completed(1));
        assert!(manifest.is_window_completed(2));

        // Duplicate is idempotent.
        manifest.mark_window_completed(0);
        assert_eq!(
            manifest
                .completed_windows
                .iter()
                .filter(|&&w| w == 0)
                .count(),
            1
        );
    }

    #[test]
    fn pending_window_indices_filters_completed() {
        let mut manifest = make_test_manifest("test-run-3");
        manifest.set_windows(vec![
            crate::TemporalWindow {
                start_frame: 0,
                end_frame_exclusive: 20,
                anchor_frames: vec![0],
            },
            crate::TemporalWindow {
                start_frame: 20,
                end_frame_exclusive: 40,
                anchor_frames: vec![20],
            },
            crate::TemporalWindow {
                start_frame: 40,
                end_frame_exclusive: 60,
                anchor_frames: vec![40],
            },
        ]);

        manifest.mark_window_completed(0);
        manifest.mark_window_completed(2);

        let pending = manifest.pending_window_indices();
        assert_eq!(pending, vec![1]);
    }

    #[test]
    fn from_existing_loads_previously_created_layout() {
        let temp = std::env::temp_dir().join(format!(
            "helioframe-resume-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        let layout = RunLayout::create(&temp).expect("run layout should be created");
        let mut manifest = make_test_manifest(&layout.run_id);
        manifest.mark_stage_completed("probe");
        manifest.mark_stage_completed("decode");
        manifest.mark_window_completed(0);
        layout
            .write_manifest(&manifest)
            .expect("write should succeed");

        // Reopen the same run directory.
        let resumed = RunLayout::from_existing(&layout.run_dir).expect("from_existing should work");
        assert_eq!(resumed.run_id, layout.run_id);

        let loaded = resumed.load_manifest().expect("load should succeed");
        assert!(loaded.is_stage_completed("probe"));
        assert!(loaded.is_stage_completed("decode"));
        assert!(!loaded.is_stage_completed("window"));
        assert!(loaded.is_window_completed(0));
        assert!(!loaded.is_window_completed(1));

        fs::remove_dir_all(temp).expect("temp directory cleanup should succeed");
    }

    #[test]
    fn completed_stages_default_for_old_manifests() {
        // Simulate a manifest written before resume support was added
        // (missing completed_stages / completed_windows fields).
        let temp = std::env::temp_dir().join(format!(
            "helioframe-compat-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        let layout = RunLayout::create(&temp).expect("run layout should be created");

        // Write a manifest JSON without the new fields.
        let json = serde_json::json!({
            "run_id": layout.run_id,
            "input": "input.mp4",
            "output": "output.mp4",
            "preset": "studio",
            "backend": "stcdit-studio",
            "probe": { "container": "mp4", "assumed_resolution": "1920x1080" },
            "windows": [],
            "window_tiles": [],
            "temporal_qc": null,
            "stage_timings": []
        });
        fs::write(&layout.manifest_path, json.to_string()).expect("write should succeed");

        let loaded = layout.load_manifest().expect("should parse without new fields");
        assert!(loaded.completed_stages.is_empty());
        assert!(loaded.completed_windows.is_empty());

        fs::remove_dir_all(temp).expect("temp directory cleanup should succeed");
    }
}
