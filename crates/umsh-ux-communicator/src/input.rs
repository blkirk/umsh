//! Input as the interface understands it, independent of how a board
//! produces it.
//!
//! What an input means depends on where focus is: Backspace erases while a
//! text field is active and is Back everywhere else, and turning the wheel
//! moves the cursor in an active field and focus elsewhere. That decision
//! lives here so every communicator makes it the same way.

use buoyant::app::Harness;

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
pub fn apply(app: &mut impl Harness, input: Input) {
    match input {
        Input::Turn(detents) => {
            for _ in 0..detents.unsigned_abs() {
                if detents > 0 {
                    app.next();
                } else {
                    app.previous();
                }
            }
        }
        Input::Press | Input::Enter => {
            app.select();
        }
        Input::Backspace => {
            app.blur();
        }
        Input::Character(_) => {}
    }
}
