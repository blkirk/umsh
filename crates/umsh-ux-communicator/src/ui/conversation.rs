//! A conversation: its header, the history ending at the newest message,
//! and the composer, which folds to one line while you read.
//!
//! Backspace goes back, unless the composer is being edited. Typing while
//! reading goes to the composer.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::TextArea;
use buoyant::view::prelude::*;
use buoyant::view::scroll_view::ScrollBarVisibility;
use buoyant::view::text_field::TextFieldStatus;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;
use heapless::Vec;

use super::label;
use crate::avatar::AvatarStyle;
use crate::model::{
    ChatId, Delivery, Direction, Draft, MAX_MESSAGES, Message, MessageId, Messages, State,
};
use crate::nav::keys;
use crate::text::DraftText;
use crate::{font, icon, theme};
use buoyant::font::PrerenderedFont;
use buoyant::primitives::geometry::CornerRadii;
use buoyant::render::AreaColors;

const HEADER: u32 = 30;
const AVATAR: u32 = 26;
/// The composer's band, and its field inside it.
const COMPOSER_INSET: u32 = 3;
const FIELD_RADIUS: u16 = 14;
/// The composer folded while reading.
const BAR: u32 = 24;
/// From the screen's edges to the bubbles.
const SIDE: u32 = 8;
const BUBBLE_MAX: u32 = 330;
const BUBBLE_PAD_X: u32 = 11;
const BUBBLE_PAD_Y: u32 = 4;
const BUBBLE_RADIUS: u16 = 14;
/// The corner on the sender's side, at the bottom.
const TAIL_RADIUS: u16 = 5;
/// The focus ring, outside the bubble.
const RING_GAP: u32 = 4;
/// Between messages from the same sender, and from different ones.
const GAP_SAME: u32 = 3;
const GAP_CHANGE: u32 = 8;
const SENDER_LINE: u32 = 18;
const STATUS_LINE: u32 = 21;
/// Keeps part of the next message in view when focus moves.
const FOCUS_MARGIN: u32 = 12;
/// How far a turn scrolls through a message taller than the history: a line.
const SCROLL_STEP: u32 = 24;
/// The field's fill while it is edited.
const ACTIVE_FIELD: Rgb565 = Rgb565::new(0x1A >> 3, 0x23 >> 2, 0x33 >> 3);

/// Indices `ForEach` can hold without borrowing the state.
static INDICES: [usize; MAX_MESSAGES] = {
    let mut a = [0; MAX_MESSAGES];
    let mut i = 0;
    while i < MAX_MESSAGES {
        a[i] = i;
        i += 1;
    }
    a
};

pub fn view(state: &State, chat: ChatId) -> impl View<Rgb565, State> + use<> {
    let summary = state.chat(chat);
    let messages = state.conversation(chat).map_or(&[][..], |c| &c.messages);
    let reading = state
        .screen()
        .focus()
        .current()
        .is_some_and(keys::is_message);
    VStack::new((
        header(
            summary.map_or("", |s| &s.name),
            summary.map(|s| &s.avatar),
            summary.and_then(|s| s.hops),
        ),
        history(messages),
        summary.map(|s| composer(chat, &s.draft, reading)),
    ))
    .with_spacing(0)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.back();
            KeyResponse::Handled
        }
        Key::Character(_) | Key::Enter => {
            s.focus().request(keys::COMPOSER);
            KeyResponse::Redirect
        }
        _ => KeyResponse::Ignored,
    })
}

/// Back, the peer or channel, and how far away a peer is.
fn header(
    name: &str,
    avatar: Option<&AvatarStyle>,
    hops: Option<u8>,
) -> impl View<Rgb565, State> + use<> {
    let mut distance: heapless::String<12> = heapless::String::new();
    match hops {
        Some(0) => _ = distance.push_str("Direct"),
        Some(1) => _ = distance.push_str("1 hop"),
        Some(n) => _ = core::fmt::write(&mut distance, format_args!("{n} hops")),
        None => {}
    }
    HStack::new((
        Image::new(&icon::BACK)
            .as_template()
            .foreground_color(theme::TEXT_TERTIARY),
        avatar.map(|style| super::avatar::view(style, AVATAR)),
        label::<32>(name, &font::BOLD, theme::BODY, theme::TEXT).fixed_size(true, false),
        label::<12>(&distance, &font::MEDIUM, theme::SMALL, theme::TEXT_TERTIARY),
        Spacer::default(),
    ))
    .with_spacing(6)
    .padding(Edges::Leading, 4)
    .frame()
    .with_height(HEADER)
    .overlay(
        Alignment::Bottom,
        Rectangle
            .foreground_color(theme::LINE)
            .frame()
            .with_height(1),
    )
}

