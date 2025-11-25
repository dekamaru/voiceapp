use std::collections::HashMap;
use iced::{border, font, Alignment, Background, Border, Color, Element, Font, Length, Padding, Task, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::font::Family;
use iced::widget::{button, container, horizontal_rule, row, rule, text, vertical_rule, Space, column, Container, scrollable, Scrollable};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use iced::widget::scrollable::{Direction, Rail, Scrollbar, Scroller};
use voiceapp_sdk::{VoiceClientEvent, ParticipantInfo};
use crate::{Message, Page};
use crate::colors::{color_alert, color_success, container_bg, debug_red, divider_bg, slider_bg, slider_thumb, text_chat_header, text_primary, text_secondary};
use crate::icons::Icons;
use crate::{VoiceCommand, VoiceCommandResult};
use crate::widgets::Widgets;

#[derive(Default)]
pub struct RoomPage {
    user_id: u64,
    muted: bool,
    chat_message: String,
    participants: HashMap<u64, ParticipantInfo>
}

#[derive(Debug, Clone)]
pub enum RoomPageMessage {
    MuteToggle,
    JoinLeaveToggle,
    ChatMessageChanged(String),
    ChatMessageSubmitted
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

    fn main_screen(&self) -> iced::widget::Container<'static, Message> {
        let rule_style = |_theme: &Theme| {
            rule::Style {
                color: divider_bg(),
                width: 1,
                radius: Radius::default(),
                fill_mode: FillMode::Full,
            }
        };

        let participants_in_voice: Vec<_> = self.participants.values().filter(|i| i.in_voice).collect();
        let participants_in_chat: Vec<_> = self.participants.values().filter(|i| !i.in_voice).collect();

        let mut sidebar_elements = Vec::new();
        sidebar_elements.extend(Self::render_members_section("IN VOICE", participants_in_voice));
        sidebar_elements.extend(Self::render_members_section("IN CHAT", participants_in_chat));

        let mut sidebar_column = iced::widget::Column::new();
        for element in sidebar_elements {
            sidebar_column = sidebar_column.push(element);
        }

        let is_in_voice = self.participants.get(&self.user_id).map(|p| p.in_voice).unwrap_or(false);

        let disconnect_button = container(
            Widgets::container_button(
                container(text(if is_in_voice { "Disconnect" } else { "Join" }).size(14))
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
        ).padding(16).align_x(Alignment::Center).width(Length::Fill);

        let left_sidebar = container(
            sidebar_column
                .push(Space::with_height(Length::Fill))
                .push(disconnect_button)
        )
            .width(214) // TODO: adaptive or not?
            .height(Length::Fill);

        let messages_container = Scrollable::with_direction(
            container(
                column!(
                    Self::chat_message("ShadowHunter".to_string(), "Привет ребята, кто готов к хорошей игровой сессии? Давайте соберёмся и покажем класс!".to_string(), "14:20".to_string()),
                    Self::chat_message("VortexStrike".to_string(), "Привет! Я только что зашёл, готов играть. Какой режим выбираем, дм или обычный?".to_string(), "14:21".to_string()),
                    Self::chat_message("NovaWings".to_string(), "Давайте на дм, там намного веселее и можно тренировать скилл в боевых ситуациях".to_string(), "14:22".to_string()),
                    Self::chat_message("ShadowHunter".to_string(), "Хорошо, собирайтесь в лобби, скоро начнём. Убедитесь что у вас есть амуниция и утеплители".to_string(), "14:23".to_string()),
                    Self::chat_message("VortexStrike".to_string(), "Я уже спавнился на стартовой позиции, жду остальных. Чекаю инвентарь, все хорошо".to_string(), "14:23".to_string()),
                    Self::chat_message("CrimsonBlade".to_string(), "Ребята, у меня интернет нестабильный сейчас, лагаю немного. Может быть подождёте минутку-две?".to_string(), "14:24".to_string()),
                    Self::chat_message("NovaWings".to_string(), "Нет проблем, ждём тебя. Используй время чтобы нормально подключиться, мы не спешим".to_string(), "14:25".to_string()),
                    Self::chat_message("ShadowHunter".to_string(), "А где CrimsonBlade? Он говорил что идёт, но я его не вижу в лобби уже пять минут".to_string(), "14:26".to_string()),
                    Self::chat_message("CrimsonBlade".to_string(), "Вот я, вот я! Прошу прощения за задержку, перезагружал роутер. Я готов начинать!".to_string(), "14:26".to_string()),
                    Self::chat_message("VortexStrike".to_string(), "Окей, все собрались! Начинаем первый раунд, будьте внимательнее и действуйте как команда!".to_string(), "14:27".to_string()),
                    Self::chat_message("NovaWings".to_string(), "Первый раунд начинается, все дружно движемся в сторону середины карты, держитесь вместе!".to_string(), "14:28".to_string()),
                    Self::chat_message("ShadowHunter".to_string(), "Ха! Я успел убить трёх врагов подряд! Они совсем не ожидали нашей тактики".to_string(), "14:29".to_string()),
                    Self::chat_message("CrimsonBlade".to_string(), "Ну ты даёшь 😅 Как ты так быстро? Я еле двух подобрал в этом раунде".to_string(), "14:30".to_string()),
                    Self::chat_message("VortexStrike".to_string(), "Осторожно за углом, враги занимают позицию! Не идите туда, обойдём их с фланга!".to_string(), "14:31".to_string()),
                    Self::chat_message("NovaWings".to_string(), "Мне хилов не хватает, уже на четверти здоровья. Кто-нибудь может прикрыть меня?".to_string(), "14:32".to_string()),
                    Self::chat_message("ShadowHunter".to_string(), "Держи аптечку и энергетик! Я их только что подобрал у павших врагов, бегу к тебе".to_string(), "14:32".to_string()),
                    Self::chat_message("CrimsonBlade".to_string(), "Второй раунд скоро закончится. Как вам игра? Может быть ещё один или уже домой?".to_string(), "14:35".to_string()),
                    Self::chat_message("VortexStrike".to_string(), "Ещё одну! Я разогрелся уже и вошёл в ритм, хочу закончить на победе!".to_string(), "14:35".to_string()),
                    Self::chat_message("NovaWings".to_string(), "Согласен, давайте финальный раунд. Постараемся выиграть и закончить сессию красиво!".to_string(), "14:36".to_string()),
                    Self::chat_message("ShadowHunter".to_string(), "Идёт! На победу, друзья! Покажем им на что мы способны! 🔥".to_string(), "14:37".to_string()),
                )
            ).padding(Padding { right: 16.0, bottom: 16.0, left: 16.0, top: 0.0 }),
            Direction::Vertical(Scrollbar::new().width(4).margin(2).scroller_width(2))
        ).height(Length::Fill).style(|theme, status| {
            let rail = Rail {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border::default(),
                scroller: Scroller {
                    color: text_chat_header(),
                    border: border::rounded(12)
                }
            };

            scrollable::Style {
                container: Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    ..Style::default()
                },
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None
            }
        });

        let chat_area = container(
            column!(
                messages_container,
                container(
                    Widgets::input_with_submit(
                        "Send message...",
                        &mut self.chat_message.clone(),
                        |v| RoomPageMessage::ChatMessageChanged(v).into(),
                        !self.chat_message.is_empty(),
                        RoomPageMessage::ChatMessageSubmitted.into(),
                        Length::Fill,
                        48
                    )
                ).padding(Padding { right: 16.0, bottom: 16.0, left: 16.0, top: 0.0 })
            )
        )
            .width(Length::Fill)
            .height(Length::Fill);

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

    fn chat_message<'a>(username: String, message: String, time: String) -> Container<'a, Message> {
        container(
            column!(
                row!(
                    text(username).color(text_chat_header()).size(12),
                    Space::with_width(Length::Fill),
                    text(time).color(text_chat_header()).size(12)
                ),
                text(message).color(text_primary()).size(14)
            ).spacing(4)
        ).padding(8)
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

    fn member(username: &str, in_voice: bool, _muted: bool) -> iced::widget::Container<'static, Message> {
        let icon = if in_voice {
            Icons::microphone_fill(color_success(), 16)
        } else {
            Icons::chat_teardrop_dots_fill(text_secondary(), 16)
        };

        let username_owned = username.to_string();
        container(
            row!(
                icon,
                container(text(username_owned).size(14).color(text_primary())).padding(Padding { top: 1.2, ..Padding::default() })
            ).spacing(8)
        ).padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 }).width(Length::Fill)
    }

    fn render_members_section(title: &str, participants: Vec<&ParticipantInfo>) -> Vec<Element<'static, Message>> {
        if participants.is_empty() {
            return Vec::new();
        }

        let mut elements: Vec<Element<'static, Message>> = Vec::new();

        // Add title
        let title_owned = title.to_string();
        elements.push(
            container(
                text(title_owned).size(12).color(text_secondary())
            ).padding(Padding {top: 16.0, right: 16.0, bottom: 4.0, left: 16.0}).width(Length::Fill)
            .into()
        );

        // Add members
        let mut members_column = iced::widget::Column::new();
        for participant in participants {
            members_column = members_column.push(Self::member(&participant.username, participant.in_voice, false));
        }

        elements.push(
            container(members_column).padding(4).width(Length::Fill).into()
        );

        elements
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
        match message {
            Message::RoomPage(room_message) => {
                match room_message {
                    RoomPageMessage::MuteToggle => {
                        self.muted = !self.muted;
                    }
                    RoomPageMessage::JoinLeaveToggle => {
                        let is_in_voice = self.participants.get(&self.user_id).map(|p| p.in_voice).unwrap_or(false);
                        if is_in_voice {
                            return Task::done(Message::ExecuteVoiceCommand(VoiceCommand::LeaveVoiceChannel))
                        }

                        return Task::done(Message::ExecuteVoiceCommand(VoiceCommand::JoinVoiceChannel));
                    }
                    RoomPageMessage::ChatMessageChanged(value) => {
                        // TODO: validate (restrict max chars?)
                        self.chat_message = value;
                    }
                    RoomPageMessage::ChatMessageSubmitted => {
                        println!("Chat message submit!")
                    }
                }
            },
            Message::VoiceCommandResult(result) => {
                match result {
                    VoiceCommandResult::JoinVoiceChannel(status) => {
                        if status.is_ok() {
                            if let Some(user) = self.participants.get_mut(&self.user_id) {
                                user.in_voice = true;
                            }
                        } else {
                            println!("FAILED TO JOIN VOICE: {}", status.err().unwrap());
                        }
                    }
                    VoiceCommandResult::LeaveVoiceChannel(status) => {
                        if status.is_ok() {
                            if let Some(user) = self.participants.get_mut(&self.user_id) {
                                user.in_voice = false;
                            }
                        } else {
                            println!("FAILED TO LEAVE VOICE: {}", status.err().unwrap());
                        }
                    }
                    _ => { println!("ignoring voice command result in room page: {:?}", result); }
                }
            }
            Message::ServerEventReceived(event) => {
                match event {
                    VoiceClientEvent::ParticipantsList { user_id, participants } => {
                        self.user_id = user_id;
                        self.participants = participants.into_iter()
                            .map(|info| (info.user_id, info))
                            .collect();
                    }
                    VoiceClientEvent::UserJoinedServer { user_id, username } => {
                        println!("User {} joined server.", username);
                        self.participants.insert(user_id, voiceapp_sdk::ParticipantInfo {
                            user_id,
                            username,
                            in_voice: false,
                        });
                    }
                    VoiceClientEvent::UserJoinedVoice { user_id } => {
                        println!("User {} joined voice.", user_id);
                        if let Some(user) = self.participants.get_mut(&user_id) {
                            user.in_voice = true;
                        }
                    }
                    VoiceClientEvent::UserLeftVoice { user_id } => {
                        println!("User {} left voice.", user_id);
                        if let Some(user) = self.participants.get_mut(&user_id) {
                            user.in_voice = false;
                        }
                    }
                    VoiceClientEvent::UserLeftServer { user_id } => {
                        println!("User {} left server.", user_id);
                        self.participants.remove(&user_id);
                    }
                }
            }
            _ => { println!("Ignoring event in RoomPage {:?}", message); }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.main_screen().into()
    }
}