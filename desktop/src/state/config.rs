use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use arc_swap::ArcSwap;
use iced::Task;
use tracing::{error, info};
use crate::application::Message;
use crate::audio::{find_best_input_stream_config, find_best_output_stream_config, find_device_by_id};
use crate::config::AppConfig;
use crate::view::room::RoomPageMessage;
use crate::view::settings::SettingsPageMessage;
use crate::state::State;
use crate::state::voice_client::VoiceCommandResult;

pub struct ConfigState {
    config: Arc<ArcSwap<AppConfig>>,
    config_dirty: Arc<AtomicBool>
}

impl ConfigState {
    pub fn new(config: Arc<ArcSwap<AppConfig>>) -> Self {
        Self { config, config_dirty: Arc::new(AtomicBool::new(false)) }
    }

    fn write_config<F>(&self, updater: F)
    where
        F: FnOnce(&mut AppConfig),
    {
        let current_config = self.config.load_full();
        let mut new_config = (*current_config).clone();
        updater(&mut new_config);

        // Only mark dirty if config actually changed
        if *current_config != new_config {
            let new_arc = Arc::new(new_config);
            self.config.store(new_arc);
            self.config_dirty.store(true, Ordering::Relaxed);
        }
    }

    fn save_config_if_dirty(&self) {
        if self.config_dirty.swap(false, Ordering::Relaxed) {
            let config = self.config.load_full();
            match config.save() {
                Ok(_) => { info!("Configuration saved to disk"); }
                Err(e) => {
                    error!("Failed to save configuration: {}", e);
                    // Set dirty flag back if save failed
                    self.config_dirty.store(true, Ordering::Relaxed);
                }
            }
        }
    }
}

impl State for ConfigState {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::VoiceCommandResult(VoiceCommandResult::Connect(Ok((_, address, username)))) => {
                self.write_config(|config| {
                    config.server.address = address.clone();
                    config.server.username = username.clone();
                });
            },
            Message::SettingsPage(SettingsPageMessage::SelectInputDevice(device_id)) => {
                let device = match find_device_by_id(device_id.clone()) {
                    Ok(dev) => dev,
                    Err(e) => {
                        error!("Failed to find device '{}': {}", device_id, e);
                        // TODO: RETURN FAIL
                        return Task::none();
                    }
                };

                let best_config = match find_best_input_stream_config(&device) {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to get config for device '{}': {}", device_id, e);
                        // TODO: RETURN FAIL
                        return Task::none();
                    }
                };

                self.write_config(|config| {
                    config.audio.input_device.device_id = device_id.clone();
                    config.audio.input_device.sample_rate = best_config.0;
                    config.audio.input_device.sample_format = best_config.1.to_string();
                    config.audio.input_device.channels = best_config.2;
                });
            },
            Message::SettingsPage(SettingsPageMessage::SelectOutputDevice(device_id)) => {
                let device = match find_device_by_id(device_id.clone()) {
                    Ok(dev) => dev,
                    Err(e) => {
                        error!("Failed to find device '{}': {}", device_id, e);
                        // TODO: RETURN FAIL
                        return Task::none();
                    }
                };

                let best_config = match find_best_output_stream_config(&device) {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to get config for device '{}': {}", device_id, e);
                        // TODO: RETURN FAIL
                        return Task::none();
                    }
                };

                self.write_config(|config| {
                    config.audio.output_device.device_id = device_id.clone();
                    config.audio.output_device.sample_rate = best_config.0;
                    config.audio.output_device.sample_format = best_config.1.to_string();
                    config.audio.output_device.channels = best_config.2;
                });
            },
            Message::SettingsPage(SettingsPageMessage::InputVolumeChanged(input_volume)) => {
                self.write_config(|config| { config.audio.input_device.volume = input_volume });
            }
            Message::SettingsPage(SettingsPageMessage::OutputVolumeChanged(output_volume)) => {
                self.write_config(|config| { config.audio.output_device.volume = output_volume });
            }
            Message::SettingsPage(SettingsPageMessage::InputSensitivityChanged(input_sensitivity)) => {
                self.write_config(|config| { config.audio.input_sensitivity = input_sensitivity });
            }
            Message::SettingsPage(SettingsPageMessage::NotificationVolumeChanged(notification_volume)) => {
                self.write_config(|config| { config.audio.notification_volume = notification_volume });
            }
            Message::RoomPage(RoomPageMessage::UserVolumeChanged(user_id, volume)) => {
                self.write_config(|config| { config.audio.users_volumes.insert(user_id, volume); });
            }
            Message::PeriodicConfigSave | Message::WindowCloseRequested(_) => { self.save_config_if_dirty() },
            _ => {}
        }

        Task::none()
    }
}