use iced::{application, border, font, window, Background, Border, Color, Font, Length, Padding, Settings, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::font::Family;
use iced::Theme::Dark;
use iced::theme::Palette;
use iced::widget::{container, row, column, vertical_rule, rule, text, Space, button, horizontal_rule, text_input, Button, stack};
use iced::widget::container::Style;
use iced::widget::rule::FillMode;

mod icon;
mod colors;

use icon::Icon;
use colors::*;

fn main() -> iced::Result {
    let settings = Settings {
        fonts: vec![
            include_bytes!("../fonts/phosphor-fill.ttf").as_slice().into(),
            include_bytes!("../fonts/phosphor-regular.ttf").as_slice().into(),
        ],
        ..Settings::default()
    };

    let theme = |_state: &Application| {
        Theme::custom("dark".to_string(), Palette {
            background: background_dark(),
            text: text_primary(),
            ..Dark.palette()
        })
    };

    iced::application("Voiceapp", Application::update, Application::view)
        .theme(theme)
        .settings(settings)
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    VoiceUrlChanged(String),
    UsernameChanged(String),
    LoginSubmitted,
}

#[derive(Default)]
struct Application {
    voice_url: String,
    username: String,
    form_filled: bool,
    login_error: String,
}

impl Application {
    fn update(&mut self, message: Message) {
        match message {
            Message::VoiceUrlChanged(content) => {
                self.voice_url = content;
                self.form_filled = self.is_form_filled()
            }
            Message::UsernameChanged(content) => {
                self.username = content;
                self.form_filled = self.is_form_filled()
            }
            Message::LoginSubmitted => {
                if !self.form_filled {
                    return
                }

                self.login_error = "not implemented".to_string();

                println!("login submitted");
            }
        }
    }

    fn is_form_filled(&self) -> bool {
        !self.username.is_empty() && !self.voice_url.is_empty()
    }

    fn view(&self) -> iced::Element<Message> {
        self.login_screen().into()
        //Self::main_screen().into()
    }

    fn login_screen(&self) -> iced::widget::Stack<Message> {
        let bold = Font {
            weight: font::Weight::ExtraBold,
            ..Font::DEFAULT
        };

        // FIXME: "Tab" support between inputs
        let form = container(
            column!(
                self.input("Voice server IP", &mut self.voice_url.clone(), Message::VoiceUrlChanged, Message::LoginSubmitted),
                self.input_with_submit("Username", &mut self.username.clone(), Message::UsernameChanged, &mut self.form_filled.clone(), Message::LoginSubmitted)
            ).spacing(8)
        );

        let login_form = column!(
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

    fn main_screen<'a>() -> iced::widget::Container<'a, Message> {
        let bold = Font {
            weight: font::Weight::ExtraBold,
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
            column!(
                text!("Room").size(24).font(bold),
                column!(
                    Self::member("penetrator666", true, false),
                    Self::member("venom1njector", true, false),
                    Self::member("boneperrrforator", true, true),
                    Self::member("RageInvader9000", false, false),
                    Self::member("BackdoorBarbarian", false, false),
                ),
                Space::with_height(Length::Fill),
                Self::button("Disconnect"),
            ).spacing(16)
        )
            .width(214) // TODO: adaptive or not?
            .height(Length::Fill)
            .padding(24);

        let bottom_bar = container(
            row!(
                Icon::cog_fill(text_secondary(), 24),
                Space::with_width(Length::Fill),
                Self::mute_slider(false)
            )
        )
            .width(Length::Fill)
            .padding(Padding { top: 20.0, right: 28.0, bottom: 20.0, left: 28.0 });

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

        let window_area = column!(
            horizontal_rule(1).style(rule_style),
            main_content_area,
            horizontal_rule(1).style(rule_style),
            bottom_bar
        );

        container(window_area)
            .padding(4)
            .width(Length::Fill)
            .height(Length::Fill)
    }

    fn input_with_submit(
        &self,
        placeholder: &str,
        value: &mut String,
        message: fn(String) -> Message,
        active: &mut bool,
        submit_message: Message
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
            .on_input(message)
            .on_submit(submit_message.clone())
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

        let submit_button_container = container(Icon::arrow_right_solid(Color::BLACK, 16))
            .width(24)
            .height(24)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .style(circle_style);

        let submit_button_style = |_theme: &iced::Theme, _status| {
            button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                ..button::Style::default()
            }
        };

        let submit_button = button(submit_button_container).padding(0).style(submit_button_style).on_press(submit_message.clone());

        container(row!(input, submit_button))
            .width(262)
            .height(48)
            .padding(Padding {top: 13.0, right: 12.0, bottom: 12.0, left: 16.0})
            .style(container_style)
    }

    fn input(&self, placeholder: &str, value: &mut String, message: fn(String) -> Message, submit_message: Message) -> iced::widget::Container<Message> {
        let container_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(container_bg())),
                border: border::rounded(24),
                ..Style::default()
            }
        };

        container(
            text_input(placeholder, value)
                .on_input(message)
                .on_submit(submit_message)
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

    fn button<Message>(str: &str) -> iced::widget::button::Button<Message> {
        let style = |_theme: &iced::Theme, _status| {
            button::Style {
                background: Some(Background::Color(container_bg())),
                border: border::rounded(24),
                text_color: text_primary(),
                ..button::Style::default()
            }
        };

        button(text(str).center().width(Length::Fill))
            .width(Length::Fill)
            .height(48)
            .style(style)
    }

    fn mute_slider<'a>(muted: bool) -> iced::widget::Container<'a, Message> {
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
            Icon::microphone_slash_fill(icon_left_color, 24),
            outer_container,
            Icon::microphone_fill(icon_right_color, 24),
        );

        container(row.spacing(8).align_y(Vertical::Center))
    }

    fn member(username: &str, in_voice: bool, muted: bool) -> iced::widget::Container<Message> {
        let icon = if in_voice {
            if muted {
                Icon::microphone_slash_fill(color_alert(), 16)
            } else {
                Icon::microphone_fill(color_success(), 16)
            }
        } else {
            Icon::moon_stars_fill(text_secondary(), 16)
        };

        container(row!(icon, text(username).size(14)).spacing(8)).padding(8).width(Length::Fill)
    }


    fn debug_border(&self) -> fn(&Theme) -> Style {
        |_theme: &Theme| {
            Style {
                border: border::width(1).color(debug_red()),
                ..Style::default()
            }
        }
    }
}