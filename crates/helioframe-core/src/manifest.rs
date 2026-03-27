use crate::{
    AppConfig, BackendKind, HelioFrameError, HelioFrameResult, TemporalWindow, UpscalePreset,
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
pub struct RunManifest {
    pub run_id: String,
    pub input: String,
    pub output: String,
    pub preset: UpscalePreset,
    pub backend: BackendKind,
    pub probe: RunProbeInfo,
    pub windows: Vec<TemporalWindow>,
    pub stage_timings: Vec<StageTiming>,
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
            stage_timings: vec![StageTiming {
                stage: "preset".into(),
                elapsed_ms: 0,
            }],
        }
    }

    pub fn set_windows(&mut self, windows: Vec<TemporalWindow>) {
        self.windows = windows;
    }

    pub fn append_stage_timing(&mut self, stage: impl Into<String>, elapsed: Duration) {
        self.stage_timings.push(StageTiming {
            stage: stage.into(),
            elapsed_ms: elapsed.as_millis(),
        });
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
}
