//! ST7796 and AW9364 backlight. The SPI handle releases the bus per stripe.
use crate::SpiHandle;
use embassy_time::{Delay, Timer};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
use esp_hal::gpio::Output;
use esp_hal::gpio::{Level, OutputConfig};
use esp_hal::peripherals::{GPIO42, GPIO46};
use umsh_pager_peripherals::display::{
    FRAME_BYTES, Framebuffer, HEIGHT, STRIPE_BYTES, STRIPE_ROWS, SentStripes, St7796,
};
pub type Error = umsh_pager_peripherals::display::Error<
    embassy_embedded_hal::shared_bus::SpiDeviceError<
        esp_hal::spi::Error,
        core::convert::Infallible,
    >,
    core::convert::Infallible,
>;
#[derive(Clone, Copy)]
pub struct Brightness(u8);
impl Brightness {
    pub const NORMAL: Self = Self(8);
    pub const ALERT: Self = Self(16);
    /// AW9364 steps for levels 1 through 10; level 5 is [`Self::NORMAL`].
    const LEVELS: [u8; 10] = [1, 2, 4, 6, 8, 10, 12, 13, 14, 16];
    /// The brightness at `level`, from 1 to 10.
    pub const fn level(level: u8) -> Self {
        let index = match level {
            0 => 0,
            1..=10 => level - 1,
            _ => 9,
        };
        Self(Self::LEVELS[index as usize])
    }
    /// This brightness dimmed to `p` permille of itself, never dark.
    pub const fn dimmed(self, p: u16) -> Self {
        let p = if p > 1000 { 1000 } else { p };
        Self(1 + ((self.0 - 1) as u16 * p / 1000) as u8)
    }
}
pub const fn brightness_from_permille(p: u16) -> Brightness {
    Brightness(1 + ((if p > 1000 { 1000 } else { p }) * 7 / 1000) as u8)
}
/// Light the board before panel, heap, or async timer initialization.
/// Ownership passes to the display without toggling either output.
pub struct BootBacklights {
    display: Output<'static>,
    keyboard: Output<'static>,
}
impl BootBacklights {
    pub fn new(display: GPIO42<'static>, keyboard: GPIO46<'static>) -> Self {
        let keyboard = Output::new(keyboard, Level::High, OutputConfig::default());
        let mut display = Output::new(display, Level::Low, OutputConfig::default());
        // Establish AW9364 step 16 even after a warm reset. Async timers are
        // not running yet; this is the only early-boot settling delay.
        esp_hal::delay::Delay::new().delay_millis(3);
        display.set_high();
        Self { display, keyboard }
    }
}
pub struct Display {
    panel: St7796<SpiHandle, Output<'static>>,
    fb: Framebuffer,
    sent: SentStripes,
    stripe: &'static mut [u8; STRIPE_BYTES],
    backlight: Output<'static>,
    keyboard_light: Output<'static>,
    /// Whether the keyboard lights with the panel.
    keyboard_lit: bool,
    brightness: u8,
    physical_brightness: u8,
    on: bool,
}
impl Display {
    pub fn invert(&mut self) {
        self.fb.invert();
    }
    /// The working frame in full color. Drawing on the display itself is
    /// black and white.
    pub fn color(&mut self) -> &mut Framebuffer {
        &mut self.fb
    }
    pub fn new(
        spi: SpiHandle,
        dc: Output<'static>,
        lights: BootBacklights,
        fb: &'static mut [u8; FRAME_BYTES],
        stripe: &'static mut [u8; STRIPE_BYTES],
    ) -> Self {
        Self {
            panel: St7796::new(spi, dc),
            fb: Framebuffer::new(fb),
            sent: SentStripes::new(),
            stripe,
            backlight: lights.display,
            keyboard_light: lights.keyboard,
            keyboard_lit: true,
            brightness: 8,
            physical_brightness: 16,
            on: false,
        }
    }
    pub async fn init(&mut self) -> Result<(), Error> {
        self.sent.forget();
        self.backlight(self.brightness).await;
        self.panel.init(&mut Delay).await
    }
    pub async fn flush(&mut self) -> Result<(), Error> {
        // Where the panel's write pointer is, after a stripe just sent: a
        // changed stripe there follows on without setting a window.
        let mut next_row = None;
        for (index, y) in (0..HEIGHT).step_by(STRIPE_ROWS).enumerate() {
            let len = self.fb.stripe(y, self.stripe);
            let Some(hash) = self.sent.changed(index, &self.stripe[..len]) else {
                continue;
            };
            let continues = next_row == Some(y);
            if let Err(error) = self.panel.stripe(y, &self.stripe[..len], continues).await {
                // A failed transfer may have written part of the stripe. Even
                // a later frame matching old history must repair those pixels.
                self.sent.forget();
                return Err(error);
            }
            self.sent.sent(index, hash);
            next_row = Some(y + STRIPE_ROWS);
            embassy_futures::yield_now().await;
        }
        Ok(())
    }
    async fn backlight(&mut self, value: u8) {
        if value == self.physical_brightness {
            return;
        }
        // AW9364 starts at step 16, subsequent falling/rising pulses decrement
        // modulo 16. Hold low to reset before enabling. No PWM on this pin.
        if value == 0 {
            self.backlight.set_low();
            Timer::after_millis(3).await;
        } else {
            if self.physical_brightness == 0 {
                self.backlight.set_high();
                Timer::after_micros(100).await;
                self.physical_brightness = 16;
            }
            let pulses = (16 + self.physical_brightness - value) % 16;
            let delay = esp_hal::delay::Delay::new();
            for _ in 0..pulses {
                // A preemption must not stretch the low pulse into a reset.
                // Mask interrupts for only this 1 us pulse, not the pulse train.
                critical_section::with(|_| {
                    self.backlight.set_low();
                    delay.delay_micros(1);
                    self.backlight.set_high();
                });
                delay.delay_micros(1);
            }
        }
        self.physical_brightness = value;
    }
    pub async fn set_brightness(&mut self, brightness: Brightness) -> Result<(), Error> {
        self.brightness = brightness.0;
        if self.on {
            self.backlight(self.brightness).await;
        }
        Ok(())
    }
    pub async fn set_display_on(&mut self, on: bool) -> Result<(), Error> {
        if !on {
            self.backlight(0).await;
            self.keyboard_light.set_low();
        }
        self.panel.on(on).await?;
        self.on = on;
        if on {
            self.backlight(self.brightness).await;
            if self.keyboard_lit {
                self.keyboard_light.set_high();
            }
        }
        Ok(())
    }
    /// Whether the keyboard lights while the panel is on.
    pub fn set_keyboard_light(&mut self, lit: bool) {
        self.keyboard_lit = lit;
        if self.on {
            self.keyboard_light.set_level(Level::from(lit));
        }
    }
}
impl OriginDimensions for Display {
    fn size(&self) -> Size {
        self.fb.size()
    }
}
impl DrawTarget for Display {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<BinaryColor>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        self.fb.binary().draw_iter(pixels)
    }
    fn clear(&mut self, color: BinaryColor) -> Result<(), Self::Error> {
        self.fb.binary().clear(color)
    }
}
