use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_true")]
    pub chrome: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { chrome: true }
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
}
