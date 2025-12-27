use crate::application::{Message, Page, PageType};
use crate::audio::{adjust_volume, calculate_dbfs, create_input_stream, list_input_devices, list_output_devices};
use crate::colors::{text_chat_header, text_primary, DARK_BACKGROUND, DARK_CONTAINER_BACKGROUND};
use crate::icons::Icons;
use crate::widgets::Widgets;
use cpal::Stream;
use iced::border::{radius, rounded, Radius};
use iced::font::{Family, Weight};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use iced::widget::slider::{Handle, HandleShape, Rail};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, rule, scrollable, slider, stack, text, Scrollable};
use iced::{border, Alignment, Background, Border, Color, Element, Font, Length, Padding, Renderer, Task, Theme};
use std::collections::HashMap;
use std::sync::{Arc};
use arc_swap::ArcSwap;
use iced::widget::scrollable::{Direction, Scrollbar, Scroller};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;
use crate::config::AppConfig;

pub struct SettingsPage {
    app_config: Arc<ArcSwap<AppConfig>>,
    // Add settings state fields here as needed
    radio_hover_indexes: HashMap<String, usize>,

    // Input
    selected_input_device_id: String,
    input_sensitivity: u8,
    input_volume: u8,
    input_devices: HashMap<String, String>,
    input_stream: Option<Stream>,
    voice_level: f32,

    // Output
    selected_output_device_id: String,
    output_devices: HashMap<String, String>,
    output_volume: u8
}

#[derive(Debug, Clone)]
pub enum SettingsPageMessage {
    SelectInputDevice(String),
    SelectOutputDevice(String),

    InputSensitivityChanged(u8),
    InputVolumeChanged(u8),
    OutputVolumeChanged(u8),

    RadioHoverEnter(String, usize),
    RadioHoverLeave(String, usize),

    // Input stream control
    InputStreamCreated(Result<(), String>),
}

impl Into<Message> for SettingsPageMessage {
    fn into(self) -> Message {
        Message::SettingsPage(self)
    }
}

impl SettingsPage {
    pub fn new(config: Arc<ArcSwap<AppConfig>>) -> Self {
        let audio_config = config.load().audio.clone();

        let input_devices = list_input_devices().unwrap_or_else(|e| {
            error!("Failed to list input devices: {}", e);
            HashMap::new()
        });

        // Get available output devices
        let output_devices = list_output_devices().unwrap_or_else(|e| {
            error!("Failed to list output devices: {}", e);
            HashMap::new()
        });

        Self {
            app_config: config.clone(),
            radio_hover_indexes: HashMap::new(),
            selected_input_device_id: audio_config.input_device.device_id.clone(),
            input_sensitivity: audio_config.input_sensitivity,
            input_devices,
            input_stream: None,
            voice_level: 0.0,
            selected_output_device_id: audio_config.output_device.device_id.clone(),
            output_devices,
            input_volume: audio_config.input_device.volume,
            output_volume: audio_config.output_device.volume,
        }
    }

    /// Starts the input stream and returns task that produces VoiceInputSamplesReceived messages
    /// Should be called when settings page is opened
    fn start_input_stream(&mut self) -> Task<Message> {
        // Create new channel for samples
        let (samples_tx, samples_rx) = mpsc::unbounded_channel();

        let config = self.app_config.load().audio.clone();

        // Create the audio input stream
        match create_input_stream(config.input_device) {
            Ok((stream, mut stream_rx)) => {
                self.input_stream = Some(stream);

                // Task to rebroadcast from stream_rx to persistent samples_tx
                let rebroadcast_task = Task::perform(
                    async move {
                        while let Some(samples) = stream_rx.recv().await {
                            if samples_tx.send(samples).is_err() {
                                tracing::warn!("Failed to send samples to persistent channel");
                                break;
                            }
                        }
                        Ok::<(), String>(())
                    },
                    |result| {
                        if let Err(ref e) = result {
                            error!("Stream rebroadcast error: {}", e);
                        }
                        SettingsPageMessage::InputStreamCreated(result).into()
                    },
                );

                // Task to listen to samples and produce messages
                let samples_stream = UnboundedReceiverStream::new(samples_rx);
                let samples_task = Task::run(samples_stream, |samples| {
                    Message::VoiceInputSamplesReceived(samples)
                });

                // Combine both tasks
                Task::batch(vec![rebroadcast_task, samples_task])
            }
            Err(e) => {
                error!("Failed to create input stream: {}", e);
                Task::none()
            }
        }
    }

