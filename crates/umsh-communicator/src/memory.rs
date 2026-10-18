//! The store in memory: for tests, the simulator, and anything before
//! durable storage.
//!
//! It keeps two parts, as a durable back end does. What it would write to
//! storage—records, archives, checkpoints, and the counters behind its
//! identifiers—survives a power cut. Its index—chats by activity, each
//! chat's messages, wire aliases, peers by hint—is kept up to date as it
//! goes and rebuilt from the first part when it is mounted again, so a
//! power cut can be armed and the store remounted to see what survived.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Bound;

use umsh_core::{ChannelTag, NodeHint, PublicKey};
use umsh_text::engine::{
    ArchiveKey, Direction, MessageMutation, MutationKind, Presence, ResolvedRef, StreamCheckpoint,
};
use umsh_text::{ConversationKey, MessageType, SenderScope, WireRef};

use crate::batch::{Batch, Change, CommitStep, Committed};
use crate::ids::{ChatId, MessageId, PeerId};
use crate::reaction::{Reacted, Reaction, Reactions};
use crate::record::{
    self, BODY_MAX, ChannelChange, ChannelRecord, ChatChange, ChatRecord, Delivery, IDENTITY_MAX,
    LinkInfo, MessageFlags, MessageRecord, PeerChange, PeerFilter, PeerRecord, ROUTE_MAX,
    StoreError, Usage, WireAlias,
};
use crate::store::ChatStore;
use crate::wire::{AliasCode, ArchiveCode, ConversationCode, SenderCode, archive_code};

/// How much a store keeps before it evicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quota {
    /// Octets of messages and archived payloads kept before the oldest
    /// messages go.
    pub bytes: usize,
    /// The newest messages of each chat that eviction never takes, however
    /// busy another chat is.
    pub keep_per_chat: usize,
    /// Heard peers kept besides the saved ones, most recently heard first.
    pub heard_peers: usize,
}

impl Quota {
    pub const DEFAULT: Self = Self {
        bytes: 300 * 1024,
        keep_per_chat: 50,
        heard_peers: 32,
    };
}

impl Default for Quota {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a message costs against the quota besides its body: about what a
/// durable back end spends on its record and its index entries.
pub const RECORD_COST: usize = 96;

// ─── What survives power loss ────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ChatEntry {
    record: ChatRecord,
    /// Orders chats by activity: the store's clock when it last had a
    /// message.
    activity: u64,
    draft: Option<String>,
}

#[derive(Clone, Debug)]
struct Reactor {
    who: SenderCode,
    mine: bool,
    reaction: Reaction,
}

#[derive(Clone, Debug)]
struct MessageEntry {
    /// The record, its reactions left to be counted on reading.
    record: MessageRecord,
    conversation: ConversationKey,
    body: String,
    /// The engine revision it was last changed at, so a mutation seen
    /// again is not applied twice.
    revision: u32,
    /// The latest reaction from each sender, in the order they came.
    reactors: Vec<Reactor>,
}

#[derive(Clone, Debug)]
struct PeerEntry {
    record: PeerRecord,
    /// The store's clock when it was last heard.
    heard: u64,
}

#[derive(Clone, Debug, Default)]
struct Disk {
    next_chat: u32,
    next_message: u32,
    next_peer: u32,
    /// Orders everything the store does.
    clock: u64,
    /// Each conversation's outbound checkpoint, and when it was last used.
    checkpoints: BTreeMap<ConversationCode, (StreamCheckpoint, u64)>,
    archives: BTreeMap<ArchiveCode, Vec<u8>>,
    chats: BTreeMap<u32, ChatEntry>,
    messages: BTreeMap<u32, MessageEntry>,
    peers: BTreeMap<[u8; 32], PeerEntry>,
    channels: BTreeMap<[u8; 16], ChannelRecord>,
}

impl Disk {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }
}

// ─── What is rebuilt on mounting ─────────────────────────────────────────

#[derive(Debug, Default, PartialEq, Eq)]
struct Index {
    chat_for: BTreeMap<ConversationCode, u32>,
    by_activity: BTreeMap<u64, u32>,
    chat_messages: BTreeMap<u32, BTreeSet<u32>>,
    aliases: BTreeMap<AliasCode, BTreeSet<u32>>,
    hints: BTreeMap<[u8; 3], BTreeSet<[u8; 32]>>,
    /// Heard peers that are not saved, by when they were heard.
    heard: BTreeMap<u64, [u8; 32]>,
    /// Octets counted against the quota.
    bytes: usize,
}

impl Index {
    fn build(disk: &Disk) -> Self {
        let mut index = Self::default();
        for (&id, chat) in &disk.chats {
            index.add_chat(id, chat);
        }
        for (&id, message) in &disk.messages {
            index.add_message(id, message);
        }
        index.bytes += disk.archives.values().map(Vec::len).sum::<usize>();
        for (key, peer) in &disk.peers {
            index.add_peer(key, peer);
        }
        index
    }

