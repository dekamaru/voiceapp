use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use cpal::traits::{DeviceTrait, HostTrait};
use crate::audio::{find_best_input_stream_config, find_best_output_stream_config};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub audio: AudioConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub address: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioConfig {
    pub input_device: AudioDevice,
    pub output_device: AudioDevice,
    pub input_sensitivity: u8,
    pub users_volumes: HashMap<u64, u8>,
    pub notification_volume: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioDevice {
    pub device_id: String,
    pub sample_rate: u32,
    pub sample_format: String,
    pub channels: u16,
    pub volume: u8,
}

impl ServerConfig {
    pub fn is_credentials_filled(&self) -> bool {
        !self.address.is_empty() && !self.username.is_empty()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        // Audio defaults
        let host = cpal::default_host();
        let input_device = host.default_input_device().expect("failed to get default input device");
        let output_device = host.default_output_device().expect("failed to get default output device");
        let input_stream_config = find_best_input_stream_config(&input_device).expect("failed to find best input stream config");
        let output_stream_config = find_best_output_stream_config(&output_device).expect("failed to find best output stream config");

        Self {
            server: ServerConfig {
                address: "".to_string(),
                username: "".to_string(),
            },
            audio: AudioConfig {
                input_device: AudioDevice {
                    device_id: input_device.id().expect("failed to get input device id").to_string(),
                    sample_rate: input_stream_config.0,
                    sample_format: input_stream_config.1.to_string(),
                    channels: input_stream_config.2,
                    volume: 100,
                },
                input_sensitivity: 0,
                output_device: AudioDevice {
                    device_id: output_device.id().expect("failed to get input device id").to_string(),
                    sample_rate: output_stream_config.0,
                    sample_format: output_stream_config.1.to_string(),
                    channels: output_stream_config.2,
                    volume: 100,
                },
                users_volumes: HashMap::new(),
                notification_volume: 100,
            }
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
