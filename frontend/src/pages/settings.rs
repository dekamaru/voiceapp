use crate::application::{Message, Page};
use crate::colors::{debug_red, text_primary, DARK_BACKGROUND, DARK_CONTAINER_BACKGROUND};
use crate::icons::Icons;
use crate::pages::room::RoomPageMessage;
use crate::widgets;
use crate::widgets::Widgets;
use iced::border::{radius, rounded, Radius};
use iced::font::{Family, Weight};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use iced::widget::{button, column, container, mouse_area, row, rule, slider, text};
use iced::{
    border, Alignment, Background, Border, Color, Element, Font, Length, Padding, Renderer, Task,
    Theme,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct SettingsPage {
    // Add settings state fields here as needed
    radio_hover_indexes: HashMap<String, usize>,
    selected_input_device: usize,
    input_sensitivity: u8
}

#[derive(Debug, Clone)]
pub enum SettingsPageMessage {
    SelectInputDevice(usize),
    InputSensitivityChanged(u8),

    RadioHoverEnter(String, usize),
    RadioHoverLeave(String, usize),
}

impl Into<Message> for SettingsPageMessage {
    fn into(self) -> Message {
        Message::SettingsPage(self)
    }
}

impl SettingsPage {
    pub fn new() -> Self {
        Self {
            input_sensitivity: 50,
            ..Self::default()
        }
    }

    fn input_radio<'a, T>(
        &self,
        values: &'a [T],
        selected_index: usize,
        group_name: &'a str,
        on_select: fn(usize) -> SettingsPageMessage,
    ) -> iced::widget::Column<'a, Message, Theme, Renderer>
    where
        T: std::fmt::Display + 'a,
    {
        // Define styling functions
        let top_style = |_theme: &Theme| Style {
            background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
            border: Border {
                radius: Radius {
                    top_left: 8.0,
                    top_right: 8.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
                ..Border::default()
            },
            ..Style::default()
        };

        let inner_style = |_theme: &Theme| Style {
            background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
            ..Style::default()
        };

        let bottom_style = |_theme: &Theme| Style {
            background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
            border: Border {
                radius: Radius {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_left: 8.0,
                    bottom_right: 8.0,
                },
                ..Border::default()
            },
            ..Style::default()
        };

        let bottom_border_style = |_theme: &Theme| rule::Style {
            color: DARK_BACKGROUND,
            radius: Radius::default(),
            fill_mode: FillMode::Full,
            snap: false,
        };

        // Helper to create circle widget
        let make_circle = |index: usize, is_selected: bool| {
            let is_hovered = self
                .radio_hover_indexes
                .get(group_name)
                .map(|&i| i == index)
                .unwrap_or(false);

            if is_selected {
                // Selected circle: outer ring + inner filled dot
                container(
                    container("")
                        .width(8)
                        .height(8)
                        .style(|_theme: &Theme| Style {
                            background: Some(Background::Color(text_primary())),
                            border: rounded(4),
                            ..Style::default()
                        }),
                )
                .width(20)
                .height(20)
                .align_y(Alignment::Center)
                .align_x(Alignment::Center)
                .style(|_theme: &Theme| Style {
                    background: Some(Background::Color(Color::from_rgb8(83, 83, 90))),
                    border: Border {
                        color: Color::from_rgb8(83, 83, 90),
                        width: 1.0,
                        radius: radius(10),
                    },
                    ..Style::default()
                })
            } else {
                // Unselected circle: just outer ring, hover-sensitive
                container("")
                    .width(20)
                    .height(20)
                    .style(move |_theme: &Theme| Style {
                        background: Some(Background::Color(DARK_CONTAINER_BACKGROUND)),
                        border: Border {
                            color: if is_hovered {
                                text_primary()
                            } else {
                                Color::from_rgb8(83, 83, 90)
                            },
                            width: 1.0,
                            radius: radius(10),
                        },
                        ..Style::default()
                    })
            }
        };

        // Build column with radio options
        let mut column = column![].spacing(0);

        for (index, value) in values.iter().enumerate() {
            // Determine style based on position
            let container_style = if index == 0 {
                top_style
            } else if index == values.len() - 1 {
                bottom_style
            } else {
                inner_style
            };

            // Create circle
            let circle = make_circle(index, index == selected_index);

            // Create content row
            let content = row!(
                container(circle).height(Length::Fill),
                container(text(value.to_string()).size(14))
                    .align_y(Alignment::Center)
                    .height(Length::Fill),
            )
            .spacing(12);

            // Wrap in mouse_area for hover + container_button for click
            let button = mouse_area(
                Widgets::container_button(
                    container(content)
                        .padding(16)
                        .style(container_style)
                        .width(Length::Fill)
                        .height(52),
                )
                .on_press(on_select(index).into())
                .style(|_theme: &Theme, _status: Status| button::Style {
                    text_color: text_primary(),
                    ..button::Style::default()
                }),
            )
            .on_enter(SettingsPageMessage::RadioHoverEnter(group_name.to_string(), index).into())
            .on_exit(SettingsPageMessage::RadioHoverLeave(group_name.to_string(), index).into());

            column = column.push(button);

            // Add divider between items (not after last)
            if index < values.len() - 1 {
                column = column.push(rule::horizontal(1).style(bottom_border_style));
            }
        }

        column
    }

    fn settings_page(&self) -> iced::widget::Container<'_, Message> {
        let bold = Font {
            family: Family::Name("Rubik"),
            weight: Weight::Semibold,
            ..Default::default()
        };

        let header = row!(
            container(
                widgets::Widgets::icon_button(Icons::arrow_left_solid(None, 24))
                    .on_press(RoomPageMessage::SettingsToggle.into())
                    .height(Length::Fill)
            ),
            container(text("Settings").font(bold).size(18))
                .height(Length::Fill)
                .padding(Padding {
                    top: 3.6,
                    ..Padding::default()
                }),
        )
        .spacing(12)
        .height(25);

        const DEVICES: &[&str] = &[
            "Realtek Digital Output (Realtek(R) Audio)",
            "Динамики (Steam Streaming Speakers)",
            "Наушники (AirPods Pro – Find My)",
        ];

        let input_device_select = self.input_radio(
            DEVICES,
            self.selected_input_device,
            "input_device",
            SettingsPageMessage::SelectInputDevice,
        );

        let input_device = column!(text("Input device").font(bold).size(12), input_device_select).spacing(12);

        let input_device_sensitivity = column!(
            text("Input device sensitivity").font(bold).size(12),
            slider(0..=100, self.input_sensitivity, |v| SettingsPageMessage::InputSensitivityChanged(v).into())
        );

        let settings_container = column!(input_device, input_device_sensitivity).spacing(24);

        container(column!(header, settings_container).spacing(32)).padding(Padding {
            top: 32.0,
            right: 24.0,
            left: 24.0,
            bottom: 32.0,
        })
    }

    fn debug_border() -> fn(&Theme) -> Style {
        |_theme: &Theme| Style {
            border: border::width(1).color(debug_red()),
            ..Style::default()
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
                        self.selected_input_device = index;
                    }
                    SettingsPageMessage::RadioHoverEnter(group, index) => {
                        self.radio_hover_indexes.insert(group, index);
                    }
                    SettingsPageMessage::RadioHoverLeave(group, index) => {
                        if let Some(stored_index) = self.radio_hover_indexes.get(&group) {
                            if *stored_index == index {
                                self.radio_hover_indexes.remove(&group);
                            }
                        }
                    },
                    SettingsPageMessage::InputSensitivityChanged(sensitivity) => {
                        self.input_sensitivity = sensitivity;
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
