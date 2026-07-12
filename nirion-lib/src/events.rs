use crate::lock::DiffEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    StdoutLine(String),
    StderrLine(String),
    Exited(ExitStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeEvent {
    ProjectStarted {
        project: String,
    },
    Process {
        project: Option<String>,
        event: ProcessEvent,
    },
    ProjectFailed {
        project: String,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitEvent {
    CommandRunning { project: String },
    CommandFinished { project: String },
    ProjectStatus { project: String },
    WaitingForHealthchecks,
    Ready,
}

#[derive(Debug, Clone)]
pub enum LockUpdateEvent {
    NoImages,
    ImageStarted { service: String, image: String },
    ImageResolved { service: String },
    UpToDate,
    ChangesDetected { diffs: Vec<DiffEntry> },
    WritingLockFile,
    LockFileWritten,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitStatus {
    pub code: Option<i32>,
    pub success: bool,
}

impl From<std::process::ExitStatus> for ExitStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn converts_success_exit_status() {
        let status = std::process::ExitStatus::from_raw(0);
        let status = ExitStatus::from(status);

        assert_eq!(status.code, Some(0));
        assert!(status.success);
    }

    #[test]
    fn converts_failed_exit_status() {
        let status = std::process::ExitStatus::from_raw(1 << 8);
        let status = ExitStatus::from(status);

        assert_eq!(status.code, Some(1));
        assert!(!status.success);
    }
}
