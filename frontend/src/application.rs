use std::collections::HashMap;
use std::time::Duration;
use crate::audio::{find_best_input_stream_config, find_best_output_stream_config, find_device_by_id, AudioManager};
use crate::pages::login::{LoginPage, LoginPageMessage};
use crate::pages::room::{RoomPage, RoomPageMessage};
use crate::pages::settings::{SettingsPage, SettingsPageMessage};
use iced::{Task, Subscription};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use arc_swap::ArcSwap;
use tracing::{error, info};
use voiceapp_sdk::{Client, ClientEvent};
use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub enum Message {
    LoginPage(LoginPageMessage),
    RoomPage(RoomPageMessage),
    SettingsPage(SettingsPageMessage),
    SwitchPage(PageType),

    // Audio manager
    MuteInput(bool),

    // Voice client message bus
    ExecuteVoiceCommand(VoiceCommand),
    VoiceCommandResult(VoiceCommandResult),
    ServerEventReceived(ClientEvent),
    VoiceInputSamplesReceived(Vec<f32>),

    // Keyboard events
    KeyPressed(iced::keyboard::Key),

    // Config persistence
    PeriodicConfigSave,
    WindowCloseRequested(iced::window::Id),
}

#[derive(Debug, Clone)]
pub enum VoiceCommand {
    Connect {
        management_addr: String,
        voice_addr: String,
        username: String,
    },
    JoinVoiceChannel,
    LeaveVoiceChannel,
    SendChatMessage(String),
}

#[derive(Debug, Clone)]
pub enum VoiceCommandResult {
    Connect(Result<(String, String), String>),  // Ok((address, username))
    JoinVoiceChannel(Result<(), String>),
    LeaveVoiceChannel(Result<(), String>),
    SendChatMessage(Result<(), String>),
}

pub trait Page {
    fn update(&mut self, message: Message) -> Task<Message>;
    fn view(&self) -> iced::Element<'_, Message>;
    fn on_open(&mut self) -> Task<Message> { Task::none() }
    fn on_close(&mut self) -> Task<Message> { Task::none() }
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum PageType {
    Login,
    Room,
    Settings,
}

pub struct Application {
    pages: HashMap<PageType, Box<dyn Page>>,
    current_page: PageType,
    voice_client: Arc<Client>,
    audio_manager: AudioManager,
    config: Arc<ArcSwap<AppConfig>>,
    config_dirty: Arc<AtomicBool>,
}

impl Application {
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load().unwrap();
        let voice_client = Arc::new(Client::new());
        let events_task = Task::run(voice_client.event_stream(), |e| Message::ServerEventReceived(e));
        let auto_login_task = if config.server.is_credentials_filled() {
            info!("Credentials read from config, performing auto login");

            Task::done(
                Message::ExecuteVoiceCommand(
                    VoiceCommand::Connect {
                        management_addr: format!("{}:9001", config.server.address),
                        voice_addr: format!("{}:9002", config.server.address),
                        username: config.server.username.clone(),
                    }
                )
            )
        } else {
            Task::none()
        };

        let config = Arc::new(ArcSwap::from_pointee(config));

        let mut audio_manager = AudioManager::new(config.clone(), voice_client.clone());
        if let Err(e) = audio_manager.init_notification_player() {
            error!("failed to initialize notification player: {}", e);
        };

        let mut pages = HashMap::<PageType, Box<dyn Page>>::from([
            (PageType::Login, Box::new(LoginPage::new(config.clone())) as Box<dyn Page>),
            (PageType::Room, Box::new(RoomPage::new(config.clone())) as Box<dyn Page>),
            (PageType::Settings, Box::new(SettingsPage::new(config.clone())) as Box<dyn Page>)
        ]);

        let on_open_task = pages.get_mut(&PageType::Login).unwrap().on_open();

