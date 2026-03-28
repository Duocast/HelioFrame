use helioframe_core::{BackendKind, DoctorSummary, UpscalePreset};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Which top-level panel is active in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    Upscale,
    Progress,
    Diagnostics,
    Settings,
    About,
}

/// Pipeline execution status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineStatus {
    Idle,
    Running,
    Completed,
    Failed(String),
}

/// Per-stage progress info.
#[derive(Debug, Clone)]
pub struct StageProgress {
    pub name: &'static str,
    pub description: &'static str,
    pub status: StageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Skipped,
    Failed(String),
}

/// Shared pipeline state accessible from the background thread.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PipelineState {
    pub status: PipelineStatus,
    pub stages: Vec<StageProgress>,
    pub current_stage_index: Option<usize>,
    pub overall_progress: f32,
    pub elapsed: Option<std::time::Duration>,
    pub run_id: Option<String>,
    pub run_dir: Option<PathBuf>,
    pub log_lines: Vec<LogEntry>,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

impl Default for PipelineState {
    fn default() -> Self {
        Self {
            status: PipelineStatus::Idle,
            stages: default_stages(),
            current_stage_index: None,
            overall_progress: 0.0,
            elapsed: None,
            run_id: None,
            run_dir: None,
            log_lines: Vec::new(),
        }
    }
}

impl PipelineState {
    pub fn push_log(&mut self, level: LogLevel, message: impl Into<String>) {
        let now = chrono_now();
        self.log_lines.push(LogEntry {
            timestamp: now,
            level,
            message: message.into(),
        });
    }
}

fn chrono_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

pub fn default_stages() -> Vec<StageProgress> {
    vec![
        StageProgress { name: "probe", description: "Probe input metadata", status: StageStatus::Pending },
        StageProgress { name: "decode", description: "Decode video frames", status: StageStatus::Pending },
        StageProgress { name: "normalize", description: "Normalize colorspace", status: StageStatus::Pending },
        StageProgress { name: "shots", description: "Detect shot boundaries", status: StageStatus::Pending },
        StageProgress { name: "anchors", description: "Select anchor frames", status: StageStatus::Pending },
        StageProgress { name: "windows", description: "Build temporal windows", status: StageStatus::Pending },
        StageProgress { name: "tiles", description: "Schedule spatial patches", status: StageStatus::Pending },
        StageProgress { name: "restore", description: "Run restoration backend", status: StageStatus::Pending },
        StageProgress { name: "detail", description: "Detail refinement pass", status: StageStatus::Pending },
        StageProgress { name: "qc", description: "Temporal quality control", status: StageStatus::Pending },
        StageProgress { name: "stitch", description: "Stitch & reconstruct", status: StageStatus::Pending },
        StageProgress { name: "encode", description: "Encode output video", status: StageStatus::Pending },
    ]
}

/// Full GUI application state.
pub struct AppState {
    pub active_panel: ActivePanel,

    // Upscale configuration
    pub input_path: String,
    pub output_path: String,
    pub selected_preset: UpscalePreset,
    pub selected_backend: Option<BackendKind>,
    pub dry_run: bool,

    // Pipeline
    pub pipeline: Arc<Mutex<PipelineState>>,
    pub pipeline_start_time: Option<Instant>,

    // Doctor
    pub doctor_summary: Option<DoctorSummary>,
    #[allow(dead_code)]
    pub doctor_running: bool,

    // Settings
    pub run_directory: String,
    pub auto_open_output: bool,
    pub log_level: String,

    // UI state
    pub file_drop_hover: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_panel: ActivePanel::Upscale,
            input_path: String::new(),
            output_path: String::new(),
            selected_preset: UpscalePreset::Studio,
            selected_backend: None,
            dry_run: false,
            pipeline: Arc::new(Mutex::new(PipelineState::default())),
            pipeline_start_time: None,
            doctor_summary: None,
            doctor_running: false,
            run_directory: ".helioframe".into(),
            auto_open_output: false,
            log_level: "info".into(),
            file_drop_hover: false,
        }
    }
}
