//! Claude Code's subagent transcripts (filled in by Task 7).

use std::path::{Path, PathBuf};

use super::BackgroundTask;

pub fn claude_config_dir() -> Option<PathBuf> {
    None
}

pub fn scan(_root: &Path, _session_id: &str, _now_ms: u64) -> Vec<BackgroundTask> {
    Vec::new()
}