    fn add_chat(&mut self, id: u32, chat: &ChatEntry) {
        self.chat_for.insert((&chat.record.conversation).into(), id);
        self.by_activity.insert(chat.activity, id);
        self.chat_messages.entry(id).or_default();
    }

    fn remove_chat(&mut self, id: u32, chat: &ChatEntry) {
        self.chat_for.remove(&(&chat.record.conversation).into());
        self.by_activity.remove(&chat.activity);
        self.chat_messages.remove(&id);
    }

    fn add_message(&mut self, id: u32, message: &MessageEntry) {
        self.chat_messages
            .entry(message.record.chat.0)
            .or_default()
            .insert(id);
        if let Some(alias) = alias_code(message) {
            self.aliases.entry(alias).or_default().insert(id);
        }
        self.bytes += cost(message);
    }

    fn remove_message(&mut self, id: u32, message: &MessageEntry) {
        if let Some(messages) = self.chat_messages.get_mut(&message.record.chat.0) {
            messages.remove(&id);
        }
        if let Some(alias) = alias_code(message)
            && let Some(ids) = self.aliases.get_mut(&alias)
        {
            ids.remove(&id);
            if ids.is_empty() {
                self.aliases.remove(&alias);
            }
        }
        self.bytes -= cost(message);
    }

    fn add_peer(&mut self, key: &[u8; 32], peer: &PeerEntry) {
        self.hints.entry(hint_of(key)).or_default().insert(*key);
        if !peer.record.saved {
            self.heard.insert(peer.heard, *key);
        }
    }

    fn remove_peer(&mut self, key: &[u8; 32], peer: &PeerEntry) {
        let hint = hint_of(key);
        if let Some(keys) = self.hints.get_mut(&hint) {
            keys.remove(key);
            if keys.is_empty() {
                self.hints.remove(&hint);
            }
        }
        if !peer.record.saved {
            self.heard.remove(&peer.heard);
        }
    }
}

fn cost(message: &MessageEntry) -> usize {
    RECORD_COST + message.body.len()
}

fn alias_code(message: &MessageEntry) -> Option<AliasCode> {
    Some((
        (&message.conversation).into(),
        (&message.record.sender).into(),
        message.record.wire_id?,
    ))
}

fn hint_of(key: &[u8; 32]) -> [u8; 3] {
    [key[0], key[1], key[2]]
}

fn reactions(reactors: &[Reactor]) -> Reactions {
    let mut counted = Reactions::new();
    for reactor in reactors {
        match counted
            .iter_mut()
            .find(|reacted| reacted.reaction == reactor.reaction)
        {
            Some(reacted) => {
                reacted.count += 1;
                reacted.mine |= reactor.mine;
            }
            None => {
                let _ = counted.push(Reacted {
                    reaction: reactor.reaction,
                    count: 1,
                    mine: reactor.mine,
                });
            }
        }
    }
    counted
}

// ─── The store ───────────────────────────────────────────────────────────

