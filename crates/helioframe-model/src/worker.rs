use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use helioframe_core::{BackendKind, RunLayout, RunManifest, WindowTileManifest};

const INPUT_SCHEMA_VERSION: &str = "1.0.0";
const WORKER_TIMEOUT: Duration = Duration::from_secs(300);

/// Returns the platform-appropriate Python executable name.
///
/// On Windows the standard Python 3 executable is `python` (the `python3`
/// name either does not exist or points to a Microsoft Store stub that exits
/// immediately).  On other platforms `python3` is the conventional name.
pub fn python_exe() -> &'static str {
    if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    }
}

/// Resolves the path to `workers/python/worker.py`.
///
/// Searches relative to the running executable and its ancestor directories
/// (handling layouts like `target/release/`), then falls back to a path
/// relative to the current working directory.
pub fn resolve_worker_script() -> PathBuf {
    let relative = Path::new("workers").join("python").join("worker.py");

    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent();
        while let Some(ancestor) = dir {
            let candidate = ancestor.join(&relative);
            if candidate.exists() {
                return candidate;
            }
            dir = ancestor.parent();
        }
    }

    relative
}

#[derive(Debug, Clone, Copy)]
pub enum WorkerAdapter {
    PythonProcess,
    OnnxRuntime,
}

#[derive(Debug, Clone)]
pub struct WorkerLaunchConfig<'a> {
    pub run_layout: &'a RunLayout,
    pub manifest: &'a RunManifest,
    pub input_frames_dir: &'a Path,
    pub frame_count: usize,
    pub window_tiles: &'a [WindowTileManifest],
    pub backend_kind: BackendKind,
    pub worker_timeout: Duration,
}

impl<'a> WorkerLaunchConfig<'a> {
    pub fn new(
        run_layout: &'a RunLayout,
        manifest: &'a RunManifest,
        input_frames_dir: &'a Path,
        frame_count: usize,
        window_tiles: &'a [WindowTileManifest],
        backend_kind: BackendKind,
    ) -> Self {
        Self {
            run_layout,
            manifest,
            input_frames_dir,
            frame_count,
            window_tiles,
            backend_kind,
            worker_timeout: WORKER_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkerRunResult {
    pub output_frames_dir: PathBuf,
    pub frame_count: usize,
    pub output_manifest_path: PathBuf,
}

#[derive(Debug, serde::Serialize)]
struct WorkerInputFrame {
    index: usize,
    file_name: String,
}

#[derive(Debug, serde::Serialize)]
struct WorkerInputManifest {
    schema_version: &'static str,
    run_id: String,
    clip_id: String,
    backend: String,
    backend_options: serde_json::Value,
    input_frames_dir: String,
    output_frames_dir: String,
    output_manifest_path: String,
    windows: Vec<helioframe_core::TemporalWindow>,
    window_tiles: Vec<WindowTileManifest>,
    frames: Vec<WorkerInputFrame>,
}

#[derive(Debug, serde::Deserialize)]
struct WorkerOutputManifest {
    status: String,
    frame_count: usize,
    output_frames_dir: String,
}

/// Optional callback that receives each line of subprocess output in real time.
pub type WorkerOutputCallback = Box<dyn Fn(&str, bool) + Send + Sync>;

impl WorkerAdapter {
    pub fn run(
        self,
        config: WorkerLaunchConfig<'_>,
    ) -> helioframe_core::HelioFrameResult<WorkerRunResult> {
        self.run_with_output(config, None)
    }

    /// Run the worker, forwarding stdout/stderr lines through `on_output`.
    ///
    /// The callback receives `(line, is_stderr)`.
    pub fn run_with_output(
        self,
        config: WorkerLaunchConfig<'_>,
        on_output: Option<WorkerOutputCallback>,
    ) -> helioframe_core::HelioFrameResult<WorkerRunResult> {
        match self {
            Self::PythonProcess => run_python_worker(config, on_output),
            Self::OnnxRuntime => crate::onnx::run_onnx_inference(&config, None),
        }
    }
}

fn run_python_worker(
    config: WorkerLaunchConfig<'_>,
    on_output: Option<WorkerOutputCallback>,
) -> helioframe_core::HelioFrameResult<WorkerRunResult> {
    let worker_io_dir = config.run_layout.intermediate_artifacts_dir.join("worker");
    fs::create_dir_all(&worker_io_dir).map_err(|err| {
        helioframe_core::HelioFrameError::Config(format!(
            "failed to create worker I/O directory {}: {err}",
            worker_io_dir.display()
        ))
    })?;

    let worker_output_frames_dir = worker_io_dir.join("frames");
    let input_manifest_path = worker_io_dir.join("worker-input.json");
    let output_manifest_path = worker_io_dir.join("worker-output.json");

    let input_manifest =
        build_worker_input_manifest(&config, &worker_output_frames_dir, &output_manifest_path);
    let payload = serde_json::to_string_pretty(&input_manifest).map_err(|err| {
        helioframe_core::HelioFrameError::Config(format!(
            "failed to serialize worker input manifest: {err}"
        ))
    })?;
    fs::write(&input_manifest_path, payload).map_err(|err| {
        helioframe_core::HelioFrameError::Config(format!(
            "failed to write worker input manifest {}: {err}",
            input_manifest_path.display()
        ))
    })?;

    let mut child = Command::new(python_exe())
        .arg(resolve_worker_script())
        .arg(&input_manifest_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to launch python worker: {err}"
            ))
        })?;

