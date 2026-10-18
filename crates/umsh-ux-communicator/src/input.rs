//! Input as the interface understands it, independent of how a board
//! produces it.
//!
//! What an input means depends on where focus is: Backspace erases while a
//! text field is active and is Back everywhere else, and turning the wheel
//! moves the cursor in an active field and focus elsewhere. The screens
//! decide that through focus; this only delivers inputs, each one to an
//! interface whose focus has settled from the last.

use buoyant::app::{App, Harness as _};
use buoyant::event::Key;
use buoyant::focus::DefaultFocus;
use buoyant::render::AnimatedJoin;
use buoyant::view::ViewLayout;

use crate::model::State;

/// One user input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// The wheel turned this many detents: positive is down or forward.
    Turn(i8),
    /// The wheel was pressed.
    Press,
    Enter,
    Backspace,
    /// A key that types text.
    Character(char),
}

/// Applies `input` to the running interface.
pub fn apply<V, F>(app: &mut App<V, State, F>, input: Input)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    // The board is turning off.
    if app.state().ending.is_some() {
        return;
    }
    match input {
        Input::Turn(detents) => {
            for _ in 0..detents.unsigned_abs() {
                if detents > 0 {
                    app.next();
                } else {
                    app.previous();
                }
                app.finalize_view();
            }
        }
        Input::Press => {
            app.select();
        }
        Input::Enter => press(app, Key::Enter),
        Input::Backspace => press(app, Key::Backspace),
        Input::Character(c) => press(app, Key::Character(c)),
    }
    app.finalize_view();
}

/// A key pressed and released.
fn press<V, F>(app: &mut App<V, State, F>, key: Key)
where
    V: ViewLayout<State>,
    V::FocusTree: DefaultFocus,
    V::Renderables: AnimatedJoin,
    F: Fn(&State) -> V,
{
    app.key_down(key);
    app.key_up(key);
}
