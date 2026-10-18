//! How a message reached this device: the rows of the iOS details sheet
//! that a reader on a communicator needs. The wheel scrolls through them;
//! Backspace goes back.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::String;

use super::conversation::header;
use super::label;
use crate::model::{Details, State};
use crate::nav::keys;
use crate::{font, theme};

const ROW: u32 = 36;
const INSET: u32 = 12;

pub fn view(details: &Details, inbound: bool) -> impl View<Rgb565, State> + use<> {
    let details = details.clone();
    VStack::new((
        header("Message details", None, None),
        super::list(
            // One focusable block, which the wheel scrolls a row at a time.
            Button::new(|_: &mut State| {}, move |_| rows(&details, inbound))
                .focus_key(keys::DETAIL_LIST),
        )
        .with_scroll_step(ROW),
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

/// The rows for what is known.
fn rows(details: &Details, inbound: bool) -> impl View<Rgb565, State> + use<> {
    let text = |value: core::fmt::Arguments| {
        let mut s: String<24> = String::new();
        _ = core::fmt::write(&mut s, value);
        s
    };
    // A true minus, as the mockup sets it.
    let minus = |negative: bool| if negative { "\u{2212}" } else { "" };
    VStack::new((
        (!details.time.is_empty()).then(|| {
            row(
                if inbound { "Received" } else { "Sent" },
                &text(format_args!("{}", details.time)),
                false,
            )
        }),
        details
            .hops
            .map(|hops| row("Hops", &text(format_args!("{hops}")), false)),
        (!details.route.is_empty())
            .then(|| row("Route", &text(format_args!("{}", details.route)), false)),
        details.rssi.map(|rssi| {
            let value = text(format_args!(
                "{}{} dBm",
                minus(rssi < 0),
                rssi.unsigned_abs()
            ));
            row("Signal", &value, false)
        }),
        details.snr.map(|tenths| {
            let (sign, tenths) = (minus(tenths < 0), tenths.unsigned_abs());
            let value = text(format_args!("{sign}{}.{} dB", tenths / 10, tenths % 10));
            row("Signal-to-noise", &value, false)
        }),
        inbound.then(|| {
            let source = if details.authenticated {
                "Authenticated"
            } else {
                "Not authenticated"
            };
            row("Source", source, false)
        }),
    ))
    .with_spacing(0)
    // The whole block, so it scrolls from its first row's top.
    .content_shape(Rectangle)
}

/// A labeled value, with a rule under it. A `muted` label is for a value
/// not known yet.
pub(super) fn row(
    name: &'static str,
    value: &str,
    muted: bool,
) -> impl View<Rgb565, State> + use<> {
    let color = if muted {
        theme::TEXT_TERTIARY
    } else {
        theme::TEXT
    };
    HStack::new((
        label::<16>(name, &font::REGULAR, theme::BODY, color),
        Spacer::default(),
        label::<48>(
            value,
            &font::REGULAR,
            theme::SECONDARY,
            theme::TEXT_SECONDARY,
        ),
    ))
    .with_spacing(8)
    .padding(Edges::Leading, INSET)
    .padding(Edges::Trailing, 14)
    .frame()
    .with_height(ROW)
    .overlay(
        Alignment::Bottom,
        Rectangle
            .foreground_color(theme::LINE)
            .frame()
            .with_height(1)
            .padding(Edges::Leading, INSET),
    )
}