    // Drain stdout and stderr in background threads so the pipe buffers never
    // fill up and block the child process.
    let captured_stderr = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let captured_stdout = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let stdout_handle = child.stdout.take().map(|pipe| {
        let captured = captured_stdout.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(pipe);
            for line in reader.lines() {
                if let Ok(line) = line {
                    captured.lock().unwrap().push(line);
                }
            }
        })
    });

    let stderr_handle = child.stderr.take().map(|pipe| {
        let captured = captured_stderr.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(pipe);
            for line in reader.lines() {
                if let Ok(line) = line {
                    captured.lock().unwrap().push(line);
                }
            }
        })
    });

    let status = wait_for_child_with_timeout(&mut child, config.worker_timeout, "python worker");

    // Wait for reader threads to finish draining.
    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }

    // Forward captured output to the callback.
    if let Some(ref cb) = on_output {
        for line in captured_stdout.lock().unwrap().iter() {
            cb(line, false);
        }
        for line in captured_stderr.lock().unwrap().iter() {
            cb(line, true);
        }
    }

    // If the process failed, include stderr in the error message.
    if let Err(mut err) = status {
        let stderr_lines = captured_stderr.lock().unwrap();
        if !stderr_lines.is_empty() {
            let detail = stderr_lines.join("\n");
            let truncated = if detail.len() > 2000 {
                format!("...{}", &detail[detail.len() - 2000..])
            } else {
                detail
            };
            err = helioframe_core::HelioFrameError::Config(format!(
                "{err}\n--- worker stderr ---\n{truncated}"
            ));
        }
        return Err(err);
    }

    let raw_output = fs::read_to_string(&output_manifest_path).map_err(|err| {
        let stderr_lines = captured_stderr.lock().unwrap();
        let stderr_context = if stderr_lines.is_empty() {
            String::new()
        } else {
            format!("\n--- worker stderr ---\n{}", stderr_lines.join("\n"))
        };
        helioframe_core::HelioFrameError::Config(format!(
            "python worker did not produce output manifest {}: {err}{stderr_context}",
            output_manifest_path.display()
        ))
    })?;
    let output_manifest: WorkerOutputManifest =
        serde_json::from_str(&raw_output).map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to parse python worker output manifest: {err}"
            ))
        })?;

    if output_manifest.status != "ok" {
        return Err(helioframe_core::HelioFrameError::Config(format!(
            "python worker failed with status `{}`",
            output_manifest.status
        )));
    }

    if output_manifest.frame_count != config.frame_count {
        return Err(helioframe_core::HelioFrameError::Config(format!(
            "python worker frame count mismatch: expected {}, got {}",
            config.frame_count, output_manifest.frame_count
        )));
    }

    Ok(WorkerRunResult {
        output_frames_dir: PathBuf::from(output_manifest.output_frames_dir),
        frame_count: output_manifest.frame_count,
        output_manifest_path,
    })
}