/// The messages, oldest first, ending at the composer. History shorter
/// than the space sits on the composer.
fn history(messages: &[Message]) -> impl View<Rgb565, State> + use<> {
    let messages = Messages::from_slice(messages).unwrap_or_default();
    let ids: Vec<MessageId, MAX_MESSAGES> = messages.iter().map(|m| m.id).collect();
    // Delivery shows under the newest message sent.
    let newest_sent = messages
        .iter()
        .rposition(|m| m.direction == Direction::Outbound);
    let count = messages.len();
    ScrollView::new(
        ForEach::<MAX_MESSAGES>::new_vertical(&INDICES[..count], move |&i| {
            item(&messages, i, newest_sent == Some(i))
        })
        .keyed(move |&i| keys::message(ids[i])),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_initial_pin_to_end(true)
    .with_short_content_at_end(true)
    .with_focus_margin(FOCUS_MARGIN)
    .with_scroll_step(SCROLL_STEP)
}

/// A message, with its sender above it in a channel when the sender changes,
/// and its delivery below it when asked.
fn item(messages: &Messages, i: usize, delivery: bool) -> impl View<Rgb565, State> + use<> {
    let message = &messages[i];
    let follows_same = i.checked_sub(1).is_some_and(|p| {
        messages[p].direction == message.direction && messages[p].sender == message.sender
    });
    let gap = if i == 0 || follows_same {
        GAP_SAME
    } else {
        GAP_CHANGE
    };
    let outbound = message.direction == Direction::Outbound;
    let alignment = if outbound {
        HorizontalAlignment::Trailing
    } else {
        HorizontalAlignment::Leading
    };
    let sender = (!follows_same).then(|| message.sender.clone()).flatten();
    VStack::new((
        sender.map(|name| {
            label::<24>(&name, &font::BOLD, theme::SMALL, theme::TEXT_SECONDARY)
                .frame()
                .with_height(SENDER_LINE)
                .padding(Edges::Leading, SIDE + 4)
        }),
        bubble(message),
        delivery.then(|| delivery_line(message.delivery)),
    ))
    .with_spacing(0)
    .with_alignment(alignment)
    .flex_infinite_width(alignment)
    .padding(Edges::Top, gap)
}

fn bubble(message: &Message) -> impl View<Rgb565, State> + use<> {
    let text = message.text.clone();
    let outbound = message.direction == Direction::Outbound;
    let mut corners = CornerRadii::equal(BUBBLE_RADIUS);
    if outbound {
        corners.bottom_right = TAIL_RADIUS;
    } else {
        corners.bottom_left = TAIL_RADIUS;
    }
    let (fill, color, side) = if outbound {
        (
            theme::OUTBOUND,
            Rgb565::WHITE,
            HorizontalAlignment::Trailing,
        )
    } else {
        (theme::INBOUND, theme::TEXT, HorizontalAlignment::Leading)
    };
    Button::new(
        |_: &mut State| {},
        move |button: ButtonState| {
            Text::new(text.clone(), &font::REGULAR)
                .with_font_size(theme::BODY)
                .foreground_color(color)
                .hint_background_color(fill)
                .padding(Edges::Horizontal, BUBBLE_PAD_X)
                .padding(Edges::Vertical, BUBBLE_PAD_Y)
                .background_color(fill, RoundedRectangle::with_radii(corners))
                .padding(Edges::All, RING_GAP)
                .overlay(
                    Alignment::Center,
                    button.is_focused().then(|| {
                        RoundedRectangle::new(BUBBLE_RADIUS + RING_GAP as u16)
                            .stroked(2)
                            .foreground_color(theme::ACCENT)
                    }),
                )
                // Text wraps to fit; a short bubble keeps to its side.
                .flex_frame()
                .with_max_width(BUBBLE_MAX + 2 * RING_GAP)
                .with_horizontal_alignment(side)
        },
    )
    .padding(Edges::Horizontal, SIDE - RING_GAP)
}

fn delivery_line(delivery: Delivery) -> impl View<Rgb565, State> + use<> {
    let (text, color) = match delivery {
        Delivery::Sending => ("Sending", theme::TEXT_TERTIARY),
        Delivery::Sent => ("Sent", theme::TEXT_TERTIARY),
        Delivery::Delivered => ("Delivered", theme::TEXT_TERTIARY),
        Delivery::Failed => ("Not delivered", theme::AMBER),
    };
    label::<16>(text, &font::SEMIBOLD, theme::SMALL, color)
        .frame()
        .with_height(STATUS_LINE)
        .padding(Edges::Trailing, SIDE)
}

/// The composer, or the line it folds to while you read.
fn composer(chat: ChatId, draft: &Draft, folded: bool) -> impl View<Rgb565, State> + use<> {
    buoyant::match_view!(folded, {
        true => folded_bar(draft),
        false => field(chat, draft),
    })
}

fn field(chat: ChatId, draft: &Draft) -> impl View<Rgb565, State> + use<> {
    let empty = draft.is_empty();
    TextField::new(draft, &font::REGULAR, move |s: &mut State| {
        s.draft_mut(chat)
    })
    .with_font_size(theme::BODY)
    .with_max_lines(3)
    .on_submit(move |s: &mut State| s.send(chat))
    .with_label(move |status: TextFieldStatus, area| field_look(status, area, empty))
    .focus_key(keys::COMPOSER)
    .padding(Edges::Horizontal, SIDE)
    .padding(Edges::Vertical, COMPOSER_INSET)
}

fn field_look(
    status: TextFieldStatus,
    area: TextArea<'static, DraftText, PrerenderedFont>,
    empty: bool,
) -> impl View<Rgb565, State> + use<> {
    let (fill, line_width) = if status.is_active() {
        (ACTIVE_FIELD, 2)
    } else {
        (theme::FIELD, 1)
    };
    let ring = if status.is_focused() {
        theme::ACCENT
    } else {
        theme::MENU_LINE
    };
    area.colors(AreaColors {
        caret: Some(theme::ACCENT),
        scroll_bar: Some((theme::MENU_LINE, theme::TEXT_SECONDARY)),
    })
    .foreground_color(theme::TEXT)
    .hint_background_color(fill)
    .overlay(
        Alignment::Leading,
        (empty && !status.is_active()).then(|| {
            label::<8>("Message", &font::REGULAR, theme::BODY, theme::TEXT_TERTIARY)
                .padding(Edges::Leading, 4)
        }),
    )
    .padding(Edges::Horizontal, 12)
    .padding(Edges::Vertical, 2)
    .background(
        Alignment::Center,
        ZStack::new((
            RoundedRectangle::new(FIELD_RADIUS).foreground_color(fill),
            RoundedRectangle::new(FIELD_RADIUS)
                .stroked(line_width)
                .foreground_color(ring),
        )),
    )
}

/// The composer folded to one dim line: the draft, or where one would go.
fn folded_bar(draft: &Draft) -> impl View<Rgb565, State> + use<> {
    let text = (!draft.is_empty()).then(|| draft.storage().clone());
    // Focusable, so turning past the newest message reaches the composer.
    Button::new(
        |_: &mut State| {},
        move |_| {
            HStack::new((
                Image::new(&icon::PENCIL)
                    .as_template()
                    .foreground_color(theme::TEXT_TERTIARY),
                text.clone().map(|text| {
                    super::line(text, &font::REGULAR, theme::SMALL, theme::TEXT_SECONDARY)
                }),
                text.is_none().then(|| {
                    label::<8>(
                        "Message",
                        &font::REGULAR,
                        theme::SMALL,
                        theme::TEXT_TERTIARY,
                    )
                }),
            ))
            .with_spacing(4)
            .padding(Edges::Leading, 8)
            .flex_infinite_width(HorizontalAlignment::Leading)
            .frame()
            .with_height(BAR)
            .background_color(theme::CHROME, Rectangle)
            .overlay(
                Alignment::Top,
                Rectangle
                    .foreground_color(theme::LINE)
                    .frame()
                    .with_height(1),
            )
        },
    )
    .focus_key(keys::COMPOSER)
}
