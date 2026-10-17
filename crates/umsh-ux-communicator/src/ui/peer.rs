//! A saved peer: who they are, their whole address, then one focusable list
//! of actions and readouts that the wheel scrolls through. Forget is last
//! and asks to confirm. Backspace goes back.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use buoyant::view::scroll_view::ScrollBarVisibility;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::String;

use super::label;
use super::member::detail_head;
use crate::model::{PeerDetail, PeerSummary, State};
use crate::nav::keys;
use crate::{font, icon, theme};

const ROW: u32 = 34;
const INSET: u32 = 12;
const RADIUS: u16 = 8;
const ADDRESS: u32 = 22;
/// The switch, and its knob's inset.
const SWITCH_WIDTH: u32 = 40;
const SWITCH_HEIGHT: u32 = 22;
const KNOB_INSET: u32 = 3;
/// The confirmation's width.
const CONFIRM_WIDTH: u32 = 300;

/// What a row shows at its end.
enum End {
    Nothing,
    /// It goes to another screen.
    Chevron,
    Value(String<24>),
    Switch(bool),
}

pub fn view(
    summary: &PeerSummary,
    detail: &PeerDetail,
    confirm: bool,
) -> impl View<Rgb565, State> + use<> {
    let mut about: String<48> = String::new();
    _ = about.push_str("Saved peer");
    match summary.hops {
        Some(0) => _ = about.push_str(" \u{B7} Direct"),
        Some(1) => _ = about.push_str(" \u{B7} 1 hop"),
        Some(n) => _ = core::fmt::write(&mut about, format_args!(" \u{B7} {n} hops")),
        None => {}
    }
    let value = |text: &str| End::Value(String::try_from(text).unwrap_or_default());
    let name = summary.name.clone();
    ScrollView::new(
        VStack::new((
            detail_head(&summary.avatar, &summary.name, &about),
            label::<44>(
                &detail.address,
                &font::REGULAR,
                theme::SMALL,
                theme::TEXT_SECONDARY,
            )
            .padding(Edges::Leading, 10)
            .flex_infinite_width(HorizontalAlignment::Leading)
            .frame()
            .with_height(ADDRESS),
            row(
                "Message",
                keys::PEER_MESSAGE,
                State::message_peer,
                End::Chevron,
                false,
            ),
            row(
                "Route",
                keys::PEER_ROUTE,
                |_| {},
                detail.route.as_deref().map_or(End::Nothing, value),
                false,
            ),
            row(
                "Last heard",
                keys::PEER_HEARD,
                |_| {},
                value(&detail.heard),
                false,
            ),
            row(
                "Forward secrecy",
                keys::PEER_SECRECY,
                State::toggle_forward_secrecy,
                End::Switch(detail.forward_secrecy),
                false,
            ),
            row(
                "Ping",
                keys::PEER_PING,
                State::ping_peer,
                detail.ping.as_deref().map_or(End::Chevron, value),
                false,
            ),
            row(
                "Forget",
                keys::PEER_FORGET,
                State::ask_forget,
                End::Nothing,
                true,
            ),
        ))
        .with_spacing(0),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_scroll_step(ROW)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.back();
            KeyResponse::Handled
        }
        _ => KeyResponse::Ignored,
    })
    .popover(confirm.then_some(name).as_ref(), confirmation)
}

