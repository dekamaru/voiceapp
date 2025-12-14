use crate::application::{Message, Page, VoiceCommand, VoiceCommandResult};
use crate::colors::{
    color_alert, color_success, debug_red, divider_bg, slider_bg, slider_thumb, text_chat_header,
    text_primary, text_secondary, DARK_CONTAINER_BACKGROUND,
};
use crate::icons::Icons;
use crate::pages::settings::SettingsPage;
use crate::widgets::Widgets;
use chrono::{DateTime, Local, Utc};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use iced::widget::scrollable::{Direction, Rail, Scrollbar, Scroller};
use iced::widget::{
    button, column, container, row, rule, scrollable, space, text, Container, Id, Scrollable, Space,
};
use iced::{border, Alignment, Background, Border, Color, Element, Length, Padding, Task, Theme};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, warn};
use voiceapp_sdk::{ParticipantInfo, VoiceClientEvent};

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub username: String,
    pub message: String,
    pub time: String,
}

impl ChatMessage {
    pub fn new(username: String, message: String, timestamp: u64) -> Self {
        let time = Self::format_time(timestamp);
        Self {
            username,
            message,
            time,
        }
    }

    fn format_time(timestamp_ms: u64) -> String {
        let secs = (timestamp_ms / 1000) as i64;
        let nanos = ((timestamp_ms % 1000) * 1_000_000) as u32;
        let datetime = DateTime::<Utc>::from_timestamp(secs, nanos)
            .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
        let local_time = datetime.with_timezone(&Local);
        local_time.format("%H:%M:%S").to_string()
    }
}

pub struct RoomPage {
    user_id: u64,
    muted: bool,
    chat_message: String,
    participants: HashMap<u64, ParticipantInfo>,
    chat_history: BTreeMap<u64, ChatMessage>,
    show_settings: bool,
    settings: SettingsPage,
}

#[derive(Debug, Clone)]
pub enum RoomPageMessage {
    MuteToggle,
    JoinLeaveToggle,
    ChatMessageChanged(String),
    ChatMessageSubmitted,
    SettingsToggle,
}

impl Into<Message> for RoomPageMessage {
    fn into(self) -> Message {
        Message::RoomPage(self)
    }
}

impl RoomPage {
    pub fn new() -> Self {
        Self {
            settings: SettingsPage::new(),
            user_id: 0,
            muted: false,
            chat_message: String::new(),
            participants: HashMap::new(),
            chat_history: BTreeMap::new(),
            show_settings: false,
        }
    }

    fn main_screen(&self) -> iced::widget::Container<'static, Message> {
        let rule_style = |_theme: &Theme| rule::Style {
            color: divider_bg(),
            radius: Radius::default(),
            fill_mode: FillMode::Full,
            snap: false,
        };

        let participants_in_voice: Vec<_> =
            self.participants.values().filter(|i| i.in_voice).collect();
        let participants_in_chat: Vec<_> =
            self.participants.values().filter(|i| !i.in_voice).collect();

        let mut sidebar_elements = Vec::new();
        sidebar_elements.extend(Self::render_members_section(
            "IN VOICE",
            participants_in_voice,
        ));
        sidebar_elements.extend(Self::render_members_section(
            "IN CHAT",
            participants_in_chat,
        ));

        let mut sidebar_column = iced::widget::Column::new();
        for element in sidebar_elements {
            sidebar_column = sidebar_column.push(element);
        }

        let is_in_voice = self
            .participants
            .get(&self.user_id)
            .map(|p| p.in_voice)
            .unwrap_or(false);

        let disconnect_button = container(
            Widgets::container_button(
                container(
                    text(if is_in_voice {
                        "Leave voice"
                    } else {
                        "Join voice"
                    })
                    .size(14),
                )
                .padding(Padding {
                    top: 16.0,
                    right: 24.0,
                    bottom: 16.0,
                    left: 24.0,
                })
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .height(48),
            )
            .on_press(RoomPageMessage::JoinLeaveToggle.into())
            .style(|theme, status| {
                if status == Status::Hovered || status == Status::Pressed {
                    button::Style {
                        background: Some(Background::Color(text_primary())),
                        text_color: Color::from_rgb8(40, 40, 40),
                        border: border::rounded(24),
                        ..button::Style::default()
                    }
                } else {
                    button::Style {
                        background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                        text_color: text_primary(),
                        border: border::rounded(24),
                        ..button::Style::default()
                    }
                }
            }),
        )
        .width(214)
        .padding(Padding {
            left: 16.0,
            right: 16.0,
            ..Padding::default()
        });

        let mute_slider = container(Self::mute_slider(self.muted))
            .padding(Padding {
                bottom: 24.0,
                left: 16.0,
                right: 16.0,
                top: 16.0,
            })
            .width(Length::Fill)
            .align_x(Alignment::Center);

