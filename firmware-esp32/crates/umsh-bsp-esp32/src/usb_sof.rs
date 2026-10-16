//! USB host presence from the USB-Serial-JTAG controller's start-of-frame
//! flag, for boards with no VBUS sense.
//!
//! A full-speed host sends a start-of-frame packet down every enabled
//! port once a millisecond, transfers pending or not, and stops when the
//! cable comes out or the host suspends the bus. The controller latches
//! each one in `INT_RAW.SOF` whether or not a driver exists, so clearing
//! the flag and reading it back a few frames later says whether a host is
//! driving the bus right now. This is the flag `esp-println` reads to pick
//! its output port.
//!
//! The controller is clocked only while the chip is awake: a light-sleeping
//! chip sees no frames. [`sample`] holds the chip awake across its window.

use embassy_time::{Duration, Timer};
use esp_hal::{peripherals::USB_DEVICE, rtc_cntl::WakeLock};

/// Forgets every start-of-frame seen so far.
pub fn clear() {
    USB_DEVICE::regs()
        .int_clr()
        .write(|w| w.sof().clear_bit_by_one());
}

/// Whether a start-of-frame has arrived since the last [`clear`]. Only
/// meaningful if the chip stayed awake in between.
pub fn seen() -> bool {
    USB_DEVICE::regs().int_raw().read().sof().bit_is_set()
}

/// Watches the bus for `window` with the chip held awake, and reports
/// whether a host sent a start-of-frame.
pub async fn sample(window: Duration) -> bool {
    let _awake = WakeLock::new();
    clear();
    Timer::after(window).await;
    seen()
}
