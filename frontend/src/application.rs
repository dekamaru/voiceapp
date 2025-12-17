use crate::audio::AudioManager;
use crate::pages::login::{LoginPage, LoginPageMessage};
use crate::pages::room::{RoomPage, RoomPageMessage};
use crate::pages::settings::SettingsPageMessage;
use async_channel::Receiver;
use iced::Task;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
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
    voice_client: Arc<Mutex<VoiceClient>>,
    events_rx: Receiver<VoiceClientEvent>,
    audio_manager: AudioManager,
    config: Arc<RwLock<AppConfig>>,
}

impl Application {
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load().unwrap();

        // Detect output device sample rate before creating VoiceClient
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let output_device = host.default_output_device()
            .expect("No output device found");
        let default_config = output_device.default_output_config()
            .expect("Failed to get output config");
        let sample_rate = default_config.sample_rate().0;
        tracing::info!("Detected output device sample rate: {} Hz", sample_rate);

        let voice_client = VoiceClient::new(sample_rate).expect("failed to init voice client");
        let audio_manager = AudioManager::new(voice_client.get_udp_send_tx());
        let events_rx = voice_client.event_stream();

        let login_task = if config.server.is_credentials_filled() {
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

        let locked_config = Arc::new(RwLock::new(config));

        (
            Self {
                // TODO: pass auto-login here so login can draw loading state instead
                page: Box::new(LoginPage::new(locked_config.clone())),
                voice_client: Arc::new(Mutex::new(voice_client)),
                events_rx,
                audio_manager,
                config: locked_config,
            },
            login_task,
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
                Task::run(self.events_rx.clone(), |e| Message::ServerEventReceived(e))
            }
            Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(Ok(()))) => {
                // Start audio when join succeeds
                // Get decoder from VoiceClient (which is already receiving voice packets)
                let voice_client = self.voice_client.blocking_lock();
                let decoder = voice_client.get_decoder();
                drop(voice_client);

                if let Err(e) = self.audio_manager.start_playback(decoder) {
                    tracing::error!("Failed to start audio playback: {}", e);
                }
                if let Err(e) = self.audio_manager.start_recording() {
                    tracing::error!("Failed to start recording: {}", e);
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

    fn handle_voice_command(&mut self, command: VoiceCommand) -> Task<Message> {
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
                    async move {
                        let mut guard = client.lock().await;
                        guard
                            .connect(&management_addr, &voice_addr, &username)
                            .await
                    },
                    move |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::Connect(
                            result
                                .map(|_| (server_addr, username_clone))
                                .map_err(|e| e.to_string()),
                        ))
                    },
                )
            }
            VoiceCommand::JoinVoiceChannel => Task::perform(
                async move {
                    let mut client_lock = client.lock().await;
                    client_lock.join_channel().await
                },
                move |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::LeaveVoiceChannel => Task::perform(
                async move {
                    let mut guard = client.lock().await;
                    guard.leave_channel().await
                },
                move |result| {
                    Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(
                        result.map_err(|e| e.to_string()),
                    ))
                },
            ),
            VoiceCommand::SendChatMessage(message) => Task::perform(
                async move {
                    let mut guard = client.lock().await;
                    guard.send_message(&message).await
                },
                move |result| {
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