fn default_backend_options(backend_kind: BackendKind) -> serde_json::Value {
    match backend_kind {
        BackendKind::RealBasicVsrBridge => serde_json::json!({
            "model_path": "models/realbasicvsr/realbasicvsr_x4.ts",
            "device": "cuda",
            "window_size": 6,
            "overlap": 2,
            "precision": "fp16"
        }),
        BackendKind::SeedvrTeacher => serde_json::json!({
            "model_path": "models/seedvr-teacher/seedvr_teacher_v1.0.0.ts",
            "model_version": "seedvr-teacher-v1.0.0",
            "weights_sha256": "c2c0d9ec5b0c8c1f8b03419e7f3e462f8ab7604f53f6ed15ec5122f7e14b8079",
            "offline_only": true,
            "device": "cuda",
            "window_size": 12,
            "overlap": 4,
            "precision": "fp32"
        }),
        BackendKind::StcditStudio => serde_json::json!({
            "model_path": "models/stcdit-studio/stcdit_studio_v1.0.0.ts",
            "model_version": "stcdit-studio-v1.0.0",
            "weights_sha256": "a7b3e1f0c4d29856e1a0f3b7c8d5e2a9f6b4c1d8e5a2f7b3c0d6e9a1f4b8c5d2",
            "device": "cuda",
            "window_size": 20,
            "overlap": 4,
            "precision": "fp16",
            "diffusion_steps": 16,
            "guidance_scale": 7.5,
            "anchor_frame_stride": 4
        }),
        BackendKind::HelioFrameMaster => serde_json::json!({
            "teacher_model_path": "models/seedvr-teacher/seedvr_teacher_v1.0.0.ts",
            "teacher_model_version": "seedvr-teacher-v1.0.0",
            "teacher_weights_sha256": "c2c0d9ec5b0c8c1f8b03419e7f3e462f8ab7604f53f6ed15ec5122f7e14b8079",
            "teacher_window_size": 12,
            "teacher_overlap": 4,
            "teacher_precision": "fp32",
            "studio_model_path": "models/stcdit-studio/stcdit_studio_v1.0.0.ts",
            "studio_model_version": "stcdit-studio-v1.0.0",
            "studio_weights_sha256": "a7b3e1f0c4d29856e1a0f3b7c8d5e2a9f6b4c1d8e5a2f7b3c0d6e9a1f4b8c5d2",
            "studio_window_size": 20,
            "studio_overlap": 4,
            "studio_precision": "fp16",
            "diffusion_steps": 24,
            "guidance_scale": 7.5,
            "anchor_frame_stride": 3,
            "refiner_model_path": "models/detail-refiner/detail_refiner_v1.0.0.ts",
            "refiner_model_version": "detail-refiner-v1.0.0",
            "refiner_weights_sha256": "b8d4f2a1e6c73950d2b1e4a8f7c3d6b9e5a2f8c1d7b4e0a3f6c9d2b5e8a1f4c7",
            "refiner_refinement_steps": 6,
            "refiner_refinement_strength": 0.4,
            "refiner_precision": "fp16",
            "device": "cuda",
            "max_qc_reruns": 2,
            "qc_max_flicker": 0.12,
            "qc_max_shimmer": 0.08
        }),
        _ => serde_json::json!({}),
    }
}

fn build_worker_input_manifest(
    config: &WorkerLaunchConfig<'_>,
    output_frames_dir: &Path,
    output_manifest_path: &Path,
) -> WorkerInputManifest {
    let frames = (0..config.frame_count)
        .map(|index| WorkerInputFrame {
            index,
            file_name: format!("frame_{index:010}.png"),
        })
        .collect();

    WorkerInputManifest {
        schema_version: INPUT_SCHEMA_VERSION,
        run_id: config.manifest.run_id.clone(),
        clip_id: "window-patch-job".to_string(),
        backend: config.backend_kind.to_string(),
        backend_options: default_backend_options(config.backend_kind),
        input_frames_dir: config.input_frames_dir.to_string_lossy().to_string(),
        output_frames_dir: output_frames_dir.to_string_lossy().to_string(),
        output_manifest_path: output_manifest_path.to_string_lossy().to_string(),
        windows: config.manifest.windows.clone(),
        window_tiles: config.window_tiles.to_vec(),
        frames,
    }
}

fn wait_for_child_with_timeout(
    child: &mut Child,
    timeout: Duration,
    process_label: &str,
) -> helioframe_core::HelioFrameResult<ExitStatus> {
    let started = Instant::now();

    loop {
        if let Some(status) = child.try_wait().map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to poll {process_label} process: {err}"
            ))
        })? {
            if status.success() {
                return Ok(status);
            }

            return Err(helioframe_core::HelioFrameError::Config(format!(
                "{process_label} exited with status {status}"
            )));
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(helioframe_core::HelioFrameError::Config(format!(
                "{process_label} timed out after {} seconds",
                timeout.as_secs()
            )));
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}
