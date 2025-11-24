use iced::{border, font, Alignment, Background, Color, Element, Font, Length, Padding, Task, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::font::Family;
use iced::widget::{button, container, horizontal_rule, row, rule, text, vertical_rule, Space, column};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use crate::{Message, Page};
use crate::colors::{color_alert, color_success, container_bg, debug_red, divider_bg, slider_bg, slider_thumb, text_primary, text_secondary};
use crate::icons::Icons;
use crate::widgets::Widgets;

#[derive(Default)]
pub struct RoomPage {
    muted: bool,
    in_voice: bool
}

#[derive(Debug, Clone)]
pub enum RoomPageMessage {
    MuteToggle,
    JoinLeaveToggle,
}

impl Into<Message> for RoomPageMessage {
    fn into(self) -> Message {
        Message::RoomPage(self)
    }
}

impl RoomPage {
    pub fn new() -> Self {
        Self::default()
    }

    fn main_screen<'a>(&self) -> iced::widget::Container<'a, Message> {
        let bold = Font {
            weight: font::Weight::Semibold,
            family: Family::Name("Rubik SemiBold"),
            ..Font::DEFAULT
        };

        let rule_style = |_theme: &Theme| {
            rule::Style {
                color: divider_bg(),
                width: 1,
                radius: Radius::default(),
                fill_mode: FillMode::Full,
            }
        };

        let left_sidebar = container(
            iced::widget::column!(
                container(
                    text("IN VOICE").size(12).color(text_secondary())
                ).padding(Padding {top: 16.0, right: 16.0, bottom: 4.0, left: 16.0}).width(Length::Fill),
                container(
                    column!(
                        Self::member("penetrator666", true, false),
                        Self::member("venom1njector", true, false),
                        Self::member("boneperrrforator", true, true),
                    )
                ).padding(4).width(Length::Fill),
                container(
                    text("IN CHAT").size(12).color(text_secondary())
                ).padding(Padding {top: 16.0, right: 16.0, bottom: 4.0, left: 16.0}).width(Length::Fill),
                container(
                    column!(
                        Self::member("RageInvader9000", false, false),
                        Self::member("BackdoorBarbarian", false, false),
                    )
                ).padding(4).width(Length::Fill),
                Space::with_height(Length::Fill),
                    container(
                    Widgets::container_button(
                        container(text(if self.in_voice { "Disconnect" } else { "Join" }).size(14))
                        .padding(Padding {top: 16.0, right: 24.0, bottom: 16.0, left: 24.0})
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center)
                    .width(Length::Fill).height(48)
                    ).on_press(RoomPageMessage::JoinLeaveToggle.into()).style(|theme, status| {
                        if status == Status::Hovered || status == Status::Pressed {
                            button::Style {
                                background: Some(Background::Color(text_primary())),
                                text_color: Color::from_rgb8(40, 40, 40),
                                border: border::rounded(24),
                                ..button::Style::default()
                            }
                        } else {
                            button::Style {
                                background: Some(Background::Color(container_bg())),
                                text_color: text_primary(),
                                border: border::rounded(24),
                                ..button::Style::default()
                            }
                        }
                    }),
                ).padding(16).align_x(Alignment::Center).width(Length::Fill),
            )
        )
            .width(214) // TODO: adaptive or not?
            .height(Length::Fill);

        let chat_area = container("TODO: implement chat")
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(24);

        let main_content_area = container(
            row!(
                left_sidebar,
                vertical_rule(1).style(rule_style),
                chat_area,
            )
        )
            .width(Length::Fill)
            .height(Length::Fill);

        let bottom_bar = container(
            row!(
                Icons::gear_six_fill(text_secondary(), 24),
                Space::with_width(Length::Fill),
                Self::mute_slider(self.muted)
            )
        )
            .width(Length::Fill)
            .padding(16);

        let window_area = iced::widget::column!(
            horizontal_rule(1).style(rule_style),
            main_content_area,
            horizontal_rule(1).style(rule_style),
            bottom_bar
        );

        container(window_area).width(Length::Fill).height(Length::Fill)
    }

    fn mute_slider<'a>(muted: bool) -> iced::widget::Button<'a, Message> {
        let inner_circle_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(slider_thumb())),
                border: border::rounded(30),
                ..Style::default()
            }
        };

        let outer_container_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(slider_bg())),
                border: border::rounded(20),
                ..Style::default()
            }
        };

        let inner_circle = container("")
            .width(12)
            .height(12)
            .style(inner_circle_style);

        let inner_circle_position = if muted {
            Horizontal::Left
        } else {
            Horizontal::Right
        };

        let outer_container = container(inner_circle)
            .padding(1)
            .width(25)
            .align_x(inner_circle_position)
            .center_y(14)
            .style(outer_container_style);

        let icon_left_color = if muted {
            color_alert()
        } else {
            text_secondary()
        };

        let icon_right_color = if muted {
            text_secondary()
        } else {
            color_success()
        };

        let row = row!(
            Icons::microphone_slash_fill(icon_left_color, 24),
            outer_container,
            Icons::microphone_fill(icon_right_color, 24),
        );

        Widgets::container_button(container(row.spacing(8).align_y(Vertical::Center))).on_press(RoomPageMessage::MuteToggle.into())
    }

    fn member(username: &str, in_voice: bool, muted: bool) -> iced::widget::Container<Message> {
        let icon = if in_voice {
            if muted {
                Icons::microphone_slash_fill(color_alert(), 16)
            } else {
                Icons::microphone_fill(color_success(), 16)
            }
        } else {
            Icons::chat_teardrop_dots_fill(text_secondary(), 16)
        };

        container(
            row!(
                icon,
                container(text(username).size(14).color(text_primary())).padding(Padding { top: 1.2, ..Padding::default() })
            ).spacing(8)
        ).padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 }).width(Length::Fill)
    }


    fn debug_border() -> fn(&Theme) -> Style {
        |_theme: &Theme| {
            Style {
                border: border::width(1).color(debug_red()),
                ..Style::default()
            }
        }
    }
}

impl Page for RoomPage {
    fn update(&mut self, message: Message) -> Task<Message> {
        if let Message::RoomPage(msg) = message {
            match msg {
                RoomPageMessage::MuteToggle => {
                    self.muted = !self.muted;
                }
                RoomPageMessage::JoinLeaveToggle => {
                    self.in_voice = !self.in_voice;
                }
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.main_screen().into()
    }
}