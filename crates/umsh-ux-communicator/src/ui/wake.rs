//! What a message wakes the panel to: the chats that got messages while it
//! was dark, the first focused. Press or Enter opens one, typing starts a
//! reply in it, and Backspace returns to where the panel went dark.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::Vec;

use super::label;
use crate::model::{Arrived, ChatSummary, MAX_CHATS, State};
use crate::nav::keys;
use crate::{font, theme};

/// The heading's line, and the space between cards.
const HEADING: u32 = 26;
const GAP: u32 = 6;
const RADIUS: u16 = 10;
const AVATAR: u32 = 28;
/// From a card's leading edge to its avatar and to its text.
const AVATAR_X: u32 = 10;
const TEXT_X: u32 = 46;
/// Above the name, and the lines of the name and of the message.
const TOP: u32 = 6;
const NAME_LINE: u32 = 24;
const MESSAGE_LINE: u32 = 20;

/// Indices `ForEach` can hold without borrowing the state.
static INDICES: [usize; MAX_CHATS] = {
    let mut a = [0; MAX_CHATS];
    let mut i = 0;
    while i < MAX_CHATS {
        a[i] = i;
        i += 1;
    }
    a
};

pub fn view(state: &State) -> impl View<Rgb565, State> + use<> {
    let cards: Vec<(Arrived, ChatSummary), MAX_CHATS> = state
        .arrived
        .iter()
        .filter_map(|arrived| Some((arrived.clone(), state.chat(arrived.chat)?.clone())))
        .collect();
    let ids: Vec<u32, MAX_CHATS> = cards.iter().map(|(a, _)| a.chat).collect();
    let count = cards.len();
    VStack::new((
        label::<32>(
            "While the screen was off",
            &font::BOLD,
            theme::SMALL,
            theme::TEXT_SECONDARY,
        )
        .padding(Edges::Leading, 10)
        .flex_infinite_width(HorizontalAlignment::Leading)
        .frame()
        .with_height(HEADING),
        super::list(
            ForEach::<MAX_CHATS>::new_vertical(&INDICES[..count], move |&i| {
                let (arrived, chat) = &cards[i];
                card(arrived, chat)
            })
            .with_spacing(GAP)
            .keyed(move |&i| keys::chat(ids[i])),
        ),
    ))
    .with_spacing(0)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| {
        let focused = s.focus().current().and_then(keys::chat_of);
        match (key, focused) {
            (Key::Backspace, _) => {
                s.back();
                KeyResponse::Handled
            }
            (Key::Enter, Some(chat)) => {
                s.open_arrival(chat);
                KeyResponse::Handled
            }
            // The chat opens with its composer focused, which takes the key.
            (Key::Character(_), Some(chat)) => {
                s.open_arrival(chat);
                KeyResponse::Redirect
            }
            _ => KeyResponse::Ignored,
        }
    })
}

/// A chat, its age, and its newest message in up to two lines.
fn card(arrived: &Arrived, chat: &ChatSummary) -> impl View<Rgb565, State> + use<> {
    let text = arrived.text.clone();
    let chat = chat.clone();
    let id = chat.id;
    Button::new(
        move |s: &mut State| s.open_arrival(id),
        move |button: ButtonState| {
            let focused = button.is_focused();
            let (fill, ring, ring_width) = if focused {
                (theme::FOCUS, theme::ACCENT, 2)
            } else {
                (theme::PANEL, theme::LINE, 1)
            };
            VStack::new((
                HStack::new((
                    label::<32>(&chat.name, &font::BOLD, theme::BODY, theme::TEXT),
                    Spacer::default(),
                    label::<6>(
                        &chat.age,
                        &font::SEMIBOLD,
                        theme::SMALL,
                        theme::TEXT_TERTIARY,
                    ),
                ))
                .with_spacing(8)
                .frame()
                .with_height(NAME_LINE),
                Text::new(text.clone(), &font::REGULAR)
                    .with_font_size(theme::SECONDARY)
                    .with_line_limit(2)
                    .foreground_color(theme::TEXT_SECONDARY)
                    .padding(Edges::Trailing, 4)
                    .flex_infinite_width(HorizontalAlignment::Leading),
            ))
            .with_spacing(0)
            .with_alignment(HorizontalAlignment::Leading)
            .padding(Edges::Top, TOP)
            .padding(Edges::Bottom, MESSAGE_LINE - theme::SECONDARY)
            .padding(Edges::Leading, TEXT_X)
            .padding(Edges::Trailing, 10)
            .overlay(
                Alignment::TopLeading,
                super::avatar::view(&chat.avatar, AVATAR)
                    .padding(Edges::Leading, AVATAR_X)
                    .padding(Edges::Top, 8),
            )
            .hint_background_color(fill)
            .background(
                Alignment::Center,
                ZStack::new((
                    RoundedRectangle::new(RADIUS).foreground_color(fill),
                    RoundedRectangle::new(RADIUS)
                        .stroked(ring_width)
                        .foreground_color(ring),
                )),
            )
        },
    )
    .padding(Edges::Horizontal, 6)
}
