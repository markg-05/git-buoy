//! Persistent, global viewing preferences for the harbor.
//!
//! Repository facts and transient navigation state never enter this file.
//! The executable owns loading and saving so [`crate::app::App::update`]
//! remains a deterministic state transition without filesystem side effects.

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::AppSettings;

const SETTINGS_VERSION: u8 = 1;

/// The durable subset of [`AppSettings`]. Animation FPS stays a CLI-only
/// runtime choice; selection, overlays, and inspection state are not settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsConfig {
    version: u8,
    reduced_motion: bool,
    auto_cycle: bool,
    cycle_interval_ms: u64,
    setting_help: bool,
    poll_interval_ms: u64,
    idle_after_ms: u64,
    github_enabled: bool,
    github_poll_interval_ms: u64,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self::from_settings(&AppSettings::default())
    }
}

impl SettingsConfig {
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            version: SETTINGS_VERSION,
            reduced_motion: settings.reduced_motion,
            auto_cycle: settings.auto_cycle,
            cycle_interval_ms: duration_millis(settings.cycle_interval),
            setting_help: settings.setting_help,
            poll_interval_ms: duration_millis(settings.poll_interval),
            idle_after_ms: duration_millis(settings.idle_after),
            github_enabled: settings.github_enabled,
            github_poll_interval_ms: duration_millis(settings.github_poll_interval),
        }
    }

    /// Apply saved preferences before explicit CLI overrides are considered.
    pub fn apply_to(&self, settings: &mut AppSettings) {
        settings.reduced_motion = self.reduced_motion;
        settings.auto_cycle = self.auto_cycle;
        settings.cycle_interval = Duration::from_millis(self.cycle_interval_ms.max(1_000));
        settings.setting_help = self.setting_help;
        settings.poll_interval = Duration::from_millis(self.poll_interval_ms.max(200));
        settings.idle_after = Duration::from_millis(self.idle_after_ms.max(1_000));
        settings.github_enabled = self.github_enabled;
        settings.github_poll_interval =
            Duration::from_millis(self.github_poll_interval_ms.max(5_000));
    }

    /// Record only values actually changed inside the running UI. This keeps
    /// unrelated command-line overrides session-scoped when another control
    /// is adjusted.
    pub fn record_changes(&mut self, before: &AppSettings, after: &AppSettings) {
        self.version = SETTINGS_VERSION;
        if before.reduced_motion != after.reduced_motion {
            self.reduced_motion = after.reduced_motion;
        }
        if before.auto_cycle != after.auto_cycle {
            self.auto_cycle = after.auto_cycle;
        }
        if before.cycle_interval != after.cycle_interval {
            self.cycle_interval_ms = duration_millis(after.cycle_interval);
        }
        if before.setting_help != after.setting_help {
            self.setting_help = after.setting_help;
        }
        if before.poll_interval != after.poll_interval {
            self.poll_interval_ms = duration_millis(after.poll_interval);
        }
        if before.idle_after != after.idle_after {
            self.idle_after_ms = duration_millis(after.idle_after);
        }
        if before.github_enabled != after.github_enabled {
            self.github_enabled = after.github_enabled;
        }
        if before.github_poll_interval != after.github_poll_interval {
            self.github_poll_interval_ms = duration_millis(after.github_poll_interval);
        }
    }
}

/// Resolve the global settings file using standard CLI configuration
/// locations without introducing a platform-directory dependency.
pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = nonempty_env("GIT_BUOY_CONFIG") {
        return Some(PathBuf::from(path));
    }

    if cfg!(windows) {
        windows_default_path(nonempty_env("APPDATA"))
    } else {
        unix_default_path(nonempty_env("XDG_CONFIG_HOME"), nonempty_env("HOME"))
    }
}

fn windows_default_path(appdata: Option<String>) -> Option<PathBuf> {
    appdata
        .map(PathBuf::from)
        .map(|base| base.join("git-buoy").join("settings.json"))
}