/// A [`ChatStore`] in memory.
pub struct MemoryStore {
    disk: Disk,
    index: Index,
    quota: Quota,
    /// Power goes once a commit finishes this step.
    cut: Option<CommitStep>,
    /// Power is gone: nothing more is written until it is back.
    off: bool,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::with_quota(Quota::DEFAULT)
    }

    pub fn with_quota(quota: Quota) -> Self {
        Self {
            disk: Disk::default(),
            index: Index::default(),
            quota,
            cut: None,
            off: false,
        }
    }

    /// Lose power once the next commit has finished `step`: nothing after
    /// it is written, and nothing more is until [`MemoryStore::power_on`].
    pub fn cut_power_after(&mut self, step: CommitStep) {
        self.cut = Some(step);
    }

    /// Mount again from what was written, as after a power cut or a
    /// reboot.
    pub fn power_on(&mut self) {
        self.off = false;
        self.cut = None;
        self.index = Index::build(&self.disk);
    }

    fn writable(&self) -> Result<(), StoreError> {
        if self.off {
            Err(StoreError::PoweredOff)
        } else {
            Ok(())
        }
    }

    /// `step` is done. Returns whether power went with it.
    fn reached(&mut self, step: CommitStep) -> bool {
        if self.cut == Some(step) {
            self.cut = None;
            self.off = true;
        }
        self.off
    }

    /// The index agrees with one rebuilt from storage.
    fn check_index(&self) {
        #[cfg(test)]
        assert_eq!(
            self.index,
            Index::build(&self.disk),
            "the index drifted from what a remount builds"
        );
    }

    // ─── Archives ────────────────────────────────────────────────────

    fn delete_archives(&mut self, conversation: ConversationCode, message_id: u8) {
        let range = (conversation, message_id, 0)..=(conversation, message_id, u8::MAX);
        let keys: Vec<ArchiveCode> = self
            .disk
            .archives
            .range(range)
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            if let Some(payload) = self.disk.archives.remove(&key) {
                self.index.bytes -= payload.len();
            }
        }
    }

    fn put_archive(&mut self, key: ArchiveCode, payload: &[u8]) {
        if let Some(old) = self.disk.archives.insert(key, payload.to_vec()) {
            self.index.bytes -= old.len();
        }
        self.index.bytes += payload.len();
    }

    // ─── Chats ───────────────────────────────────────────────────────

    fn open_chat(&mut self, conversation: &ConversationKey, name: Option<&str>) -> u32 {
        if let Some(&id) = self.index.chat_for.get(&conversation.into()) {
            return id;
        }
        let id = self.disk.next_chat;
        self.disk.next_chat += 1;
        let activity = self.disk.tick();
        let chat = ChatEntry {
            record: ChatRecord {
                id: ChatId(id),
                conversation: *conversation,
                name: name.map(record::name),
                muted: false,
                unread: 0,
                last_activity: None,
                has_draft: false,
            },
            activity,
            draft: None,
        };
        self.index.add_chat(id, &chat);
        self.disk.chats.insert(id, chat);
        id
    }

    /// A message was kept in `chat` at `time`.
    fn touch_chat(&mut self, chat: u32, time: Option<u32>, unread: bool) {
        let activity = self.disk.tick();
        let Some(entry) = self.disk.chats.get_mut(&chat) else {
            return;
        };
        self.index.by_activity.remove(&entry.activity);
        entry.activity = activity;
        self.index.by_activity.insert(activity, chat);
        if time.is_some() {
            entry.record.last_activity = time;
        }
        if unread {
            entry.record.unread += 1;
        }
    }

    // ─── Messages ────────────────────────────────────────────────────

    /// Change a kept message, keeping the index with it.
    fn update_message(&mut self, id: u32, change: impl FnOnce(&mut MessageEntry)) -> bool {
        let Some(mut message) = self.disk.messages.remove(&id) else {
            return false;
        };
        self.index.remove_message(id, &message);
        change(&mut message);
        self.index.add_message(id, &message);
        self.disk.messages.insert(id, message);
        true
    }

    /// Remove a message, and its resend material with it unless a newer
    /// message now holds its wire ID.
    fn remove_message(&mut self, id: u32) {
        let Some(message) = self.disk.messages.get(&id) else {
            return;
        };
        if message.record.direction == Direction::Outbound
            && let Some(alias) = alias_code(message)
            && self.index.aliases.get(&alias).and_then(BTreeSet::last) == Some(&id)
        {
            self.delete_archives(alias.0, alias.2);
        }
        if let Some(message) = self.disk.messages.remove(&id) {
            self.index.remove_message(id, &message);
        }
    }

    fn record(&self, id: u32) -> Option<MessageRecord> {
        let message = self.disk.messages.get(&id)?;
        let mut record = message.record.clone();
        record.reactions = reactions(&message.reactors);
        Some(record)
    }

    fn resolve(
        &self,
        batch: &Batch<'_>,
        committed: &Committed,
        handle: umsh_text::engine::sequence::MessageHandle,
    ) -> Option<u32> {
        committed
            .id_of(handle)
            .or_else(|| batch.resolve(handle))
            .map(|id| id.0)
            .filter(|id| self.disk.messages.contains_key(id))
    }

    /// The message `reference` names in `conversation`.
    ///
    /// A one-to-one Regarding is one octet, a wire ID with no stream, so
    /// the engine leaves it unresolved, scoped to the peer, when the ID
    /// could be either party's. With `either_party`, such a reference
    /// matches the newest message with that ID in either stream. An edit
    /// or delete never does: its original is always its own sender's.
    fn resolve_ref(
        &self,
        batch: &Batch<'_>,
        committed: &Committed,
        reference: &ResolvedRef,
        conversation: &ConversationKey,
        either_party: bool,
    ) -> Option<u32> {
        match reference {
            ResolvedRef::Handle(handle) => self.resolve(batch, committed, *handle),
            ResolvedRef::Unresolved(WireRef::SenderScoped { sender, message_id }) => {
                let alias = WireAlias {
                    conversation: *conversation,
                    sender: *sender,
                    epoch: None,
                    wire_id: *message_id,
                };
                let one_to_one = matches!(
                    conversation,
                    ConversationKey::Direct { .. } | ConversationKey::ChannelDirect { .. }
                );
                let ours = (either_party && one_to_one && *sender != SenderScope::Local)
                    .then(|| {
                        self.resolve_wire(&WireAlias {
                            sender: SenderScope::Local,
                            ..alias
                        })
                    })
                    .flatten();
                self.resolve_wire(&alias).max(ours).map(|id| id.0)
            }
            // A room numbers its messages itself, whoever sent them.
            ResolvedRef::Unresolved(WireRef::RoomCanonical { message_id }) => {
                let conversation = ConversationCode::from(conversation);
                self.index
                    .aliases
                    .range(
                        (conversation, SenderCode::MIN, 0)
                            ..=(conversation, SenderCode::MAX, u8::MAX),
                    )
                    .filter(|((_, _, wire_id), _)| wire_id == message_id)
                    .filter_map(|(_, ids)| ids.last().copied())
                    .max()
            }
        }
    }

    fn apply(&mut self, batch: &Batch<'_>) -> Committed {
        let mut committed = Committed::default();
        for change in batch.changes() {
            match change {
                Change::Message {
                    mutation,
                    body,
                    sender_handle,
                    link,
                } => self.mutate(batch, &mut committed, mutation, body, sender_handle, link),
                Change::Delivery {
                    handle,
                    fragment,
                    state,
                } => {
                    if let Some(id) = self.resolve(batch, &committed, handle)
                        && let Some(message) = self.disk.messages.get_mut(&id)
                    {
                        message.record.delivery.set(fragment, state);
                        committed.touch_message(MessageId(id));
                    }
                }
            }
        }
        committed
    }

    fn mutate(
        &mut self,
        batch: &Batch<'_>,
        committed: &mut Committed,
        mutation: &MessageMutation,
        body: &str,
        sender_handle: Option<&str>,
        link: Option<LinkInfo>,
    ) {
        let revision = mutation.revision;
        match mutation.kind {
            MutationKind::Insert {
                conversation,
                sender,
                direction,
                message_type,
                wire_id,
                epoch,
                regarding,
                bg_color,
                text_color,
                status,
                presence,
                late,
                notify,
                ..
            } => {
                let regarding_id = regarding.and_then(|reference| {
                    self.resolve_ref(batch, committed, &reference, &conversation, true)
                });
                // A slot already kept under this handle is filled in place.
                if let Some(id) = self.resolve(batch, committed, mutation.handle) {
                    let mut noticed = false;
                    let mut shown = false;
                    self.update_message(id, |message| {
                        if revision <= message.revision {
                            return;
                        }
                        let record = &mut message.record;
                        noticed = notify && !record.flags.notify && direction == Direction::Inbound;
                        shown =
                            presence == Presence::Present && record.presence != Presence::Present;
                        message.revision = revision;
                        message.body = record::clip(body, BODY_MAX).into();
                        record.sender = sender;
                        record.sender_handle = sender_handle.map(record::name);
                        record.wire_id = wire_id;
                        record.epoch = epoch;
                        record.message_type = message_type;
                        record.regarding = regarding_id.map(MessageId);
                        record.body_len = message.body.len() as u16;
                        record.bg_color = bg_color;
                        record.text_color = text_color;
                        record.status = status;
                        record.presence = presence;
                        record.flags.late |= late;
                        record.flags.notify |= notify;
                        record.link = link.or(record.link);
                        record.time = record.time.or(batch.time());
                    });
                    if let Some(chat) = self.disk.messages.get(&id).map(|m| m.record.chat.0)
                        && (noticed || shown)
                    {
                        self.touch_chat(chat, batch.time(), noticed);
                        committed.touch_chat(ChatId(chat));
                    }
                    committed.touch_message(MessageId(id));
                    return;
                }

                // A reaction folds into what it is about.
                if message_type == MessageType::Status && regarding.is_some() {
                    if let Some(target) = regarding_id {
                        let who = SenderCode::from(&sender);
                        let reaction = Reaction::from_body(body);
                        self.update_message(target, |message| {
                            message.reactors.retain(|reactor| reactor.who != who);
                            if let Some(reaction) = reaction {
                                message.reactors.push(Reactor {
                                    who,
                                    mine: direction == Direction::Outbound,
                                    reaction,
                                });
                            }
                        });
                        committed.touch_message(MessageId(target));
                    }
                    return;
                }

                let chat = self.open_chat(&conversation, None);
                let id = self.disk.next_message;
                self.disk.next_message += 1;
                let body = record::clip(body, BODY_MAX);
                let message = MessageEntry {
                    record: MessageRecord {
                        id: MessageId(id),
                        chat: ChatId(chat),
                        direction,
                        sender,
                        sender_handle: sender_handle.map(record::name),
                        wire_id,
                        epoch,
                        message_type,
                        regarding: regarding_id.map(MessageId),
                        body_len: body.len() as u16,
                        bg_color,
                        text_color,
                        status,
                        presence,
                        flags: MessageFlags {
                            late,
                            notify,
                            ..MessageFlags::default()
                        },
                        delivery: Delivery::default(),
                        link,
                        time: batch.time(),
                        reactions: Reactions::new(),
                    },
                    conversation,
                    body: body.into(),
                    revision,
                    reactors: Vec::new(),
                };
                self.index.add_message(id, &message);
                self.disk.messages.insert(id, message);
                if presence == Presence::Present {
                    let noticed = notify && direction == Direction::Inbound;
                    self.touch_chat(chat, batch.time(), noticed);
                }
                committed.inserted.push((mutation.handle, MessageId(id)));
                committed.touch_message(MessageId(id));
                committed.touch_chat(ChatId(chat));
            }
            MutationKind::UpdateBody {
                status,
                late,
                notify,
                ..
            } => {
                let Some(id) = self.resolve(batch, committed, mutation.handle) else {
                    return;
                };
                let mut noticed = false;
                self.update_message(id, |message| {
                    if revision <= message.revision {
                        return;
                    }
                    let record = &mut message.record;
                    noticed =
                        notify && !record.flags.notify && record.direction == Direction::Inbound;
                    message.revision = revision;
                    message.body = record::clip(body, BODY_MAX).into();
                    record.body_len = message.body.len() as u16;
                    record.status = status;
                    record.flags.late |= late;
                    record.flags.notify |= notify;
                });
                if noticed && let Some(chat) = self.disk.messages.get(&id).map(|m| m.record.chat.0)
                {
                    self.touch_chat(chat, batch.time(), true);
                    committed.touch_chat(ChatId(chat));
                }
                committed.touch_message(MessageId(id));
            }
            MutationKind::Edit {
                conversation,
                original,
                ..
            } => {
                let Some(id) = self.resolve_ref(batch, committed, &original, &conversation, false)
                else {
                    return;
                };
                self.update_message(id, |message| {
                    message.body = record::clip(body, BODY_MAX).into();
                    message.record.body_len = message.body.len() as u16;
                    message.record.flags.edited = true;
                });
                committed.touch_message(MessageId(id));
            }
            MutationKind::Delete {
                conversation,
                original,
            } => {
                let Some(id) = self.resolve_ref(batch, committed, &original, &conversation, false)
                else {
                    return;
                };
                let held = self
                    .disk
                    .messages
                    .get(&id)
                    .is_some_and(|message| message.record.presence == Presence::GapPending);
                if held {
                    // A slot held for a message that never came, and never
                    // will: nothing was shown, so nothing stays.
                    self.remove_message(id);
                } else {
                    // The tombstone stays as long as its wire alias does.
                    self.update_message(id, |message| {
                        message.body.clear();
                        message.record.body_len = 0;
                        message.record.flags.deleted = true;
                        message.reactors.clear();
                    });
                }
                committed.touch_message(MessageId(id));
            }
        }
    }

    /// Make room: the oldest messages go first, across every chat, except
    /// each chat's newest.
    fn evict(&mut self) {
        while self.index.bytes > self.quota.bytes {
            let keep = self.quota.keep_per_chat;
            let victim = self.disk.messages.iter().find_map(|(&id, message)| {
                let held = self
                    .index
                    .chat_messages
                    .get(&message.record.chat.0)
                    .map_or(0, BTreeSet::len);
                (held > keep).then_some(id)
            });
            let Some(victim) = victim else {
                break;
            };
            self.remove_message(victim);
        }
    }

    // ─── Peers ───────────────────────────────────────────────────────

    /// Forget the heard peers beyond the quota, least recently heard first.
    fn forget_unheard(&mut self) {
        while self.index.heard.len() > self.quota.heard_peers {
            let Some((_, key)) = self.index.heard.pop_first() else {
                break;
            };
            if let Some(peer) = self.disk.peers.remove(&key) {
                self.index.remove_peer(&key, &peer);
            }
        }
    }
}

