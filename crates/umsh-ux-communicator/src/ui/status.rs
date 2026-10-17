//! The status strip: device name, time, and the few indicators worth a
//! permanent place.

use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;

use super::{STRIP, label};
use crate::model::{State, StatusStrip};
use crate::{font, icon, theme};

pub fn view(s: &StatusStrip) -> impl View<Rgb565, State> + use<> {
    ZStack::new((
        // One stack, not a stack of indicators inside it: see `root_view`.
        HStack::new((
            label::<24>(
                &s.device_name,
                &font::SEMIBOLD,
                theme::SMALL,
                theme::TEXT_SECONDARY,
            ),
            Spacer::default(),
            s.silent.then(|| indicator(&icon::BELL_SLASH)),
            s.phone_linked.then(|| indicator(&icon::BLUETOOTH)),
            s.gnss.then(|| indicator(&icon::GNSS)),
            battery(s.battery_segments, s.battery_low),
        ))
        .with_spacing(3)
        .padding(Edges::Horizontal, 8),
        label::<8>(&s.time, &font::BOLD, theme::SMALL, theme::TEXT),
    ))
    .frame()
    .with_height(STRIP)
    .background_color(theme::CHROME, Rectangle)
}

fn indicator(
    image: &'static embedded_graphics::image::ImageRaw<
        'static,
        embedded_graphics::pixelcolor::Gray4,
    >,
) -> impl View<Rgb565, State> + use<> {
    Image::new(image)
        .as_template()
        .foreground_color(theme::TEXT_SECONDARY)
}

/// A battery outline with up to three charge segments, and its terminal,
/// amber while the charge is `low`.
fn battery(segments: u8, low: bool) -> impl View<Rgb565, State> + use<> {
    let outline = if low {
        theme::AMBER
    } else {
        theme::TEXT_SECONDARY
    };
    let segment = move |i: u8| {
        Rectangle
            .foreground_color(if segments > i {
                theme::TEXT
            } else {
                theme::CHROME
            })
            .frame_sized(5, 7)
            .offset(4 + 6 * i32::from(i), 0)
    };
    ZStack::new((
        RoundedRectangle::new(3)
            .stroked(1)
            .foreground_color(outline)
            .frame_sized(25, 12),
        segment(0),
        segment(1),
        segment(2),
        Rectangle
            .foreground_color(outline)
            .frame_sized(2, 5)
            .offset(26, 0),
    ))
    .with_alignment(Alignment::Leading)
    .frame_sized(28, 12)
}
