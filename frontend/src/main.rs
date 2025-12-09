use iced::{Font, Theme};
use iced::Theme::Dark;
use iced::theme::Palette;
use tracing_subscriber;

mod icons;
mod colors;
mod pages;
mod widgets;
mod audio;
mod application;

use colors::*;
use crate::application::Application;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    
    let theme = |_state: &Application| {
        Theme::custom("dark".to_string(), Palette {
            background: DARK_BACKGROUND,
            text: text_primary(),
            ..Dark.palette()
        })
    };

    iced::application(Application::new, Application::update, Application::view)
        .theme(theme)
        .font(include_bytes!("../fonts/phosphor-fill.ttf").as_slice())
        .font(include_bytes!("../fonts/phosphor-regular.ttf").as_slice())
        .font(include_bytes!("../fonts/rubik-regular.ttf").as_slice())
        .font(include_bytes!("../fonts/rubik-semibold.ttf").as_slice())
        .default_font(Font::with_name("Rubik"))
        .antialiasing(true)
        .run()
}