    fn input_radio<'a, K, V>(
        &self,
        values: &'a HashMap<K, V>,
        selected_value: K,
        group_name: &'a str,
        on_select: fn(&K) -> SettingsPageMessage,
    ) -> iced::widget::Column<'a, Message, Theme, Renderer>
    where
        K: std::fmt::Display + 'a + std::cmp::PartialEq,
        V: std::fmt::Display + 'a + std::cmp::PartialEq,
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

        for (index, (key, value)) in values.iter().enumerate() {
            // Determine style based on position
            let container_style = if index == 0 {
                top_style
            } else if index == values.len() - 1 {
                bottom_style
            } else {
                inner_style
            };

            // Create circle
            let circle = make_circle(index, key == &selected_value);

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
                .on_press(on_select(key).into())
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

    /// Stops the input stream
    fn stop_input_stream(&mut self) {
        self.input_stream = None;
        tracing::info!("Input stream stopped");
    }

    fn settings_page(&self) -> iced::widget::Container<'_, Message> {
        let bold = Font {
            family: Family::Name("Rubik"),
            weight: Weight::Semibold,
            ..Default::default()
        };

        let header = row!(
            container(
                Widgets::icon_button(Icons::arrow_left_solid(None, 24))
                    .on_press(Message::SwitchPage(PageType::Room))
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

        let input_device_select = self.input_radio(
            &self.input_devices,
            self.selected_input_device_id.clone(),
            "input_device",
            |v| SettingsPageMessage::SelectInputDevice(v.clone()),
        );

        let input_device = column!(
            text("Input device").font(bold).size(12),
            input_device_select
        )
        .spacing(12);

        let output_device_select = self.input_radio(
            &self.output_devices,
            self.selected_output_device_id.clone(),
            "output_device",
            |v| SettingsPageMessage::SelectOutputDevice(v.clone()),
        );

        let output_device = column!(
            text("Output device").font(bold).size(12),
            output_device_select
        )
            .spacing(12);

        let progress_bar = container(progress_bar(0.0..=1.0, self.voice_level).girth(4).style(|_theme: &Theme| {
            progress_bar::Style {
                background: Background::Color(Color::TRANSPARENT),
                bar: Background::Color(Color::from_rgba8(0, 0, 0, 0.3)),
                border: rounded(2),
            }
        })).padding(Padding { top: 6.0, ..Padding::default() });

        let sensitivity_slider = slider(0..=100, self.input_sensitivity, |v| {
            SettingsPageMessage::InputSensitivityChanged(v).into()
        })
        .style(|_theme: &Theme, _status: slider::Status| slider::Style {
            rail: Rail {
                backgrounds: (
                    Background::Color(Color::from_rgb8(206, 157, 92)),
                    Background::Color(Color::from_rgb8(67, 162, 91)),
                ),
                width: 4.0,
                border: rounded(2),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 8.0 },
                background: Background::Color(text_primary()),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        });

        let input_device_sensitivity = column!(
            text("Input sensitivity").font(bold).size(12),
            stack!(sensitivity_slider, progress_bar)
        )
        .spacing(12);

        let input_volume_slider = slider(0..=100, self.input_volume, |v| {
            SettingsPageMessage::InputVolumeChanged(v).into()
        })
            .style(|_theme: &Theme, _status: slider::Status| slider::Style {
                rail: Rail {
                    backgrounds: (
                        Background::Color(text_primary()),
                        Background::Color(DARK_CONTAINER_BACKGROUND),
                    ),
                    width: 4.0,
                    border: rounded(2),
                },
                handle: Handle {
                    shape: HandleShape::Circle { radius: 8.0 },
                    background: Background::Color(text_primary()),
                    border_width: 0.0,
                    border_color: Color::TRANSPARENT,
                },
            });

        let output_volume_slider = slider(0..=100, self.output_volume, |v| {
            SettingsPageMessage::OutputVolumeChanged(v).into()
        })
            .style(|_theme: &Theme, _status: slider::Status| slider::Style {
                rail: Rail {
                    backgrounds: (
                        Background::Color(text_primary()),
                        Background::Color(DARK_CONTAINER_BACKGROUND),
                    ),
                    width: 4.0,
                    border: rounded(2),
                },
                handle: Handle {
                    shape: HandleShape::Circle { radius: 8.0 },
                    background: Background::Color(text_primary()),
                    border_width: 0.0,
                    border_color: Color::TRANSPARENT,
                },
            });

        let input_volume = column!(
            text("Input volume").font(bold).size(12),
            row!(input_volume_slider, text(self.input_volume).font(bold).size(12)).spacing(12),
        )
            .spacing(12);

        let output_volume = column!(
            text("Output volume").font(bold).size(12),
            row!(output_volume_slider, text(self.output_volume).font(bold).size(12)).spacing(12),
        )
            .spacing(12);

        let settings_container = column!(input_device, input_volume, input_device_sensitivity, output_device, output_volume).spacing(24);

        container(
            Scrollable::with_direction(
                container(column!(header, settings_container).spacing(32)).padding(Padding {
                    right: 24.0,
                    left: 24.0,
                    ..Padding::default()
                }),
                Direction::Vertical(Scrollbar::new().width(4).margin(9).scroller_width(2))
            ).style(|theme, status| {
                let rail = iced::widget::scrollable::Rail {
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
            })
        ).padding(Padding { top: 32.0, bottom: 32.0, ..Padding::default() })
    }
}

impl Page for SettingsPage {
    fn on_open(&mut self) -> Task<Message> { self.start_input_stream() }
    fn on_close(&mut self) -> Task<Message> { self.stop_input_stream(); Task::none() }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SettingsPage(settings_message) => {
                match settings_message {
                    // Handle settings messages here
                    SettingsPageMessage::SelectInputDevice(device_id) => {
                        self.selected_input_device_id = device_id;

                        // Recreate input stream with new device
                        self.stop_input_stream();
                        return self.start_input_stream();
                    }
                    SettingsPageMessage::SelectOutputDevice(device_id) => {
                        self.selected_output_device_id = device_id;
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
                    }
                    SettingsPageMessage::InputSensitivityChanged(sensitivity) => {
                        self.input_sensitivity = sensitivity;
                    }
                    SettingsPageMessage::InputVolumeChanged(volume) => {
                        self.input_volume = volume;
                    }
                    SettingsPageMessage::OutputVolumeChanged(volume) => {
                        self.output_volume = volume;
                    }
                    SettingsPageMessage::InputStreamCreated(result) => match result {
                        Ok(()) => {
                            tracing::info!("Input stream task completed");
                        }
                        Err(e) => {
                            error!("Input stream task error: {}", e);
                        }
                    },
                }
            },
            Message::KeyPressed(key) => {
                if matches!(key,iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)) {
                    return Task::done(Message::SwitchPage(PageType::Room))
                }
            }
            Message::VoiceInputSamplesReceived(mut samples) => {
                adjust_volume(samples.as_mut(), self.input_volume as f32 / 100.0);
                if !samples.is_empty() {
                    self.voice_level = calculate_dbfs(samples.to_vec());
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
