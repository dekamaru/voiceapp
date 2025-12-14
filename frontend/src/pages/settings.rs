use iced::{border, Alignment, Background, Border, Color, Element, Font, Length, Padding, Renderer, Task, Theme};
use iced::border::{radius, rounded, Radius};
use iced::font::{Family, Weight};
use iced::widget::{button, column, container, row, rule, text};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use crate::application::{Message, Page};
use crate::colors::{debug_red, text_primary, DARK_BACKGROUND, DARK_CONTAINER_BACKGROUND};
use crate::icons::Icons;
use crate::pages::room::RoomPageMessage;
use crate::widgets;
use crate::widgets::Widgets;

#[derive(Default)]
pub struct SettingsPage {
    // Add settings state fields here as needed
}

#[derive(Debug, Clone)]
pub enum SettingsPageMessage {
    SelectInputDevice(usize)
}

impl Into<Message> for SettingsPageMessage {
    fn into(self) -> Message {
        Message::SettingsPage(self)
    }
}

impl SettingsPage {
    pub fn new() -> Self {
        Self::default()
    }


    fn settings_page(&self) -> iced::widget::Container<'static, Message> {
        let bold = Font {
            family: Family::Name("Rubik"),
            weight: Weight::Semibold,
            ..Default::default()
        };

        let header = row!(
            container(widgets::Widgets::icon_button(Icons::arrow_left_solid(None, 24)).on_press(RoomPageMessage::SettingsToggle.into()).height(Length::Fill)),
            container(text("Settings").font(bold).size(18)).height(Length::Fill).padding(Padding { top: 3.6, ..Padding::default() }),
        ).spacing(12).height (25);

        let top_style = |_theme: &Theme| {
            Style {
                background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                border: Border {
                    radius: Radius { top_left: 8.0, top_right: 8.0, bottom_left: 0.0, bottom_right: 0.0 },
                    ..Border::default()
                },
                ..Style::default()
            }
        };

        let inner_style = |_theme: &Theme| {
            Style {
                background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                ..Style::default()
            }
        };

        let bottom_style = |_theme: &Theme| {
            Style {
                background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                border: Border {
                    radius: Radius { top_left: 0.0, top_right: 0.0, bottom_left: 8.0, bottom_right: 8.0 },
                    ..Border::default()
                },
                ..Style::default()
            }
        };

        let bottom_border_style = |_theme: &Theme| {
            rule::Style {
                color: DARK_BACKGROUND,
                radius: Radius::default(),
                fill_mode: FillMode::Full,
                snap: false,
            }
        };

        let unselected_circle = container("").width(20).height(20).style(|_theme: &Theme| {
            Style {
                background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                border: Border {
                    color: Color::from_rgb8(83, 83, 90),
                    width: 1.0,
                    radius: radius(10), // half of the size
                },
                ..Style::default()
            }
        });

        let unselected_circle_2 = container("").width(20).height(20).style(|_theme: &Theme| {
            Style {
                background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                border: Border {
                    color: Color::from_rgb8(83, 83, 90),
                    width: 1.0,
                    radius: radius(10), // half of the size
                },
                ..Style::default()
            }
        });

        let selected_circle = container(
            container("").width(8).height(8).style(|_theme: &Theme| {
                Style {
                    background: Some(Background::Color(text_primary())),
                    border: rounded(4),
                    ..Style::default()
                }
            })
        ).width(20).height(20).align_y(Alignment::Center).align_x(Alignment::Center).style(|_theme: &Theme| {
            Style {
                background: Some(Background::Color(Color::from_rgb8(83, 83, 90))),
                border: Border {
                    color: Color::from_rgb8(83, 83, 90),
                    width: 1.0,
                    radius: radius(10), // half of the size
                },
                ..Style::default()
            }
        });

        let select_content = row!(
            container(unselected_circle).height(Length::Fill),
            container(text("Realtek Digital Output (Realtek(R) Audio)").size(14)).align_y(Alignment::Center).height(Length::Fill),
        ).spacing(12);

        let select_content_2 = row!(
            container(unselected_circle_2).height(Length::Fill),
            container(text("Динамики (Steam Streaming Speakers)").size(14)).align_y(Alignment::Center).height(Length::Fill),
        ).spacing(12);

        let select_content_3 = row!(
            container(selected_circle).height(Length::Fill),
            container(text("Наушники (AirPods Pro – Find My)").size(14)).align_y(Alignment::Center).height(Length::Fill),
        ).spacing(12);

        let select = column!(
            Widgets::container_button(container(select_content).padding(16).style(top_style).width(Length::Fill).height(52))
                .on_press(SettingsPageMessage::SelectInputDevice(0).into())
                .style(|_theme: &Theme, status: Status| {
                    button::Style {
                        text_color: text_primary(),
                        ..button::Style::default()
                    }
                }), // TODO: responsive?,
            rule::horizontal(1).style(bottom_border_style),
            container(select_content_2).padding(16).style(inner_style).width(Length::Fill).height(52), // TODO: responsive?,
            rule::horizontal(1).style(bottom_border_style),
            container(select_content_3).padding(16).style(bottom_style).width(Length::Fill).height(52), // TODO: responsive?
        );

        let input_device: iced::widget::Column<'_, Message, Theme, Renderer> = column!(
            text("Input device").font(bold).size(12),
            select
        ).spacing(12);

        let settings_container = column!(
            input_device
        ).spacing(24);

        container(
            column!(
                header,
                settings_container
            ).spacing(32)
        )
            .padding(Padding { top: 32.0, right: 24.0, left: 24.0, bottom: 32.0 })
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

impl Page for SettingsPage {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SettingsPage(settings_message) => {
                match settings_message {
                    // Handle settings messages here
                    SettingsPageMessage::SelectInputDevice(index) => {
                        println!("Selected input device: {}", index);
                    }
                }
            }
            _ => {}
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.settings_page().into()
    }
}
