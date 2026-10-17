//! Stand-in content until a real node backs the interface: the chats from
//! the T-LoRa Pager mockup, so a render compares directly with it.

use crate::avatar::{AvatarStyle, BuiltinChannel, PeerShape};
use crate::model::{ChatSummary, State, StatusStrip};
use heapless::String;

fn s<const N: usize>(text: &str) -> String<N> {
    String::try_from(text).expect("fixture text fits")
}

fn chat(
    avatar: AvatarStyle,
    name: &str,
    preview: &str,
    age: &str,
    unread: bool,
    muted: bool,
    draft: bool,
) -> ChatSummary {
    ChatSummary {
        avatar,
        name: s(name),
        preview: s(preview),
        age: s(age),
        unread,
        muted,
        draft,
    }
}

pub fn state() -> State {
    let mut state = State {
        status: StatusStrip {
            device_name: s("Juniper"),
            time: s("14:32"),
            battery_segments: 3,
            charging: false,
            phone_linked: true,
            gnss: true,
            silent: false,
        },
        ..State::default()
    };
    let chats = [
        chat(
            AvatarStyle::peer([235, 187, 21], "kBkQ", PeerShape::Circle),
            "Ridge Medic",
            "On my way, save me some hot coffee",
            "2m",
            false,
            false,
            true,
        ),
        chat(
            AvatarStyle::channel(None, "Trail Crew", [203, 24, 112]),
            "Trail Crew",
            "Sam: Will wait 10 min",
            "9m",
            true,
            false,
            false,
        ),
        chat(
            AvatarStyle::peer([179, 246, 158], "YmRG", PeerShape::Circle),
            "Pat Okafor",
            "You: Bring the spare antenna?",
            "41m",
            false,
            false,
            false,
        ),
        chat(
            AvatarStyle::channel(Some(BuiltinChannel::Public), "public", [0, 0, 0]),
            "public",
            "Kit: Anyone near Mill Creek?",
            "1h",
            false,
            true,
            false,
        ),
        chat(
            AvatarStyle::peer([94, 10, 224], "H1x*", PeerShape::Circle),
            "Jo Reyes",
            "Thanks!",
            "3h",
            false,
            false,
            false,
        ),
    ];
    for c in chats {
        let _ = state.chats.push(c);
    }
    state
}
