use serde::Serialize;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorSummary {
    pub platform_notice: &'static str,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorSummary {
    pub fn is_ok(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    pub fn failed_checks(&self) -> Vec<&DoctorCheck> {
        self.checks.iter().filter(|check| !check.passed).collect()
    }
}

pub fn run_doctor() -> DoctorSummary {
    let run_dir = detect_run_dir();

    DoctorSummary {
        platform_notice: "Linux/NVIDIA/SDR only",
        checks: vec![
            command_check(
                "ffmpeg",
                "ffmpeg",
                "Install FFmpeg and ensure `ffmpeg` is available on PATH.",
            ),
            command_check(
                "ffprobe",
                "ffprobe",
                "Install FFmpeg and ensure `ffprobe` is available on PATH.",
            ),
            command_check(
                "python3",
                "python3",
                "Install Python 3 and ensure `python3` is available on PATH.",
            ),
            gpu_check(),
            writable_dir_check(
                "temp_dir_writable",
                &env::temp_dir(),
                "Use a writable temp directory and/or set TMPDIR to a writable location.",
            ),
            writable_dir_check(
                "run_dir_writable",
                &run_dir,
                "Set HELIOFRAME_RUN_DIR (or XDG_RUNTIME_DIR) to a writable directory for runtime files.",
            ),
        ],
    }
}

fn command_check(name: &'static str, command: &str, action: &str) -> DoctorCheck {
    if command_exists(command) {
        DoctorCheck {
            name,
            passed: true,
            detail: format!("`{command}` found on PATH"),
            action: None,
        }
    } else {
        DoctorCheck {
            name,
            passed: false,
            detail: format!("`{command}` not found on PATH"),
            action: Some(action.to_string()),
        }
    }
}

fn gpu_check() -> DoctorCheck {
    if env::consts::OS != "linux" {
        return DoctorCheck {
            name: "gpu_visibility",
            passed: false,
            detail: format!("unsupported OS: {}", env::consts::OS),
            action: Some("Run HelioFrame on Linux with NVIDIA drivers installed.".to_string()),
        };
    }

    if !command_exists("nvidia-smi") {
        return DoctorCheck {
            name: "gpu_visibility",
            passed: false,
            detail: "`nvidia-smi` not found on PATH".to_string(),
            action: Some("Install NVIDIA drivers so `nvidia-smi` is available.".to_string()),
        };
    }

    let output = Command::new("nvidia-smi").arg("-L").output();
    match output {
        Ok(result) if result.status.success() => {
            let gpu_list = String::from_utf8_lossy(&result.stdout);
            if gpu_list.trim().is_empty() {
                DoctorCheck {
                    name: "gpu_visibility",
                    passed: false,
                    detail: "`nvidia-smi -L` returned no visible GPUs".to_string(),
                    action: Some(
                        "Verify that an NVIDIA GPU is attached and visible in this runtime."
                            .to_string(),
                    ),
                }
            } else {
                DoctorCheck {
                    name: "gpu_visibility",
                    passed: true,
                    detail: format!(
                        "visible NVIDIA GPU(s): {}",
                        gpu_list.lines().next().unwrap_or_default()
                    ),
                    action: None,
                }
            }
        }
        Ok(result) => DoctorCheck {
            name: "gpu_visibility",
            passed: false,
            detail: format!("`nvidia-smi -L` failed with status {}", result.status),
            action: Some(
                "Check NVIDIA driver/runtime setup and container GPU passthrough configuration."
                    .to_string(),
            ),
        },
        Err(err) => DoctorCheck {
            name: "gpu_visibility",
            passed: false,
            detail: format!("failed to run `nvidia-smi -L`: {err}"),
            action: Some(
                "Check NVIDIA driver/runtime setup and container GPU passthrough configuration."
                    .to_string(),
            ),
        },
    }
}

fn writable_dir_check(name: &'static str, dir: &Path, action: &str) -> DoctorCheck {
    if let Err(err) = fs::create_dir_all(dir) {
        return DoctorCheck {
            name,
            passed: false,
            detail: format!("unable to create directory {}: {err}", dir.display()),
            action: Some(action.to_string()),
        };
    }

    let test_file = dir.join(format!(".helioframe-doctor-{}", std::process::id()));
    let write_result = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&test_file)
        .and_then(|mut file| file.write_all(b"ok"));

    match write_result {
        Ok(()) => {
            let _ = fs::remove_file(&test_file);
            DoctorCheck {
                name,
                passed: true,
                detail: format!("writable directory: {}", dir.display()),
                action: None,
            }
        }
        Err(err) => DoctorCheck {
            name,
            passed: false,
            detail: format!("directory not writable {}: {err}", dir.display()),
            action: Some(action.to_string()),
        },
    }
}

fn detect_run_dir() -> PathBuf {
    if let Ok(path) = env::var("HELIOFRAME_RUN_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(path).join("helioframe");
    }
    PathBuf::from("/run/helioframe")
}

fn command_exists(command: &str) -> bool {
    let paths = env::var_os("PATH").map(|raw| env::split_paths(&raw).collect::<Vec<_>>());
    paths
        .into_iter()
        .flatten()
        .map(|path| path.join(command))
        .any(|candidate| candidate.is_file())
}
