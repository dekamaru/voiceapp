use iced::font::Font;
use iced::{Background, Color, Element};
use iced::widget::{button, text, Container};
use crate::Message;

pub struct Widgets;

impl Widgets {
    pub fn container_button(container: Container<Message>) -> iced::widget::Button<'_, Message> {
        let style = |_theme: &iced::Theme, _status| {
            button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                ..button::Style::default()
            }
        };

        button(container).padding(0).style(style)
    }
}