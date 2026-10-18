//! The pieces of a settings list: rows that act or read out, the switch an
//! on/off setting wears, and the card a row's choices open in.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::FocusKey;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::String;

use super::label;
use crate::model::State;
use crate::{font, icon, theme};

const INSET: u32 = 12;
const RADIUS: u16 = 8;
/// The switch, and its knob's inset.
const SWITCH_WIDTH: u32 = 40;
const SWITCH_HEIGHT: u32 = 22;
const KNOB_INSET: u32 = 3;
/// How a switch moves from one side to the other.
const SWITCH_MOTION: Animation = Animation::ease_in_out(core::time::Duration::from_millis(180));
/// A choice in a card, and the room its check mark takes.
const CHOICE: u32 = 34;
const CHECK: u32 = 26;

/// What a row shows at its end.
pub(super) enum End {
    Nothing,
    /// It goes to another screen, or opens choices.
    Chevron,
    Value(String<24>),
    /// A value, and that pressing changes it.
    ValueChevron(String<24>),
    Switch(bool),
}

impl End {
    pub(super) fn value(text: &str) -> Self {
        Self::Value(String::try_from(text).unwrap_or_default())
    }

    pub(super) fn value_chevron(text: &str) -> Self {
        Self::ValueChevron(String::try_from(text).unwrap_or_default())
    }
}

/// A row `height` tall: an action, or a readout that takes focus so the
/// wheel can pass through it. `danger` sets its name in red.
// Kept out of line: inlined into a screen, its calls make the esp
// toolchain's Xtensa back end fail with `Cannot select:
// XtensaISD::PCREL_WRAPPER TargetConstantPool` (rustc 1.95.0-nightly esp).
#[inline(never)]
pub(super) fn row<A: Fn(&mut State)>(
    name: &'static str,
    key: FocusKey,
    act: A,
    end: End,
    danger: bool,
    height: u32,
) -> impl View<Rgb565, State> + use<A> {
    let (value, chevron, switch) = match end {
        End::Nothing => (None, false, None),
        End::Chevron => (None, true, None),
        End::Value(value) => (Some(value), false, None),
        End::ValueChevron(value) => (Some(value), true, None),
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
        .with_height(height - 2)
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

/// An on/off setting: where the knob sits and the fill say which. Changed,
/// the knob slides across as the fill changes color.
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
    .animated(SWITCH_MOTION, on)
    // Moves within the switch, whatever the row does.
    .geometry_group()
    .frame_sized(SWITCH_WIDTH, SWITCH_HEIGHT)
}

/// Choices in a card `width` wide, over the middle of the screen at its
/// `alignment`. Backspace does `cancel`.
pub(super) fn card<V: View<Rgb565, State>>(
    choices: V,
    width: u32,
    alignment: HorizontalAlignment,
    cancel: fn(&mut State),
) -> impl View<Rgb565, State> + use<V> {
    choices
        .padding(Edges::Vertical, 5)
        .frame()
        .with_width(width)
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
        .padding(Edges::Horizontal, 10)
        .flex_infinite_width(alignment)
        .flex_infinite_height(VerticalAlignment::Center)
        .on_key(move |key, s: &mut State| match key {
            Key::Backspace => {
                cancel(s);
                KeyResponse::Handled
            }
            _ => KeyResponse::Ignored,
        })
}

/// One of a card's choices. `checked` marks the current one of choices that
/// show which is current; `None` leaves no room for the mark.
pub(super) fn choice(
    text: String<48>,
    key: FocusKey,
    act: fn(&mut State),
    color: Rgb565,
    checked: Option<bool>,
) -> impl View<Rgb565, State> + use<> {
    Button::new(act, move |button: ButtonState| {
        let focused = button.is_focused();
        let face = if focused { &font::BOLD } else { &font::REGULAR };
        HStack::new((
            checked.map(|checked| {
                checked
                    .then(|| {
                        Image::new(&icon::CHECK)
                            .as_template()
                            .foreground_color(theme::ACCENT)
                    })
                    .frame()
                    .with_width(CHECK)
            }),
            label::<48>(&text, face, theme::BODY, color),
        ))
        .with_spacing(0)
        .hint_background_color(if focused { theme::FOCUS } else { theme::MENU })
        // The check mark's column brings its own margin.
        .padding(Edges::Leading, if checked.is_some() { 6 } else { 11 })
        .flex_infinite_width(HorizontalAlignment::Leading)
        .frame()
        .with_height(CHOICE - 2)
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
