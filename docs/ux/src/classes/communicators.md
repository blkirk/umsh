# Communicators

Communicators are self-contained UMSH clients. A phone is never part of the
critical path for setup, reading, composing, sending, status, settings, or
recovery. Their default screen should answer:

- What conversation or activity needs attention?
- Is the radio/device connected and able to send?
- What is the battery state?
- What will the primary control do now?

## Proposed information architecture

1. **Home / Inbox**—conversations, unread state, delivery summary
2. **Conversation**—message history and compose entry
3. **Nodes / Contacts**—reachable identities and details
4. **Channels**—membership and channel activity
5. **Device status**—radio, location, battery, storage, companion state
6. **Settings**—notifications, display, input, radio policy
7. **Maintenance**—pairing, diagnostics, firmware update, destructive resets

Maintenance actions should not compete visually with messaging.

## Composition

- Preserve drafts when leaving a conversation accidentally.
- Make recipient/channel and Send state visible while composing.
- Distinguish queued, transmitted, acknowledged, delivered (if the protocol
  can prove it), and failed. Never invent a stronger delivery claim.
- Provide an explicit cancel path for queued work when technically possible.

## Keyboard and auxiliary controls

Use hardware navigation controls for focus and scrolling; use the keyboard for
text. Rotary encoders, trackballs, or touch can accelerate navigation but must
not be the only path when the keyboard contains usable navigation keys.

A press acts when the wheel comes back up. Holding it down instead opens
what is focused's menu where it has one: a direct chat's offers Show peer
and Delete, which asks to confirm.

Haptic or key-click feedback should be short, optional, and rate-limited. A
speaker is for notifications and media-quality tones, not confirmation for
every keystroke by default.

Modifiers, typing aids, the character picker, and the keyboard light are in
[Communicator Keyboard](communicator-keyboard.md).

## Locking

A communicator locks when its screen times out and a second passes without
input, or at once on a short press of the lock button, which shows the lock
screen. A dark screen is always a locked one. On the lock screen, the lock
button darkens it; while dark, it wakes to the lock screen. Holding it still
shuts down.

- **The lock screen** shows the device name, the time and date when the clock
  is known, the unread count, and one card per chat heard from since locking.
- **Unlocking** is five wheel steps down, which fill a bar, then a press within
  2 s. Turning up empties the bar; keys do nothing. Unlocking returns to
  where the user was.
- **While locked**, the screen stays lit 15 s, then fades and goes dark. Every
  message that would alert lights it again, with its tick, and restarts the
  15 s.

## Clock

**No time or date is shown anywhere until the clock is known.** Ages measured
since power-on are not read off the clock and may still show.

- **Time zones** are POSIX TZ rules the device keeps, so daylight saving needs
  no host. The list names zones ("Pacific Time"), not cities, west to east,
  with today's offset.
- **First run** asks for the zone after the radio profile, even with a GNSS
  fix: GNSS gives the time, not the zone.
- **Device › Clock** holds whether GNSS sets the time, the date and time set by
  hand, the zone, 12- or 24-hour time, and where the time last came from.
- **Every time shown** is in the offset its zone kept at that instant, so a
  message from before a daylight-saving change keeps its own hour.

## Optional phone integration

A communicator may optionally pair with a phone for synchronization, backup,
maps, bulk configuration, or accessibility. That connection is an enhancement,
not the communicator's primary UI and not a prerequisite for UMSH messaging.

If phone integration exists, pairing and bond management are visible
Maintenance actions. A communicator with a screen must not require users to
memorize click counts to clear trust or enter pairing. Loss of the phone or its
bond must not impair standalone communicator operation.
