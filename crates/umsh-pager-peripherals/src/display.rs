//! ST7796 controller, 480×222 landscape window. Transport handles bus sharing.
use embedded_graphics::{
    pixelcolor::{BinaryColor, Rgb565, raw::RawU16},
    prelude::*,
};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 222;
/// RGB565, big-endian: the panel's own pixel format, so a stripe is sent
/// as it is stored.
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 2;
pub const STRIPE_ROWS: usize = 4;
pub const STRIPE_BYTES: usize = WIDTH * STRIPE_ROWS * 2;

pub struct Framebuffer {
    bytes: &'static mut [u8; FRAME_BYTES],
}
impl Framebuffer {
    pub fn invert(&mut self) {
        for byte in self.bytes.iter_mut() {
            *byte = !*byte;
        }
    }
    pub fn new(bytes: &'static mut [u8; FRAME_BYTES]) -> Self {
        bytes.fill(0);
        Self { bytes }
    }
    fn stripe_range(y: usize) -> core::ops::Range<usize> {
        assert!(y < HEIGHT);
        y * WIDTH * 2..(y + STRIPE_ROWS).min(HEIGHT) * WIDTH * 2
    }
    pub fn stripe(&self, y: usize, out: &mut [u8; STRIPE_BYTES]) -> usize {
        let range = Self::stripe_range(y);
        let len = range.len();
        out[..len].copy_from_slice(&self.bytes[range]);
        len
    }
    /// The same frame drawn in black and white, for screens that have
    /// only two colors: on is white.
    pub fn binary(&mut self) -> Binary<'_> {
        Binary(self)
    }
    fn put(&mut self, x: i32, y: i32, color: Rgb565) {
        if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
            return;
        }
        let i = (y as usize * WIDTH + x as usize) * 2;
        self.bytes[i..i + 2].copy_from_slice(&RawU16::from(color).into_inner().to_be_bytes());
    }
}
impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}
impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<T: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: T,
    ) -> Result<(), Self::Error> {
        for Pixel(Point { x, y }, c) in pixels {
            self.put(x, y, c);
        }
        Ok(())
    }
    fn fill_solid(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        color: Rgb565,
    ) -> Result<(), Self::Error> {
        let area = area.intersection(&self.bounding_box());
        let Some(bottom_right) = area.bottom_right() else {
            return Ok(());
        };
        let pixel = RawU16::from(color).into_inner().to_be_bytes();
        for y in area.top_left.y..=bottom_right.y {
            let row = y as usize * WIDTH * 2;
            let start = row + area.top_left.x as usize * 2;
            let end = row + (bottom_right.x as usize + 1) * 2;
            fill_pixels(&mut self.bytes[start..end], pixel);
        }
        Ok(())
    }
    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        fill_pixels(
            &mut self.bytes[..],
            RawU16::from(color).into_inner().to_be_bytes(),
        );
        Ok(())
    }
}

/// Fills `bytes`, which starts on a pixel, with `pixel`. Solid fills are
/// most of what a frame draws, and the frame is in external RAM: the bulk
/// goes as aligned 32-bit stores, two pixels each, rather than one byte or
/// one pixel at a time.
pub(crate) fn fill_pixels(bytes: &mut [u8], pixel: [u8; 2]) {
    // Safety: any bit pattern is a valid u32.
    let (head, words, tail) = unsafe { bytes.align_to_mut::<u32>() };
    for (i, byte) in head.iter_mut().enumerate() {
        *byte = pixel[i % 2];
    }
    // The words start partway into a pixel when the head is odd.
    let phase = head.len() % 2;
    let word = u32::from_ne_bytes([
        pixel[phase],
        pixel[1 - phase],
        pixel[phase],
        pixel[1 - phase],
    ]);
    words.fill(word);
    for (i, byte) in tail.iter_mut().enumerate() {
        *byte = pixel[(phase + i) % 2];
    }
}

/// Stripes in a frame; the last one is short.
pub const STRIPES: usize = HEIGHT.div_ceil(STRIPE_ROWS);

/// What the panel last showed, as one hash per stripe. Rendering clears
/// and redraws the whole working frame, so write-time dirty flags would
/// mark every stripe; comparing against what was sent finds the ones that
/// changed. A hash does that without a second frame to compare against.
pub struct SentStripes([Option<u64>; STRIPES]);

impl SentStripes {
    /// Nothing is known to be on the panel: every stripe goes out.
    pub const fn new() -> Self {
        Self([None; STRIPES])
    }
    /// After a failed transfer, which may have written part of a stripe.
    pub fn forget(&mut self) {
        self.0 = [None; STRIPES];
    }
    /// The hash of `pixels` for stripe `index`, or `None` when the panel
    /// already shows them.
    pub fn changed(&self, index: usize, pixels: &[u8]) -> Option<u64> {
        let hash = stripe_hash(pixels);
        (self.0[index] != Some(hash)).then_some(hash)
    }
    /// Stripe `index` reached the panel.
    pub fn sent(&mut self, index: usize, hash: u64) {
        self.0[index] = Some(hash);
    }
}

impl Default for SentStripes {
    fn default() -> Self {
        Self::new()
    }
}

