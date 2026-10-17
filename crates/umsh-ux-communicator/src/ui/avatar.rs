//! An avatar drawn from its [`AvatarStyle`]: the tinted outline with the
//! lettering centered over it.

use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;

use crate::avatar::{AvatarStyle, Outline};
use crate::font;
use crate::model::State;

/// `style` at `size` pixels across.
pub fn view(style: &AvatarStyle, size: u32) -> impl View<Rgb565, State> + use<> {
    let (face, text_size) = match style.outline {
        // A peer's hint is set in the monospaced face, two over two.
        Outline::Peer(_) => (&font::MONO_BOLD, size * 3 / 8),
        Outline::Channel if style.lines[1].is_empty() && style.lines[0].chars().count() == 1 => {
            (&font::BOLD, size * 3 / 4)
        }
        Outline::Channel => (&font::BOLD, size * 2 / 5),
    };
    let line = |s: &heapless::String<2>| {
        Text::new(s.clone(), face)
            .with_font_size(text_size)
            .foreground_color(style.text)
    };
    let second = (!style.lines[1].is_empty()).then(|| line(&style.lines[1]));
    ZStack::new((
        style
            .outline
            .mask(size)
            .map(|mask| Image::new(mask).as_template().foreground_color(style.fill)),
        VStack::new((line(&style.lines[0]), second))
            .with_spacing(0)
            .hint_background_color(style.fill),
    ))
    .frame()
    .with_height(size)
    .with_width(size)
}
