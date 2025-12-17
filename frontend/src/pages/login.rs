use crate::application::{Message, Page, VoiceCommand, VoiceCommandResult};
use crate::colors::{
    color_error, text_primary, text_secondary, text_selection, DARK_CONTAINER_BACKGROUND,
};
use crate::widgets::Widgets;
use iced::alignment::{Horizontal, Vertical};
use iced::font::{Family, Weight};
use iced::widget::container::Style;
use iced::widget::{button, container, row, space, stack, text, text_input, Space};
use iced::{border, Background, Border, Color, Element, Font, Length, Padding, Task};
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

#[derive(Default)]
pub struct LoginPage {
    voice_url: String,
    username: String,
    form_filled: bool,
    login_error: String,
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
    pub fn new(config: Arc<RwLock<crate::config::AppConfig>>) -> Self {
        let config = config.read().unwrap();

        let mut page = Self {
            voice_url: config.server.address.clone(),
            username: config.server.username.clone(),
            form_filled: false,
            login_error: "".to_string(),
        };

        page.form_filled = page.is_form_filled();
        page
    }

    fn is_form_filled(&self) -> bool {
        !self.username.is_empty() && !self.voice_url.is_empty()
    }

    fn login_screen(&self) -> iced::widget::Stack<Message> {
        let bold = Font {
            family: Family::Name("Rubik"),
            weight: Weight::Semibold,
            ..Default::default()
        };

        // FIXME: "Tab" support between inputs
        let form = container(
            iced::widget::column!(
                self.input(
                    "Voice server IP",
                    &mut self.voice_url.clone(),
                    LoginPageMessage::VoiceUrlChanged,
                    LoginPageMessage::LoginSubmitted
                ),
                Widgets::input_with_submit(
                    "Username",
                    &mut self.username.clone(),
                    |v| LoginPageMessage::UsernameChanged(v).into(),
                    self.form_filled,
                    LoginPageMessage::LoginSubmitted.into(),
                    262,
                    48
                )
            )
            .spacing(8),
        );

        let login_form = iced::widget::column!(
            space::vertical(),
            text!("Voiceapp").size(32).font(bold),
            form,
            space::vertical(),
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

        stack!(error_area, login_form)
            .width(Length::Fill)
            .height(Length::Fill)
    }

    fn input(
        &self,
        placeholder: &str,
        value: &mut String,
        message: fn(String) -> LoginPageMessage,
        submit_message: LoginPageMessage,
    ) -> iced::widget::Container<Message> {
        let container_style = |_theme: &iced::Theme| Style {
            background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
            border: border::rounded(24),
            ..Style::default()
        };

        container(
            text_input(placeholder, value)
                .on_input(move |t| message(t).into())
                .on_submit(submit_message.into())
                .padding(0)
                .style(|_theme, _status| text_input::Style {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    icon: Color::TRANSPARENT,
                    placeholder: text_secondary(),
                    value: text_primary(),
                    selection: text_selection(),
                }),
        )
        .width(262)
        .height(48)
        .padding(Padding {
            top: 13.0,
            right: 16.0,
            bottom: 12.0,
            left: 16.0,
        })
        .style(container_style)
    }
}

impl Page for LoginPage {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoginPage(msg) => {
                match msg {
                    LoginPageMessage::VoiceUrlChanged(content) => {
                        if !self.login_error.is_empty() {
                            self.login_error.clear();
                        }

                        self.voice_url = content;
                        self.form_filled = self.is_form_filled()
                    }
                    LoginPageMessage::UsernameChanged(content) => {
                        if !self.login_error.is_empty() {
                            self.login_error.clear();
                        }

                        self.username = content;
                        self.form_filled = self.is_form_filled()
                    }
                    LoginPageMessage::LoginSubmitted => {
                        if self.form_filled {
                            // TODO: inputs should be blocked (buttons as well)
                            return Task::done(Message::ExecuteVoiceCommand(
                                VoiceCommand::Connect {
                                    management_addr: format!("{}:9001", self.voice_url),
                                    voice_addr: format!("{}:9002", self.voice_url),
                                    username: self.username.clone(),
                                },
                            ));
                        }
                    }
                }
            }
            Message::VoiceCommandResult(VoiceCommandResult::Connect(result)) => match result {
                Err(err) => {
                    self.login_error = err;
                }
                _ => {}
            },
            _ => {
                debug!("Ignored message in login page: {:?}", message);
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.login_screen().into()
    }
}
