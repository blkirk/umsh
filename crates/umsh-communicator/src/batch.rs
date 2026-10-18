//! One commit: the text engine's outputs, in the order the store writes
//! them.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use umsh_text::ConversationKey;
use umsh_text::engine::sequence::MessageHandle;
use umsh_text::engine::{
    ArchiveKey, BodyRef, DeliveryState, Event, MAX_FRAME, MessageMutation, MutationKind, Output,
    StreamCheckpoint,
};

use crate::ids::{ChatId, MessageId};
use crate::record::{BODY_MAX, LinkInfo, NAME_MAX, clip};

/// The steps of a commit, in the order a store takes them. Power lost
/// after any of them leaves the store safe:
///
/// | Lost after | Effect |
/// |---|---|
/// | [`Checkpoint`](Self::Checkpoint) | one wire ID goes unused; a resend request for it is answered Message Unavailable |
/// | [`ArchiveDeletes`](Self::ArchiveDeletes) | an edited original no longer resends: it is answered Unavailable, never with superseded content |
/// | [`Archives`](Self::Archives) | resend material for a message neither sent nor shown |
/// | [`Transcript`](Self::Transcript) | complete: the transmissions are released |
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommitStep {
    Checkpoint,
    ArchiveDeletes,
    Archives,
    Transcript,
}

impl CommitStep {
    pub const ALL: [Self; 4] = [
        Self::Checkpoint,
        Self::ArchiveDeletes,
        Self::Archives,
        Self::Transcript,
    ];
}

/// A transcript change, as a store reads it out of a [`Batch`].
#[derive(Clone, Copy, Debug)]
pub enum Change<'b> {
    /// One of the engine's transcript mutations, its body and Sender
    /// Handle resolved to text.
    Message {
        mutation: &'b MessageMutation,
        body: &'b str,
        sender_handle: Option<&'b str>,
        /// What the radio said about the frame behind it, for a mutation
        /// a received frame produced.
        link: Option<LinkInfo>,
    },
    /// Word on one fragment of an outbound message.
    Delivery {
        handle: MessageHandle,
        fragment: Option<u8>,
        state: DeliveryState,
    },
}

// Most entries are messages, so the mutation is kept inline rather than
// boxed for the sake of the rarer deliveries.
#[allow(clippy::large_enum_variant)]
enum Entry {
    Message {
        mutation: MessageMutation,
        body: Range<usize>,
        sender_handle: Option<Range<usize>>,
        link: Option<LinkInfo>,
    },
    Delivery {
        handle: MessageHandle,
        fragment: Option<u8>,
        state: DeliveryState,
    },
}

/// What one commit writes: the outbound checkpoints, then the archive
/// deletes, then the archives, then the transcript.
///
/// A batch is filled from the engine's outputs as they are drained,
/// [`Batch::push`] taking each in turn. It copies what it keeps—bodies
/// out of the engine's render arena, which is reused once the queue is
/// empty, and payloads out of the outputs—so it outlives both. What it
/// borrows is the caller's map from the engine's message handles to the
/// store's message IDs, for mutations of messages kept by an earlier
/// commit.
pub struct Batch<'a> {
    time: Option<u32>,
    handles: &'a dyn Fn(MessageHandle) -> Option<MessageId>,
    checkpoints: Vec<StreamCheckpoint>,
    archive_deletes: Vec<(ConversationKey, u8)>,
    archives: Vec<(ArchiveKey, heapless::Vec<u8, MAX_FRAME>)>,
    entries: Vec<Entry>,
    text: String,
}

impl<'a> Batch<'a> {
    /// A batch whose messages are kept at `time`, in seconds of wall-clock
    /// time when it is known. `handles` maps the engine's handles to the
    /// store's IDs, as the [`Committed`] of earlier commits gave them.
    pub fn new(time: Option<u32>, handles: &'a dyn Fn(MessageHandle) -> Option<MessageId>) -> Self {
        Self {
            time,
            handles,
            checkpoints: Vec::new(),
            archive_deletes: Vec::new(),
            archives: Vec::new(),
            entries: Vec::new(),
            text: String::new(),
        }
    }

