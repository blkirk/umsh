//! Colors and type sizes shared by every screen.
//!
//! The palette is the T-LoRa Pager mockup's: a near-black background, so
//! the emissive panel spends little on what is not content.

use embedded_graphics::pixelcolor::Rgb565;

/// A 24-bit `0xRRGGBB` color, truncated to the panel's format.
const fn rgb(hex: u32) -> Rgb565 {
    Rgb565::new(
        (hex >> 19) as u8 & 0x1f,
        (hex >> 10) as u8 & 0x3f,
        (hex >> 3) as u8 & 0x1f,
    )
}

pub const BACKGROUND: Rgb565 = rgb(0x0B0D10);
pub const CHROME: Rgb565 = rgb(0x0E1115);
pub const LINE: Rgb565 = rgb(0x20252C);
pub const TEXT: Rgb565 = rgb(0xEEF1F5);
pub const TEXT_SECONDARY: Rgb565 = rgb(0x9AA3AE);
pub const TEXT_TERTIARY: Rgb565 = rgb(0x7F8995);
pub const ACCENT: Rgb565 = rgb(0x4DA3FF);
pub const FOCUS: Rgb565 = rgb(0x18304D);
pub const OUTBOUND: Rgb565 = rgb(0x2A62C9);
pub const INBOUND: Rgb565 = rgb(0x23272E);
pub const GREEN: Rgb565 = rgb(0x3DD68C);
pub const AMBER: Rgb565 = rgb(0xF5B544);
pub const RED: Rgb565 = rgb(0xFF6B5E);
pub const FIELD: Rgb565 = rgb(0x161A20);
pub const MENU: Rgb565 = rgb(0x1B2027);
pub const MENU_LINE: Rgb565 = rgb(0x2E3540);

/// Body text: about 2.3 mm on the Pager's panel.
pub const BODY: u32 = 20;
/// Secondary lines such as chat previews.
pub const SECONDARY: u32 = 17;
/// The smallest type anywhere: timestamps, labels, the status strip.
pub const SMALL: u32 = 16;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 222;
