use iced::theme::Palette;
use iced::Theme::Dark;
use iced::{Font, Theme};
use tracing_subscriber::{self, EnvFilter};

mod application;
mod audio;
mod colors;
mod config;
mod icons;
mod view;
mod widgets;
mod state;

use crate::application::{Application};
use colors::*;

fn main() -> iced::Result {
    configure_logging();

    let theme = |_state: &Application| {
        Theme::custom(
            "dark".to_string(),
            Palette {
                background: DARK_BACKGROUND,
                text: text_primary(),
                ..Dark.palette()
            },
        )
    };

    iced::application(Application::new, Application::update, Application::render)
        .theme(theme)
        .subscription(Application::subscription)
        .title("Voiceapp")
        .font(include_bytes!("../resources/fonts/phosphor-fill.ttf").as_slice())
        .font(include_bytes!("../resources/fonts/phosphor-regular.ttf").as_slice())
        .font(include_bytes!("../resources/fonts/rubik-regular.ttf").as_slice())
        .font(include_bytes!("../resources/fonts/rubik-semibold.ttf").as_slice())
        .default_font(Font::with_name("Rubik"))
        .antialiasing(true)
        .window(iced::window::Settings {
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}

fn configure_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::new("voiceapp_frontend=debug,voiceapp_sdk=debug")
        )
        .init();

    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.as_str()
        } else {
            "unknown panic payload"
        };

        let location = if let Some(loc) = panic_info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "unknown location".to_string()
        };

        tracing::error!("Panic occurred: {} at {}", message, location);
    }));
}
