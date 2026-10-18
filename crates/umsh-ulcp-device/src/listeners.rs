//! What one listener's request means for the others.
//!
//! A device answers whoever asked and publishes to everyone listening: an
//! attached host, and its own interface. What one of them changes reaches
//! the rest as publications (spec §Attach, Detach, and Synchronization),
//! and a change made at the device keeps an existing snapshot in step
//! (spec §Changes Made at the Device). The session answers; whoever
//! carries its frames uses this to tell the others.

use umsh_ulcp::Frame;
use umsh_ulcp::frame::{
    Cmd, MultiEntries, PropPayload, TID_UNSOLICITED, prop_inserted, prop_is, prop_removed,
};
use umsh_ulcp::ids::prop;

use crate::session::{MULTI_MAX, is_saved};

/// What a request changes, as far as its carrier's bookkeeping goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Writes {
    /// It sets, inserts, or removes properties.
    pub any: bool,
    /// One of them is part of the saved snapshot.
    pub saved: bool,
}

impl Writes {
    pub fn of(request: &[u8]) -> Self {
        let mut writes = Self::default();
        let Ok(frame) = Frame::parse(request) else {
            return writes;
        };
        let mut note = |key: u32| {
            writes.any = true;
            writes.saved |= is_saved(key);
        };
        match frame.command() {
            Some(Cmd::PropSet | Cmd::PropInsert | Cmd::PropRemove) => {
                if let Ok(payload) = PropPayload::parse(frame.payload) {
                    note(payload.key);
                }
            }
            Some(Cmd::PropMultiSet) => {
                for entry in MultiEntries::new(frame.payload).flatten() {
                    note(entry.key);
                }
            }
            _ => {}
        }
        writes
    }
}

/// The publications that tell everyone else listening what the answer to
/// a write tells whoever made it: each property it reports, one at a time
/// and unsolicited, as any change the listener did not make reaches it. A
/// status is the requester's alone.
///
/// A write sent with TID 0 is answered with nothing, so it is told to no
/// one.
pub struct Mirrors<'a> {
    answer: Option<Frame<'a>>,
    /// The entries of a `CMD_PROP_ARE` not yet told.
    entries: Option<MultiEntries<'a>>,
    buf: [u8; MULTI_MAX],
}

impl<'a> Mirrors<'a> {
    pub fn new(answer: &'a [u8]) -> Self {
        let answer = Frame::parse(answer).ok();
        let entries = answer
            .filter(|frame| frame.command() == Some(Cmd::PropAre))
            .map(|frame| MultiEntries::new(frame.payload));
        Self {
            answer,
            entries,
            buf: [0; MULTI_MAX],
        }
    }

    /// The next publication, until there are none.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&[u8]> {
        if let Some(entries) = &mut self.entries {
            for entry in entries.by_ref() {
                let entry = entry.ok()?;
                if entry.key == prop::LAST_STATUS {
                    continue;
                }
                let len = prop_is(&mut self.buf, TID_UNSOLICITED, entry.key, entry.value).ok()?;
                return Some(&self.buf[..len]);
            }
            return None;
        }
        let answer = self.answer.take()?;
        let payload = PropPayload::parse(answer.payload).ok()?;
        if payload.key == prop::LAST_STATUS {
            return None;
        }
        let build = match answer.command()? {
            Cmd::PropIs => prop_is,
            Cmd::PropInserted => prop_inserted,
            Cmd::PropRemoved => prop_removed,
            _ => return None,
        };
        let len = build(&mut self.buf, TID_UNSOLICITED, payload.key, payload.value).ok()?;
        Some(&self.buf[..len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umsh_ulcp::Status;
    use umsh_ulcp::frame;

    fn told(frame: &[u8]) -> (u8, u32, std::vec::Vec<u8>) {
        let parsed = Frame::parse(frame).unwrap();
        let payload = PropPayload::parse(parsed.payload).unwrap();
        (parsed.header.tid(), payload.key, payload.value.to_vec())
    }

    #[test]
    fn writes_are_told_apart_from_reads() {
        let mut buf = [0u8; 32];
        let len = frame::prop_get(&mut buf, 1, prop::DEV_NAME).unwrap();
        assert_eq!(Writes::of(&buf[..len]), Writes::default());
        let len = frame::prop_set(&mut buf, 1, prop::DEV_NAME, b"A").unwrap();
        assert_eq!(
            Writes::of(&buf[..len]),
            Writes {
                any: true,
                saved: true
            }
        );
        let len = frame::prop_set(&mut buf, 1, prop::ALERT, &[1]).unwrap();
        assert_eq!(
            Writes::of(&buf[..len]),
            Writes {
                any: true,
                saved: false
            }
        );
        let len = frame::prop_multi_set(
            &mut buf,
            1,
            &[(prop::ALERT, &[1]), (prop::PHY_TX_POWER, &[9])],
        )
        .unwrap();
        assert_eq!(
            Writes::of(&buf[..len]),
            Writes {
                any: true,
                saved: true
            }
        );
    }

    #[test]
    fn an_answer_is_told_without_its_status() {
        let mut answer = [0u8; 64];
        let len = frame::prop_is(&mut answer, 3, prop::DEV_NAME, b"Pocket").unwrap();
        let mut mirrors = Mirrors::new(&answer[..len]);
        assert_eq!(
            told(mirrors.next().unwrap()),
            (0, prop::DEV_NAME, b"Pocket".to_vec())
        );
        assert!(mirrors.next().is_none());

        let len = frame::last_status(&mut answer, 3, Status::INVALID_ARGUMENT).unwrap();
        assert!(Mirrors::new(&answer[..len]).next().is_none());

        let mut writer = frame::prop_are(&mut answer, 4).unwrap();
        writer.write_entry(prop::PHY_LORA_SF, &[9]).unwrap();
        writer.write_status_entry(Status::INVALID_ARGUMENT).unwrap();
        let len = writer.finish();
        let mut mirrors = Mirrors::new(&answer[..len]);
        assert_eq!(
            told(mirrors.next().unwrap()),
            (0, prop::PHY_LORA_SF, vec![9])
        );
        assert!(mirrors.next().is_none());
    }
}
