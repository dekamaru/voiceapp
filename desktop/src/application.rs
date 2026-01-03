use std::collections::HashMap;
use std::time::Duration;
use crate::audio::{AudioManager};
use crate::view::login::{LoginPage, LoginPageMessage};
use crate::view::room::{RoomPage, RoomPageMessage};
use crate::view::settings::{SettingsPage, SettingsPageMessage};
use iced::{Task, Subscription};
use std::sync::{Arc};
use arc_swap::ArcSwap;
use tracing::info;
use voiceapp_sdk::{Client, ClientEvent};
use crate::config::AppConfig;
use crate::state::audio_manager::AudioManagerState;
use crate::state::config::ConfigState;
use crate::state::State;
use crate::state::voice_client::{VoiceClientState, VoiceCommand, VoiceCommandResult};
use crate::view::view::View;

#[derive(Debug, Clone)]
pub enum Message {
    LoginPage(LoginPageMessage),
    RoomPage(RoomPageMessage),
    SettingsPage(SettingsPageMessage),
    SwitchView(ViewType),

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

    None,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum ViewType {
    Login,
    Room,
    Settings
}

pub struct Application {
    state_handlers: Vec<Box<dyn State>>,
    views: HashMap<ViewType, Box<dyn View>>,
    current_view: ViewType,
}

impl Application {
    pub fn new() -> (Self, Task<Message>) {
        let config = Arc::new(ArcSwap::from_pointee(AppConfig::load().unwrap()));
        let voice_client = Arc::new(Client::new());
        let audio_manager = AudioManager::new(config.clone(), voice_client.clone());

        let state_handlers: Vec<Box<dyn State>> = vec![
            Box::new(ConfigState::new(config.clone())),
            Box::new(AudioManagerState::new(audio_manager)),
            Box::new(VoiceClientState::new(voice_client.clone()))
        ];

        let views = HashMap::<ViewType, Box<dyn View>>::from([
            (ViewType::Login, Box::new(LoginPage::new(config.clone())) as Box<dyn View>),
            (ViewType::Room, Box::new(RoomPage::new(config.clone())) as Box<dyn View>),
            (ViewType::Settings, Box::new(SettingsPage::new(config.clone())) as Box<dyn View>)
        ]);

        let mut application = Self {
            state_handlers,
            views,
            current_view: ViewType::Login,
        };

        let task = application.init(config.clone());

        (application, task)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let state_tasks: Vec<Task<Message>> = self.state_handlers.iter_mut()
            .map(|s| s.update(message.clone()))
            .collect();

        let view_tasks: Vec<Task<Message>> = self.views.iter_mut()
            .map(|(_, v)| { v.update(message.clone()) })
            .collect();

        let application_task = match message.clone() {
            Message::SwitchView(view_type) =>  {
                let on_close_task = self.views.get_mut(&self.current_view).unwrap().on_close();
                let on_open_task = self.views.get_mut(&view_type).unwrap().on_open();
                self.current_view = view_type.clone();

                Task::batch([on_close_task, on_open_task])
            },
            Message::WindowCloseRequested(id) => { iced::window::close(id) }
            _ => Task::none(),
        };

        Task::batch([Task::batch(state_tasks), Task::batch(view_tasks), application_task])
    }

    pub fn render(&self) -> iced::Element<'_, Message> {
        self.views[&self.current_view].render()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            // Keyboard events
            iced::event::listen().filter_map(|event| {
                if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
                    Some(Message::KeyPressed(key))
                } else {
                    None
                }
            }),
            iced::time::every(Duration::from_secs(5)).map(|_| Message::ExecuteVoiceCommand(VoiceCommand::Ping)),
            iced::time::every(Duration::from_millis(500)).map(|_| Message::ExecuteVoiceCommand(VoiceCommand::GetVoiceStats)),
            iced::time::every(Duration::from_secs(10)).map(|_| Message::PeriodicConfigSave),
            iced::window::close_requests().map(Message::WindowCloseRequested)
        ])
    }

    fn init(&mut self, config: Arc<ArcSwap<AppConfig>>) -> Task<Message> {
        let config = config.load();

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

        let mut tasks: Vec<Task<Message>> = self
            .state_handlers
            .iter_mut()
            .map(|s| s.init())
            .filter(|task| task.units() != 0)
            .collect();

        tasks.push(auto_login_task);

        Task::batch(tasks)
    }
}