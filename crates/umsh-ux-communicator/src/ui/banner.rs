//! A message that arrived in another chat: a card lying on the header and
//! narrower than it, so the screen under it still shows at both ends. It
//! takes no input and goes by itself.

use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;

use super::{STRIP, label};
use crate::model::{ChatSummary, State};
use crate::{font, theme};

const WIDTH: u32 = 330;
const HEIGHT: u32 = 32;
const RADIUS: u16 = 11;
/// The size the header draws avatars at.
const AVATAR: u32 = 26;

/// The chat as the list shows it: its name and last message.
pub fn view(summary: &ChatSummary) -> impl View<Rgb565, State> + use<> {
    HStack::new((
        super::avatar::view(&summary.avatar, AVATAR),
        HStack::new((
            label::<32>(&summary.name, &font::BOLD, theme::SECONDARY, theme::TEXT)
                .fixed_size(true, false),
            label::<64>(
                &summary.preview,
                &font::REGULAR,
                theme::SECONDARY,
                theme::TEXT_SECONDARY,
            ),
            Spacer::default(),
        ))
        .with_spacing(8),
    ))
    .with_spacing(6)
    .padding(Edges::Leading, 4)
    .padding(Edges::Trailing, 10)
    .hint_background_color(theme::BANNER)
    .frame_sized(WIDTH, HEIGHT)
    .background(
        Alignment::Center,
        ZStack::new((
            RoundedRectangle::new(RADIUS)
                .foreground_color(Rgb565::BLACK)
                .offset(1, 2),
            RoundedRectangle::new(RADIUS).foreground_color(theme::BANNER),
            RoundedRectangle::new(RADIUS)
                .stroked(1)
                .foreground_color(theme::BANNER_LINE),
        )),
    )
    .padding(Edges::Top, STRIP + 1)
    .flex_infinite_width(HorizontalAlignment::Center)
    .flex_infinite_height(VerticalAlignment::Top)
}
