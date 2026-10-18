//! The regions the radio can be set for, one to a row. Pressing one sets
//! it; Backspace goes back. First run and Device › Radio › Region both
//! show this list.

use buoyant::event::{Key, KeyResponse};
use buoyant::focus::BoundaryBehavior;
use buoyant::view::prelude::*;
use embedded_graphics::pixelcolor::Rgb565;
use heapless::Vec;

use super::conversation::header;
use super::setting::{End, row};
use crate::model::State;
use crate::nav::keys;

const ROW: u32 = 36;
/// The most regions the list shows.
const MAX_REGIONS: usize = 12;

/// Indices `ForEach` can hold without borrowing the state.
static INDICES: [usize; MAX_REGIONS] = {
    let mut a = [0; MAX_REGIONS];
    let mut i = 0;
    while i < MAX_REGIONS {
        a[i] = i;
        i += 1;
    }
    a
};

/// The list under `title`.
pub fn view(state: &State, title: &'static str) -> impl View<Rgb565, State> + use<> {
    let names: Vec<&'static str, MAX_REGIONS> = state
        .node
        .regions()
        .iter()
        .take(MAX_REGIONS)
        .copied()
        .collect();
    let count = names.len();
    VStack::new((
        header(title, None, None),
        super::list(
            ForEach::<MAX_REGIONS>::new_vertical(&INDICES[..count], move |&i| {
                row(
                    names[i],
                    keys::region(i),
                    move |s: &mut State| s.choose_region(i),
                    End::Nothing,
                    false,
                    ROW,
                )
            })
            .keyed(|&i| keys::region(i)),
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
