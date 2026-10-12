//! Bearers: shared broadcast media a node forwards onto beside its radio.
//!
//! A bearer is joined by every node attached to it, so what one node
//! offers to it every other node hears. The MAC does not drive a bearer
//! itself. A forwarding decision—or a local origination—that a bearer
//! should carry is placed in [`BearerExports`], and whatever owns the
//! bearer drains it from there. The bearer's own receptions reach the MAC
//! like any other frame, marked [`RxOrigin::Bearer`](umsh_hal::RxOrigin).
//!
//! Spec: [LAN Bearer](https://darconeous.github.io/umsh/docs/protocol/lan-bearer.html).

use heapless::Vec;
use umsh_hal::BearerId;

/// A set of a node's bearers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BearerSet(u8);

const _: () = assert!(BearerId::COUNT <= u8::BITS as usize);

impl BearerSet {
    /// No bearers.
    pub const EMPTY: Self = Self(0);

    /// The set holding only `bearer`.
    pub const fn only(bearer: BearerId) -> Self {
        Self(1 << bearer.index())
    }

    /// Whether `bearer` is in the set.
    pub const fn contains(self, bearer: BearerId) -> bool {
        self.0 & (1 << bearer.index()) != 0
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// This set with `bearer` added or removed.
    pub const fn with(self, bearer: BearerId, present: bool) -> Self {
        if present {
            Self(self.0 | (1 << bearer.index()))
        } else {
            Self(self.0 & !(1 << bearer.index()))
        }
    }

    /// This set less `bearer`, if there is one.
    pub const fn without(self, bearer: Option<BearerId>) -> Self {
        match bearer {
            Some(bearer) => self.with(bearer, false),
            None => self,
        }
    }
}

/// Which bearers a node forwards onto.
///
/// Independent of [`RepeaterConfig::enabled`](crate::RepeaterConfig): a
/// bearer is a forwarding egress of its own, and enabling one forwards
/// between it and the radio whether or not the node repeats radio to
/// radio. Every other forwarding rule—hop accounting, region policy,
/// duplicate suppression—comes from the one forwarding decision both
/// egresses share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BearerConfig {
    /// The bearers this node exports to and forwards from.
    pub enabled: BearerSet,
}

/// A frame waiting to be carried by one or more bearers.
#[derive(Clone, Debug)]
pub(crate) struct BearerExport<const FRAME: usize> {
    frame: Vec<u8, FRAME>,
    pending: BearerSet,
}

/// The bounded queue of frames the MAC has offered to its bearers.
///
/// A frame stays until every bearer it was offered to has taken it. A
/// full queue makes room by dropping its oldest frame: a bearer that has
/// fallen that far behind gains nothing from stale traffic, and the
/// newest frame is the one most likely still to matter.
#[derive(Clone, Debug)]
pub(crate) struct BearerExports<const N: usize, const FRAME: usize> {
    queue: Vec<BearerExport<FRAME>, N>,
}

impl<const N: usize, const FRAME: usize> BearerExports<N, FRAME> {
    pub(crate) const fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Offer `frame` to `bearers`, returning how many frames were lost to
    /// make room—the oldest one, or this one if it cannot be held at all.
    pub(crate) fn push(&mut self, frame: &[u8], bearers: BearerSet) -> u32 {
        if bearers.is_empty() {
            return 0;
        }
        let Ok(frame) = Vec::from_slice(frame) else {
            return 1;
        };
        if N == 0 {
            return 1;
        }
        let mut dropped = 0;
        if self.queue.is_full() {
            self.queue.remove(0);
            dropped = 1;
        }
        let _ = self.queue.push(BearerExport {
            frame,
            pending: bearers,
        });
        dropped
    }

    /// Take the oldest frame still owed to `bearer`, copying it into
    /// `buf` and returning its length.
    pub(crate) fn pop(&mut self, bearer: BearerId, buf: &mut [u8]) -> Option<usize> {
        let index = self
            .queue
            .iter()
            .position(|export| export.pending.contains(bearer))?;
        let export = &mut self.queue[index];
        let len = export.frame.len().min(buf.len());
        buf[..len].copy_from_slice(&export.frame[..len]);
        export.pending = export.pending.with(bearer, false);
        if export.pending.is_empty() {
            self.queue.remove(index);
        }
        Some(len)
    }

    /// Forget every frame still owed to `bearer`.
    pub(crate) fn clear(&mut self, bearer: BearerId) {
        for export in self.queue.iter_mut() {
            export.pending = export.pending.with(bearer, false);
        }
        self.queue.retain(|export| !export.pending.is_empty());
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAN: BearerId = BearerId::LAN;

    fn other() -> BearerId {
        BearerId::new(1).unwrap()
    }

    #[test]
    fn a_frame_stays_until_every_bearer_has_it() {
        let mut exports = BearerExports::<4, 16>::new();
        let both = BearerSet::only(LAN).with(other(), true);
        assert_eq!(exports.push(&[1, 2, 3], both), 0);
        let mut buf = [0; 16];
        assert_eq!(exports.pop(LAN, &mut buf), Some(3));
        assert_eq!(&buf[..3], &[1, 2, 3]);
        assert_eq!(exports.pop(LAN, &mut buf), None);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports.pop(other(), &mut buf), Some(3));
        assert_eq!(exports.len(), 0);
    }

    #[test]
    fn a_full_queue_drops_its_oldest_frame() {
        let mut exports = BearerExports::<2, 16>::new();
        let lan = BearerSet::only(LAN);
        assert_eq!(exports.push(&[1], lan), 0);
        assert_eq!(exports.push(&[2], lan), 0);
        assert_eq!(exports.push(&[3], lan), 1);
        let mut buf = [0; 16];
        assert_eq!(exports.pop(LAN, &mut buf), Some(1));
        assert_eq!(buf[0], 2);
    }

    #[test]
    fn a_queue_with_no_room_counts_every_frame_dropped() {
        let mut exports = BearerExports::<0, 16>::new();
        assert_eq!(exports.push(&[1], BearerSet::only(LAN)), 1);
        assert_eq!(exports.push(&[1], BearerSet::EMPTY), 0);
    }

    #[test]
    fn clearing_one_bearer_keeps_frames_owed_to_another() {
        let mut exports = BearerExports::<4, 16>::new();
        exports.push(&[1], BearerSet::only(LAN));
        exports.push(&[2], BearerSet::only(LAN).with(other(), true));
        exports.clear(LAN);
        assert_eq!(exports.len(), 1);
        let mut buf = [0; 16];
        assert_eq!(exports.pop(other(), &mut buf), Some(1));
        assert_eq!(buf[0], 2);
    }
}