        (
            Self {
                current_page: PageType::Login,
                pages,
                voice_client,
                audio_manager,
                config,
                config_dirty: Arc::new(AtomicBool::new(false)),
            },
            Task::batch([on_open_task, events_task, auto_login_task]),
        )
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        self.pages.get(&self.current_page).unwrap().view()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let app_task = match &message {
            Message::ExecuteVoiceCommand(command) => self.handle_voice_command(command.clone()),
            Message::VoiceCommandResult(VoiceCommandResult::Connect(Ok((address, username)))) => {
                self.write_config(|config| {
                    config.server.address = address.clone();
                    config.server.username = username.clone();
                });

                Task::done(Message::SwitchPage(PageType::Room))
            }
            Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(Ok(()))) => {
                self.audio_manager.play_notification("join_voice");
                // Start audio when join succeeds
                let users_in_voice = self.voice_client.get_users_in_voice();

                // Start recording
                if let Err(e) = self.audio_manager.start_recording() {
                    error!("Failed to start recording: {}", e);
                }

                // Create output streams for all users currently in voice
                for user_id in users_in_voice {
                    if let Err(e) = self.audio_manager.create_output_stream_for_user(user_id) {
                        error!("Failed to create output stream for user {}: {}", user_id, e);
                    }
                };

                Task::none()
            }
            Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(Ok(()))) => {
                self.audio_manager.play_notification("leave_voice");
                // Stop audio when leave succeeds
                self.audio_manager.stop_recording();
                self.audio_manager.remove_all_output_streams();

                Task::none()
            }
            Message::ServerEventReceived(ClientEvent::UserJoinedVoice { user_id }) => {
                // Create output stream for new user in voice
                if self.voice_client.is_in_voice_channel() {
                    self.audio_manager.play_notification("join_voice");
                    if let Err(e) = self.audio_manager.create_output_stream_for_user(*user_id) {
                        error!("Failed to create output stream for user {}: {}", user_id, e);
                    };
                }

                Task::none()
            }
            Message::ServerEventReceived(ClientEvent::UserLeftVoice { user_id }) => {
                if self.voice_client.is_in_voice_channel() {
                    self.audio_manager.play_notification("leave_voice");
                    self.audio_manager.remove_output_stream_for_user(*user_id);
                }

                Task::none()
            },
            Message::SwitchPage(page_type) => {
                let on_close_task =self.pages.get_mut(&self.current_page).unwrap().on_close();
                let on_open_task = self.pages.get_mut(page_type).unwrap().on_open();
                self.current_page = page_type.clone();

                Task::batch([on_close_task, on_open_task])
            }
            Message::SettingsPage(SettingsPageMessage::SelectInputDevice(device_id)) => {
                info!("Selected input device: {}", device_id);

                // Try to find device by name
                let device = match find_device_by_id(device_id.clone()) {
                    Ok(dev) => dev,
                    Err(e) => {
                        error!("Failed to find device '{}': {}", device_id, e);
                        return Task::none();
                    }
                };

                // Try to get best config for the device
                let best_config = match find_best_input_stream_config(&device) {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to get config for device '{}': {}", device_id, e);
                        return Task::none();
                    }
                };

                // Only write config if everything succeeded
                self.write_config(|config| {
                    config.audio.input_device.device_id = device_id.clone();
                    config.audio.input_device.sample_rate = best_config.0;
                    config.audio.input_device.sample_format = best_config.1.to_string();
                    config.audio.input_device.channels = best_config.2;
                });

                // Refresh input in case if it's changed
                if self.voice_client.is_in_voice_channel() {
                    self.audio_manager.stop_recording();
                    // TODO: error handling
                    let _ = self.audio_manager.start_recording();
                }

