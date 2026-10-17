//! The screens, as Buoyant views over [`State`].
//!
//! Views own what they draw: Buoyant keeps the previous frame's render tree
//! to animate from, so nothing in it may borrow the state that changed.

use buoyant::font::PrerenderedFont;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;

use crate::model::{Section, State};
use crate::theme;

mod avatar;
mod chats;
mod rail;
mod status;

/// Height of the status strip.
pub const STRIP: u32 = 22;
/// Width of the rail beside the top-level lists.
pub const RAIL: u32 = 66;

/// The whole screen for `state`.
///
/// The screen's parts have fixed places, so they are laid over one another
/// and inset rather than stacked: a stack lays each child out several times
/// to learn how flexible it is, nested stacks multiply that, and on the
/// device layout is the larger part of a frame.
#[must_use]
pub fn root_view(state: &State) -> impl View<Rgb565, State> + use<> {
    ZStack::new((
        status::view(&state.status),
        rail::view(state.section, state.unread_chats()).padding(Edges::Top, STRIP),
        match state.section {
            // Peers and Device are not built yet.
            Section::Chats | Section::Peers | Section::Device => chats::view(&state.chats),
        }
        .padding(Edges::Top, STRIP)
        .padding(Edges::Leading, RAIL),
    ))
    .with_alignment(Alignment::TopLeading)
    .flex_infinite_width(HorizontalAlignment::Leading)
    .flex_infinite_height(VerticalAlignment::Top)
    .background_color(theme::BACKGROUND, Rectangle)
}

/// One line of text, cut short with an ellipsis when it does not fit, and
/// owned so the view borrows nothing.
fn label<const N: usize>(
    s: &str,
    face: &'static PrerenderedFont,
    size: u32,
    color: Rgb565,
) -> impl View<Rgb565, State> + use<N> {
    let mut owned: heapless::String<N> = heapless::String::new();
    for c in s.chars() {
        if owned.push(c).is_err() {
            break;
        }
    }
    Text::new(owned, face)
        .with_font_size(size)
        .with_line_limit(1)
        .foreground_color(color)
}
