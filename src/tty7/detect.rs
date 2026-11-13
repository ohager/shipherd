//! Finds agent panes from tty7's own detection. `tty7 agents` lists only agents whose status
//! hooks have fired, so idle sessions started before the hooks were installed are missing there.

use std::collections::HashMap;

use super::Tty7Cli;
use super::types::PaneInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedPane {
    pub pane: u32,
    /// tty7's agent name shortened to its first word ("Claude Code" → "Claude"), matching what
    /// `tty7 agents` reports for the same agent.
    pub agent: String,
    pub workspace: String,
    /// The tab's label, which tty7 derives from the agent's task.
    pub label: Option<String>,
    pub cwd: Option<String>,
}

/// Costs one `tab ls` per workspace, so callers should run it off the hot path.
pub async fn detect(cli: &dyn Tty7Cli) -> anyhow::Result<Vec<DetectedPane>> {
    let workspaces = cli.workspaces().await?;
    let panes: HashMap<u32, PaneInfo> = cli
        .panes()
        .await?
        .panes
        .into_iter()
        .map(|p| (p.pane, p))
        .collect();
    let mut detected = Vec::new();
    for workspace in workspaces.workspaces {
        for tab in cli.tabs(&workspace.id).await?.tabs {
            let Some(agent) = tab.agent else { continue };
            let single = tab.panes.len() == 1;
            for pane in &tab.panes {
                let info = panes.get(pane);
                if !single && !runs_agent(info.and_then(|i| i.title.as_deref()), &agent) {
                    continue;
                }
                detected.push(DetectedPane {
                    pane: *pane,
                    agent: agent
                        .split_whitespace()
                        .next()
                        .unwrap_or(&agent)
                        .to_string(),
                    workspace: workspace.name.clone(),
                    label: tab.label.clone(),
                    cwd: info.and_then(|i| i.cwd.clone()),
                });
            }
        }
    }
    Ok(detected)
}

/// A split tab's agent could be in any of its panes; the one whose process title starts with the
/// agent's first word ("claude.exe" for "Claude Code") is it.
fn runs_agent(title: Option<&str>, agent: &str) -> bool {
    let Some(word) = agent.split_whitespace().next() else {
        return false;
    };
    title.is_some_and(|t| t.to_lowercase().starts_with(&word.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty7::fake::FakeCli;

    fn fixture<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }

    fn fake() -> FakeCli {
        let cli = FakeCli::default();
        *cli.workspaces.lock().unwrap() = fixture(include_str!("../../tests/fixtures/ls.json"));
        *cli.panes.lock().unwrap() = fixture(include_str!("../../tests/fixtures/panes.json"));
        let mut tabs = cli.tabs.lock().unwrap();
        tabs.insert(
            "aaaaaaaa-0000-0000-0000-000000000001".into(),
            fixture(include_str!("../../tests/fixtures/tabs-default.json")),
        );
        tabs.insert(
            "bbbbbbbb-0000-0000-0000-000000000002".into(),
            fixture(include_str!("../../tests/fixtures/tabs-reviews.json")),
        );
        drop(tabs);
        cli
    }

    #[tokio::test]
    async fn finds_agents_in_every_workspace() {
        let detected = detect(&fake()).await.unwrap();
        assert_eq!(
            detected,
            vec![
                DetectedPane {
                    pane: 54,
                    agent: "Claude".into(),
                    workspace: "Default".into(),
                    label: Some("Herdr and tty7 comparison".into()),
                    cwd: Some("/home/dev/code/hermes".into()),
                },
                DetectedPane {
                    pane: 22,
                    agent: "Claude".into(),
                    workspace: "PR Reviews".into(),
                    label: Some("Routinen ausführen".into()),
                    cwd: Some("/home/dev/code/e2e".into()),
                },
            ]
        );
    }

    #[test]
    fn split_tabs_pick_the_pane_running_the_agent() {
        assert!(runs_agent(Some("claude.exe"), "Claude Code"));
        assert!(runs_agent(Some("codex"), "Codex"));
        assert!(!runs_agent(Some("shipherd"), "Claude Code"));
        assert!(!runs_agent(None, "Claude Code"));
    }
}
