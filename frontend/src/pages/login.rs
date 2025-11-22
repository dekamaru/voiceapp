use iced::{border, font, Background, Border, Color, Element, Font, Length, Padding, Task};
use iced::alignment::{Horizontal, Vertical};
use iced::font::{Family, Weight};
use iced::widget::{button, container, row, stack, text, text_input, Space};
use iced::widget::container::Style;
use voiceapp_sdk::VoiceClient;
use crate::{Message, Page};
use crate::colors::{color_error, container_bg, text_primary, text_secondary, text_selection};
use crate::icons::Icons;
use crate::pages::room::RoomPage;
use crate::widgets::Widgets;

#[derive(Default)]
pub struct LoginPage {
    voice_url: String,
    username: String,
    form_filled: bool,
    login_error: String,
    client: Option<VoiceClient>
}

#[derive(Debug, Clone)]
pub enum LoginPageMessage {
    VoiceUrlChanged(String),
    UsernameChanged(String),
    LoginSubmitted,
}

impl Into<Message> for LoginPageMessage {
    fn into(self) -> Message {
        Message::LoginPage(self)
    }
}

impl LoginPage {
    pub fn new() -> Self {
        Self::default()
    }

    fn is_form_filled(&self) -> bool {
        !self.username.is_empty() && !self.voice_url.is_empty()
    }

    fn login_screen(&self) -> iced::widget::Stack<Message> {
        let bold = Font {
            family: Family::Name("Rubik SemiBold"),
            weight: Weight::Semibold,
            ..Default::default()
        };

        // FIXME: "Tab" support between inputs
        let form = container(
            iced::widget::column!(
                self.input("Voice server IP", &mut self.voice_url.clone(), LoginPageMessage::VoiceUrlChanged, LoginPageMessage::LoginSubmitted),
                self.input_with_submit("Username", &mut self.username.clone(), LoginPageMessage::UsernameChanged, &mut self.form_filled.clone(), LoginPageMessage::LoginSubmitted)
            ).spacing(8)
        );

        let login_form = iced::widget::column!(
          Space::with_height(Length::Fill),
          text!("Voiceapp").size(32).font(bold),
          form,
          Space::with_height(Length::Fill),
      )
            .spacing(32)
            .align_x(Horizontal::Center)
            .width(Length::Fill)
            .height(Length::Fill);

        let error_area = container(text(self.login_error.clone()).color(color_error()))
            .align_y(Vertical::Bottom)
            .align_x(Horizontal::Center)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(32);

        stack!(error_area, login_form).width(Length::Fill).height(Length::Fill)
    }

    fn input_with_submit(
        &self,
        placeholder: &str,
        value: &mut String,
        message: fn(String) -> LoginPageMessage,
        active: &mut bool,
        submit_message: LoginPageMessage
    ) -> iced::widget::Container<Message> {
        let container_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(container_bg())),
                border: border::rounded(24),
                ..Style::default()
            }
        };

        let color = if *active {
            text_primary()
        } else {
            text_secondary()
        };

        let circle_style = move |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(color)),
                border: border::rounded(40),
                ..Style::default()
            }
        };

        let input = text_input(placeholder, value)
            .on_input(move |t| { message(t).into() })
            .on_submit(submit_message.clone().into())
            .padding(0)
            .style(|_theme, _status| {
                text_input::Style {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    icon: Color::TRANSPARENT,
                    placeholder: text_secondary(),
                    value: text_primary(),
                    selection: text_selection()
                }
            });

        let submit_button_container = container(Icons::arrow_right_solid(Color::BLACK, 16))
            .width(24)
            .height(24)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .style(circle_style);

        let submit_button = Widgets::container_button(submit_button_container).on_press(submit_message.clone().into());

        container(row!(input, submit_button))
            .width(262)
            .height(48)
            .padding(Padding {top: 13.0, right: 12.0, bottom: 12.0, left: 16.0})
            .style(container_style)
    }

    fn input(&self, placeholder: &str, value: &mut String, message: fn(String) -> LoginPageMessage, submit_message: LoginPageMessage) -> iced::widget::Container<Message> {
        let container_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(container_bg())),
                border: border::rounded(24),
                ..Style::default()
            }
        };

        container(
            text_input(placeholder, value)
                .on_input(move |t| { message(t).into() })
                .on_submit(submit_message.into())
                .padding(0)
                .style(|_theme, _status| {
                    text_input::Style {
                        background: Background::Color(Color::TRANSPARENT),
                        border: Border::default(),
                        icon: Color::TRANSPARENT,
                        placeholder: text_secondary(),
                        value: text_primary(),
                        selection: text_selection()
                    }
                }),
        ).width(262).height(48).padding(Padding {top: 13.0, right: 16.0, bottom: 12.0, left: 16.0}).style(container_style)
    }
}

impl Page for LoginPage {
    fn update(&mut self, message: Message) -> Option<Box<dyn Page>> {
        if let Message::LoginPage(msg) = message {
            match msg {
                LoginPageMessage::VoiceUrlChanged(content) => {
                    self.voice_url = content;
                    self.form_filled = self.is_form_filled()
                }
                LoginPageMessage::UsernameChanged(content) => {
                    self.username = content;
                    self.form_filled = self.is_form_filled()
                }
                LoginPageMessage::LoginSubmitted => {
                    if self.form_filled {
                        //self.login_error = "not implemented".to_string();
                        return Some(Box::new(RoomPage::new()))
                    }
                },
            }
        }
        None
    }

    fn view(&self) -> Element<'_, Message> {
        self.login_screen().into()
    }
}