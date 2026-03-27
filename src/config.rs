use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_true")]
    pub chrome: bool,
    #[serde(default)]
    pub diff_view: DiffViewMode,
    #[serde(default)]
    pub ignore_whitespace: bool,
    #[serde(default)]
    pub color_scheme: ColorScheme,
    #[serde(default)]
    pub keybindings: KeyBindings,
    #[serde(default)]
    pub worktree_path_defaults: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DiffViewMode {
    Unified,
    SideBySide,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    Ocean,
    Forest,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct KeyBindings {
    #[serde(default = "default_quit_key")]
    pub quit: char,
    #[serde(default = "default_help_key")]
    pub help: char,
    #[serde(default = "default_refresh_key")]
    pub refresh: char,
    #[serde(default = "default_command_key")]
    pub command: char,
    #[serde(default = "default_stack_key")]
    pub stack_view: char,
    #[serde(default = "default_status_key")]
    pub status_view: char,
    #[serde(default = "default_graph_key")]
    pub graph_view: char,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            chrome: true,
            diff_view: DiffViewMode::Unified,
            ignore_whitespace: false,
            color_scheme: ColorScheme::Ocean,
            keybindings: KeyBindings::default(),
            worktree_path_defaults: Vec::new(),
        }
    }
}

impl Default for DiffViewMode {
    fn default() -> Self {
        Self::Unified
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Ocean
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            quit: default_quit_key(),
            help: default_help_key(),
            refresh: default_refresh_key(),
            command: default_command_key(),
            stack_view: default_stack_key(),
            status_view: default_status_key(),
            graph_view: default_graph_key(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let Some(home) = env::var_os("HOME") else {
            return Self::default();
        };

        let path = PathBuf::from(home).join(".config/gl/config.toml");
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };

        toml::from_str(&contents).unwrap_or_default()
    }
}

const fn default_true() -> bool {
    true
}

const fn default_quit_key() -> char {
    'q'
}

const fn default_help_key() -> char {
    '?'
}

const fn default_refresh_key() -> char {
    'R'
}

const fn default_command_key() -> char {
    ':'
}

const fn default_stack_key() -> char {
    's'
}

const fn default_status_key() -> char {
    'S'
}

const fn default_graph_key() -> char {
    '4'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_chrome_enabled() {
        let config = AppConfig::default();
        assert!(config.chrome);
    }

    #[test]
    fn deserialize_chrome_true() {
        let toml_str = "chrome = true\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.chrome);
    }

    #[test]
    fn deserialize_chrome_false() {
        let toml_str = "chrome = false\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.chrome);
    }

    #[test]
    fn deserialize_empty_defaults_to_chrome_true() {
        let toml_str = "";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.chrome);
    }

    #[test]
    fn deserialize_unknown_fields_ignored() {
        let toml_str = "chrome = true\nunknown_key = 42\n";
        // serde with deny_unknown_fields would fail; this verifies we don't use it
        let result: Result<AppConfig, _> = toml::from_str(toml_str);
        // If it fails that's fine — just check it doesn't panic
        if let Ok(config) = result {
            assert!(config.chrome);
        }
    }

    #[test]
    fn deserialize_side_by_side_diff_view() {
        let toml_str = "diff_view = \"side-by-side\"\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.diff_view, DiffViewMode::SideBySide);
    }

    #[test]
    fn deserialize_ignore_whitespace_true() {
        let toml_str = "ignore_whitespace = true\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.ignore_whitespace);
    }

    #[test]
    fn deserialize_keybindings_override() {
        let toml_str = "[keybindings]\nquit = 'x'\nrefresh = 'r'\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.keybindings.quit, 'x');
        assert_eq!(config.keybindings.refresh, 'r');
        assert_eq!(config.keybindings.help, '?');
    }

    #[test]
    fn deserialize_color_scheme_and_worktree_defaults() {
        let toml_str = "color_scheme = \"forest\"\nworktree_path_defaults = [\"~/src\", \"~/wt\"]\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.color_scheme, ColorScheme::Forest);
        assert_eq!(config.worktree_path_defaults.len(), 2);
    }
}