fn unix_default_path(xdg_config_home: Option<String>, home: Option<String>) -> Option<PathBuf> {
    if let Some(base) = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Some(base.join("git-buoy").join("settings.json"));
    }
    home.map(PathBuf::from)
        .map(|home| home.join(".config").join("git-buoy").join("settings.json"))
}

pub fn load(path: &Path) -> Result<SettingsConfig> {
    if !path.exists() {
        return Ok(SettingsConfig::default());
    }
    let bytes =
        fs::read(path).with_context(|| format!("cannot read settings from {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("cannot parse settings from {}", path.display()))
}

/// Write beside the destination and rename into place so a partial write
/// cannot leave the preferences file truncated.
pub fn save(path: &Path, settings: &SettingsConfig) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("cannot create settings directory {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.json");
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(settings).context("cannot encode settings")?;
        bytes.push(b'\n');

        let mut file = File::create(&temporary).with_context(|| {
            format!(
                "cannot create temporary settings file {}",
                temporary.display()
            )
        })?;
        file.write_all(&bytes).with_context(|| {
            format!(
                "cannot write temporary settings file {}",
                temporary.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "cannot flush temporary settings file {}",
                temporary.display()
            )
        })?;
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "cannot replace settings file {} with {}",
                path.display(),
                temporary.display()
            )
        })?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_replace_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/settings.json");
        let mut settings = AppSettings::default();
        settings.reduced_motion = true;
        settings.auto_cycle = false;
        settings.poll_interval = Duration::from_millis(500);
        let first = SettingsConfig::from_settings(&settings);
        save(&path, &first).unwrap();
        assert_eq!(load(&path).unwrap(), first);

        settings.setting_help = false;
        let second = SettingsConfig::from_settings(&settings);
        save(&path, &second).unwrap();
        assert_eq!(load(&path).unwrap(), second);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn recording_ui_changes_does_not_capture_unrelated_cli_overrides() {
        let mut saved = SettingsConfig::default();
        let mut before = AppSettings::default();
        before.poll_interval = Duration::from_secs(10);
        let mut after = before.clone();
        after.auto_cycle = false;

        saved.record_changes(&before, &after);
        let mut restored = AppSettings::default();
        saved.apply_to(&mut restored);

        assert!(!restored.auto_cycle);
        assert_eq!(restored.poll_interval, Duration::from_secs(2));
    }

    #[test]
    fn missing_fields_take_current_defaults() {
        let saved: SettingsConfig = serde_json::from_str(r#"{"reduced_motion":true}"#).unwrap();
        let mut restored = AppSettings::default();
        saved.apply_to(&mut restored);

        assert!(restored.reduced_motion);
        assert!(restored.auto_cycle);
        assert_eq!(restored.idle_after, Duration::from_secs(30));
    }

    #[test]
    fn windows_default_path_uses_appdata() {
        let appdata = PathBuf::from("profile").join("AppData").join("Roaming");

        assert_eq!(
            windows_default_path(Some(appdata.to_string_lossy().into_owned())),
            Some(appdata.join("git-buoy").join("settings.json"))
        );
        assert_eq!(windows_default_path(None), None);
    }

    #[test]
    fn unix_default_path_prefers_absolute_xdg_then_falls_back_to_home() {
        let xdg = std::env::temp_dir().join("xdg");
        let home = PathBuf::from("home");

        assert_eq!(
            unix_default_path(
                Some(xdg.to_string_lossy().into_owned()),
                Some(home.to_string_lossy().into_owned())
            ),
            Some(xdg.join("git-buoy").join("settings.json"))
        );
        assert_eq!(
            unix_default_path(Some("relative".into()), Some("home".into())),
            Some(
                PathBuf::from("home")
                    .join(".config")
                    .join("git-buoy")
                    .join("settings.json")
            )
        );
        assert_eq!(unix_default_path(None, None), None);
    }
}
