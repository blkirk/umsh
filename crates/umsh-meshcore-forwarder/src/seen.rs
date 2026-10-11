//! Duplicate memory: a fixed ring of recent packet identities.

use crate::packet::PacketHash;

/// Identities remembered, the baseline repeater's count: 1,280 octets.
pub const SEEN_CAPACITY: usize = 160;

/// The most recent [`SEEN_CAPACITY`] packet identities; the oldest is
/// overwritten when a new one arrives.
pub struct SeenTable {
    hashes: [PacketHash; SEEN_CAPACITY],
    /// Slot the next identity is written to.
    next: usize,
    /// Slots written since the last clear. Only these are compared, so an
    /// empty table matches nothing—not even a packet whose identity
    /// happens to be all zeros.
    filled: usize,
}

impl SeenTable {
    pub const fn new() -> Self {
        Self {
            hashes: [[0; 8]; SEEN_CAPACITY],
            next: 0,
            filled: 0,
        }
    }

    pub fn contains(&self, hash: &PacketHash) -> bool {
        self.hashes[..self.filled].contains(hash)
    }

    /// Remember `hash`, forgetting the oldest identity when full.
    pub fn insert(&mut self, hash: PacketHash) {
        self.hashes[self.next] = hash;
        self.next = (self.next + 1) % SEEN_CAPACITY;
        self.filled = (self.filled + 1).min(SEEN_CAPACITY);
    }

    pub fn clear(&mut self) {
        self.next = 0;
        self.filled = 0;
    }
}

impl Default for SeenTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(n: u32) -> PacketHash {
        let mut hash = [0; 8];
        hash[..4].copy_from_slice(&n.to_be_bytes());
        hash
    }

    #[test]
    fn an_empty_table_matches_nothing() {
        let table = SeenTable::new();
        assert!(!table.contains(&[0; 8]));
    }

    #[test]
    fn the_oldest_identity_is_forgotten_first() {
        let mut table = SeenTable::new();
        for n in 0..SEEN_CAPACITY as u32 {
            table.insert(hash(n));
        }
        assert!(table.contains(&hash(0)));
        table.insert(hash(1_000));
        assert!(!table.contains(&hash(0)));
        assert!(table.contains(&hash(1)));
        assert!(table.contains(&hash(1_000)));
        table.clear();
        assert!(!table.contains(&hash(1_000)));
    }
}