        let left_sidebar = container(sidebar_column.push(space::vertical()).push(
            if self.is_in_voice() {
                mute_slider
            } else {
                container("")
            },
        ))
        .width(214) // TODO: adaptive or not?
        .height(Length::Fill);

        let mut messages_column = column!();
        for chat_msg in self.chat_history.values() {
            messages_column = messages_column.push(Self::chat_message(
                chat_msg.username.clone(),
                chat_msg.message.clone(),
                chat_msg.time.clone(),
            ));
        }

        let messages_container = Scrollable::with_direction(
            container(messages_column)
                .align_y(Alignment::End)
                .padding(Padding {
                    right: 16.0,
                    bottom: 16.0,
                    left: 16.0,
                    top: 0.0,
                }),
            Direction::Vertical(Scrollbar::new().width(4).margin(2).scroller_width(2)),
        )
        .id(Id::new("chat_area_scroll"))
        .height(Length::Fill)
        .style(|theme, status| {
            let rail = Rail {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border::default(),
                scroller: Scroller {
                    background: Background::Color(text_chat_header()),
                    border: border::rounded(12),
                },
            };

            scrollable::Style {
                container: Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    ..Style::default()
                },
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None,
                ..scrollable::default(theme, status)
            }
        });

        let chat_area = container(column!(
            messages_container,
            container(Widgets::input_with_submit(
                "Send message...",
                &mut self.chat_message.clone(),
                |v| RoomPageMessage::ChatMessageChanged(v).into(),
                !self.chat_message.is_empty(),
                RoomPageMessage::ChatMessageSubmitted.into(),
                Length::Fill,
                48
            ))
            .padding(Padding {
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
                top: 0.0
            })
        ))
        .width(Length::Fill)
        .height(Length::Fill);

        let main_content_area = container(row!(
            left_sidebar,
            rule::vertical(1).style(rule_style),
            chat_area,
        ))
        .width(Length::Fill)
        .height(Length::Fill);

        let settings_button = container(
            Widgets::icon_button(Icons::gear_six_fill(None, 24))
                .on_press(RoomPageMessage::SettingsToggle.into()),
        )
        .align_y(Alignment::Center)
        .height(48);

        let bottom_bar = container(row!(
            disconnect_button,
            space::horizontal(),
            settings_button,
        ))
        .width(Length::Fill)
        .padding(Padding {
            right: 16.0,
            bottom: 16.0,
            left: 0.0,
            top: 16.0,
        });

        let window_area = iced::widget::column!(
            rule::horizontal(1).style(rule_style),
            main_content_area,
            rule::horizontal(1).style(rule_style),
            bottom_bar
        );

        container(window_area)
            .width(Length::Fill)
            .height(Length::Fill)
    }

    fn chat_message<'a>(username: String, message: String, time: String) -> Container<'a, Message> {
        container(
            column!(
                row!(
                    text(username).color(text_chat_header()).size(12),
                    space::horizontal(),
                    text(time).color(text_chat_header()).size(12)
                ),
                text(message).color(text_primary()).size(14)
            )
            .spacing(4),
        )
        .padding(8)
    }

    fn mute_slider<'a>(muted: bool) -> iced::widget::Button<'a, Message> {
        let inner_circle_style = |_theme: &iced::Theme| Style {
            background: Some(Background::Color(slider_thumb())),
            border: border::rounded(30),
            ..Style::default()
        };

        let outer_container_style = |_theme: &iced::Theme| Style {
            background: Some(Background::Color(slider_bg())),
            border: border::rounded(20),
            ..Style::default()
        };

        let inner_circle = container("").width(12).height(12).style(inner_circle_style);

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

        Widgets::container_button(container(row.spacing(8).align_y(Vertical::Center)))
            .on_press(RoomPageMessage::MuteToggle.into())
    }

    fn member(
        username: &str,
        in_voice: bool,
        _muted: bool,
    ) -> iced::widget::Container<'static, Message> {
        let icon = if in_voice {
            Icons::microphone_fill(color_success(), 16)
        } else {
            Icons::chat_teardrop_dots_fill(text_secondary(), 16)
        };

        let username_owned = username.to_string();
        container(
            row!(
                icon,
                container(text(username_owned).size(14).color(text_primary())).padding(Padding {
                    top: 1.2,
                    ..Padding::default()
                })
            )
            .spacing(8),
        )
        .padding(Padding {
            top: 8.0,
            right: 12.0,
            bottom: 8.0,
            left: 12.0,
        })
        .width(Length::Fill)
    }

    fn render_members_section(
        title: &str,
        participants: Vec<&ParticipantInfo>,
    ) -> Vec<Element<'static, Message>> {
        if participants.is_empty() {
            return Vec::new();
        }

        let mut elements: Vec<Element<'static, Message>> = Vec::new();

        // Add title
        let title_owned = title.to_string();
        elements.push(
            container(text(title_owned).size(12).color(text_secondary()))
                .padding(Padding {
                    top: 16.0,
                    right: 16.0,
                    bottom: 4.0,
                    left: 16.0,
                })
                .width(Length::Fill)
                .into(),
        );

        // Add members
        let mut members_column = iced::widget::Column::new();
        for participant in participants {
            members_column = members_column.push(Self::member(
                &participant.username,
                participant.in_voice,
                false,
            ));
        }

        elements.push(
            container(members_column)
                .padding(4)
                .width(Length::Fill)
                .into(),
        );

        elements
    }

    fn debug_border() -> fn(&Theme) -> Style {
        |_theme: &Theme| Style {
            border: border::width(1).color(debug_red()),
            ..Style::default()
        }
    }

    fn is_in_voice(&self) -> bool {
        self.participants
            .get(&self.user_id)
            .map(|p| p.in_voice)
            .unwrap_or(false)
    }
}

