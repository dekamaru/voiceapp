use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub address: String,
    pub username: String,
}

impl ServerConfig {
    pub fn is_credentials_filled(&self) -> bool {
        !self.address.is_empty() && !self.username.is_empty()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                address: "".to_string(),
                username: "".to_string(),
            },
        }
    }
}

impl AppConfig {
    fn config_path() -> PathBuf {
        let exe_path = std::env::current_exe().expect("Failed to get executable path");
        let exe_dir = exe_path.parent().expect("Failed to get executable directory");
        exe_dir.join("config.toml")
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::config_path();

        if !path.exists() {
            tracing::info!("Config file not found, creating default at {:?}", path);
            return Self::create_default_and_save();
        }

        let contents = fs::read_to_string(&path).map_err(|e| format!("Failed to read config: {}", e))?;

        toml::from_str(&contents).or_else(|e| {
            tracing::warn!("Config file cannot be parsed and will be recreated with defaults, error: {}", e);
            Self::create_default_and_save()
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        let contents = toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(&path, contents).map_err(|e| format!("Failed to write config: {}", e))?;
        Ok(())
    }

    fn create_default_and_save() -> Result<Self, String> {
        let default_config = Self::default();
        default_config.save()?;
        Ok(default_config)
    }
}
