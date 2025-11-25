use iced::{stream, window, Font, Settings, Subscription, Task, Theme};
use iced::application::Appearance;
use iced::Theme::Dark;
use iced::theme::Palette;
use iced::window::settings::PlatformSpecific;
use std::sync::Arc;
use tokio::sync::Mutex;

mod icons;
mod colors;
mod pages;
mod widgets;
mod voice_messages;

use colors::*;
use pages::login::LoginPageMessage;
use pages::login::LoginPage;
use pages::room::RoomPageMessage;
use voiceapp_sdk::{VoiceClient, VoiceClientEvent};
use crate::pages::room::RoomPage;
use voice_messages::{VoiceCommand, VoiceCommandResult};
use async_channel::Receiver;

fn main() -> iced::Result {
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

trait Page {
    fn update(&mut self, message: Message) -> Task<Message>;
    fn view(&self) -> iced::Element<'_, Message>;
}

struct Application {
    page: Box<dyn Page>,
    voice_client: Arc<Mutex<VoiceClient>>,
    events_rx: Receiver<VoiceClientEvent>,
}

impl Application {
    fn new() -> (Self, Task<Message>) {
        let voice_client = VoiceClient::new().expect("failed to init voice client");
        let events_rx = voice_client.event_stream();

        (
            Self {
                page: Box::new(LoginPage::new()),
                voice_client: Arc::new(Mutex::new(voice_client)),
                events_rx
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
        }
    }
}