use iced::font::Font;
use iced::widget::text;
use iced::{Color, Element, Pixels};

pub struct Icons;

impl Icons {
    pub fn gear_six_fill<'a, Message>(color: Option<Color>, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E272}', color, size)
    }

    pub fn microphone_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E326}', Some(color), size)
    }

    pub fn microphone_slash_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E328}', Some(color), size)
    }

    pub fn chat_teardrop_dots_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E176}', Some(color), size)
    }

    pub fn arrow_right_solid<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_solid('\u{E06C}', Some(color), size)
    }

    pub fn arrow_left_solid<'a, Message>(color: Option<Color>, size: u16) -> Element<'a, Message> {
        Self::icon_solid('\u{E058}', color, size)
    }

    fn icon_fill<'a, Message>(
        codepoint: char,
        color: Option<Color>,
        size: u16,
    ) -> Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor-Fill");
        let elem = text(codepoint)
            .font(ICON_FONT)
            .size(Pixels::from(size as u32));

        if color.is_some() {
            elem.color(color.unwrap())
        } else {
            elem
        }
        .into()
    }

    fn icon_solid<'a, Message>(
        codepoint: char,
        color: Option<Color>,
        size: u16,
    ) -> Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor");
        let elem = text(codepoint)
            .font(ICON_FONT)
            .size(Pixels::from(size as u32));

        if color.is_some() {
            elem.color(color.unwrap())
        } else {
            elem
        }
        .into()
    }
}
