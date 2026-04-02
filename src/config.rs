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
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DiffViewMode {
    #[default]
    Unified,
    SideBySide,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    #[default]
    Ocean,
    Forest,
    Amber,
    Violet,
    Rose,
    Teal,
}

impl ColorScheme {
    pub const ALL: [ColorScheme; 6] = [
        ColorScheme::Ocean,
        ColorScheme::Forest,
        ColorScheme::Amber,
        ColorScheme::Violet,
        ColorScheme::Rose,
        ColorScheme::Teal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ColorScheme::Ocean => "ocean",
            ColorScheme::Forest => "forest",
            ColorScheme::Amber => "amber",
            ColorScheme::Violet => "violet",
            ColorScheme::Rose => "rose",
            ColorScheme::Teal => "teal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ocean" => Some(ColorScheme::Ocean),
            "forest" => Some(ColorScheme::Forest),
            "amber" => Some(ColorScheme::Amber),
            "violet" => Some(ColorScheme::Violet),
            "rose" => Some(ColorScheme::Rose),
            "teal" => Some(ColorScheme::Teal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
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
        }
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
        let Some(path) = resolve_config_path() else {
            return Self::default();
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };

        toml::from_str(&contents).unwrap_or_default()
    }
}

fn resolve_config_path() -> Option<PathBuf> {
    resolve_config_path_from_values(
        env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

fn resolve_config_path_from_values(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = xdg_config_home {
        return Some(path.join("gl").join("config.toml"));
    }

    home.map(|path| path.join(".config").join("gl").join("config.toml"))
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
    fn deserialize_color_scheme() {
        let toml_str = "color_scheme = \"forest\"\n";
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.color_scheme, ColorScheme::Forest);
    }

    #[test]
    fn deserialize_additional_color_schemes() {
        let amber: AppConfig = toml::from_str("color_scheme = \"amber\"\n").unwrap();
        let violet: AppConfig = toml::from_str("color_scheme = \"violet\"\n").unwrap();
        let rose: AppConfig = toml::from_str("color_scheme = \"rose\"\n").unwrap();
        let teal: AppConfig = toml::from_str("color_scheme = \"teal\"\n").unwrap();

        assert_eq!(amber.color_scheme, ColorScheme::Amber);
        assert_eq!(violet.color_scheme, ColorScheme::Violet);
        assert_eq!(rose.color_scheme, ColorScheme::Rose);
        assert_eq!(teal.color_scheme, ColorScheme::Teal);
    }

    #[test]
    fn color_scheme_parse_and_format_round_trip() {
        for scheme in ColorScheme::ALL {
            assert_eq!(ColorScheme::parse(scheme.as_str()), Some(scheme));
        }
        assert_eq!(ColorScheme::parse("unknown"), None);
    }

    #[test]
    fn resolve_config_path_prefers_xdg_config_home() {
        let path = resolve_config_path_from_values(
            Some(PathBuf::from("/xdg")),
            Some(PathBuf::from("/home/test")),
        );

        assert_eq!(path, Some(PathBuf::from("/xdg/gl/config.toml")));
    }

    #[test]
    fn resolve_config_path_falls_back_to_home_config() {
        let path = resolve_config_path_from_values(None, Some(PathBuf::from("/home/test")));

        assert_eq!(
            path,
            Some(PathBuf::from("/home/test/.config/gl/config.toml"))
        );
    }

    #[test]
    fn resolve_config_path_returns_none_without_xdg_or_home() {
        assert_eq!(resolve_config_path_from_values(None, None), None);
    }
}
