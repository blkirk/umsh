//! A conversation: its header, the history ending at the newest message,
//! and the composer, which folds to one line while you read.
//!
//! Backspace goes back, unless the composer is being edited. Typing while
//! reading goes to the composer.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use buoyant::view::scroll_view::ScrollBarVisibility;
use buoyant::view::text_field::TextFieldStatus;
use buoyant::view::{Lazy, TextArea};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::RgbColor;
use heapless::Vec;

use super::label;
use crate::avatar::AvatarStyle;
use crate::model::{
    ChatId, ChatKind, Delivery, Direction, Draft, Invite, MAX_MESSAGES, Message, MessageId,
    Messages, Reacted, Reaction, Reactions, State,
};
use crate::nav::{Screen, keys};
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
/// Where a channel's received bubbles begin, beside their senders' avatars,
/// and those avatars' size and place.
const IN_X: u32 = 42;
const MEMBER_AVATAR: u32 = 30;
const MEMBER_AVATAR_X: u32 = 5;
const STATUS_LINE: u32 = 21;
/// Keeps part of the next message in view when focus moves.
const FOCUS_MARGIN: u32 = 12;
/// How far a turn scrolls through a message taller than the history: a line.
const SCROLL_STEP: u32 = 24;
/// What an empty composer says: "Message" and the chat's name.
type Placeholder = heapless::String<48>;
/// An action in the actions menu, which can name a channel.
type MenuText = heapless::String<40>;
/// The field's fill while it is edited.
const ACTIVE_FIELD: Rgb565 = Rgb565::new(0x1A >> 3, 0x23 >> 2, 0x33 >> 3);
/// The field's fill while it is edited in an open channel.
const ACTIVE_FIELD_OPEN: Rgb565 = Rgb565::new(0x15 >> 3, 0x2B >> 2, 0x1F >> 3);

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
    let (actions, reply) = match state.screen() {
        Screen::Conversation { actions, reply, .. } => (*actions, *reply),
        _ => (None, None),
    };
    // A message's actions are part of reading it.
    let reading = actions.is_some()
        || state
            .screen()
            .focus()
            .current()
            .is_some_and(keys::is_message);
    let find = |id: Option<MessageId>| id.and_then(|id| messages.iter().find(|m| m.id == id));
    // The menu offers my reaction as the one already chosen, and a
    // member's message offers its sender.
    let menu = find(actions).map(|m| {
        let join = m.invite.as_ref().map(|invite| {
            let mut text = MenuText::new();
            _ = core::fmt::write(&mut text, format_args!("Join {}", invite.name.as_str()));
            text
        });
        (m.my_reaction(), m.sender.is_some(), join)
    });
    let quoted = find(reply).map(|message| quote(message, summary.map_or("", |s| &s.name)));
    let kind = summary.map(|s| s.kind);
    let channel = matches!(kind, Some(ChatKind::Channel { .. }));
    let open = kind.is_some_and(ChatKind::is_open);
    let mut placeholder = Placeholder::new();
    _ = core::fmt::write(
        &mut placeholder,
        format_args!("Message {}", summary.map_or("", |s| &s.name)),
    );
    VStack::new((
        header(
            summary.map_or("", |s| &s.name),
            summary.map(|s| &s.avatar),
            summary.map(|s| s.kind),
        ),
        history(messages, actions, channel, open),
        state.airtime_wait.map(airtime_line),
        quoted,
        summary.map(|s| composer(chat, &s.draft, reading, open, &placeholder)),
    ))
    .with_spacing(0)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| match key {
        // A reply is dropped first, keeping its text; then Back.
        Key::Backspace => {
            if !s.drop_reply() {
                s.back();
            }
            KeyResponse::Handled
        }
        Key::Character(_) | Key::Enter => {
            s.focus().request(keys::COMPOSER);
            KeyResponse::Redirect
        }
        _ => KeyResponse::Ignored,
    })
    .popover(menu.as_ref(), |(mine, member, join)| {
        actions_menu(*mine, *member, join.clone())
    })
}

