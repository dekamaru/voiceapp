use iced::{border, Alignment, Element, Font, Length, Padding, Task, Theme};
use iced::font::{Family, Weight};
use iced::widget::{column, container, row, text};
use iced::widget::container::Style;
use crate::application::{Message, Page};
use crate::colors::debug_red;
use crate::icons::Icons;
use crate::pages::room::RoomPageMessage;
use crate::widgets;

#[derive(Default)]
pub struct SettingsPage {
    // Add settings state fields here as needed
}

#[derive(Debug, Clone)]
pub enum SettingsPageMessage {
    // Add settings-specific messages here as needed
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
            container(text("Settings").font(bold).size(18)).height(Length::Fill).padding(Padding { top: 3.0, ..Padding::default() }),
        ).spacing(12).height(25);

        container(
            column!(
                header
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
