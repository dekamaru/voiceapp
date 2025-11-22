use iced::{window, Font, Settings, Task, Theme};
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
use crate::pages::room::RoomPage;

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
                page: Box::new(RoomPage::new()),
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