/// Back, the peer or channel, and how far away a peer is or that it is a
/// channel; or a screen's title alone.
pub(super) fn header(
    name: &str,
    avatar: Option<&AvatarStyle>,
    kind: Option<ChatKind>,
) -> impl View<Rgb565, State> + use<> {
    let mut distance: heapless::String<12> = heapless::String::new();
    match kind {
        Some(ChatKind::Peer { hops: Some(0) }) => _ = distance.push_str("Direct"),
        Some(ChatKind::Peer { hops: Some(1) }) => _ = distance.push_str("1 hop"),
        Some(ChatKind::Peer { hops: Some(n) }) => {
            _ = core::fmt::write(&mut distance, format_args!("{n} hops"));
        }
        Some(ChatKind::Channel { .. }) => _ = distance.push_str("Channel"),
        Some(ChatKind::Peer { hops: None }) | None => {}
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
/// than the space sits on the composer. The message whose actions are open
/// stays marked while focus is in them.
fn history(
    messages: &[Message],
    acting_on: Option<MessageId>,
    channel: bool,
    open: bool,
) -> impl View<Rgb565, State> + use<> {
    let messages = Messages::from_slice(messages).unwrap_or_default();
    let ids: Vec<MessageId, MAX_MESSAGES> = messages.iter().map(|m| m.id).collect();
    // Delivery shows under the newest message sent, the newest delivered,
    // and every one unconfirmed.
    let newest_sent = messages
        .iter()
        .rposition(|m| m.direction == Direction::Outbound);
    let newest_delivered = messages
        .iter()
        .rposition(|m| m.direction == Direction::Outbound && m.delivery == Delivery::Delivered);
    let count = messages.len();
    ScrollView::new(
        ForEach::<MAX_MESSAGES>::new_vertical(&INDICES[..count], move |&i| {
            let delivery = Some(i) == newest_sent
                || Some(i) == newest_delivered
                || messages[i].delivery == Delivery::Unconfirmed;
            item(&messages, i, delivery, acting_on, channel, open)
        })
        .keyed(move |&i| keys::message(ids[i])),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_initial_pin_to_end(true)
    .with_short_content_at_end(true)
    .with_focus_margin(FOCUS_MARGIN)
    .with_scroll_step(SCROLL_STEP)
}

/// A message, with its delivery below it when asked. In a channel, each run
/// of messages from one member has the member's name above its first and
/// their avatar beside its last.
fn item(
    messages: &Messages,
    i: usize,
    delivery: bool,
    acting_on: Option<MessageId>,
    channel: bool,
    open: bool,
) -> impl View<Rgb565, State> + use<> {
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
    let ends_run = messages
        .get(i + 1)
        .is_none_or(|next| next.direction != message.direction || next.sender != message.sender);
    // A channel's received bubbles stand clear of their senders' avatars.
    let beside_avatar = channel && !outbound;
    let avatar = message
        .sender
        .as_ref()
        .filter(|_| beside_avatar && ends_run)
        .map(|sender| {
            Lazy::new(sender.avatar.clone(), member_avatar)
                .padding(Edges::Leading, MEMBER_AVATAR_X)
                .padding(Edges::Bottom, RING_GAP)
        });
    let lead = if beside_avatar { IN_X } else { SIDE };
    // Chips over the bubble take room above it, which a sender's name
    // beside them shares.
    let reserve = chip_reserve(&message.reactions).saturating_sub(if sender.is_some() {
        SENDER_LINE
    } else {
        0
    });
    VStack::new((
        sender.map(|sender| {
            label::<24>(
                &sender.name,
                &font::BOLD,
                theme::SMALL,
                theme::TEXT_SECONDARY,
            )
            .frame()
            .with_height(SENDER_LINE)
            .padding(Edges::Leading, lead + 4)
        }),
        bubble(message, acting_on == Some(message.id), open).padding(Edges::Leading, lead - SIDE),
        delivery.then(|| delivery_line(message.delivery)),
    ))
    .with_spacing(0)
    .with_alignment(alignment)
    .flex_infinite_width(alignment)
    // Received messages have no delivery line, so the bottom is the bubble's.
    .overlay(Alignment::BottomLeading, avatar)
    .padding(Edges::Top, gap + reserve)
}

/// A message in its bubble. Mine are in the color that says who can read
/// them: green in an `open` channel.
fn bubble(message: &Message, acting_on: bool, open: bool) -> impl View<Rgb565, State> + use<> {
    let id = message.id;
    let text = message.text.clone();
    let reactions = message.reactions.clone();
    let invite = message.invite.clone();
    let outbound = message.direction == Direction::Outbound;
    let mut corners = CornerRadii::equal(BUBBLE_RADIUS);
    if outbound {
        corners.bottom_right = TAIL_RADIUS;
    } else {
        corners.bottom_left = TAIL_RADIUS;
    }
    let (fill, color, side) = if outbound {
        (
            if open {
                theme::OUTBOUND_OPEN
            } else {
                theme::OUTBOUND
            },
            Rgb565::WHITE,
            HorizontalAlignment::Trailing,
        )
    } else {
        (theme::INBOUND, theme::TEXT, HorizontalAlignment::Leading)
    };
    let corner = if outbound {
        Alignment::TopLeading
    } else {
        Alignment::TopTrailing
    };
    // Pressing a message opens its actions; pressing an unconfirmed one
    // sends it again.
    let unconfirmed = message.delivery == Delivery::Unconfirmed;
    Button::new(
        move |s: &mut State| {
            if unconfirmed {
                s.retry(id);
            } else {
                s.open_actions(id);
            }
        },
        move |button: ButtonState| {
            VStack::new((
                // An invitation alone is only its card.
                (!text.is_empty() || invite.is_none()).then(|| {
                    Text::new(text.clone(), &font::REGULAR)
                        .with_font_size(theme::BODY)
                        .foreground_color(color)
                        .hint_background_color(fill)
                        .padding(Edges::Horizontal, BUBBLE_PAD_X)
                        .padding(Edges::Vertical, BUBBLE_PAD_Y)
                        .background_color(fill, RoundedRectangle::with_radii(corners))
                }),
                invite
                    .as_ref()
                    .map(|invite| Lazy::new((invite.clone(), fill, color, corners), invite_card)),
            ))
            .with_spacing(GAP_SAME)
            .with_alignment(side)
            .padding(Edges::All, RING_GAP)
            .overlay(
                Alignment::Center,
                (button.is_focused() || acting_on).then(|| {
                    RoundedRectangle::new(BUBBLE_RADIUS + RING_GAP as u16)
                        .stroked(2)
                        .foreground_color(theme::ACCENT)
                }),
            )
            // Over the ring, so a chip cuts it as it cuts the bubble.
            .overlay(
                corner,
                (!reactions.is_empty())
                    .then(|| Lazy::new((reactions.clone(), outbound, open), chips)),
            )
            // Text wraps to fit; a short bubble keeps to its side.
            .flex_frame()
            .with_max_width(BUBBLE_MAX + 2 * RING_GAP)
            .with_horizontal_alignment(side)
        },
    )
    .padding(Edges::Horizontal, SIDE - RING_GAP)
}

/// An invitation's card, and its avatar.
const CARD_WIDTH: u32 = 300;
const CARD_HEIGHT: u32 = 64;
const CARD_AVATAR: u32 = 44;

/// The channel an invitation is to, in a card that a press can take: a link
/// inside text could not be focused.
///
/// Built from its data each time it is drawn ([`Lazy`]), as the chips are.
fn invite_card(
    (invite, fill, color, corners): &(Invite, Rgb565, Rgb565, CornerRadii<u16>),
) -> impl View<Rgb565, ()> + use<> {
    let (fill, color, corners) = (*fill, *color, *corners);
    HStack::new((
        super::avatar::view(&invite.avatar(), CARD_AVATAR),
        VStack::new((
            Text::new(invite.name.clone(), &font::BOLD)
                .with_font_size(theme::BODY)
                .with_line_limit(1)
                .foreground_color(color),
            Text::new("Channel invite", &font::REGULAR)
                .with_font_size(theme::SECONDARY)
                .with_line_limit(1)
                .foreground_color(theme::TEXT_SECONDARY),
        ))
        .with_spacing(0)
        .with_alignment(HorizontalAlignment::Leading),
        Spacer::default(),
        Image::new(&icon::CHEVRON)
            .as_template()
            .foreground_color(theme::TEXT_TERTIARY),
    ))
    .with_spacing(10)
    .hint_background_color(fill)
    .padding(Edges::Leading, 10)
    .padding(Edges::Trailing, 12)
    .frame()
    .with_width(CARD_WIDTH)
    .with_height(CARD_HEIGHT)
    .background_color(fill, RoundedRectangle::with_radii(corners))
}

/// A reaction chip's diameter, the background ring that sets it off, and how
/// far apart chips overlap.
const CHIP: u32 = 28;
const CHIP_HALO: u32 = 2;
const CHIP_STEP: i32 = 21;
/// How far a chip's center sits above the bubble's top edge.
const CHIP_RISE: u32 = 6;
/// The count on a chip shared by several people.
const BADGE: u32 = 18;

/// Room above a bubble for its chips, and for a count when there is one.
fn chip_reserve(reactions: &[Reacted]) -> u32 {
    if reactions.is_empty() {
        return 0;
    }
    let counted = reactions.iter().any(|r| r.count > 1);
    CHIP_RISE + CHIP / 2 + CHIP_HALO + if counted { 6 } else { 0 }
}

/// A bubble's reactions as round chips over its outer top corner, like a
/// hand of cards: mine frontmost at the corner, with one dot trail falling
/// outside the bubble. Placed from the corner outward and inward: offsets
/// here are from the corner of the ring around the bubble.
///
/// Built from its data each time it is drawn ([`Lazy`]): kept in the render
/// trees, every bubble's chips would outweigh the rest of the screen.
fn chips((reactions, outbound, open): &(Reactions, bool, bool)) -> impl View<Rgb565, ()> + use<> {
    let outbound = *outbound;
    // Mine takes the color of my bubbles.
    let (mine_fill, mine_pad) = if *open {
        (theme::OUTBOUND_OPEN, icon::Pad::MineOpen)
    } else {
        (theme::OUTBOUND, icon::Pad::Mine)
    };
    // Toward the outside of the bubble: left on a sent message.
    let out: i32 = if outbound { -1 } else { 1 };
    let ring = RING_GAP as i32;
    // How far an element's outer edge reaches beyond the corner, as an
    // offset from the side the cluster is aligned to.
    let edge = move |beyond: i32| out * (beyond - ring);
    let top = move |above_bubble: i32| ring + above_bubble;
    let halo = (CHIP + 2 * CHIP_HALO) as i32;
    // Mine at the corner, then others inward, drawn back to front.
    let mut order: Vec<Reacted, { Reaction::ALL.len() }> = Vec::new();
    for r in reactions.iter().filter(|r| r.mine) {
        _ = order.push(*r);
    }
    for r in reactions.iter().filter(|r| !r.mine) {
        _ = order.push(*r);
    }
    let count = order.len();
    let front = order.first().map_or(theme::CHIP_THEIRS, |r| {
        if r.mine {
            mine_fill
        } else {
            theme::CHIP_THEIRS
        }
    });
    // The `k`th chip drawn: the innermost first, mine at the corner last.
    let chip = move |k: usize| {
        let place = count.checked_sub(1 + k)?;
        order.get(place).map(|r| {
            let inward = CHIP_STEP * place as i32;
            let (fill, pad) = if r.mine {
                (mine_fill, mine_pad)
            } else {
                (theme::CHIP_THEIRS, icon::Pad::Theirs)
            };
            let number = (r.count > 1).then(|| {
                let mut n: heapless::String<4> = heapless::String::new();
                _ = core::fmt::write(&mut n, format_args!("{}", r.count));
                ZStack::new((
                    Circle.foreground_color(theme::BACKGROUND),
                    Text::new(n, &font::BOLD)
                        .with_font_size(theme::SMALL)
                        .with_line_limit(1)
                        .foreground_color(theme::TEXT),
                ))
                .frame_sized(BADGE, BADGE)
                .offset(
                    edge(BADGE as i32 / 2 + 11 - inward),
                    top(-(CHIP_RISE as i32) - 11 - BADGE as i32 / 2),
                )
            });
            ZStack::new((
                ZStack::new((
                    Circle.foreground_color(theme::BACKGROUND),
                    Circle.foreground_color(fill).frame_sized(CHIP, CHIP),
                    Image::new(icon::reaction(r.reaction, pad)),
                ))
                .frame_sized(halo as u32, halo as u32)
                .offset(edge(halo / 2 - inward), top(-(CHIP_RISE as i32) - halo / 2)),
                number,
            ))
            .with_alignment(if outbound {
                Alignment::TopLeading
            } else {
                Alignment::TopTrailing
            })
        })
    };
    // The dot trail, from the corner chip outward and down.
    let dot = move |distance: i32, radius: u32, color: Rgb565| {
        let size = 2 * radius;
        Circle
            .foreground_color(color)
            .frame_sized(size, size)
            .offset(
                edge(distance + radius as i32),
                top(distance - CHIP_RISE as i32 - radius as i32),
            )
    };
    ZStack::new((
        dot(15, 6, theme::BACKGROUND),
        dot(15, 4, front),
        dot(21, 4, theme::BACKGROUND),
        dot(21, 2, front),
        chip(0),
        chip(1),
        chip(2),
        chip(3),
        chip(4),
        chip(5),
    ))
    .with_alignment(if outbound {
        Alignment::TopLeading
    } else {
        Alignment::TopTrailing
    })
}

fn delivery_line(delivery: Delivery) -> impl View<Rgb565, State> + use<> {
    let (text, color) = match delivery {
        Delivery::Sending => ("Sending", theme::TEXT_TERTIARY),
        Delivery::Sent => ("Sent", theme::TEXT_TERTIARY),
        Delivery::Delivered => ("Delivered", theme::TEXT_TERTIARY),
        Delivery::Unconfirmed => ("Unconfirmed", theme::AMBER),
    };
    label::<16>(text, &font::SEMIBOLD, theme::SMALL, color)
        .frame()
        .with_height(STATUS_LINE)
        .padding(Edges::Trailing, SIDE)
}

/// When sending returns, while airtime is spent.
fn airtime_line(wait: u16) -> impl View<Rgb565, State> + use<> {
    let mut text: heapless::String<40> = heapless::String::new();
    _ = core::fmt::write(
        &mut text,
        format_args!(
            "Airtime limit \u{B7} Send in {}:{:02}",
            wait / 60,
            wait % 60
        ),
    );
    label::<40>(&text, &font::BOLD, theme::SMALL, theme::AMBER)
        .frame()
        .with_height(STATUS_LINE)
        .flex_infinite_width(HorizontalAlignment::Trailing)
        .padding(Edges::Trailing, SIDE)
}

/// The composer, or the line it folds to while you read.
fn composer(
    chat: ChatId,
    draft: &Draft,
    folded: bool,
    open: bool,
    placeholder: &Placeholder,
) -> impl View<Rgb565, State> + use<> {
    buoyant::match_view!(folded, {
        true => folded_bar(draft, placeholder.clone()),
        false => field(chat, draft, open, placeholder.clone()),
    })
}

fn field(
    chat: ChatId,
    draft: &Draft,
    open: bool,
    placeholder: Placeholder,
) -> impl View<Rgb565, State> + use<> {
    let empty = draft.is_empty();
    TextField::new(draft, &font::REGULAR, move |s: &mut State| {
        s.draft_mut(chat)
    })
    .with_font_size(theme::BODY)
    .with_max_lines(3)
    .on_submit(move |s: &mut State| s.send(chat))
    .with_label(move |status: TextFieldStatus, area| {
        field_look(status, area, open, empty.then_some(&placeholder))
    })
    .focus_key(keys::COMPOSER)
    .padding(Edges::Horizontal, SIDE)
    .padding(Edges::Vertical, COMPOSER_INSET)
}

fn field_look(
    status: TextFieldStatus,
    area: TextArea<'static, DraftText, PrerenderedFont>,
    open: bool,
    placeholder: Option<&Placeholder>,
) -> impl View<Rgb565, State> + use<> {
    // Editing, the field takes the color of what it will send: green where
    // anyone can read it.
    let (active_fill, active_ring) = if open {
        (ACTIVE_FIELD_OPEN, theme::ACCENT_OPEN)
    } else {
        (ACTIVE_FIELD, theme::ACCENT)
    };
    let (fill, line_width) = if status.is_active() {
        (active_fill, 2)
    } else {
        (theme::FIELD, 1)
    };
    let ring = if status.is_active() {
        active_ring
    } else if status.is_focused() {
        theme::ACCENT
    } else {
        theme::MENU_LINE
    };
    area.colors(AreaColors {
        caret: Some(active_ring),
        scroll_bar: Some((theme::MENU_LINE, theme::TEXT_SECONDARY)),
    })
    .foreground_color(theme::TEXT)
    .hint_background_color(fill)
    .overlay(
        Alignment::Leading,
        placeholder
            .filter(|_| !status.is_active())
            .map(|placeholder| {
                label::<48>(
                    placeholder,
                    &font::REGULAR,
                    theme::BODY,
                    theme::TEXT_TERTIARY,
                )
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
fn folded_bar(draft: &Draft, placeholder: Placeholder) -> impl View<Rgb565, State> + use<> {
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
                    label::<48>(
                        &placeholder,
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

/// The quote above the composer while the draft is a reply.
const QUOTE: u32 = 42;

/// Who and what the draft replies to.
fn quote(message: &Message, chat_name: &str) -> impl View<Rgb565, State> + use<> {
    let mut title: heapless::String<48> = heapless::String::new();
    let whom = match (message.direction, &message.sender) {
        (Direction::Outbound, _) => "yourself",
        (Direction::Inbound, Some(sender)) => sender.name.as_str(),
        (Direction::Inbound, None) => chat_name,
    };
    _ = core::fmt::write(&mut title, format_args!("Replying to {whom}"));
    let text = message.text.clone();
    HStack::new((
        RoundedRectangle::new(2)
            .foreground_color(theme::ACCENT)
            .frame_sized(3, QUOTE - 6),
        VStack::new((
            label::<48>(&title, &font::BOLD, theme::SMALL, theme::ACCENT),
            super::line(
                text,
                &font::REGULAR,
                theme::SECONDARY,
                theme::TEXT_SECONDARY,
            ),
        ))
        .with_spacing(0)
        .with_alignment(HorizontalAlignment::Leading),
    ))
    .with_spacing(9)
    .padding(Edges::Leading, 12)
    .padding(Edges::Trailing, SIDE)
    .flex_infinite_width(HorizontalAlignment::Leading)
    .frame()
    .with_height(QUOTE)
}

/// The actions menu's width, and its palette's row.
const MENU_WIDTH: u32 = 236;
const PALETTE: u32 = 44;
const PAD: u32 = 34;
/// A menu item, and its focused look inside it.
const ITEM: u32 = 32;
const ITEM_INSET: u32 = 5;

/// What pressing a message gives: reactions first, then Reply and Details,
/// at the top of the screen beside the history. Backspace closes it, and
/// typing closes it and goes to the composer.
fn actions_menu(
    mine: Option<Reaction>,
    member: bool,
    join: Option<MenuText>,
) -> impl View<Rgb565, State> + use<> {
    let has_join = join.is_some();
    let pad = |reaction: Reaction| {
        Button::new(
            move |s: &mut State| s.react_from_actions(reaction),
            move |button: ButtonState| {
                let ring = if button.is_focused() {
                    Some(theme::ACCENT)
                } else if mine == Some(reaction) {
                    Some(theme::TEXT_TERTIARY)
                } else {
                    None
                };
                ZStack::new((
                    Circle.foreground_color(theme::REACTION_PAD),
                    Image::new(icon::reaction(reaction, icon::Pad::Palette)),
                    ring.map(|color| Circle.stroked(2).foreground_color(color)),
                ))
                .frame_sized(PAD, PAD)
            },
        )
        .focus_key(keys::reaction(reaction))
    };
    let [heart, up, down, laugh, exclaim, question] = Reaction::ALL;
    VStack::new((
        HStack::new((
            pad(heart),
            pad(up),
            pad(down),
            pad(laugh),
            pad(exclaim),
            pad(question),
        ))
        .with_spacing(3)
        .padding(Edges::Leading, 8)
        .flex_infinite_width(HorizontalAlignment::Leading)
        .frame()
        .with_height(PALETTE),
        Rectangle
            .foreground_color(theme::MENU_LINE)
            .frame()
            .with_height(1),
        // An invitation is for joining, first.
        join.map(|text| menu_item(text, keys::JOIN, State::join_from_actions))
            .padding(Edges::Top, 2),
        menu_item(menu_text("Reply"), keys::REPLY, State::reply_from_actions)
            .padding(Edges::Top, if has_join { 0 } else { 2 }),
        member.then(|| {
            menu_item(
                menu_text("View member"),
                keys::VIEW_MEMBER,
                State::view_member_from_actions,
            )
        }),
        menu_item(
            menu_text("Details"),
            keys::DETAILS,
            State::details_from_actions,
        ),
    ))
    .with_spacing(0)
    .padding(Edges::Bottom, 6)
    .frame()
    .with_width(MENU_WIDTH)
    .background(
        Alignment::Center,
        ZStack::new((
            RoundedRectangle::new(12)
                .foreground_color(Rgb565::BLACK)
                .offset(1, 3),
            RoundedRectangle::new(12).foreground_color(theme::MENU),
            RoundedRectangle::new(12)
                .stroked(1)
                .foreground_color(theme::MENU_LINE),
        )),
    )
    .padding(Edges::Top, HEADER + 6)
    .padding(Edges::Trailing, SIDE)
    .flex_infinite_width(HorizontalAlignment::Trailing)
    .flex_infinite_height(VerticalAlignment::Top)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.close_actions();
            KeyResponse::Handled
        }
        Key::Character(_) | Key::Enter => {
            s.close_actions();
            s.focus().request(keys::COMPOSER);
            KeyResponse::Redirect
        }
        _ => KeyResponse::Ignored,
    })
}

/// A fixed action's label.
fn menu_text(text: &str) -> MenuText {
    MenuText::try_from(text).unwrap_or_default()
}

/// One of the menu's actions.
fn menu_item(
    text: MenuText,
    key: buoyant::focus::FocusKey,
    action: fn(&mut State),
) -> impl View<Rgb565, State> + use<> {
    Button::new(action, move |button: ButtonState| {
        let focused = button.is_focused();
        let face = if focused { &font::BOLD } else { &font::REGULAR };
        label::<40>(&text, face, theme::BODY, theme::TEXT)
            .hint_background_color(if focused { theme::FOCUS } else { theme::MENU })
            .padding(Edges::Leading, 11)
            .flex_infinite_width(HorizontalAlignment::Leading)
            .frame()
            .with_height(ITEM - 2)
            .background(
                Alignment::Center,
                focused.then(|| {
                    ZStack::new((
                        RoundedRectangle::new(7).foreground_color(theme::FOCUS),
                        RoundedRectangle::new(7)
                            .stroked(2)
                            .foreground_color(theme::ACCENT),
                    ))
                }),
            )
            .padding(Edges::Horizontal, ITEM_INSET)
            .padding(Edges::Bottom, 2)
    })
    .focus_key(key)
}

/// A channel member's avatar beside their messages. Built from its style
/// each time it is drawn ([`Lazy`]): kept in the render trees, one for every
/// run would outweigh the messages themselves.
fn member_avatar(style: &AvatarStyle) -> impl View<Rgb565, ()> + use<> {
    super::avatar::view(style, MEMBER_AVATAR)
}
