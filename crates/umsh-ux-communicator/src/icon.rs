//! Interface icons, rasterized at build time from the mockup's glyphs and
//! drawn as templates in the foreground color, and the reaction glyphs,
//! drawn in their own colors.

use embedded_graphics::image::ImageRaw;
use embedded_graphics::pixelcolor::{Gray4, Rgb565};

use crate::model::Reaction;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));
include!(concat!(env!("OUT_DIR"), "/reactions.rs"));

/// What a reaction glyph is drawn on. Each is composited onto its own color,
/// since the panel is drawn without reading it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pad {
    /// The actions menu's palette, in [`theme::REACTION_PAD`](crate::theme::REACTION_PAD).
    Palette,
    /// A chip for my reaction, in [`theme::OUTBOUND`](crate::theme::OUTBOUND).
    Mine,
    /// A chip for someone else's, in [`theme::CHIP_THEIRS`](crate::theme::CHIP_THEIRS).
    Theirs,
    /// A chip for my reaction in an open channel, in
    /// [`theme::OUTBOUND_OPEN`](crate::theme::OUTBOUND_OPEN).
    MineOpen,
    /// The screen itself, in [`theme::BACKGROUND`](crate::theme::BACKGROUND).
    Screen,
}

/// The glyph for `reaction` on `pad`.
#[must_use]
pub fn reaction(reaction: Reaction, pad: Pad) -> &'static ImageRaw<'static, Rgb565> {
    &REACTIONS[reaction as usize][pad as usize]
}