/// Two independent 32-bit lanes over 32-bit words, together 64 bits wide
/// so that a changed stripe hashing the same is not a practical concern.
/// The pager's core has no 64-bit multiply; 32-bit lanes keep this to a
/// couple of milliseconds a frame.
fn stripe_hash(pixels: &[u8]) -> u64 {
    // FNV-1a, and a rotate-xor-multiply with the golden-ratio constant.
    let (mut a, mut b) = (0x811c_9dc5_u32, 0x9e37_79b9_u32);
    for word in pixels.chunks_exact(4) {
        let word = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        a = (a ^ word).wrapping_mul(0x0100_0193);
        b = (b.rotate_left(5) ^ word).wrapping_mul(0x9e37_79b1);
    }
    // Rows are an even number of pixels, so a stripe is whole words.
    debug_assert!(pixels.len() % 4 == 0);
    u64::from(a) << 32 | u64::from(b)
}

/// A [`Framebuffer`] drawn in [`BinaryColor`].
pub struct Binary<'a>(&'a mut Framebuffer);

fn rgb(color: BinaryColor) -> Rgb565 {
    if color.is_on() {
        Rgb565::WHITE
    } else {
        Rgb565::BLACK
    }
}

impl OriginDimensions for Binary<'_> {
    fn size(&self) -> Size {
        self.0.size()
    }
}
impl DrawTarget for Binary<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;
    fn draw_iter<T: IntoIterator<Item = Pixel<BinaryColor>>>(
        &mut self,
        pixels: T,
    ) -> Result<(), Self::Error> {
        for Pixel(Point { x, y }, c) in pixels {
            self.0.put(x, y, rgb(c));
        }
        Ok(())
    }
    fn clear(&mut self, color: BinaryColor) -> Result<(), Self::Error> {
        self.0.clear(rgb(color))
    }
}

#[derive(Debug)]
pub enum Error<S, P> {
    Spi(S),
    Pin(P),
}
pub struct St7796<S, P> {
    spi: S,
    dc: P,
}
impl<S: SpiDevice<u8>, P: OutputPin> St7796<S, P> {
    pub fn new(spi: S, dc: P) -> Self {
        Self { spi, dc }
    }
    pub async fn command(
        &mut self,
        command: u8,
        data: &[u8],
    ) -> Result<(), Error<S::Error, P::Error>> {
        self.dc.set_low().map_err(Error::Pin)?;
        self.spi.write(&[command]).await.map_err(Error::Spi)?;
        self.dc.set_high().map_err(Error::Pin)?;
        if !data.is_empty() {
            self.spi.write(data).await.map_err(Error::Spi)?;
        }
        Ok(())
    }
    pub async fn init(
        &mut self,
        delay: &mut impl DelayNs,
    ) -> Result<(), Error<S::Error, P::Error>> {
        self.command(0x01, &[]).await?;
        // ST7796S §9.2.2: reset while asleep requires 120 ms before SLPOUT.
        // Use that interval on both cold and warm boots; prior mode is unknown.
        delay.delay_ms(120).await;
        self.command(0x11, &[]).await?;
        // §9.2.13: new commands may follow SLPOUT after 5 ms. The separate
        // 120 ms restriction applies to SLPIN, which this driver never sends.
        delay.delay_ms(5).await;
        // Board-specific ST7796 settings from LilyGo's panel initialization.
        for (cmd, data) in [
            (0xf0, &[0xc3][..]),
            (0xf0, &[0x96]),
            (0x36, &[0xe8]),
            (0x3a, &[0x55]),
            (0xb4, &[1]),
            (0xb6, &[0x80, 2, 0x3b]),
            (0xe8, &[0x40, 0x8a, 0, 0, 0x29, 0x19, 0xa5, 0x33]),
            (0xc1, &[6]),
            (0xc2, &[0xa7]),
            (0xc5, &[0x18]),
            (
                0xe0,
                &[
                    0xf0, 9, 0x0b, 6, 4, 0x15, 0x2f, 0x54, 0x42, 0x3c, 0x17, 0x14, 0x18, 0x1b,
                ],
            ),
            (
                0xe1,
                &[
                    0xe0, 9, 0x0b, 6, 4, 3, 0x2b, 0x43, 0x42, 0x3b, 0x16, 0x14, 0x17, 0x1b,
                ],
            ),
            (0xf0, &[0x3c]),
            (0xf0, &[0x69]),
            (0x21, &[]),
        ] {
            self.command(cmd, data).await?;
        }
        self.command(0x28, &[]).await
    }
    pub async fn on(&mut self, on: bool) -> Result<(), Error<S::Error, P::Error>> {
        self.command(if on { 0x29 } else { 0x28 }, &[]).await
    }
    /// Writes whole rows from row `y`. With `continues`, the rows follow
    /// straight on from the last ones written, and only the pixels go out;
    /// otherwise the window is set first, from `y` to the bottom of the
    /// screen, so that later rows can follow on the same way.
    pub async fn stripe(
        &mut self,
        y: usize,
        pixels: &[u8],
        continues: bool,
    ) -> Result<(), Error<S::Error, P::Error>> {
        assert!(y < HEIGHT && pixels.len() <= STRIPE_BYTES && pixels.len() % (WIDTH * 2) == 0);
        let rows = pixels.len() / (WIDTH * 2);
        assert!(rows != 0 && y + rows <= HEIGHT);
        if continues {
            // Write Memory Continue: on from where the last write stopped.
            return self.command(0x3c, pixels).await;
        }
        // Landscape rotation maps the portrait column offset to the Y address.
        let start = (49 + y) as u16;
        let end = (49 + HEIGHT - 1) as u16;
        self.command(0x2a, &[0, 0, 1, 0xdf]).await?;
        self.command(
            0x2b,
            &[(start >> 8) as u8, start as u8, (end >> 8) as u8, end as u8],
        )
        .await?;
        self.command(0x2c, pixels).await
    }
}
