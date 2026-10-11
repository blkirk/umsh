//! A MeshCore v1 forwarder.
//!
//! An interoperable repeater for MeshCore v1 traffic, sharing a radio
//! with a UMSH node. It parses MeshCore frames, suppresses duplicates,
//! and forwards exactly what a baseline MeshCore repeater would forward,
//! mutated exactly as the baseline mutates it, so that no neighbor can
//! tell from what it receives that the repeater is not a MeshCore one.
//!
//! The wire behavior follows MeshCore's `repeater-v1.17.1` release,
//! commit [`BASELINE_COMMIT`]. Local policy—airtime estimation, channel
//! access, duty limiting, randomness, and table and queue sizes—belongs
//! to the device it runs on and reaches the engine through
//! [`Environment`] and the caller's own transmit path.
//!
//! The engine holds the device's public key and nothing else of its
//! identity. Its only cryptography is SHA-256, HMAC-SHA256 for region
//! codes, and Ed25519 verification of advertisements: nothing here signs,
//! agrees on a key, or decrypts, and nothing originates a packet.
//!
//! Portions of this crate follow the structure of MeshCore, which is
//! distributed under the MIT License:
//!
//! > Copyright (c) 2025 Scott Powell / rippleradios.com
//!
//! The full notice is in `LICENSE-MESHCORE` alongside this crate.

#![cfg_attr(not(test), no_std)]

mod engine;
pub mod packet;
mod queue;
pub mod region;
mod seen;

pub use engine::{
    Counters, DIRECT_DELAY_FACTOR_MILLI, Declined, Environment, FLOOD_DELAY_FACTOR_MILLI,
    FLOOD_MAX, FLOOD_MAX_ADVERT, FLOOD_MAX_UNSCOPED, Forwarder, Heard, MAX_REGIONS, Settings,
    UNMEASURED_TRACE_SNR, Verdict,
};
pub use region::{RegionPolicy, TransportKey};
pub use seen::SEEN_CAPACITY;

/// The MeshCore commit whose wire behavior this crate reproduces: the
/// `repeater-v1.17.1` release.
pub const BASELINE_COMMIT: &str = "d92964352441e53b93e8667b802e04f6e072b39e";

/// Whether a device whose public key starts with `first_octet` may run a
/// MeshCore repeater. The baseline reserves the 0x00 and 0xFF one-octet
/// identity hashes and never runs a repeater under either.
pub const fn identity_allowed(first_octet: u8) -> bool {
    !matches!(first_octet, 0x00 | 0xFF)
}

#[cfg(test)]
mod tests;
