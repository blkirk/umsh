//! The store's own names for what it keeps.
//!
//! Each is assigned by the store when the thing is first kept, counts up,
//! and is never reused, even after what it named is evicted or forgotten:
//! an identifier held across an eviction names nothing rather than
//! something else.

/// A chat: one conversation's transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChatId(pub u32);

/// A message in a chat. Later messages have larger identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(pub u32);

/// A peer the store knows: saved, or heard recently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(pub u32);
