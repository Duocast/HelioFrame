use std::sync::Arc;

/// Log level for pipeline events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// A single pipeline log message.
#[derive(Debug, Clone)]
pub struct PipelineLogMessage {
    pub level: PipelineLogLevel,
    pub message: String,
}

/// Thread-safe callback for forwarding pipeline logs to the GUI or CLI.
///
/// The orchestrator calls `log()` at key points: stage transitions, subprocess
/// output lines, timing info, and detailed error context.  Consumers clone the
/// `Arc` and read messages at their own pace.
#[derive(Clone)]
pub struct PipelineLogger {
    sink: Arc<dyn Fn(PipelineLogMessage) + Send + Sync>,
}

impl PipelineLogger {
    /// Create a logger that forwards every message to `sink`.
    pub fn new(sink: impl Fn(PipelineLogMessage) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }

    /// Create a no-op logger that discards all messages.
    pub fn noop() -> Self {
        Self::new(|_| {})
    }

    pub fn debug(&self, msg: impl Into<String>) {
        (self.sink)(PipelineLogMessage {
            level: PipelineLogLevel::Debug,
            message: msg.into(),
        });
    }

    pub fn info(&self, msg: impl Into<String>) {
        (self.sink)(PipelineLogMessage {
            level: PipelineLogLevel::Info,
            message: msg.into(),
        });
    }

    pub fn warn(&self, msg: impl Into<String>) {
        (self.sink)(PipelineLogMessage {
            level: PipelineLogLevel::Warn,
            message: msg.into(),
        });
    }

    pub fn error(&self, msg: impl Into<String>) {
        (self.sink)(PipelineLogMessage {
            level: PipelineLogLevel::Error,
            message: msg.into(),
        });
    }
}

impl std::fmt::Debug for PipelineLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineLogger").finish()
    }
}
