//! The shipherd-questions Claude Code plugin (in this repo under `plugins/`): whether it is
//! enabled, and installing it with `shipherd setup claude`.

use std::fs;
use std::io::ErrorKind;
use std::process::Command;

use anyhow::bail;
use serde_json::Value;

use super::background::claude_config_dir;

const MARKETPLACE: &str = "shipherd";
const MARKETPLACE_SOURCE: &str = "ohager/shipherd";
pub const PLUGIN: &str = "shipherd-questions@shipherd";

/// What Claude Code's user settings say about the plugin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginSettings {
    pub marketplace_known: bool,
    pub enabled: bool,
}

/// Anything unreadable counts as "not set up".
pub fn parse_settings(settings: &str) -> PluginSettings {
    let Ok(v) = serde_json::from_str::<Value>(settings) else {
        return PluginSettings::default();
    };
    PluginSettings {
        marketplace_known: v
            .get("extraKnownMarketplaces")
            .and_then(|m| m.get(MARKETPLACE))
            .is_some(),
        enabled: v
            .get("enabledPlugins")
            .and_then(|p| p.get(PLUGIN))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// `<config>/settings.json`. Blocking file I/O: call off the core loop.
pub fn user_settings() -> PluginSettings {
    claude_config_dir()
        .and_then(|dir| fs::read_to_string(dir.join("settings.json")).ok())
        .map(|s| parse_settings(&s))
        .unwrap_or_default()
}

/// Adds the marketplace unless it is known (`marketplace add` refuses a known name, even when it
/// points elsewhere), then installs and enables the plugin. Each step is idempotent.
pub fn setup() -> anyhow::Result<()> {
    if !user_settings().marketplace_known {
        claude(&["plugin", "marketplace", "add", MARKETPLACE_SOURCE])?;
    }
    claude(&["plugin", "install", PLUGIN])?;
    if !user_settings().enabled {
        claude(&["plugin", "enable", PLUGIN])?;
    }
    println!("{PLUGIN} is enabled; it applies to Claude Code sessions started from now on.");
    Ok(())
}

fn claude(args: &[&str]) -> anyhow::Result<()> {
    match Command::new("claude").args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => bail!("`claude {}` failed", args.join(" ")),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            bail!("shipherd setup claude needs the claude CLI on your PATH")
        }
        Err(err) => bail!("running `claude {}` failed: {err}", args.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_marketplace_and_enabled_flag() {
        let settings = r#"{
            "extraKnownMarketplaces": { "shipherd": { "source": { "source": "github", "repo": "ohager/shipherd" } } },
            "enabledPlugins": { "shipherd-questions@shipherd": true, "brag@brag": true }
        }"#;
        assert_eq!(
            parse_settings(settings),
            PluginSettings {
                marketplace_known: true,
                enabled: true
            }
        );
    }

    #[test]
    fn a_disabled_or_missing_plugin_is_not_enabled() {
        let disabled = r#"{ "enabledPlugins": { "shipherd-questions@shipherd": false } }"#;
        assert_eq!(parse_settings(disabled), PluginSettings::default());
        assert_eq!(parse_settings("{}"), PluginSettings::default());
    }

    #[test]
    fn unreadable_settings_count_as_not_set_up() {
        assert_eq!(parse_settings("not json"), PluginSettings::default());
        assert_eq!(parse_settings(""), PluginSettings::default());
    }
}
