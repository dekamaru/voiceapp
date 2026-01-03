use iced::Color;

pub const DARK_BACKGROUND: Color = Color::from_rgb8(22, 23, 26);
pub const DARK_CONTAINER_BACKGROUND: Color = Color::from_rgb8(38, 39, 41);

pub fn divider_bg() -> Color {
    Color::from_rgb8(48, 48, 52)
}

// Text colors
pub fn text_primary() -> Color {
    Color::from_rgb8(242, 242, 242)
}

pub fn text_chat_header() -> Color {
    Color::from_rgb8(116, 116, 116)
}

pub fn text_secondary() -> Color {
    Color::from_rgb8(76, 76, 76)
}

// Status colors
pub fn color_error() -> Color {
    Color::from_rgb8(228, 66, 69)
}

pub fn color_success() -> Color {
    Color::from_rgb8(52, 199, 89)
}

pub fn color_alert() -> Color {
    Color::from_rgb8(255, 56, 60)
}

// UI colors
pub fn slider_bg() -> Color {
    Color::from_rgb8(76, 76, 76)
}

pub fn slider_thumb() -> Color {
    Color::from_rgb8(242, 242, 242)
}

pub fn text_selection() -> Color {
    Color::from_rgba8(242, 242, 242, 0.1)
}