/// One of the list's rows: an action, or a readout that takes focus so the
/// wheel can pass through it.
// Kept out of line: inlined into the screen, its six calls make the esp
// toolchain's Xtensa back end fail with `Cannot select:
// XtensaISD::PCREL_WRAPPER TargetConstantPool` (rustc 1.95.0-nightly esp).
#[inline(never)]
fn row(
    name: &'static str,
    key: buoyant::focus::FocusKey,
    act: fn(&mut State),
    end: End,
    danger: bool,
) -> impl View<Rgb565, State> + use<> {
    let (value, chevron, switch) = match end {
        End::Nothing => (None, false, None),
        End::Chevron => (None, true, None),
        End::Value(value) => (Some(value), false, None),
        End::Switch(on) => (None, false, Some(on)),
    };
    Button::new(act, move |button: ButtonState| {
        let focused = button.is_focused();
        let face = if focused { &font::BOLD } else { &font::REGULAR };
        let color = if danger { theme::RED } else { theme::TEXT };
        HStack::new((
            // Static, so drawn as it is rather than copied.
            super::line(name, face, theme::BODY, color),
            Spacer::default(),
            value.as_ref().map(|value| {
                label::<24>(
                    value,
                    &font::REGULAR,
                    theme::SECONDARY,
                    theme::TEXT_SECONDARY,
                )
            }),
            chevron.then(|| {
                Image::new(&icon::CHEVRON)
                    .as_template()
                    .foreground_color(theme::TEXT_TERTIARY)
            }),
            switch.map(switch_look),
        ))
        .with_spacing(8)
        .hint_background_color(if focused {
            theme::FOCUS
        } else {
            theme::BACKGROUND
        })
        .padding(Edges::Leading, INSET - 4)
        .padding(Edges::Trailing, 10)
        .frame()
        .with_height(ROW - 2)
        .background(
            Alignment::Center,
            focused.then(|| {
                ZStack::new((
                    RoundedRectangle::new(RADIUS).foreground_color(theme::FOCUS),
                    RoundedRectangle::new(RADIUS)
                        .stroked(2)
                        .foreground_color(theme::ACCENT),
                ))
            }),
        )
        .padding(Edges::Horizontal, 4)
        .padding(Edges::Vertical, 1)
        .overlay(
            Alignment::Bottom,
            (!focused).then(|| {
                Rectangle
                    .foreground_color(theme::LINE)
                    .frame()
                    .with_height(1)
                    .padding(Edges::Leading, INSET)
            }),
        )
    })
    .focus_key(key)
}

/// An on/off setting: where the knob sits and the fill say which.
fn switch_look(on: bool) -> impl View<Rgb565, State> + use<> {
    let (track, knob, alignment) = if on {
        (
            theme::ACCENT,
            Rgb565::new(31, 63, 31),
            HorizontalAlignment::Trailing,
        )
    } else {
        (
            theme::SWITCH_OFF,
            theme::KNOB_OFF,
            HorizontalAlignment::Leading,
        )
    };
    let diameter = SWITCH_HEIGHT - 2 * KNOB_INSET;
    ZStack::new((
        RoundedRectangle::new((SWITCH_HEIGHT / 2) as u16).foreground_color(track),
        Circle
            .foreground_color(knob)
            .frame_sized(diameter, diameter)
            .padding(Edges::Horizontal, KNOB_INSET)
            .flex_infinite_width(alignment),
    ))
    .frame_sized(SWITCH_WIDTH, SWITCH_HEIGHT)
}

/// Forget, confirmed, or Cancel, which is focused.
fn confirmation(name: &String<32>) -> impl View<Rgb565, State> + use<> {
    let mut forget: String<48> = String::new();
    _ = core::fmt::write(&mut forget, format_args!("Forget {name}"));
    VStack::new((
        choice(forget, keys::FORGET_CONFIRM, State::forget_peer, true),
        choice(
            String::try_from("Cancel").unwrap_or_default(),
            keys::FORGET_CANCEL,
            State::cancel_forget,
            false,
        ),
    ))
    .with_spacing(0)
    .padding(Edges::Vertical, 6)
    .frame()
    .with_width(CONFIRM_WIDTH)
    .background(
        Alignment::Center,
        ZStack::new((
            RoundedRectangle::new(12)
                .foreground_color(Rgb565::new(0, 0, 0))
                .offset(1, 3),
            RoundedRectangle::new(12).foreground_color(theme::MENU),
            RoundedRectangle::new(12)
                .stroked(1)
                .foreground_color(theme::MENU_LINE),
        )),
    )
    .flex_infinite_width(HorizontalAlignment::Center)
    .flex_infinite_height(VerticalAlignment::Center)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.cancel_forget();
            KeyResponse::Handled
        }
        _ => KeyResponse::Ignored,
    })
}

fn choice(
    text: String<48>,
    key: buoyant::focus::FocusKey,
    act: fn(&mut State),
    danger: bool,
) -> impl View<Rgb565, State> + use<> {
    Button::new(act, move |button: ButtonState| {
        let focused = button.is_focused();
        let face = if focused { &font::BOLD } else { &font::REGULAR };
        let color = if danger { theme::RED } else { theme::TEXT };
        label::<48>(&text, face, theme::BODY, color)
            .hint_background_color(if focused { theme::FOCUS } else { theme::MENU })
            .padding(Edges::Leading, 11)
            .flex_infinite_width(HorizontalAlignment::Leading)
            .frame()
            .with_height(ROW - 2)
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
            .padding(Edges::Horizontal, 5)
            .padding(Edges::Bottom, 2)
    })
    .focus_key(key)
}