    /// Take one drained output. `body` resolves the output's body
    /// references, which is [`Engine::body`] while the output is fresh,
    /// and `link` is what the radio said about the frame that produced it,
    /// if a frame did. Returns whether the output was the store's.
    ///
    /// A transmission's payload is kept for resending when it carries an
    /// archive key; the transmission itself is the caller's to hold until
    /// the batch is committed.
    ///
    /// [`Engine::body`]: umsh_text::engine::Engine::body
    pub fn push<'e>(
        &mut self,
        output: &Output,
        body: impl Fn(&BodyRef) -> &'e str,
        link: Option<LinkInfo>,
    ) -> bool {
        match output {
            Output::StoreCheckpoint {
                conversation,
                next_id,
                epoch,
            } => {
                let checkpoint = StreamCheckpoint {
                    conversation: *conversation,
                    next_id: *next_id,
                    epoch: *epoch,
                };
                match self
                    .checkpoints
                    .iter_mut()
                    .find(|kept| kept.conversation == *conversation)
                {
                    Some(kept) => *kept = checkpoint,
                    None => self.checkpoints.push(checkpoint),
                }
            }
            Output::DeleteArchive {
                conversation,
                message_id,
            } => self.archive_deletes.push((*conversation, *message_id)),
            Output::StoreArchive { key, payload } => self.archives.push((*key, payload.clone())),
            Output::Transmit(transmission) => {
                let Some(key) = transmission.archive else {
                    return false;
                };
                self.archives.push((key, transmission.payload.clone()));
            }
            Output::StoreMessage(mutation) => {
                let (text, handle) = match &mutation.kind {
                    MutationKind::Insert {
                        body: text,
                        sender_handle,
                        ..
                    } => (Some(body(text)), sender_handle.map(|handle| body(&handle))),
                    MutationKind::UpdateBody { body: text, .. }
                    | MutationKind::Edit { body: text, .. } => (Some(body(text)), None),
                    MutationKind::Delete { .. } => (None, None),
                };
                let body = self.keep(text.unwrap_or(""), BODY_MAX);
                let sender_handle = handle.map(|handle| self.keep(handle, NAME_MAX));
                self.entries.push(Entry::Message {
                    mutation: *mutation,
                    body,
                    sender_handle,
                    link,
                });
            }
            Output::Event(Event::DeliveryStateChanged {
                handle,
                fragment,
                state,
            }) => self.entries.push(Entry::Delivery {
                handle: *handle,
                fragment: *fragment,
                state: *state,
            }),
            Output::Event(_) | Output::LookupOutbound { .. } | Output::Diagnostic(_) => {
                return false;
            }
        }
        true
    }

    fn keep(&mut self, text: &str, max: usize) -> Range<usize> {
        let start = self.text.len();
        self.text.push_str(clip(text, max));
        start..self.text.len()
    }

    /// When the batch's messages were kept.
    pub fn time(&self) -> Option<u32> {
        self.time
    }

    /// The store's ID for a message an earlier commit kept, by the
    /// engine's handle for it.
    pub fn resolve(&self, handle: MessageHandle) -> Option<MessageId> {
        (self.handles)(handle)
    }

    pub fn checkpoints(&self) -> &[StreamCheckpoint] {
        &self.checkpoints
    }

    /// Message IDs whose archived fragments are all to go.
    pub fn archive_deletes(&self) -> &[(ConversationKey, u8)] {
        &self.archive_deletes
    }

    pub fn archives(&self) -> impl Iterator<Item = (&ArchiveKey, &[u8])> + '_ {
        self.archives
            .iter()
            .map(|(key, payload)| (key, payload.as_slice()))
    }

    pub fn changes(&self) -> impl Iterator<Item = Change<'_>> + '_ {
        self.entries.iter().map(|entry| match entry {
            Entry::Message {
                mutation,
                body,
                sender_handle,
                link,
            } => Change::Message {
                mutation,
                body: &self.text[body.clone()],
                sender_handle: sender_handle.clone().map(|range| &self.text[range]),
                link: *link,
            },
            Entry::Delivery {
                handle,
                fragment,
                state,
            } => Change::Delivery {
                handle: *handle,
                fragment: *fragment,
                state: *state,
            },
        })
    }

    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
            && self.archive_deletes.is_empty()
            && self.archives.is_empty()
            && self.entries.is_empty()
    }
}

/// What a commit did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Committed {
    /// The IDs given to messages the batch inserted, by the engine's
    /// handles for them.
    pub inserted: Vec<(MessageHandle, MessageId)>,
    /// Messages inserted or changed, in the order first touched.
    pub messages: Vec<MessageId>,
    /// Chats made or changed, in the order first touched.
    pub chats: Vec<ChatId>,
}

impl Committed {
    pub(crate) fn touch_message(&mut self, id: MessageId) {
        if !self.messages.contains(&id) {
            self.messages.push(id);
        }
    }

    pub(crate) fn touch_chat(&mut self, id: ChatId) {
        if !self.chats.contains(&id) {
            self.chats.push(id);
        }
    }

    /// The ID this commit gave the message `handle` names.
    pub fn id_of(&self, handle: MessageHandle) -> Option<MessageId> {
        self.inserted
            .iter()
            .find(|(inserted, _)| *inserted == handle)
            .map(|(_, id)| *id)
    }
}
