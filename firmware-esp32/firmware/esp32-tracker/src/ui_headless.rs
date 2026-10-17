//! The panel's calls on a board with no panel, where the button is the
//! whole interface and the status LED the whole display.
//!
//! The same three calls `ui.rs` makes for a board with a menu; see there.

use super::*;

/// How long the button must stay down into boot to open the pairing
/// window.
const PAIRING_HOLD: Duration = Duration::from_secs(1);

/// A single press sends a check-in. The LED confirms it only once the MAC
/// accepts the send (`device_node`'s confirmation hook), so a refused one
/// stays dark. Nothing else is bound.
pub(crate) async fn gesture(event: ButtonEvent) {
    if matches!(event, ButtonEvent::Single) {
        device_node::request_beacon(device_node::BeaconTrigger::Button);
    }
}

/// Nothing to say goodbye on.
pub(crate) async fn farewell(_cause: ShutdownCause) {}

/// With no menu, holding the button through boot is how the pairing
/// window is opened over existing bonds. Two flashes acknowledge it.
///
/// A tap that wakes the board from deep sleep ends well inside the hold,
/// so only a deliberate hold counts.
pub(crate) async fn boot_gesture(button: &mut Input<'static>) -> bool {
    if button.is_high()
        || with_timeout(PAIRING_HOLD, button.wait_for_high())
            .await
            .is_ok()
    {
        return false;
    }
    #[cfg(feature = "status-led")]
    status_led::play(umsh_ux_tracker::led::LedSequence::PairingHold);
    true
}
