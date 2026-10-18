#![cfg_attr(not(test), no_std)]

//! A communicator's durable chat state.
//!
//! - [`ChatStore`]: what a back end keeps—chats, messages, the resend
//!   archive, outbound checkpoints, peers, and channels—and the order it
//!   commits them in.
//! - [`Batch`]: one commit's worth of the text engine's outputs, in the
//!   order the store writes them.
//! - [`MemoryStore`]: the back end for tests, the simulator, and anything
//!   before durable storage.
//! - [`conformance`]: the suite every back end passes, power cut between
//!   each step of the commit order included.
//!
//! Device settings and security state are not here: the device keeps the
//! first, and the second stays with the MAC.

extern crate alloc;

mod batch;
mod ids;
mod memory;
mod reaction;
mod record;
mod store;
mod wire;

#[cfg(any(test, feature = "conformance"))]
pub mod conformance;

pub use batch::{Batch, Change, CommitStep, Committed};
pub use ids::{ChatId, MessageId, PeerId};
pub use memory::{MemoryStore, Quota};
pub use reaction::{Reacted, Reaction, Reactions};
pub use record::{
    BODY_MAX, BuiltinChannel, ChannelAccess, ChannelChange, ChannelRecord, ChatChange, ChatRecord,
    Delivery, IDENTITY_MAX, LinkInfo, MessageFlags, MessageRecord, NAME_MAX, Name, PeerChange,
    PeerFilter, PeerRecord, ROUTE_MAX, StoreError, Usage, WireAlias,
};
pub use store::ChatStore;
