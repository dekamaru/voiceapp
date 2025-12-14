use crate::application::Message;
use crate::colors::{text_primary, text_secondary, text_selection, DARK_CONTAINER_BACKGROUND};
use crate::icons::Icons;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::{button, container, row, text, text_input, Container};
use iced::{border, Background, Border, Color, Element, Length, Padding};

pub struct Widgets;

impl Widgets {
    // Wraps container with a button
    pub fn container_button(container: Container<Message>) -> iced::widget::Button<'_, Message> {
        let style = |_theme: &iced::Theme, _status| button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            ..button::Style::default()
        };

        button(container).padding(0).style(style)
    }

    pub fn icon_button(icon: Element<'_, Message>) -> iced::widget::Button<'_, Message> {
        let style = |_theme: &iced::Theme, status| {
            if status == Status::Hovered || status == Status::Pressed {
                button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color: text_primary(),
                    ..button::Style::default()
                }
            } else {
                button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color: text_secondary(),
                    ..button::Style::default()
                }
            }
        };

        button(icon).padding(0).style(style)
    }

    pub fn input_with_submit<'a>(
        placeholder: &str,
        value: &mut String,
        message: fn(String) -> Message,
        active: bool,
        submit_message: Message,
        width: impl Into<Length>,
        height: impl Into<Length>,
    ) -> iced::widget::Container<'a, Message> {
        let container_style = |_theme: &iced::Theme| Style {
            background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
            border: border::rounded(24),
            ..Style::default()
        };

        let active_color = if active {
            text_primary()
        } else {
            text_secondary()
        };

        let circle_style = move |_theme: &iced::Theme| Style {
            background: Some(Background::Color(active_color)),
            border: border::rounded(40),
            ..Style::default()
        };

        let input = text_input(placeholder, value)
            .on_input(move |t| message(t).into())
            .on_submit(submit_message.clone().into())
            .padding(0)
            .style(|_theme, _status| text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                icon: Color::TRANSPARENT,
                placeholder: text_secondary(),
                value: text_primary(),
                selection: text_selection(),
            });

        let submit_button_container = container(Icons::arrow_right_solid(Color::BLACK, 16))
            .width(24)
            .height(24)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .style(circle_style);

        let submit_button = Widgets::container_button(submit_button_container)
            .on_press(submit_message.clone().into());

        container(row!(input, submit_button))
            .width(width)
            .height(height)
            .padding(Padding {
                top: 13.0,
                right: 12.0,
                bottom: 12.0,
                left: 16.0,
            })
            .style(container_style)
    }
}
