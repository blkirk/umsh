//! The member who sent a channel message. A channel message carries only a
//! hint of its sender, so until their identity is known the screen offers to
//! ask the channel for it. Backspace goes back.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::String;

use super::conversation::header;
use super::details::row;
use super::label;
use crate::model::{Member, State};
use crate::nav::keys;
use crate::{font, theme};

/// The head's height, and its avatar's size and place.
const HEAD: u32 = 62;
const AVATAR: u32 = 44;
const AVATAR_X: u32 = 10;
const ROW: u32 = 36;

/// `member`, heard in the channel named `channel`.
pub fn view(member: &Member, channel: &str) -> impl View<Rgb565, State> + use<> {
    let known = member.address.is_some();
    let shown = member.clone();
    VStack::new((
        header("Member", None, None),
        head(member, channel),
        (!known).then(|| {
            action(
                "Request identity",
                keys::REQUEST_IDENTITY,
                State::request_identity,
            )
        }),
        // With nothing to do, what is known takes focus, so Back has
        // somewhere to be pressed from.
        known.then(|| {
            Button::new(|_: &mut State| {}, move |_| info(&shown)).focus_key(keys::MEMBER_INFO)
        }),
        (!known).then(|| info(member)),
    ))
    .with_spacing(0)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.back();
            KeyResponse::Handled
        }
        _ => KeyResponse::Ignored,
    })
}

/// Their hint, and their address once it is known.
fn info(member: &Member) -> impl View<Rgb565, State> + use<> {
    VStack::new((
        row("Hint", &member.hint, false),
        row(
            "Address",
            member.address.as_deref().unwrap_or("Not yet known"),
            member.address.is_none(),
        ),
    ))
    .with_spacing(0)
    .content_shape(Rectangle)
}

/// Their avatar and name, and where and when they were last heard.
fn head(member: &Member, channel: &str) -> impl View<Rgb565, State> + use<> {
    let mut heard: String<48> = String::new();
    _ = core::fmt::write(
        &mut heard,
        format_args!("Heard in {channel} \u{B7} {}", member.heard),
    );
    HStack::new((
        super::avatar::view(&member.sender.avatar, AVATAR),
        VStack::new((
            label::<24>(&member.sender.name, &font::BOLD, theme::BODY, theme::TEXT),
            label::<48>(&heard, &font::MEDIUM, theme::SMALL, theme::TEXT_TERTIARY),
        ))
        .with_spacing(0)
        .with_alignment(HorizontalAlignment::Leading),
    ))
    .with_spacing(10)
    .padding(Edges::Leading, AVATAR_X)
    .flex_infinite_width(HorizontalAlignment::Leading)
    .frame()
    .with_height(HEAD)
}

/// A row that does something when pressed.
fn action(
    text: &'static str,
    key: buoyant::focus::FocusKey,
    act: fn(&mut State),
) -> impl View<Rgb565, State> + use<> {
    Button::new(act, move |button: ButtonState| {
        let focused = button.is_focused();
        let face = if focused { &font::BOLD } else { &font::REGULAR };
        label::<24>(text, face, theme::BODY, theme::TEXT)
            .hint_background_color(if focused {
                theme::FOCUS
            } else {
                theme::BACKGROUND
            })
            .padding(Edges::Leading, 8)
            .flex_infinite_width(HorizontalAlignment::Leading)
            .frame()
            .with_height(ROW - 2)
            .background(
                Alignment::Center,
                focused.then(|| {
                    ZStack::new((
                        RoundedRectangle::new(8).foreground_color(theme::FOCUS),
                        RoundedRectangle::new(8)
                            .stroked(2)
                            .foreground_color(theme::ACCENT),
                    ))
                }),
            )
            .padding(Edges::Horizontal, 4)
            .padding(Edges::Vertical, 1)
    })
    .focus_key(key)
}
