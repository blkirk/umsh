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
use super::setting::{End, card, choice, row};
use crate::model::{PeerDetail, PeerSummary, State};
use crate::nav::keys;
use crate::{font, theme};

const ROW: u32 = 34;
const ADDRESS: u32 = 22;
/// The confirmation's width.
const CONFIRM_WIDTH: u32 = 300;

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
                ROW,
            ),
            row(
                "Route",
                keys::PEER_ROUTE,
                |_| {},
                detail.route.as_deref().map_or(End::Nothing, End::value),
                false,
                ROW,
            ),
            row(
                "Last heard",
                keys::PEER_HEARD,
                |_| {},
                End::value(&detail.heard),
                false,
                ROW,
            ),
            row(
                "Forward secrecy",
                keys::PEER_SECRECY,
                State::toggle_forward_secrecy,
                End::Switch(detail.forward_secrecy),
                false,
                ROW,
            ),
            row(
                "Ping",
                keys::PEER_PING,
                State::ping_peer,
                detail.ping.as_deref().map_or(End::Chevron, End::value),
                false,
                ROW,
            ),
            row(
                "Forget",
                keys::PEER_FORGET,
                State::ask_forget,
                End::Nothing,
                true,
                ROW,
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

/// Forget, confirmed, or Cancel, which is focused.
// Kept out of line, as `setting::row` is and for the same back-end failure,
// here on `State::forget_peer`'s address once `root_view` took the setup
// screens.
#[inline(never)]
fn confirmation(name: &String<32>) -> impl View<Rgb565, State> + use<> {
    let mut forget: String<48> = String::new();
    _ = core::fmt::write(&mut forget, format_args!("Forget {name}"));
    card(
        VStack::new((
            choice(
                forget,
                keys::FORGET_CONFIRM,
                State::forget_peer,
                theme::RED,
                None,
            ),
            choice(
                String::try_from("Cancel").unwrap_or_default(),
                keys::FORGET_CANCEL,
                State::cancel_forget,
                theme::TEXT,
                None,
            ),
        ))
        .with_spacing(0),
        CONFIRM_WIDTH,
        HorizontalAlignment::Center,
        State::cancel_forget,
    )
}
