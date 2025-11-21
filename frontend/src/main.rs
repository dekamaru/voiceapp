use iced::{application, border, font, window, Background, Border, Color, Font, Length, Padding, Settings, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::font::Family;
use iced::Theme::Dark;
use iced::theme::Palette;
use iced::widget::{container, row, column, vertical_rule, rule, text, Space, button, horizontal_rule, text_input, Button};
use iced::widget::container::Style;
use iced::widget::rule::FillMode;

fn main() -> iced::Result {
    iced::application("My App", Application::update, Application::view).theme(|_state: &Application| {
        Theme::custom("dark".to_string(), Palette {
            background: Color::from_rgb8(22, 23, 26),
            ..Dark.palette()
        })
    }).settings(Settings {
        fonts: vec![
            include_bytes!("../fonts/phosphor-fill.ttf").as_slice().into(),
            include_bytes!("../fonts/phosphor-regular.ttf").as_slice().into(),
        ],
        ..Settings::default()
    }).run()
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
    login_valid: bool,
}

impl Application {
    fn update(&mut self, message: Message) {
        match message {
            Message::VoiceUrlChanged(content) => {
                self.voice_url = content;
                self.login_valid = self.is_login_valid()
            }
            Message::UsernameChanged(content) => {
                self.username = content;
                self.login_valid = self.is_login_valid()
            }
            Message::LoginSubmitted => {
                if !self.login_valid {
                    return
                }

                println!("login submitted");
            }
        }
    }

    fn is_login_valid(&self) -> bool {
        !self.username.is_empty() && !self.voice_url.is_empty()
    }

    fn view(&self) -> iced::Element<Message> {
        self.login_screen().into()
        //Self::main_screen().into()
    }

    fn login_screen(&self) -> iced::widget::Container<Message> {
        let bold = Font {
            weight: font::Weight::ExtraBold,
            ..Font::DEFAULT
        };

        // FIXME: "Tab" support between inputs
        let form = container(
            column!(
                self.input("Voice server IP", &mut self.voice_url.clone(), Message::VoiceUrlChanged, Message::LoginSubmitted),
                self.input_with_submit("Username", &mut self.username.clone(), Message::UsernameChanged, &mut self.login_valid.clone(), Message::LoginSubmitted)
            ).spacing(8)
        );

        let window_area = container(column!(
            text!("Voiceapp").size(32).font(bold),
            form
        ).spacing(32).align_x(Horizontal::Center));

        container(window_area)
            .padding(4)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
    }

    fn main_screen<'a>() -> iced::widget::Container<'a, Message> {
        let bold = Font {
            weight: font::Weight::ExtraBold,
            ..Font::DEFAULT
        };

        let rule_style = |_theme: &Theme| {
            rule::Style {
                color: Color::from_rgb8(48, 48, 52),
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
                Self::i_cog(Color::from_rgb8(76, 76, 76), 24),
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
                background: Some(Background::Color(Color::from_rgb8(38, 39, 41))),
                border: border::rounded(24),
                ..Style::default()
            }
        };

        let color = if *active {
            Color::from_rgb8(242, 242, 242)
        } else {
            Color::from_rgb8(76, 76, 76)
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
                    placeholder: Color::from_rgb8(76, 76, 76),
                    value: Color::from_rgb8(242, 242, 242),
                    selection: Color::from_rgba8(242, 242, 242, 0.1)
                }
            });

        let submit_button_container = container(Self::i_arrow_right(Color::BLACK, 16))
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
                background: Some(Background::Color(Color::from_rgb8(38, 39, 41))),
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
                        placeholder: Color::from_rgb8(76, 76, 76),
                        value: Color::from_rgb8(242, 242, 242),
                        selection: Color::from_rgba8(242, 242, 242, 0.1)
                    }
                }),
        ).width(262).height(48).padding(Padding {top: 13.0, right: 16.0, bottom: 12.0, left: 16.0}).style(container_style)
    }

    fn button<Message>(str: &str) -> iced::widget::button::Button<Message> {
        let style = |_theme: &iced::Theme, _status| {
            button::Style {
                background: Some(Background::Color(Color::from_rgb8(38, 39, 41))),
                border: border::rounded(24),
                text_color: Color::from_rgb8(242, 242, 242),
                ..button::Style::default()
            }
        };

        button(text(str).center().width(Length::Fill))
            .width(Length::Fill)
            .height(48)
            .style(style)
    }

    fn mute_slider<'a>(muted: bool) -> iced::widget::Container<'a, Message> {
        let green_color: Color = Color::from_rgb8(52, 199, 89);
        let red_color: Color = Color::from_rgb8(255, 56, 60);
        let gray_color: Color = Color::from_rgb8(76, 76, 76);

        let inner_circle_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(Color::from_rgb8(242, 242, 242))),
                border: border::rounded(30),
                ..Style::default()
            }
        };

        let outer_container_style = |_theme: &iced::Theme| {
            Style {
                background: Some(Background::Color(Color::from_rgb8(76, 76, 76))),
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
            red_color
        } else {
            gray_color
        };

        let icon_right_color = if muted {
            gray_color
        } else {
            green_color
        };

        let row = row!(
            Self::i_microphone_slash(icon_left_color, 24),
            outer_container,
            Self::i_microphone(icon_right_color, 24),
        );

        container(row.spacing(8).align_y(Vertical::Center))
    }

    fn member(username: &str, in_voice: bool, muted: bool) -> iced::widget::Container<Message> {
        let green_color: Color = Color::from_rgb8(52, 199, 89);
        let red_color: Color = Color::from_rgb8(255, 56, 60);
        let gray_color: Color = Color::from_rgb8(76, 76, 76);

        let icon = if in_voice {
            if muted {
                Self::i_microphone_slash(red_color, 16)
            } else {
                Self::i_microphone(green_color, 16)
            }
        } else {
            Self::i_moon_stars(gray_color, 16)
        };

        container(row!(icon, text(username).size(14)).spacing(8)).padding(8).width(Length::Fill)
    }

    fn i_cog<'a, Message>(color: Color, size: u16) -> iced::Element<'a, Message> {
        Self::icon('\u{E272}', color, size)
    }

    fn i_microphone<'a, Message>(color: Color, size: u16) -> iced::Element<'a, Message> {
        Self::icon('\u{E326}', color, size)
    }

    fn i_microphone_slash<'a, Message>(color: Color, size: u16) -> iced::Element<'a, Message> {
        Self::icon('\u{E328}', color, size)
    }

    fn i_moon_stars<'a, Message>(color: Color, size: u16) -> iced::Element<'a, Message> {
        Self::icon('\u{E58E}', color, size)
    }

    fn i_arrow_right<'a, Message>(color: Color, size: u16) -> iced::Element<'a, Message> {
        Self::icon_regular('\u{E06C}', color, size)
    }

    fn icon<'a, Message>(codepoint: char, color: Color, size: u16) -> iced::Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor-Fill");
        text(codepoint).font(ICON_FONT).size(size).color(color).into()
    }

    fn icon_regular<'a, Message>(codepoint: char, color: Color, size: u16) -> iced::Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor");
        text(codepoint).font(ICON_FONT).size(size).color(color).into()
    }

    fn debug_border(&self) -> fn(&Theme) -> Style {
        |_theme: &Theme| {
            Style {
                border: border::width(1).color(Color::from_rgb8(255, 0, 0)),
                ..Style::default()
            }
        }
    }
}