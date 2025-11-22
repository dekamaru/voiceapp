use iced::{window, Settings, Task, Theme};
use iced::application::Appearance;
use iced::Theme::Dark;
use iced::theme::Palette;
use iced::window::settings::PlatformSpecific;

mod icons;
mod colors;
mod pages;
mod widgets;

use colors::*;
use pages::login::LoginPageMessage;
use pages::login::LoginPage;
use pages::room::RoomPageMessage;

fn main() -> iced::Result {
    let settings = Settings {
        fonts: vec![
            include_bytes!("../fonts/phosphor-fill.ttf").as_slice().into(),
            include_bytes!("../fonts/phosphor-regular.ttf").as_slice().into(),
        ],
        ..Settings::default()
    };

    let theme = |_state: &Application| {
        Theme::custom("dark".to_string(), Palette {
            background: background_dark(),
            text: text_primary(),
            ..Dark.palette()
        })
    };

    let window_settings = window::Settings {
        platform_specific: PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true
        },
        ..window::Settings::default()
    };

    iced::application("Voiceapp", Application::update, Application::view)
        .theme(theme)
        .settings(settings)
        .window(window_settings)
        .run_with(Application::new)
}

#[derive(Debug, Clone)]
enum Message {
    LoginPage(LoginPageMessage),
    RoomPage(RoomPageMessage),
}

trait Page {
    fn update(&mut self, message: Message) -> Option<Box<dyn Page>>;
    fn view(&self) -> iced::Element<'_, Message>;
}

struct Application {
    page: Box<dyn Page>
}

impl Application {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                page: Box::new(LoginPage::new()),
            },
            Task::none()
        )
    }

    fn update(&mut self, message: Message) {
        let page = self.page.update(message);
        if let Some(p) = page {
            self.page = p;
        }
    }

    fn view(&self) -> iced::Element<Message> {
        self.page.view()
    }
}