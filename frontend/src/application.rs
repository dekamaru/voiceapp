use std::collections::HashMap;
use crate::audio::AudioManager;
use crate::pages::login::{LoginPage, LoginPageMessage};
use crate::pages::room::{RoomPage, RoomPageMessage};
use crate::pages::settings::{SettingsPage, SettingsPageMessage};
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

    SwitchPage(PageType),

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

        let mut pages = HashMap::<PageType, Box<dyn Page>>::from([
            (PageType::Login, Box::new(LoginPage::new(config.clone())) as Box<dyn Page>),
            (PageType::Room, Box::new(RoomPage::new()) as Box<dyn Page>),
            (PageType::Settings, Box::new(SettingsPage::new()) as Box<dyn Page>)
        ]);

        let on_open_task = pages.get_mut(&PageType::Login).unwrap().on_open();

        (
            Self {
                current_page: PageType::Login,
                pages,
                voice_client,
                audio_manager,
                config,
            },
            Task::batch([on_open_task, events_task, auto_login_task]),
        )
    }

    pub fn view(&self) -> iced::Element<Message> {
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
                // Start audio when join succeeds
                let users_in_voice = self.voice_client.get_users_in_voice();

                // Start recording
                if let Err(e) = self.audio_manager.start_recording() {
                    error!("Failed to start recording: {}", e);
                }

                // Create output streams for all users currently in voice
                for user_id in users_in_voice {
                    if let Err(e) = self.audio_manager.create_stream_for_user(user_id) {
                        error!("Failed to create output stream for user {}: {}", user_id, e);
                    }
                };

                Task::none()
            }
            Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(Ok(()))) => {
                // Stop audio when leave succeeds
                self.audio_manager.stop_recording();
                self.audio_manager.stop_playback();

                Task::none()
            }
            Message::ServerEventReceived(VoiceClientEvent::UserJoinedVoice { user_id }) => {
                // Create output stream for new user in voice
                if self.voice_client.is_in_voice_channel() {
                    if let Err(e) = self.audio_manager.create_stream_for_user(*user_id) {
                        error!("Failed to create output stream for user {}: {}", user_id, e);
                    };
                }

                Task::none()
            }
            Message::ServerEventReceived(VoiceClientEvent::UserLeftVoice { user_id }) => {
                if self.voice_client.is_in_voice_channel() {
                    self.audio_manager.remove_stream_for_user(*user_id);
                }

                Task::none()
            },
            Message::SwitchPage(pageType) => {
                let on_close_task =self.pages.get_mut(&self.current_page).unwrap().on_close();
                let on_open_task = self.pages.get_mut(pageType).unwrap().on_open();
                self.current_page = pageType.clone();

                Task::batch([on_close_task, on_open_task])
            }
            _ => Task::none()
        };

        let mut tasks = vec![app_task];
        tasks.extend(self.pages.values_mut().map(|page| page.update(message.clone())));
        Task::batch(tasks)
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