impl Page for RoomPage {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RoomPage(room_message) => match room_message {
                RoomPageMessage::MuteToggle => {
                    self.muted = !self.muted;
                }
                RoomPageMessage::JoinLeaveToggle => {
                    if self.is_in_voice() {
                        return Task::done(Message::ExecuteVoiceCommand(
                            VoiceCommand::LeaveVoiceChannel,
                        ));
                    }

                    return Task::done(Message::ExecuteVoiceCommand(
                        VoiceCommand::JoinVoiceChannel,
                    ));
                }
                RoomPageMessage::ChatMessageChanged(value) => {
                    if value.len() <= 2000 {
                        self.chat_message = value;
                    }
                }
                RoomPageMessage::ChatMessageSubmitted => {
                    if !self.chat_message.is_empty() {
                        let message = self.chat_message.clone();
                        self.chat_message.clear();
                        return Task::done(Message::ExecuteVoiceCommand(
                            VoiceCommand::SendChatMessage(message),
                        ));
                    }
                }
                RoomPageMessage::SettingsToggle => {
                    self.show_settings = !self.show_settings;
                }
            },
            Message::SettingsPage(_) => {
                return self.settings.update(message);
            }
            Message::VoiceCommandResult(result) => match result {
                VoiceCommandResult::JoinVoiceChannel(status) => {
                    if status.is_ok() {
                        if let Some(user) = self.participants.get_mut(&self.user_id) {
                            user.in_voice = true;
                        }
                    } else {
                        warn!("Failed to join voice: {}", status.err().unwrap());
                    }
                }
                VoiceCommandResult::LeaveVoiceChannel(status) => {
                    if status.is_ok() {
                        if let Some(user) = self.participants.get_mut(&self.user_id) {
                            user.in_voice = false;
                        }
                    } else {
                        warn!("Failed to leave voice: {}", status.err().unwrap());
                    }
                }
                VoiceCommandResult::SendChatMessage(status) => {
                    if let Err(e) = status {
                        warn!("Failed to send message: {}", e);
                    }
                }
                _ => {
                    debug!("Ignoring voice command result in room page: {:?}", result);
                }
            },
            Message::ServerEventReceived(event) => match event {
                VoiceClientEvent::ParticipantsList {
                    user_id,
                    participants,
                } => {
                    self.user_id = user_id;
                    self.participants = participants
                        .into_iter()
                        .map(|info| (info.user_id, info))
                        .collect();
                }
                VoiceClientEvent::UserJoinedServer { user_id, username } => {
                    debug!("User {} joined server", username);
                    self.participants.insert(
                        user_id,
                        voiceapp_sdk::ParticipantInfo {
                            user_id,
                            username,
                            in_voice: false,
                        },
                    );
                }
                VoiceClientEvent::UserJoinedVoice { user_id } => {
                    debug!("User {} joined voice", user_id);
                    if let Some(user) = self.participants.get_mut(&user_id) {
                        user.in_voice = true;
                    }
                }
                VoiceClientEvent::UserLeftVoice { user_id } => {
                    debug!("User {} left voice", user_id);
                    if let Some(user) = self.participants.get_mut(&user_id) {
                        user.in_voice = false;
                    }
                }
                VoiceClientEvent::UserLeftServer { user_id } => {
                    debug!("User {} left server", user_id);
                    self.participants.remove(&user_id);
                }
                VoiceClientEvent::UserSentMessage {
                    user_id,
                    timestamp,
                    message,
                } => {
                    if let Some(participant) = self.participants.get(&user_id) {
                        let chat_msg =
                            ChatMessage::new(participant.username.clone(), message, timestamp);
                        self.chat_history.insert(timestamp, chat_msg);

                        return iced::widget::operation::snap_to(
                            Id::new("chat_area_scroll"),
                            scrollable::RelativeOffset::END,
                        );
                    }
                }
            },
            Message::KeyPressed(key) => {
                if self.show_settings
                    && matches!(
                        key,
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    )
                {
                    self.show_settings = false;
                }
            }
            _ => {
                debug!("Ignoring event in RoomPage {:?}", message);
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.show_settings {
            self.settings.view()
        } else {
            self.main_screen().into()
        }
    }
}