                Task::none()
            },
            Message::SettingsPage(SettingsPageMessage::SelectOutputDevice(device_id)) => {
                info!("Selected output device: {}", device_id);

                // Try to find device by name
                let device = match find_device_by_id(device_id.clone()) {
                    Ok(dev) => dev,
                    Err(e) => {
                        error!("Failed to find device '{}': {}", device_id, e);
                        return Task::none();
                    }
                };

                // Try to get best config for the device
                let best_config = match find_best_output_stream_config(&device) {
                    Ok(config) => config,
                    Err(e) => {
                        error!("Failed to get config for device '{}': {}", device_id, e);
                        return Task::none();
                    }
                };

                // Only write config if everything succeeded
                self.write_config(|config| {
                    config.audio.output_device.device_id = device_id.clone();
                    config.audio.output_device.sample_rate = best_config.0;
                    config.audio.output_device.sample_format = best_config.1.to_string();
                    config.audio.output_device.channels = best_config.2;
                });

                if let Err(e) = self.audio_manager.init_notification_player() {
                    error!("failed to initialize notification player: {}", e);
                };

                // Sound feedback on notification player change
                self.audio_manager.play_notification("unmute");

                // Refresh input in case if it's changed
                if self.voice_client.is_in_voice_channel() {
                    self.audio_manager.remove_all_output_streams();

                    let users_in_voice = self.voice_client.get_users_in_voice();
                    // Create output streams for all users currently in voice
                    for user_id in users_in_voice {
                        if let Err(e) = self.audio_manager.create_output_stream_for_user(user_id) {
                            error!("Failed to create output stream for user {}: {}", user_id, e);
                        }
                    };
                }

                Task::none()
            },
            Message::SettingsPage(SettingsPageMessage::InputVolumeChanged(input_volume)) => {
                self.write_config(|config| { config.audio.input_device.volume = *input_volume });
                Task::none()
            }
            Message::SettingsPage(SettingsPageMessage::OutputVolumeChanged(output_volume)) => {
                self.write_config(|config| { config.audio.output_device.volume = *output_volume });
                Task::none()
            }
            Message::SettingsPage(SettingsPageMessage::InputSensitivityChanged(input_sensitivity)) => {
                self.write_config(|config| { config.audio.input_sensitivity = *input_sensitivity });
                Task::none()
            }
            Message::SettingsPage(SettingsPageMessage::NotificationVolumeChanged(notification_volume)) => {
                self.write_config(|config| { config.audio.notification_volume = *notification_volume });
                Task::none()
            }
            Message::RoomPage(RoomPageMessage::UserVolumeChanged(user_id, volume)) => {
                self.write_config(|config| { config.audio.users_volumes.insert(*user_id, *volume); });
                Task::none()
            }
            Message::MuteInput(muted) => {
                if *muted {
                    self.audio_manager.mute_input();
                    self.audio_manager.play_notification("mute");
                } else {
                    self.audio_manager.unmute_input();
                    self.audio_manager.play_notification("unmute");
                }

                let _ = self.voice_client.send_mute_state(*muted);

                Task::none()
            }
            Message::PeriodicConfigSave => {
                self.save_config_if_dirty();
                Task::none()
            }
            Message::WindowCloseRequested(id) => {
                info!("Window close requested, flushing config to disk");
                self.save_config_if_dirty();
                iced::window::close(*id)
            }
            _ => Task::none()
        };

        let mut tasks = vec![app_task];
        tasks.extend(self.pages.values_mut().map(|page| page.update(message.clone())));
        Task::batch(tasks)
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        Subscription::batch([
            // Keyboard events
            iced::event::listen().filter_map(|event| {
                if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
                    Some(Message::KeyPressed(key))
                } else {
                    None
                }
            }),

            // Periodic config save (every 10 seconds)
            iced::time::every(Duration::from_secs(10)).map(|_| Message::PeriodicConfigSave),

            // Save config on window close request
            iced::window::close_requests().map(Message::WindowCloseRequested),
        ])
    }

    fn handle_voice_command(&self, command: VoiceCommand) -> Task<Message> {
        let client = self.voice_client.clone();

        match command {
            VoiceCommand::Connect {
                management_addr,
                voice_addr,
                username,
            } => {
                // Extract server address from management_addr (remove port)
                let server_addr = management_addr.split(':').next().unwrap_or("").to_string();
                let username_clone = username.clone();

                Task::perform(
                    async move { client.connect(&management_addr, &voice_addr, &username).await },
                    |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::Connect(
                            result
                                .map(|_| (server_addr, username_clone))
                                .map_err(|e| e.to_string()),
                        ))
                    },
                )
            }
            VoiceCommand::JoinVoiceChannel => Task::perform(
                async move { client.join_channel().await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::LeaveVoiceChannel => Task::perform(
                async move { client.leave_channel().await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::SendChatMessage(message) => Task::perform(
                async move { client.send_message(&message).await },
                |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::SendChatMessage(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
        }
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
                Ok(_) => {
                    info!("Configuration saved to disk");
                }
                Err(e) => {
                    error!("Failed to save configuration: {}", e);
                    // Set dirty flag back if save failed
                    self.config_dirty.store(true, Ordering::Relaxed);
                }
            }
        }
    }
}
