use iced::font::Font;
use iced::{Color, Element};
use iced::widget::text;

pub struct Icons;

impl Icons {
    pub fn cog_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E272}', color, size)
    }

    pub fn microphone_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E326}', color, size)
    }

    pub fn microphone_slash_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E328}', color, size)
    }

    pub fn moon_stars_fill<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_fill('\u{E58E}', color, size)
    }

    pub fn arrow_right_solid<'a, Message>(color: Color, size: u16) -> Element<'a, Message> {
        Self::icon_solid('\u{E06C}', color, size)
    }

    fn icon_fill<'a, Message>(codepoint: char, color: Color, size: u16) -> Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor-Fill");
        text(codepoint).font(ICON_FONT).size(size).color(color).into()
    }

    fn icon_solid<'a, Message>(codepoint: char, color: Color, size: u16) -> Element<'a, Message> {
        const ICON_FONT: Font = Font::with_name("Phosphor");
        text(codepoint).font(ICON_FONT).size(size).color(color).into()
    }
}