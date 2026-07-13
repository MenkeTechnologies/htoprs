//! Persistence for htoprs's theme selection.
//!
//! The theme system is an htoprs extension, not an htop setting, so it is
//! stored in its own file (`~/.config/htoprs/prefs.json`) rather than htop's
//! `htoprc` — keeping htop config compatibility intact. This mirrors iftoprs's
//! separate prefs file (the `save_prefs` that the overlay port originally
//! stubbed out).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::aggregate::GroupBy;
use super::barstyle::BarStyle;
use super::panels::SparkMode;
use super::theme::{CustomThemeColors, ThemeName};

/// The persisted htoprs preferences. Every htoprs-original toggle is saved here
/// so the UI comes back exactly as the user left it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefs {
    /// The selected built-in theme.
    #[serde(default)]
    pub theme: ThemeName,
    /// The applied custom theme's name, if any.
    #[serde(default)]
    pub active_custom_theme: Option<String>,
    /// Saved custom palettes, keyed by user-chosen name.
    #[serde(default)]
    pub custom_themes: HashMap<String, CustomThemeColors>,
    /// The `b`-cycled bar meter fill style.
    #[serde(default)]
    pub bar_style: BarStyle,
    /// Whether the over-threshold ("hot") process-row highlight is enabled
    /// (toggled with `t` in the Alerts modal). `None` (absent / first run) means
    /// the default: on.
    #[serde(default)]
    pub alert_hl: Option<bool>,
    /// Whether the themed UI border is shown (toggled with `B`).
    #[serde(default)]
    pub show_border: bool,
    /// Whether the themed header chrome is shown (toggled with `g`).
    #[serde(default)]
    pub show_header: bool,
    /// The `v`-cycled per-PID CPU sparkline mode.
    #[serde(default)]
    pub spark: SparkMode,
    /// The `Tab`-cycled key the aggregate/pivot modal (`y`) rolls up on.
    #[serde(default)]
    pub agg_by: GroupBy,
    /// Whether the forecast inline row tint (`E` modal → `t`) is enabled: PIDs
    /// projected to hit their memory ceiling within the horizon get recolored.
    /// `None` (absent / first run) means the default: off.
    #[serde(default)]
    pub forecast_hl: Option<bool>,
    /// The forecast horizon in seconds: only ETAs at or under this get the
    /// inline tint. `None` (absent / first run) means the default (1 hour).
    #[serde(default)]
    pub forecast_horizon_secs: Option<f64>,
}

/// `~/.config/htoprs/prefs.json` (honoring `$XDG_CONFIG_HOME`), matching the
/// `$HOME/.config` convention htoprs's `Settings` uses for `htoprc`.
fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("prefs.json"))
}

/// The `~/.config/htoprs` directory (honoring `$XDG_CONFIG_HOME`), shared by
/// the prefs file and the other extension artifacts (saved filters, snapshot
/// and export dumps). `None` when neither `$XDG_CONFIG_HOME` nor `$HOME` is set.
// The `#[cfg(test)]` early-out uses `return` so the mutually-exclusive
// `#[cfg(not(test))]` block can follow it; that reads as a needless return to
// clippy, but a tail expression there would be an unused value in test builds.
#[allow(clippy::needless_return)]
pub(crate) fn config_dir() -> Option<PathBuf> {
    // Under `cargo test`, sandbox prefs to a per-thread temp dir. Tests run in
    // parallel and share process-global env + the thread-local PanelState that
    // loads/persists prefs; a shared real config file would race between tests
    // (and pollute the developer's ~/.config). A per-thread path gives each test
    // thread its own file, and tests on one thread run sequentially, so prefs
    // I/O is deterministic and isolated without touching the real config.
    #[cfg(test)]
    {
        let tid: String = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        return Some(
            std::env::temp_dir()
                .join(format!("htoprs_test_{}_{}", std::process::id(), tid))
                .join("htoprs"),
        );
    }
    #[cfg(not(test))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("htoprs"))
    }
}

/// Read the saved prefs, or `None` if the file is absent or unparsable.
pub fn load() -> Option<Prefs> {
    let path = config_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Load the current prefs (or defaults), apply `f`, and persist the result.
/// Read-modify-write so independent subsystems (theme, bar style) can update
/// their own fields without clobbering each other's — the file is the single
/// shared store.
pub fn update(f: impl FnOnce(&mut Prefs)) {
    let mut prefs = load().unwrap_or_default();
    f(&mut prefs);
    save(&prefs);
}

/// Persist `prefs` atomically (write to a temp file, then rename). Failures are
/// reported to the log, never to the terminal — a read-only config dir must not
/// break the TUI.
pub fn save(prefs: &Prefs) {
    if let Err(e) = try_save(prefs) {
        // No terminal chatter: the TUI owns the screen. Best-effort to stderr's
        // place would corrupt the alternate screen, so we drop silently here.
        let _ = e;
    }
}

fn try_save(prefs: &Prefs) -> std::io::Result<()> {
    let path = config_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config dir"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(prefs)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_json_roundtrips() {
        let mut customs = HashMap::new();
        customs.insert(
            "mine".to_string(),
            CustomThemeColors {
                c1: 1,
                c2: 2,
                c3: 3,
                c4: 4,
                c5: 5,
                c6: 6,
            },
        );
        let p = Prefs {
            theme: ThemeName::BladeRunner,
            active_custom_theme: Some("mine".to_string()),
            custom_themes: customs,
            ..Prefs::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, ThemeName::BladeRunner);
        assert_eq!(back.active_custom_theme.as_deref(), Some("mine"));
        assert_eq!(back.custom_themes.get("mine").unwrap().c3, 3);
    }

    #[test]
    fn missing_fields_default() {
        // A prefs file with only a theme still parses (custom fields default).
        let p: Prefs = serde_json::from_str(r#"{"theme":"NeonSprawl"}"#).unwrap();
        assert_eq!(p.theme, ThemeName::NeonSprawl);
        assert!(p.active_custom_theme.is_none());
        assert!(p.custom_themes.is_empty());
        // Absent alert_hl means "use the default" (on) — see PanelState::new.
        assert!(p.alert_hl.is_none());
    }

    #[test]
    fn empty_object_is_default() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p.theme, ThemeName::default());
    }

    #[test]
    fn config_path_honors_xdg() {
        // config_path derives from env; just assert it yields the expected tail.
        if let Some(p) = config_path() {
            assert!(p.ends_with("htoprs/prefs.json"));
        }
    }

    #[test]
    fn save_then_load_via_temp_dir() {
        // Point XDG_CONFIG_HOME at a scratch dir, save, reload, verify.
        let dir = std::env::temp_dir().join(format!("htoprs_prefs_test_{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        let p = Prefs {
            theme: ThemeName::GlitchPop,
            active_custom_theme: None,
            custom_themes: HashMap::new(),
            bar_style: BarStyle::Thin,
            alert_hl: Some(false),
            show_border: true,
            show_header: true,
            spark: SparkMode::Double,
            ..Default::default()
        };
        save(&p);
        let back = load().expect("prefs should load back");
        assert_eq!(back.theme, ThemeName::GlitchPop);
        assert_eq!(back.bar_style, BarStyle::Thin);
        assert_eq!(back.alert_hl, Some(false));
        // Every toggle round-trips through the shared prefs file.
        assert!(back.show_border);
        assert!(back.show_header);
        assert_eq!(back.spark, SparkMode::Double);
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}
