//! The Chats list: two lines per chat, newest first. The last row is
//! clipped by the screen edge when the list goes on.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use buoyant::view::scroll_view::ScrollBarVisibility;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::Vec;

use super::{RAIL, label};
use crate::model::{ChatSummary, MAX_CHATS, State};
use crate::nav::keys;
use crate::{font, icon, theme};

const ROW: u32 = 44;
/// The lines' heights: a line of each one's type, which together fill a row.
const NAME_LINE: u32 = 24;
const PREVIEW_LINE: u32 = ROW - NAME_LINE;
const AVATAR: u32 = 34;
/// From the list's leading edge to the text.
const TEXT_X: u32 = 51;
const LIST_WIDTH: u32 = theme::WIDTH - RAIL;

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

/// The list. Backspace moves to the rail, Enter opens the focused chat, as a
/// press does, and a letter starts Find with it.
pub fn view(chats: &Vec<ChatSummary, MAX_CHATS>) -> impl View<Rgb565, State> + use<> {
    let rows = chats.clone();
    let ids: Vec<u32, MAX_CHATS> = chats.iter().map(|c| c.id).collect();
    let count = rows.len();
    ScrollView::new(
        ForEach::<MAX_CHATS>::new_vertical(&INDICES[..count], move |&i| row(&rows[i]))
            .keyed(move |&i| keys::chat(ids[i])),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.focus_rail();
            KeyResponse::Handled
        }
        Key::Enter => match s.focus().current().and_then(keys::chat_of) {
            Some(chat) => {
                s.open_chat(chat);
                KeyResponse::Handled
            }
            None => KeyResponse::Ignored,
        },
        // Find's field, focused as it opens, takes the key.
        Key::Character(c) if !c.is_whitespace() => {
            s.start_find();
            KeyResponse::Redirect
        }
        _ => KeyResponse::Ignored,
    })
}

fn row(chat: &ChatSummary) -> impl View<Rgb565, State> + use<> {
    let chat = chat.clone();
    let id = chat.id;
    Button::new(
        move |s: &mut State| s.open_chat(id),
        move |button: ButtonState| {
            let focused = button.is_focused();
            let background = if focused {
                theme::FOCUS
            } else {
                theme::BACKGROUND
            };
            // Each line has a fixed height and the avatar is an overlay, so
            // no stack nests in another: every size a stack probes a line
            // with is the same, and its text is measured once.
            VStack::new((
                name_line(&chat).frame().with_height(NAME_LINE),
                preview_line(&chat).frame().with_height(PREVIEW_LINE),
            ))
            .with_spacing(0)
            .with_alignment(HorizontalAlignment::Leading)
            .padding(Edges::Leading, TEXT_X)
            .flex_infinite_width(HorizontalAlignment::Leading)
            .frame()
            .with_height(ROW)
            // Inside the background, so its edges blend into the row's color.
            .overlay(
                Alignment::Leading,
                super::avatar::view(&chat.avatar, AVATAR)
                    .frame()
                    .with_width(TEXT_X),
            )
            // Only the focused row is filled: the others show the screen's
            // background, and painting it again is most of what a frame
            // would cost on the device.
            .hint_background_color(background)
            .background(
                Alignment::Center,
                focused.then(|| Rectangle.foreground_color(theme::FOCUS)),
            )
            .overlay(
                Alignment::Leading,
                focused.then(|| {
                    Rectangle
                        .foreground_color(theme::ACCENT)
                        .frame_sized(4, ROW)
                }),
            )
            .overlay(
                Alignment::BottomTrailing,
                (!focused).then(|| {
                    Rectangle
                        .foreground_color(theme::LINE)
                        .frame_sized(LIST_WIDTH - TEXT_X, 1)
                }),
            )
        },
    )
}

/// The chat's name, then its age at the trailing edge.
fn name_line(chat: &ChatSummary) -> impl View<Rgb565, State> + use<> {
    let age_color = if chat.unread {
        theme::ACCENT
    } else {
        theme::TEXT_TERTIARY
    };
    let name_face = if chat.unread {
        &font::BOLD
    } else {
        &font::SEMIBOLD
    };
    let name_color = if chat.name.as_str() == "EMERGENCY" {
        theme::RED
    } else {
        theme::TEXT
    };
    HStack::new((
        label::<32>(&chat.name, name_face, theme::BODY, name_color),
        Spacer::default(),
        chat.muted.then(|| {
            Image::new(&icon::BELL_SLASH)
                .as_template()
                .foreground_color(theme::TEXT_TERTIARY)
        }),
        label::<6>(&chat.age, &font::SEMIBOLD, theme::SMALL, age_color).fixed_size(true, false),
    ))
    .with_spacing(4)
    .padding(Edges::Trailing, 10)
}

/// The last message, or the draft, with the unread dot at the trailing edge.
fn preview_line(chat: &ChatSummary) -> impl View<Rgb565, State> + use<> {
    let color = if chat.unread {
        theme::TEXT_SECONDARY
    } else {
        theme::TEXT_TERTIARY
    };
    let draft = (!chat.draft.is_empty()).then(|| chat.draft.storage().clone());
    HStack::new((
        draft.is_some().then(|| {
            label::<8>("Draft", &font::BOLD, theme::SECONDARY, theme::AMBER).fixed_size(true, false)
        }),
        draft.map(|draft| super::line(draft, &font::REGULAR, theme::SECONDARY, color)),
        chat.draft
            .is_empty()
            .then(|| label::<64>(&chat.preview, &font::REGULAR, theme::SECONDARY, color)),
        Spacer::default(),
        chat.unread
            .then(|| Circle.foreground_color(theme::ACCENT).frame_sized(10, 10)),
    ))
    .with_spacing(6)
    .padding(Edges::Trailing, 9)
}
