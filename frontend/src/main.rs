use iced::{application, border, font, window, Background, Border, Color, Font, Length, Padding, Settings, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::Theme::Dark;
use iced::theme::Palette;
use iced::widget::{container, row, column, vertical_rule, rule, text, Space, button};
use iced::widget::container::Style;
use iced::widget::rule::FillMode;

fn main() -> iced::Result {
    iced::application("My App", MyApp::update, MyApp::view).theme(|_state: &MyApp| {
        Theme::custom("dark".to_string(), Palette {
            background: Color::from_rgb8(22, 23, 26),
            ..Dark.palette()
        })
    }).settings(Settings {
        fonts: vec![include_bytes!("../fonts/phosphor-fill.ttf").as_slice().into()],
        ..Settings::default()
    }).run()
}

type Message = ();

#[derive(Default)]
struct MyApp;

impl MyApp {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> iced::Element<Message> {
        let bold = Font {
            weight: font::Weight::ExtraBold,
            ..Font::DEFAULT
        };

        container(
            column!(
                container(
                    row!(
                        container(
                            column!(
                                text!("Room").size(24).font(bold),
                                column!(
                                    container(
                                        row!(
                                            Self::icon('\u{E326}', Color::from_rgb8(52, 199, 89), 16),
                                            text!("penetrator666").size(14)
                                        ).spacing(8)
                                    ).padding(8).width(Length::Fill),
                                    container(
                                        row!(
                                            Self::icon('\u{E326}', Color::from_rgb8(52, 199, 89), 16),
                                            text!("venom1njector").size(14)
                                        ).spacing(8)
                                    ).padding(8).width(Length::Fill),
                                    container(
                                        row!(
                                            Self::icon('\u{E328}', Color::from_rgb8(255, 56, 60), 16),
                                            text!("boneperrrforator").size(14)
                                        ).spacing(8)
                                    ).padding(8).width(Length::Fill),
                                    container(
                                        row!(
                                            Self::icon('\u{E58E}', Color::from_rgb8(76, 76, 76), 16),
                                            text!("RageInvader9000").size(14).color(Color::from_rgb8(76, 76, 76))
                                        ).spacing(8)
                                    ).padding(8).width(Length::Fill),
                                    container(
                                        row!(
                                            Self::icon('\u{E58E}', Color::from_rgb8(76, 76, 76), 16),
                                            text!("BackdoorBarbarian").size(14).color(Color::from_rgb8(76, 76, 76))
                                        ).spacing(8)
                                    ).padding(8).width(Length::Fill),
                                ),
                                Space::with_height(Length::Fill),
                                button(text!("Disconnect").center().width(Length::Fill)).width(Length::Fill).height(48).style(|_theme: &iced::Theme, status| {
                                    button::Style {
                                        background: Some(Background::Color(Color::from_rgb8(38, 39, 41))),
                                        border: border::rounded(24),
                                        text_color: Color::from_rgb8(242, 242, 242),
                                        ..button::Style::default()
                                    }
                                }),
                            ).spacing(16)
                        )
                            .width(214) // TODO: adaptive or not?
                            .height(Length::Fill)
                            .padding(24),
                        vertical_rule(1)
                            .style(|_theme: &Theme| {
                                rule::Style {
                                    color: Color::from_rgb8(22, 23, 26),
                                    width: 1,
                                    radius: Radius::default(),
                                    fill_mode: FillMode::Full,
                                }
                            }),
                        container("TODO: implement chat")
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .padding(24)
                    )
                )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_theme: &iced::Theme| {
                        Style {
                            background: Some(Background::Color(Color::from_rgb8(31, 32, 34))),
                            border: border::rounded(23),
                            ..Style::default()
                        }
                    }),
                container(
                    row!(
                        Self::icon('\u{E272}', Color::from_rgb8(76, 76, 76), 24),
                        Space::with_width(Length::Fill),
                        container(
                            row!(
                                Self::icon('\u{E328}', Color::from_rgb8(76, 76, 76), 24),
                                container(
                                    container("").width(12).height(12).style(|_theme: &iced::Theme| {
                                    Style {
                                        background: Some(Background::Color(Color::from_rgb8(242, 242, 242))),
                                        border: border::rounded(30),
                                        ..Style::default()
                                    }
                                }),
                                ).padding(1).width(25).align_x(Horizontal::Right).center_y(14).style(|_theme: &iced::Theme| {
                                    Style {
                                        background: Some(Background::Color(Color::from_rgb8(76, 76, 76))),
                                        border: border::rounded(20),
                                        ..Style::default()
                                    }
                                }),
                                Self::icon('\u{E326}', Color::from_rgb8(52, 199, 89), 24),
                            ).spacing(8).align_y(Vertical::Center)
                        )
                    )
                )
                    .width(Length::Fill)
                    .padding(Padding { top: 20.0, right: 28.0, bottom: 20.0, left: 28.0 })
            )
        )
            .padding(4)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn icon<'a, Message>(codepoint: char, color: Color, size: u16) -> iced::Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor-Fill");
        text(codepoint).font(ICON_FONT).size(size).color(color).into()
    }
}