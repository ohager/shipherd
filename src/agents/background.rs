//! Claude Code's background agents, read from the subagent transcripts it writes next to each
//! session: `<config>/projects/<project>/<session_id>/subagents/agent-<id>.jsonl` and `.meta.json`.
//! The format is internal to Claude Code; anything unexpected means "no background work".

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::BackgroundTask;

/// Stopped agents and crashed sessions never write a final message; this lets them drop out.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
/// Enough for the last message of a transcript; reading whole transcripts (often MBs) is waste.
const TAIL_BYTES: u64 = 64 * 1024;

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"))
}

/// Running subagents of one session, oldest first.
pub fn scan(root: &Path, session_id: &str, now_ms: u64) -> Vec<BackgroundTask> {
    // tty7 reports UUIDs; anything else could point outside the projects directory.
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-')
    {
        return Vec::new();
    }
    let Some(dir) = subagents_dir(root, session_id) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut tasks: Vec<BackgroundTask> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|transcript| running_task(&transcript, now_ms))
        .collect();
    tasks.sort_by_key(|t| t.since_ms);
    tasks
}

/// Session ids are unique, so the project directory (a slug of the cwd) needn't be rebuilt.
fn subagents_dir(root: &Path, session_id: &str) -> Option<PathBuf> {
    fs::read_dir(root.join("projects"))
        .ok()?
        .filter_map(Result::ok)
        .map(|project| project.path().join(session_id).join("subagents"))
        .find(|dir| dir.is_dir())
}

fn running_task(transcript: &Path, now_ms: u64) -> Option<BackgroundTask> {
    let modified = to_ms(fs::metadata(transcript).ok()?.modified().ok()?);
    if now_ms.saturating_sub(modified) > STALE_AFTER.as_millis() as u64 {
        return None;
    }
    if finished(&last_line(transcript)?) {
        return None;
    }
    let meta_path = transcript.with_extension("meta.json");
    let meta: Meta = serde_json::from_slice(&fs::read(&meta_path).ok()?).ok()?;
    let since_ms = fs::metadata(&meta_path)
        .and_then(|m| m.modified())
        .map(to_ms)
        .unwrap_or(modified);
    Some(BackgroundTask {
        kind: meta.agent_type.unwrap_or_else(|| "agent".into()),
        description: meta.description.unwrap_or_default(),
        since_ms,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    agent_type: Option<String>,
    description: Option<String>,
}

fn last_line(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    tail.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::to_string)
}

/// A finished subagent's transcript ends with its final answer: an assistant message that
/// stopped at `end_turn`. A half-written line doesn't parse and counts as running.
fn finished(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    value["type"] == "assistant" && value["message"]["stop_reason"] == "end_turn"
}

pub(crate) fn to_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    const SESSION: &str = "7d085caf-a845-4fa1-a0d8-db963b1c766d";
    const FINISHED: &str = r#"{"type":"user","message":{"role":"user","content":"go"}}
{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}
"#;
    const RUNNING: &str = r#"{"type":"user","message":{"role":"user","content":"go"}}
{"type":"assistant","message":{"stop_reason":null,"content":[{"type":"thinking","thinking":"…"}]}}
"#;

    fn subagents_dir(root: &Path) -> PathBuf {
        let dir = root
            .join("projects/-Users-dev-code-app")
            .join(SESSION)
            .join("subagents");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_agent(dir: &Path, id: &str, transcript: &str, kind: &str, description: &str) {
        fs::write(dir.join(format!("agent-{id}.jsonl")), transcript).unwrap();
        fs::write(
            dir.join(format!("agent-{id}.meta.json")),
            format!(r#"{{"agentType":"{kind}","description":"{description}","requestShape":"background"}}"#),
        )
        .unwrap();
    }

    fn now_ms() -> u64 {
        to_ms(SystemTime::now())
    }

    #[test]
    fn lists_running_agents_and_skips_finished_ones() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        write_agent(&dir, "a1", RUNNING, "researcher", "Locating prompts");
        write_agent(&dir, "a2", FINISHED, "coder", "Done already");
        let tasks = scan(root.path(), SESSION, now_ms());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].kind, "researcher");
        assert_eq!(tasks[0].description, "Locating prompts");
        assert!(tasks[0].since_ms <= now_ms());
    }

    #[test]
    fn agents_silent_for_30_minutes_count_as_gone() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        write_agent(&dir, "a1", RUNNING, "researcher", "Stuck");
        let later = now_ms() + STALE_AFTER.as_millis() as u64 + 1_000;
        assert!(scan(root.path(), SESSION, later).is_empty());
    }

    #[test]
    fn a_missing_directory_means_no_background_work() {
        let root = tempfile::tempdir().unwrap();
        assert!(scan(root.path(), SESSION, now_ms()).is_empty());
    }

    #[test]
    fn session_ids_that_could_leave_the_directory_are_ignored() {
        let root = tempfile::tempdir().unwrap();
        subagents_dir(root.path());
        for bad in ["../x", "", "7D085CAF", "a/b", ".."] {
            assert!(scan(root.path(), bad, now_ms()).is_empty(), "{bad}");
        }
    }

    #[test]
    fn a_half_written_last_line_counts_as_running() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        write_agent(
            &dir,
            "a1",
            "{\"type\":\"assistant\",\"mess",
            "researcher",
            "Writing",
        );
        assert_eq!(scan(root.path(), SESSION, now_ms()).len(), 1);
    }

    #[test]
    fn only_the_tail_of_a_long_transcript_is_read() {
        let root = tempfile::tempdir().unwrap();
        let dir = subagents_dir(root.path());
        let filler = format!("{}\n", r#"{"type":"user","message":{"content":"x"}}"#).repeat(5_000);
        write_agent(&dir, "a1", &(filler + FINISHED), "coder", "Long");
        assert!(scan(root.path(), SESSION, now_ms()).is_empty());
    }
}
