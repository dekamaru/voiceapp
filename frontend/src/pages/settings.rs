use crate::application::{Message, Page, PageType};
use crate::audio::{create_input_stream, list_input_devices, list_output_devices};
use crate::colors::{text_primary, DARK_BACKGROUND, DARK_CONTAINER_BACKGROUND};
use crate::icons::Icons;
use crate::widgets::Widgets;
use cpal::Stream;
use iced::border::{radius, rounded, Radius};
use iced::font::{Family, Weight};
use iced::widget::button::Status;
use iced::widget::container::Style;
use iced::widget::rule::FillMode;
use iced::widget::slider::{Handle, HandleShape, Rail};
use iced::widget::{button, column, container, mouse_area, progress_bar, row, rule, slider, stack, text};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Padding, Renderer, Task,
    Theme,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;
use crate::config::AppConfig;

pub struct SettingsPage {
    // Add settings state fields here as needed
    radio_hover_indexes: HashMap<String, usize>,

    // Input
    selected_input_device_name: String,
    input_sensitivity: u8,
    input_device_names: Vec<String>,
    input_stream: Option<Stream>,
    voice_level: f32,

    // Output
    selected_output_device_name: String,
    output_device_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SettingsPageMessage {
    SelectInputDevice(String),
    SelectOutputDevice(String),

    InputSensitivityChanged(u8),

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
    pub fn new(config: Arc<RwLock<AppConfig>>) -> Self {
        let config = config.read().unwrap();

        let input_device_names = list_input_devices().unwrap_or_else(|e| {
            error!("Failed to list input devices: {}", e);
            vec!["No devices available".to_string()]
        });

        // Get available output devices
        let output_device_names = list_output_devices().unwrap_or_else(|e| {
            error!("Failed to list output devices: {}", e);
            vec!["No devices available".to_string()]
        });

        Self {
            radio_hover_indexes: HashMap::new(),
            selected_input_device_name: config.audio.input_device.device_name.clone(),
            input_sensitivity: config.audio.input_sensitivity,
            input_device_names,
            input_stream: None,
            voice_level: 0.0,
            selected_output_device_name: config.audio.output_device.device_name.clone(),
            output_device_names,
        }
    }

    /// Starts the input stream and returns task that produces VoiceInputSamplesReceived messages
    /// Should be called when settings page is opened
    fn start_input_stream(&mut self) -> Task<Message> {
        // Create new channel for samples
        let (samples_tx, samples_rx) = mpsc::unbounded_channel();

        // Create the audio input stream
        match create_input_stream() {
            Ok((stream, _sample_rate, mut stream_rx)) => {
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

    fn input_radio<'a, T>(
        &self,
        values: &'a [T],
        selected_value: T,
        group_name: &'a str,
        on_select: fn(&T) -> SettingsPageMessage,
    ) -> iced::widget::Column<'a, Message, Theme, Renderer>
    where
        T: std::fmt::Display + 'a + std::cmp::PartialEq,
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
            let circle = make_circle(index, value == &selected_value);

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
                .on_press(on_select(value).into())
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
            &self.input_device_names,
            self.selected_input_device_name.clone(),
            "input_device",
            |v| SettingsPageMessage::SelectInputDevice(v.clone()),
        );

        let input_device = column!(
            text("Input device").font(bold).size(12),
            input_device_select
        )
        .spacing(12);

        let output_device_select = self.input_radio(
            &self.output_device_names,
            self.selected_output_device_name.clone(),
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

        let settings_container = column!(input_device, input_device_sensitivity, output_device).spacing(24);

        container(column!(header, settings_container).spacing(32)).padding(Padding {
            top: 32.0,
            right: 24.0,
            left: 24.0,
            bottom: 32.0,
        })
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
                    SettingsPageMessage::SelectInputDevice(name) => {
                        self.selected_input_device_name = name;

                        // Recreate input stream with new device
                        self.stop_input_stream();
                        return self.start_input_stream();
                    }
                    SettingsPageMessage::SelectOutputDevice(name) => {
                        self.selected_output_device_name = name;
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
            Message::VoiceInputSamplesReceived(samples) => {
                // Calculate RMS (root mean square) of the samples
                if !samples.is_empty() {
                    let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
                    let rms = (sum_squares / samples.len() as f32).sqrt();

                    // Convert to dBFS: 20 * log10(rms)
                    let db_fs = if rms > 0.0 {
                        20.0 * rms.log10()
                    } else {
                        -100.0
                    };

                    const NOISE_GATE_DB: f32 = -70.0;
                    const MAX_DB: f32 = 0.0;

                    self.voice_level = if db_fs < NOISE_GATE_DB {
                        0.0
                    } else {
                        // Map from NOISE_GATE_DB to 0 dBFS as 0.0 to 1.0
                        ((db_fs - NOISE_GATE_DB) / (MAX_DB - NOISE_GATE_DB)).clamp(0.0, 1.0)
                    };
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
