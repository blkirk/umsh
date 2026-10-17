//! The rail: Chats, Peers, and Device, beside the top-level lists.

use buoyant::view::prelude::*;
use embedded_graphics::image::ImageRaw;
use embedded_graphics::pixelcolor::{Gray4, Rgb565};

use super::{RAIL, STRIP, label};
use crate::model::{Section, State};
use crate::{font, icon, theme};

pub fn view(active: Section, unread_chats: usize) -> impl View<Rgb565, State> + use<> {
    VStack::new((
        item(
            "Chats",
            &icon::CHATS,
            active == Section::Chats,
            unread_chats,
        ),
        item("Peers", &icon::PEERS, active == Section::Peers, 0),
        item("Device", &icon::DEVICE, active == Section::Device, 0),
    ))
    .with_spacing(0)
    .with_alignment(HorizontalAlignment::Leading)
    .frame()
    .with_width(RAIL)
    .with_height(theme::HEIGHT - STRIP)
    .with_alignment(Alignment::Leading)
    .overlay(
        Alignment::Trailing,
        Rectangle
            .foreground_color(theme::LINE)
            .frame()
            .with_width(1),
    )
    .background_color(theme::CHROME, Rectangle)
}

fn item(
    name: &str,
    glyph: &'static ImageRaw<'static, Gray4>,
    active: bool,
    badge: usize,
) -> impl View<Rgb565, State> + use<> {
    let color = if active {
        theme::ACCENT
    } else {
        theme::TEXT_TERTIARY
    };
    let face = if active { &font::BOLD } else { &font::MEDIUM };
    let cell = (theme::HEIGHT - STRIP) / 3;
    VStack::new((
        Image::new(glyph)
            .as_template()
            .foreground_color(color)
            .overlay(
                Alignment::TopTrailing,
                (badge > 0)
                    .then(|| badge_view(badge))
                    .map(|b| b.offset(7, -5)),
            ),
        label::<8>(name, face, theme::SMALL, color),
    ))
    .with_spacing(2)
    .frame()
    .with_width(RAIL - 1)
    .with_height(cell)
    .overlay(
        Alignment::Leading,
        active.then(|| {
            RoundedRectangle::new(2)
                .foreground_color(theme::ACCENT)
                .frame_sized(4, cell - 20)
        }),
    )
}

/// The count of chats with unread messages, on the Chats glyph.
fn badge_view(count: usize) -> impl View<Rgb565, State> + use<> {
    let mut digits: heapless::String<4> = heapless::String::new();
    let _ = core::fmt::write(&mut digits, format_args!("{count}"));
    ZStack::new((
        Circle.foreground_color(theme::RED).frame_sized(19, 19),
        Text::new(digits, &font::BOLD)
            .with_font_size(theme::SMALL)
            .foreground_color(Rgb565::new(31, 63, 31))
            .hint_background_color(theme::RED),
    ))
}