impl ChatStore for MemoryStore {
    fn commit(&mut self, batch: &Batch<'_>) -> Result<Committed, StoreError> {
        self.writable()?;
        for checkpoint in batch.checkpoints() {
            let used = self.disk.tick();
            self.disk
                .checkpoints
                .insert((&checkpoint.conversation).into(), (*checkpoint, used));
        }
        if self.reached(CommitStep::Checkpoint) {
            return Err(StoreError::Io);
        }
        for (conversation, message_id) in batch.archive_deletes() {
            self.delete_archives(conversation.into(), *message_id);
        }
        if self.reached(CommitStep::ArchiveDeletes) {
            return Err(StoreError::Io);
        }
        for (key, payload) in batch.archives() {
            self.put_archive(archive_code(key), payload);
        }
        if self.reached(CommitStep::Archives) {
            return Err(StoreError::Io);
        }
        let committed = self.apply(batch);
        self.evict();
        self.check_index();
        // Power lost after the transcript loses nothing: the commit is
        // whole.
        self.reached(CommitStep::Transcript);
        Ok(committed)
    }

    fn messages(
        &self,
        chat: ChatId,
        before: Option<MessageId>,
    ) -> impl Iterator<Item = Result<MessageRecord, StoreError>> + '_ {
        let upper = before.map_or(Bound::Unbounded, |before| Bound::Excluded(before.0));
        self.index
            .chat_messages
            .get(&chat.0)
            .into_iter()
            .flat_map(move |ids| ids.range((Bound::Unbounded, upper)).rev())
            .map(|&id| self.record(id).ok_or(StoreError::Corrupt))
    }

    fn message(&self, id: MessageId) -> Result<Option<MessageRecord>, StoreError> {
        Ok(self.record(id.0))
    }

    fn body<'b>(&self, id: MessageId, out: &'b mut [u8]) -> Result<Option<&'b str>, StoreError> {
        let Some(message) = self.disk.messages.get(&id.0) else {
            return Ok(None);
        };
        copy_text(&message.body, out).map(Some)
    }

    fn resolve_wire(&self, alias: &WireAlias) -> Option<MessageId> {
        let code: AliasCode = (
            (&alias.conversation).into(),
            (&alias.sender).into(),
            alias.wire_id,
        );
        self.index
            .aliases
            .get(&code)?
            .iter()
            .rev()
            .find(|id| {
                alias.epoch.is_none_or(|epoch| {
                    self.disk
                        .messages
                        .get(id)
                        .is_some_and(|message| message.record.epoch == epoch)
                })
            })
            .map(|&id| MessageId(id))
    }

    fn archive<'b>(
        &self,
        key: &ArchiveKey,
        out: &'b mut [u8],
    ) -> Result<Option<&'b [u8]>, StoreError> {
        let Some(payload) = self.disk.archives.get(&archive_code(key)) else {
            return Ok(None);
        };
        let out = out
            .get_mut(..payload.len())
            .ok_or(StoreError::BufferTooSmall)?;
        out.copy_from_slice(payload);
        Ok(Some(out))
    }

    fn checkpoint(&self, conversation: &ConversationKey) -> Option<StreamCheckpoint> {
        self.disk
            .checkpoints
            .get(&conversation.into())
            .map(|(checkpoint, _)| *checkpoint)
    }

    fn recent_checkpoints(&self, limit: usize) -> impl Iterator<Item = StreamCheckpoint> + '_ {
        let mut used: Vec<(u64, StreamCheckpoint)> = self
            .disk
            .checkpoints
            .values()
            .map(|(checkpoint, used)| (*used, *checkpoint))
            .collect();
        used.sort_unstable_by_key(|(used, _)| *used);
        let skip = used.len().saturating_sub(limit);
        used.into_iter()
            .skip(skip)
            .map(|(_, checkpoint)| checkpoint)
    }

    fn chats(&self) -> impl Iterator<Item = Result<ChatRecord, StoreError>> + '_ {
        self.index.by_activity.values().rev().map(|id| {
            self.disk
                .chats
                .get(id)
                .map(|chat| chat.record.clone())
                .ok_or(StoreError::Corrupt)
        })
    }

    fn chat_for(&self, conversation: &ConversationKey) -> Option<ChatId> {
        self.index
            .chat_for
            .get(&conversation.into())
            .map(|&id| ChatId(id))
    }

    fn draft<'b>(&self, chat: ChatId, out: &'b mut [u8]) -> Result<Option<&'b str>, StoreError> {
        let Some(draft) = self
            .disk
            .chats
            .get(&chat.0)
            .and_then(|chat| chat.draft.as_ref())
        else {
            return Ok(None);
        };
        copy_text(draft, out).map(Some)
    }

    fn update_chat(&mut self, chat: ChatId, change: ChatChange<'_>) -> Result<ChatId, StoreError> {
        self.writable()?;
        if let ChatChange::Open { conversation, name } = change {
            let id = self.open_chat(&conversation, name);
            self.check_index();
            return Ok(ChatId(id));
        }
        if !self.disk.chats.contains_key(&chat.0) {
            return Err(StoreError::NotFound);
        }
        if let ChatChange::Delete = change {
            let messages: Vec<u32> = self
                .index
                .chat_messages
                .get(&chat.0)
                .into_iter()
                .flatten()
                .copied()
                .collect();
            for id in messages {
                self.remove_message(id);
            }
            if let Some(entry) = self.disk.chats.remove(&chat.0) {
                self.index.remove_chat(chat.0, &entry);
            }
            self.check_index();
            return Ok(chat);
        }
        let Some(entry) = self.disk.chats.get_mut(&chat.0) else {
            return Err(StoreError::NotFound);
        };
        match change {
            ChatChange::Rename(name) => entry.record.name = name.map(record::name),
            ChatChange::Mute(muted) => entry.record.muted = muted,
            ChatChange::Read => entry.record.unread = 0,
            ChatChange::Draft(draft) => {
                entry.draft = draft
                    .filter(|draft| !draft.is_empty())
                    .map(|draft| record::clip(draft, BODY_MAX).into());
                entry.record.has_draft = entry.draft.is_some();
            }
            ChatChange::Open { .. } | ChatChange::Delete => unreachable!(),
        }
        self.check_index();
        Ok(chat)
    }

    fn peer(&self, key: &PublicKey) -> Result<Option<PeerRecord>, StoreError> {
        Ok(self.disk.peers.get(&key.0).map(|peer| peer.record.clone()))
    }

    fn peers_with_hint(&self, hint: NodeHint) -> impl Iterator<Item = PublicKey> + '_ {
        self.index
            .hints
            .get(&hint.0)
            .into_iter()
            .flatten()
            .map(|key| PublicKey(*key))
    }

    fn peers(
        &self,
        filter: PeerFilter,
    ) -> impl Iterator<Item = Result<PeerRecord, StoreError>> + '_ {
        let keys: Vec<[u8; 32]> = match filter {
            PeerFilter::All => self.disk.peers.keys().copied().collect(),
            PeerFilter::Saved => self
                .disk
                .peers
                .iter()
                .filter(|(_, peer)| peer.record.saved)
                .map(|(key, _)| *key)
                .collect(),
            PeerFilter::Heard => self.index.heard.values().rev().copied().collect(),
        };
        keys.into_iter().map(|key| {
            self.disk
                .peers
                .get(&key)
                .map(|peer| peer.record.clone())
                .ok_or(StoreError::Corrupt)
        })
    }

    fn update_peer(
        &mut self,
        key: &PublicKey,
        change: PeerChange<'_>,
    ) -> Result<PeerId, StoreError> {
        self.writable()?;
        let existing = self.disk.peers.remove(&key.0);
        if let Some(peer) = &existing {
            self.index.remove_peer(&key.0, peer);
        }
        let mut peer = match (existing, change) {
            (Some(peer), PeerChange::Forget) => {
                self.check_index();
                return Ok(peer.record.id);
            }
            (Some(peer), _) => peer,
            (None, PeerChange::Heard { .. } | PeerChange::Save { .. }) => {
                let id = PeerId(self.disk.next_peer);
                self.disk.next_peer += 1;
                PeerEntry {
                    record: PeerRecord {
                        id,
                        key: *key,
                        name: None,
                        identity: heapless::Vec::new(),
                        last_heard: None,
                        route: None,
                        forward_secrecy: false,
                        saved: false,
                    },
                    heard: 0,
                }
            }
            (None, _) => return Err(StoreError::NotFound),
        };
        match change {
            PeerChange::Heard { at, name, identity } => {
                peer.heard = self.disk.tick();
                peer.record.last_heard = at.or(peer.record.last_heard);
                // What it calls itself, unless someone here named it.
                if let Some(name) = name
                    && !(peer.record.saved && peer.record.name.is_some())
                {
                    peer.record.name = Some(record::name(name));
                }
                if let Some(identity) = identity
                    && identity.len() <= IDENTITY_MAX
                {
                    peer.record.identity = heapless::Vec::from_slice(identity).unwrap_or_default();
                }
            }
            PeerChange::Route(route) => {
                peer.record.route = route.map(|hints| {
                    heapless::Vec::from_slice(&hints[..hints.len().min(ROUTE_MAX)])
                        .unwrap_or_default()
                });
            }
            PeerChange::Save { name } => {
                peer.record.saved = true;
                if let Some(name) = name {
                    peer.record.name = Some(record::name(name));
                }
            }
            PeerChange::Unsave => {
                peer.record.saved = false;
                peer.heard = self.disk.tick();
            }
            PeerChange::Rename(name) => peer.record.name = name.map(record::name),
            PeerChange::ForwardSecrecy(preferred) => peer.record.forward_secrecy = preferred,
            PeerChange::Forget => unreachable!(),
        }
        let id = peer.record.id;
        self.index.add_peer(&key.0, &peer);
        self.disk.peers.insert(key.0, peer);
        self.forget_unheard();
        self.check_index();
        Ok(id)
    }

    fn channels(&self) -> impl Iterator<Item = Result<ChannelRecord, StoreError>> + '_ {
        self.disk.channels.values().cloned().map(Ok)
    }

    fn update_channel(
        &mut self,
        tag: &ChannelTag,
        change: ChannelChange<'_>,
    ) -> Result<(), StoreError> {
        self.writable()?;
        match change {
            ChannelChange::Add {
                name,
                access,
                builtin,
            } => {
                self.disk.channels.insert(
                    tag.0,
                    ChannelRecord {
                        tag: *tag,
                        name: record::name(name),
                        access,
                        builtin,
                    },
                );
            }
            ChannelChange::Rename(name) => {
                self.disk
                    .channels
                    .get_mut(&tag.0)
                    .ok_or(StoreError::NotFound)?
                    .name = record::name(name);
            }
            ChannelChange::Remove => {
                self.disk
                    .channels
                    .remove(&tag.0)
                    .ok_or(StoreError::NotFound)?;
            }
        }
        Ok(())
    }

    fn usage(&self) -> Usage {
        Usage {
            chats: self.disk.chats.len(),
            messages: self.disk.messages.len(),
            peers: self.disk.peers.len(),
            channels: self.disk.channels.len(),
            archives: self.disk.archives.len(),
            checkpoints: self.disk.checkpoints.len(),
            bytes: self.index.bytes,
            quota: self.quota.bytes,
        }
    }

    fn erase(&mut self) -> Result<(), StoreError> {
        self.writable()?;
        self.disk = Disk {
            next_chat: self.disk.next_chat,
            next_message: self.disk.next_message,
            next_peer: self.disk.next_peer,
            clock: self.disk.clock,
            ..Disk::default()
        };
        self.index = Index::build(&self.disk);
        Ok(())
    }
}

