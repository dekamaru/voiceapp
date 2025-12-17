use crate::audio::AudioManager;
use crate::pages::login::{LoginPage, LoginPageMessage};
use crate::pages::room::{RoomPage, RoomPageMessage};
use crate::pages::settings::SettingsPageMessage;
use iced::Task;
use std::sync::{Arc, RwLock};
use tracing::{error, info};
use voiceapp_sdk::{VoiceClient, VoiceClientEvent};
use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub enum Message {
    LoginPage(LoginPageMessage),
    RoomPage(RoomPageMessage),
    SettingsPage(SettingsPageMessage),

    // Voice client message bus
    ExecuteVoiceCommand(VoiceCommand),
    VoiceCommandResult(VoiceCommandResult),
    ServerEventReceived(VoiceClientEvent),
    VoiceInputSamplesReceived(Vec<f32>),

    // Keyboard events
    KeyPressed(iced::keyboard::Key),
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
}

pub struct Application {
    page: Box<dyn Page>,
    voice_client: Arc<VoiceClient>,
    audio_manager: AudioManager,
    config: Arc<RwLock<AppConfig>>,
}

impl Application {
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load().unwrap();
        let voice_client = Arc::new(VoiceClient::new(config.audio.output_device.sample_rate).expect("failed to init voice client"));
        let audio_manager = AudioManager::new(voice_client.clone());

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

        let config = Arc::new(RwLock::new(config));

        (
            Self {
                // TODO: pass auto-login here so login can draw loading state instead
                page: Box::new(LoginPage::new(config.clone())),
                voice_client,
                audio_manager,
                config,
            },
            Task::batch([events_task, auto_login_task]),
        )
    }

    pub fn view(&self) -> iced::Element<Message> {
        self.page.view()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ExecuteVoiceCommand(command) => self.handle_voice_command(command),
            Message::VoiceCommandResult(VoiceCommandResult::Connect(Ok((address, username)))) => {
                self.write_config(|config| {
                    config.server.address = address;
                    config.server.username = username;
                });

                self.page = Box::new(RoomPage::new());
                Task::none()
            }
            Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(Ok(()))) => {
                // Start audio when join succeeds
                let users_in_voice = self.voice_client.get_users_in_voice();

                // Start recording
                if let Err(e) = self.audio_manager.start_recording() {
                    tracing::error!("Failed to start recording: {}", e);
                }

                // Create output streams for all users currently in voice
                for user_id in users_in_voice {
                    if let Err(e) = self.audio_manager.create_stream_for_user(user_id) {
                        tracing::error!("Failed to create output stream for user {}: {}", user_id, e);
                    } else {
                        tracing::info!("Created output stream for existing user {}", user_id);
                    }
                }

                // propagate further
                self.page.update(message)
            }
            Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(Ok(()))) => {
                // Stop audio when leave succeeds
                self.audio_manager.stop_recording();
                self.audio_manager.stop_playback();

                // propagate further
                self.page.update(message)
            }
            Message::ServerEventReceived(VoiceClientEvent::UserJoinedVoice { user_id }) => {
                // Create output stream for new user in voice
                if let Err(e) = self.audio_manager.create_stream_for_user(user_id) {
                    tracing::error!("Failed to create output stream for user {}: {}", user_id, e);
                }

                // Propagate to page for UI updates
                self.page.update(message)
            }
            Message::ServerEventReceived(VoiceClientEvent::UserLeftVoice { user_id }) => {
                // Remove output stream for user who left voice
                self.audio_manager.remove_stream_for_user(user_id);

                // propagate further
                self.page.update(message)
            }
            other => self.page.update(other),
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        iced::event::listen().filter_map(|event| {
            if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
                Some(Message::KeyPressed(key))
            } else {
                None
            }
        })
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
        let mut config = self.config.write().expect("cannot obtain config write lock");
        updater(&mut config);
        match config.save() {
            Ok(_) => {}
            Err(e) => {
                error!("failed to save configuration: {}", e);
            }
        }
    }
}
