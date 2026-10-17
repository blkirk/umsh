//! The screens, as Buoyant views over [`State`].
//!
//! Views own what they draw: Buoyant keeps the previous frame's render tree
//! to animate from, so nothing in it may borrow the state that changed.

use buoyant::app::App;
use buoyant::focus::{DefaultFocus, Role};
use buoyant::font::PrerenderedFont;
use buoyant::match_view;
use buoyant::render::AnimatedJoin;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;

use crate::model::{Section, State};
use crate::nav::{Screen, keys};
use crate::theme;

mod add_channel;
mod add_peer;
mod avatar;
mod banner;
mod channels;
mod chats;
mod conversation;
mod details;
mod device;
mod join;
mod member;
mod peer;
mod peers;
mod rail;
mod region;
mod setting;
mod setup;
mod share;
mod status;
mod wake;

/// Height of the status strip.
pub const STRIP: u32 = 22;
/// Width of the rail beside the top-level lists.
pub const RAIL: u32 = 66;

/// Readies an app over [`root_view`]: what can take focus, and where focus
/// is kept.
pub fn bind<V, F>(app: &mut App<V, State, F>)
where
    V: buoyant::view::ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    app.set_roles(Role::Button | Role::TextEntry);
    app.set_focus_binding(State::focus);
    // Focus settles where the state asks, before the first input.
    app.finalize_view();
}

/// The whole screen for `state`.
///
/// The screen's parts have fixed places, so they are laid over one another
/// and inset rather than stacked: a stack lays each child out several times
/// to learn how flexible it is, nested stacks multiply that, and on the
/// device layout is the larger part of a frame.
#[must_use]
pub fn root_view(state: &State) -> impl View<Rgb565, State> + use<> {
    ZStack::new((
        status::view(&state.status, state.in_setup()),
        match_view!(state.screen(), {
            Screen::Chats { .. } => chats_screen(state),
            Screen::Conversation { chat, .. } => conversation::view(state, *chat),
            Screen::Details { message, details, .. } => details::view(details, state.inbound(*message)),
            Screen::Member {
                member, requested, ..
            } => member::view(
                member,
                *requested,
                state
                    .conversation
                    .as_ref()
                    .and_then(|c| state.chat(c.chat))
                    .map_or("", |s| &s.name),
            ),
            Screen::Join { invite, name, .. } => join::view(invite, name),
            Screen::AddPeer { text, .. } => add_peer::view(state, text),
            Screen::Peer {
                peer, detail, confirm, ..
            } => state
                .peer(*peer)
                .map(|summary| peer::view(summary, detail, *confirm)),
            Screen::Device { page, .. } => device::page(state, page),
            Screen::Setup { page, .. } => setup::view(state, page),
            Screen::Wake { .. } => wake::view(state),
        })
        .padding(Edges::Top, STRIP),
        // Over everything, and taking no input.
        state.banner.map(|banner| banner::view(state, banner.notice)),
        // The last thing shown before power goes, over even the strip.
        state.battery_empty.map(empty_battery),
    ))
    .with_alignment(Alignment::TopLeading)
    .flex_infinite_width(HorizontalAlignment::Leading)
    .flex_infinite_height(VerticalAlignment::Top)
    .background_color(theme::BACKGROUND, Rectangle)
}

/// The empty battery, alone on the screen, `lit` or not as it blinks. No
/// words to translate, and no controls.
fn empty_battery(lit: bool) -> impl View<Rgb565, State> + use<> {
    lit.then(|| {
        Image::new(&crate::icon::BATTERY_EMPTY)
            .as_template()
            .foreground_color(theme::RED)
    })
    .flex_infinite_width(HorizontalAlignment::Center)
    .flex_infinite_height(VerticalAlignment::Center)
    .background_color(theme::BACKGROUND, Rectangle)
}

/// The rail and the list beside it. Turning on the rail shows each section
/// as it is reached.
fn chats_screen(state: &State) -> impl View<Rgb565, State> + use<> {
    let focus = state.screen().focus().current();
    let shown = focus.and_then(keys::section).unwrap_or(state.section);
    ZStack::new((
        rail::view(shown, state.unread_chats()),
        match_view!(shown, {
            Section::Chats => chats::view(&state.chats),
            Section::Peers => peers::view(&state.peers),
            Section::Device => device::view(state),
        })
        .padding(Edges::Leading, RAIL),
    ))
    .with_alignment(Alignment::TopLeading)
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
    line(owned, face, size, color)
}

/// One line of `text`, which the view holds as it is given: shared text
/// is shown without being copied.
fn line<T: AsRef<str> + Clone>(
    text: T,
    face: &'static PrerenderedFont,
    size: u32,
    color: Rgb565,
) -> impl View<Rgb565, State> + use<T> {
    Text::new(text, face)
        .with_font_size(size)
        .with_line_limit(1)
        .foreground_color(color)
}
