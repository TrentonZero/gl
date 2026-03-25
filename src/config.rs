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
