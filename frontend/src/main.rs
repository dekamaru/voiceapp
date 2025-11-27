use iced::{Font, Task, Theme};
use iced::Theme::Dark;
use iced::theme::Palette;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing_subscriber;

mod icons;
mod colors;
mod pages;
mod widgets;
mod audio;

use colors::*;
use pages::login::LoginPageMessage;
use pages::login::LoginPage;
use pages::room::RoomPageMessage;
use voiceapp_sdk::{VoiceClient, VoiceClientEvent};
use crate::pages::room::RoomPage;
use async_channel::Receiver;
use crate::audio::AudioManager;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    
    let theme = |_state: &Application| {
        Theme::custom("dark".to_string(), Palette {
            background: background_dark(),
            text: text_primary(),
            ..Dark.palette()
        })
    };

    iced::application("Voiceapp", Application::update, Application::view)
        .theme(theme)
        .font(include_bytes!("../fonts/phosphor-fill.ttf").as_slice())
        .font(include_bytes!("../fonts/phosphor-regular.ttf").as_slice())
        .font(include_bytes!("../fonts/rubik-regular.ttf").as_slice())
        .font(include_bytes!("../fonts/rubik-semibold.ttf").as_slice())
        .default_font(Font::with_name("Rubik"))
        .antialiasing(true)
        .run_with(Application::new)
}

#[derive(Debug, Clone)]
enum Message {
    LoginPage(LoginPageMessage),
    RoomPage(RoomPageMessage),

    // Voice client message bus
    ExecuteVoiceCommand(VoiceCommand),
    VoiceCommandResult(VoiceCommandResult),
    ServerEventReceived(VoiceClientEvent)
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
    Connect(Result<(), String>),
    JoinVoiceChannel(Result<(), String>),
    LeaveVoiceChannel(Result<(), String>),
    SendChatMessage(Result<(), String>),
}

trait Page {
    fn update(&mut self, message: Message) -> Task<Message>;
    fn view(&self) -> iced::Element<'_, Message>;
}

struct Application {
    page: Box<dyn Page>,
    voice_client: Arc<Mutex<VoiceClient>>,
    events_rx: Receiver<VoiceClientEvent>,
    audio_manager: AudioManager,
}

impl Application {
    fn new() -> (Self, Task<Message>) {
        let voice_client = VoiceClient::new().expect("failed to init voice client");
        let audio_manager = AudioManager::new(voice_client.voice_input_sender(), voice_client.get_decoder());
        let events_rx = voice_client.event_stream();

        (
            Self {
                page: Box::new(LoginPage::new()),
                voice_client: Arc::new(Mutex::new(voice_client)),
                events_rx,
                audio_manager
            },
            Task::none()
        )
    }

    fn view(&self) -> iced::Element<Message> {
        self.page.view()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ExecuteVoiceCommand(command) => self.handle_voice_command(command),
            Message::VoiceCommandResult(VoiceCommandResult::Connect(Ok(()))) => {
                self.page = Box::new(RoomPage::new());
                Task::run(self.events_rx.clone(), |e| { Message::ServerEventReceived(e) })
            }
            Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(Ok(()))) => {
                // Start audio when join succeeds
                if let Err(e) = self.audio_manager.start_playback() {
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

    fn handle_voice_command(&mut self, command: VoiceCommand) -> Task<Message> {
        let client = self.voice_client.clone();

        match command {
            VoiceCommand::Connect { management_addr, voice_addr, username } => {
                Task::perform(
                    async move {
                        let mut guard = client.lock().await;
                        guard.connect(&management_addr, &voice_addr, &username).await
                    },
                    move |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::Connect(
                            result.map_err(|e| e.to_string())
                        ))
                    }
                )
            }
            VoiceCommand::JoinVoiceChannel => {
                Task::perform(
                    async move {
                        let mut client_lock = client.lock().await;
                        client_lock.join_channel().await
                    },
                    move |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::JoinVoiceChannel(
                            result.map_err(|e| e.to_string())
                        ))
                    }
                )
            }
            VoiceCommand::LeaveVoiceChannel => {
                Task::perform(
                    async move {
                        let mut guard = client.lock().await;
                        guard.leave_channel().await
                    },
                    move |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::LeaveVoiceChannel(
                            result.map_err(|e| e.to_string())
                        ))
                    }
                )
            }
            VoiceCommand::SendChatMessage(message) => {
                Task::perform(
                    async move {
                        let mut guard = client.lock().await;
                        guard.send_message(&message).await
                    },
                    move |result| {
                        Message::VoiceCommandResult(VoiceCommandResult::SendChatMessage(
                            result.map_err(|e| e.to_string())
                        ))
                    }
                )
            }
        }
    }
}