//! Ordered keys for what the engine names, so a back end can keep them in
//! sorted maps. Each is the thing's identity laid out as octets: a kind,
//! then the key material, padded to one length.

use umsh_text::engine::ArchiveKey;
use umsh_text::{ConversationKey, SenderScope};

/// A conversation, ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ConversationCode([u8; 49]);

impl From<&ConversationKey> for ConversationCode {
    fn from(conversation: &ConversationKey) -> Self {
        let mut code = [0u8; 49];
        match conversation {
            ConversationKey::Direct { peer } => {
                code[0] = 1;
                code[17..].copy_from_slice(&peer.0);
            }
            ConversationKey::ChannelGroup { channel } => {
                code[0] = 2;
                code[1..17].copy_from_slice(&channel.0);
            }
            ConversationKey::ChannelDirect { channel, peer } => {
                code[0] = 3;
                code[1..17].copy_from_slice(&channel.0);
                code[17..].copy_from_slice(&peer.0);
            }
            ConversationKey::Room { room } => {
                code[0] = 4;
                code[17..].copy_from_slice(&room.0);
            }
        }
        Self(code)
    }
}

/// A sender's stream within a conversation, ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SenderCode([u8; 33]);

impl SenderCode {
    pub(crate) const MIN: Self = Self([0; 33]);
    pub(crate) const MAX: Self = Self([u8::MAX; 33]);
}

impl From<&SenderScope> for SenderCode {
    fn from(sender: &SenderScope) -> Self {
        let mut code = [0u8; 33];
        match sender {
            SenderScope::Local => {}
            SenderScope::Peer(key) => {
                code[0] = 1;
                code[1..].copy_from_slice(&key.0);
            }
            SenderScope::ClaimedMember(hint) => {
                code[0] = 2;
                code[1..4].copy_from_slice(&hint.0);
            }
        }
        Self(code)
    }
}

/// Where a wire ID points: a conversation, a sender's stream in it, and
/// the ID.
pub(crate) type AliasCode = (ConversationCode, SenderCode, u8);

/// One archived fragment: its message's conversation and wire ID, then
/// the fragment, with 0 for an unfragmented message and `n + 1` for
/// fragment `n`.
pub(crate) type ArchiveCode = (ConversationCode, u8, u8);

pub(crate) fn archive_code(key: &ArchiveKey) -> ArchiveCode {
    (
        (&key.conversation).into(),
        key.message_id,
        key.fragment
            .map_or(0, |fragment| fragment.saturating_add(1)),
    )
}
