//! Device: the settings' categories, each with what it is set to, and the
//! Radio screen one of them opens. Changes apply at once; Backspace goes
//! back.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::match_view;
use buoyant::view::prelude::*;
use buoyant::view::scroll_view::ScrollBarVisibility;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::String;

use super::conversation::header;
use super::setting::{End, card, choice, row};
use crate::model::{ChatKind, Preset, RadioSettings, State};
use crate::nav::{DevicePage, keys};
use crate::theme;

const ROW: u32 = 36;
/// Preset's choices.
const PICKER_WIDTH: u32 = 260;

/// The categories, beside the rail.
pub fn view(state: &State) -> impl View<Rgb565, State> + use<> {
    let channels = state
        .chats
        .iter()
        .filter(|c| matches!(c.kind, ChatKind::Channel { .. }))
        .count();
    let mut joined: String<24> = String::new();
    _ = core::fmt::write(&mut joined, format_args!("{channels} joined"));
    let category = |name, key, act, value: &str| {
        let end = if value.is_empty() {
            End::Chevron
        } else {
            End::value_chevron(value)
        };
        row(name, key, act, end, false, ROW)
    };
    ScrollView::new(
        VStack::new((
            category(
                "Identity",
                keys::IDENTITY,
                State::open_share,
                &state.status.device_name,
            ),
            category("Channels", keys::CHANNELS, State::open_channels, &joined),
            category(
                "Radio",
                keys::RADIO,
                State::open_radio,
                state.node.radio().preset.name(),
            ),
            category(
                "Alerts",
                keys::ALERTS,
                |_| {},
                if state.status.silent { "Silent" } else { "" },
            ),
            category("Display", keys::DISPLAY, |_| {}, ""),
            category(
                "Phone link",
                keys::PHONE_LINK,
                |_| {},
                if state.status.phone_linked {
                    "Paired"
                } else {
                    ""
                },
            ),
            category("Maintenance", keys::MAINTENANCE, |_| {}, ""),
        ))
        .with_spacing(0),
    )
    .with_bar_visibility(ScrollBarVisibility::Never)
    .with_scroll_step(ROW)
    .bound_focus(BoundaryBehavior::Stop)
    .on_key(|key, s: &mut State| match key {
        Key::Backspace => {
            s.focus_rail();
            KeyResponse::Handled
        }
        _ => KeyResponse::Ignored,
    })
}

/// A screen under Device.
pub fn page(state: &State, page: &DevicePage) -> impl View<Rgb565, State> + use<> {
    match_view!(page, {
        DevicePage::Radio { settings, picker } => radio(settings, *picker),
        DevicePage::Share => super::share::view(state),
        DevicePage::Channels => super::channels::view(state),
        DevicePage::AddChannel { text } => super::add_channel::view(state, text),
    })
}

/// The radio's settings, and Preset's choices while they are open.
fn radio(settings: &RadioSettings, picker: bool) -> impl View<Rgb565, State> + use<> {
    let mut airtime: String<24> = String::new();
    _ = core::fmt::write(
        &mut airtime,
        format_args!("{}% this hour", settings.airtime),
    );
    let preset = settings.preset;
    VStack::new((
        header("Radio", None, None),
        ScrollView::new(
            VStack::new((
                row(
                    "Region",
                    keys::REGION,
                    |_| {},
                    End::value_chevron(&settings.region),
                    false,
                    ROW,
                ),
                row(
                    "Preset",
                    keys::PRESET_ROW,
                    State::open_presets,
                    End::value_chevron(preset.name()),
                    false,
                    ROW,
                ),
                row(
                    "Forwarding",
                    keys::FORWARDING,
                    State::toggle_forwarding,
                    End::Switch(settings.forwarding),
                    false,
                    ROW,
                ),
                row(
                    "Share location",
                    keys::SHARE_LOCATION,
                    State::toggle_share_location,
                    End::Switch(settings.share_location),
                    false,
                    ROW,
                ),
                row(
                    "Airtime",
                    keys::AIRTIME,
                    |_| {},
                    End::value(&airtime),
                    false,
                    ROW,
                ),
            ))
            .with_spacing(0),
        )
        .with_bar_visibility(ScrollBarVisibility::Never)
        .with_scroll_step(ROW)
        .popover(picker.then_some(preset).as_ref(), presets),
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

/// Preset's choices, the current one checked.
fn presets(current: &Preset) -> impl View<Rgb565, State> + use<> {
    let option = |preset: Preset, act: fn(&mut State)| {
        choice(
            String::try_from(preset.name()).unwrap_or_default(),
            keys::preset(preset),
            act,
            theme::TEXT,
            Some(preset == *current),
        )
    };
    card(
        VStack::new((
            option(Preset::LongRange, |s| s.choose_preset(Preset::LongRange)),
            option(Preset::MediumRange, |s| {
                s.choose_preset(Preset::MediumRange)
            }),
            option(Preset::ShortRange, |s| s.choose_preset(Preset::ShortRange)),
        ))
        .with_spacing(0),
        PICKER_WIDTH,
        HorizontalAlignment::Trailing,
        State::close_presets,
    )
}
