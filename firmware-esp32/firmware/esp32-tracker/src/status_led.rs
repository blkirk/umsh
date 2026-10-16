//! The status LED: one firmware-owned light carrying the heartbeat, the
//! pairing blink, one-shot confirmations, and the locate alert.
//!
//! The deep-sleep heartbeat task drives it (`heartbeat_task` in
//! `main.rs`), which is what ties the RWDT feed to something visible: if
//! the light stops, the reset follows. Everything else reaches it through
//! the functions here.
//!
//! When two uses want the LED at once, the locate alert wins—someone is
//! looking for this board right now, and on a board with no sounder the
//! blink is the whole alert—then the pairing blink, then one-shots, then
//! the heartbeat.

use super::*;
use umsh_ux_tracker::led::{LedEngine, LedSequence, LedTimings};

/// Board-side mirror of `PROP_ALERT`. The session is authoritative; the
/// LED reads this to blink and the button task to swallow the press that
/// cancels the alert.
static ALERT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Wakes the LED on a locate-alert edge, rather than at the end of a
/// multi-second heartbeat wait.
static ALERT_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// A one-shot sequence another task wants played.
static REQUEST: Signal<CriticalSectionRawMutex, LedSequence> = Signal::new();

/// Apply a `PROP_ALERT` transition. Idempotent: the session reports every
/// transition, including ones that change nothing.
pub(crate) fn set_alert(active: bool) {
    if ALERT_ACTIVE.swap(active, Ordering::AcqRel) != active {
        ALERT_CHANGED.signal(());
    }
}

/// Whether a locate alert is running.
pub(crate) fn alert_active() -> bool {
    ALERT_ACTIVE.load(Ordering::Acquire)
}

/// One flash for an accepted local action. A plain `fn()`, because that
/// is the shape `NodeHooks::beacon_confirm` takes.
pub(crate) fn confirm_action() {
    play(LedSequence::ActionConfirm);
}

/// Play a one-shot sequence, replacing any that is still running.
pub(crate) fn play(sequence: LedSequence) {
    REQUEST.signal(sequence);
}

/// The LED pin and the engine that decides what it shows.
pub(crate) struct StatusLed {
    pin: Output<'static>,
    engine: LedEngine,
}

impl StatusLed {
    /// Take the LED, active high and starting dark.
    pub(crate) fn new(pin: Output<'static>) -> Self {
        Self {
            pin,
            engine: LedEngine::new(
                LedTimings {
                    // Four seconds apart matches every other UMSH board.
                    // The pulse is the board's: a bright LED needs only a
                    // flick to read as alive.
                    heartbeat_interval: core::time::Duration::from_secs(4),
                    heartbeat_pulse: core::time::Duration::from_millis(board::STATUS_LED_PULSE_MS),
                },
                Instant::now().as_millis(),
            ),
        }
    }

    /// Show what the LED should show now, then wait until that changes, an
    /// alert edge or a one-shot request arrives, or `deadline` passes.
    ///
    /// Cancellation-safe: dropped mid-wait, it loses nothing that the next
    /// call does not pick up.
    pub(crate) async fn show_until(&mut self, deadline: Instant) {
        let next = Instant::from_millis(self.show(Instant::now().as_millis()));
        if let Either3::Third(sequence) = select3(
            Timer::at(next.min(deadline)),
            ALERT_CHANGED.wait(),
            REQUEST.wait(),
        )
        .await
        {
            self.engine.play(sequence, Instant::now().as_millis());
        }
    }

    /// Flash three times for power-off, played to the end whatever else
    /// the LED was doing, and leave it dark.
    pub(crate) async fn power_off(&mut self) {
        self.engine.stop_alert();
        self.engine
            .play(LedSequence::PowerOff, Instant::now().as_millis());
        loop {
            let now = Instant::now().as_millis();
            if !self.engine.sequence_running(now) {
                break;
            }
            let decision = self.engine.tick(now);
            self.drive(decision.on);
            Timer::at(Instant::from_millis(decision.next_deadline_ms)).await;
        }
        self.drive(false);
    }

    /// Drive the LED for `now` and return when it next has to change.
    fn show(&mut self, now: u64) -> u64 {
        if alert_active() {
            self.engine.start_alert(now);
        } else {
            self.engine.stop_alert();
        }
        // The pairing blink yields to the alert: on one LED the two would
        // only fight.
        if !self.engine.alert_active() && ble::pairing_indicated() {
            let (on, next) = umsh_ux_tracker::led::pairing_blink(now);
            self.drive(on);
            return next;
        }
        let decision = self.engine.tick(now);
        self.drive(decision.on);
        decision.next_deadline_ms
    }

    fn drive(&mut self, on: bool) {
        self.pin
            .set_level(if on { Level::High } else { Level::Low });
    }
}