fn copy_text<'b>(text: &str, out: &'b mut [u8]) -> Result<&'b str, StoreError> {
    let out = out
        .get_mut(..text.len())
        .ok_or(StoreError::BufferTooSmall)?;
    out.copy_from_slice(text.as_bytes());
    core::str::from_utf8(out).map_err(|_| StoreError::Corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::{self, Harness};

    struct Memory;

    impl Harness for Memory {
        type Store = MemoryStore;

        fn fresh(&mut self) -> MemoryStore {
            MemoryStore::new()
        }

        fn cut_power_after(&mut self, store: &mut MemoryStore, step: CommitStep) {
            store.cut_power_after(step);
        }

        fn power_on(&mut self, store: &mut MemoryStore) {
            store.power_on();
        }
    }

    use crate::conformance::{Rig, archived, direct, group, record};

    /// What one short message costs against the quota, its archive
    /// included.
    fn cost_of_one() -> usize {
        let mut store = MemoryStore::new();
        Rig::new().say(&mut store, group(), "busy 000");
        store.usage().bytes
    }

    #[test]
    fn eviction_takes_the_oldest_across_chats_but_each_keeps_its_newest() {
        let cost = cost_of_one();
        let mut store = MemoryStore::with_quota(Quota {
            bytes: 80 * cost,
            ..Quota::DEFAULT
        });
        let mut rig = Rig::new();
        let quiet: Vec<MessageId> = (0..10)
            .map(|n| {
                rig.say(&mut store, direct(), &alloc::format!("calm {n:03}"))
                    .1
            })
            .collect();
        let mut first_wire = None;
        let busy: Vec<MessageId> = (0..200)
            .map(|n| {
                let id = rig
                    .say(&mut store, group(), &alloc::format!("busy {n:03}"))
                    .1;
                first_wire = first_wire.or(record(&store, id).wire_id);
                id
            })
            .collect();

        // The quiet chat is older than all of it, and loses nothing.
        for &id in &quiet {
            assert!(store.message(id).unwrap().is_some());
        }
        // The busy chat lost its oldest, and only as much as it had to.
        let usage = store.usage();
        assert!(usage.bytes <= usage.quota);
        let kept = store
            .messages(store.chat_for(&group()).unwrap(), None)
            .count();
        assert!((60..=70).contains(&kept), "{kept}");
        let oldest = busy[200 - kept];
        assert!(store.message(oldest).unwrap().is_some());
        assert!(store.message(busy[199 - kept]).unwrap().is_none());

        // An evicted message's resend material went with it.
        assert_eq!(archived(&store, group(), first_wire.unwrap()), None);
        let newest_wire = record(&store, busy[199]).wire_id.unwrap();
        assert!(archived(&store, group(), newest_wire).is_some());
        assert!(store.checkpoint(&group()).is_some());
    }

    #[test]
    fn no_chat_is_cut_below_its_newest() {
        let cost = cost_of_one();
        let mut store = MemoryStore::with_quota(Quota {
            bytes: 10 * cost,
            ..Quota::DEFAULT
        });
        let mut rig = Rig::new();
        for n in 0..20 {
            rig.say(&mut store, direct(), &alloc::format!("calm {n:03}"));
        }
        for n in 0..120 {
            rig.say(&mut store, group(), &alloc::format!("busy {n:03}"));
        }
        let count = |conversation| {
            store
                .messages(store.chat_for(&conversation).unwrap(), None)
                .count()
        };
        // Over quota, and still each chat keeps its newest 50.
        assert_eq!(count(direct()), 20);
        assert_eq!(count(group()), 50);
        assert!(store.usage().bytes > store.usage().quota);
    }

    #[test]
    fn transcript() {
        conformance::transcript(&mut Memory);
    }

    #[test]
    fn paging() {
        conformance::paging(&mut Memory);
    }

    #[test]
    fn delivery() {
        conformance::delivery(&mut Memory);
    }

    #[test]
    fn wire_aliases() {
        conformance::wire_aliases(&mut Memory);
    }

    #[test]
    fn reactions() {
        conformance::reactions(&mut Memory);
    }

    #[test]
    fn edits_and_deletes() {
        conformance::edits_and_deletes(&mut Memory);
    }

    #[test]
    fn chats() {
        conformance::chats(&mut Memory);
    }

    #[test]
    fn peers() {
        conformance::peers(&mut Memory);
    }

    #[test]
    fn channels() {
        conformance::channels(&mut Memory);
    }

    #[test]
    fn buffers() {
        conformance::buffers(&mut Memory);
    }

    #[test]
    fn remount() {
        conformance::remount(&mut Memory);
    }

    #[test]
    fn power_cuts() {
        conformance::power_cuts(&mut Memory);
    }

    #[test]
    fn erase() {
        conformance::erase(&mut Memory);
    }
}
