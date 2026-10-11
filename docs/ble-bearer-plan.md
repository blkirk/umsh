# BLE Bearer—Research and Implementation Plan

Let nearby UMSH devices exchange frames over Bluetooth LE without pairing or
connections, so that a device with the feature enabled bridges between that
local BLE space and its LoRa mesh. Two devices on different LoRa frequencies,
placed near each other with the feature on, join their meshes through the BLE
space between them.

This document is a plan, not a protocol specification. None of the proposed
identifiers or behavior is implemented yet. The protocol changes are collected
in [Protocol changes](#protocol-changes) so they can be reviewed as a unit.

## Goals

- **Bridge mode.** A LoRa node with the bearer enabled exports what it
  forwards and originates to the BLE space, and forwards what it hears there
  onto LoRa. No server, no configuration beyond one toggle.
- **Frequency bridging.** Two bridges on different LoRa channels in BLE range
  of each other join the two meshes. Any number of bridges may share one BLE
  space, on the same channel or on different ones, without collisions or
  duplicate airtime on LoRa.
- **Reuse the forwarding engine.** Duplicate suppression, hop accounting,
  region policy, trace, and the duty ceiling apply to bridged traffic exactly
  as they do to repeated traffic, without a second implementation.
- **Ship on every board that can do it**: all five nRF52840 images and the
  ESP32-S3 device images.

## Non-goals

- **Bluetooth Mesh (SIG) interoperability.** Its 29-octet network PDUs,
  11-octet transport segments, provisioning, and key model duplicate UMSH and
  would carry a 255-octet frame as about a dozen segments.
- **Connection-oriented meshing** (GATT or L2CAP links between neighbors).
  Each link costs roughly 2.5 KB of SDC memory, broadcast becomes N unicasts,
  and discovery still needs advertising. UMSH is flood-native; the bearer
  should be too.
- **Multi-hop relaying inside the BLE space** and **BLE-only nodes**. The
  first increment treats a BLE space as one broadcast domain. Both are
  sketched under [Later work](#later-work) and the design leaves room for
  them.
- **MeshCore frames.** The bearer header reserves room, but only UMSH frames
  cross.
- **iPhones as bearer participants.** iOS cannot transmit arbitrary
  advertising data; phones stay tethered over ULCP.
- **Heltec V2.** Its BT 4.2 controller has legacy advertising only (31 octets).

## Research findings

### Extended advertising carries whole frames

The existing [BLE As A Local Bearer](protocol/src/ulcp.md#ble-as-a-local-bearer)
text treats advertising as too small for full frames. That is true of legacy
advertising and not of BLE 5 extended advertising. A non-connectable,
non-scannable extended advertising set puts a short `ADV_EXT_IND` on the three
primary channels pointing at an `AUX_ADV_IND` on a secondary channel. The
`AUX_ADV_IND` PDU payload is up to 255 octets. Its extended header costs 4
octets for an anonymous set (header length and mode, flags, ADI), or 10 with an
advertiser address, leaving 251 or 245 octets of AdvData. Larger AdvData is
carried in `AUX_CHAIN_IND` PDUs, up to 1650 octets, fragmented by the
controller.

One AD structure holds at most 254 octets after its type octet, so a 255-octet
frame never fits in a single AD structure with any identifier in front of it.
The encoding below splits a frame across AD structures when it must.

Airtime is negligible next to LoRa: a full frame is about 2 ms at LE 1M and
about 17 ms at LE Coded S8, against 0.8–2.2 s for LoRa at typical settings.
The bearer is never the throughput bottleneck; LoRa airtime is.

### A crossing is already defined

The [internet bridge](protocol/src/internet-bridging.md) already defines what
it means to join two meshes through a non-LoRa medium:

- A crossing is two repeater hops: the ingress node forwards, the egress node
  forwards.
- What crosses is the ingress node's rewritten output.
- Egress receives it as an unmeasured backhaul reception.
- The participants' duplicate caches, not the medium, stop circulation.

The integrated client ([ulcp-bridge.md](protocol/src/ulcp-bridge.md#integrated-clients),
`firmware-esp32/firmware/esp32-tracker/src/bridge.rs`) attaches to the radio
multiplexer through `BridgePort` (`crates/umsh-ulcp-runtime/src/radio_mux.rs:98`).
Ingress goes to the node as `RxOrigin::Backhaul`, and egress is the node's
successful transmissions. The BLE bearer follows the same model with three
deliberate departures, each forced by the medium being a shared broadcast space
rather than a point-to-point tunnel:

1. **Ingress must contend on LoRa.** The MAC skips the contention window for
   unmeasured receptions because "a frame handed over a point-to-point link
   was heard by exactly one repeater"
   (`crates/umsh-mac/src/coordinator.rs:5930`). On BLE that is false. Every
   bridge in range hears every bearer frame, and bridges on the same LoRa
   channel would all transmit it at once.
2. **Export must happen at the forwarding decision, not the transmission.** A
   bridge that loses LoRa contention to an ordinary repeater has its forward
   suppressed (`defer_pending_forward`, `coordinator.rs:5999`), so under
   export-on-transmission it would bridge only the frames it happened to win.
   In a dense mesh that is most of them. The integrated internet client has
   the same weakness; see [Risks and decisions](#risks-and-decisions).
3. **Only over-the-air copies may speak for the LoRa channel.** A bearer copy
   does not say which LoRa channel, if any, its sender transmitted on. Today
   any duplicate defers a pending forward, and an unmeasured one reschedules it
   with zero delay. Any copy matching a post-TX listen also ends it
   (`observe_forwarding_confirmation`, called at `coordinator.rs:2688` with no
   origin check). Both are LoRa-channel observations and must come only from
   `RxOrigin::Air`.

### Platform capabilities

| Platform | Controller | Extended adv / scan | Coded PHY | Status |
|---|---|---|---|---|
| nRF52840 (T1000-E, T-Echo, Solar P1, Wio L1, XIAO) | nrf-sdc multirole | Yes | Yes | Library already multirole (`peripheral`+`central` features); nothing enabled |
| ESP32-S3 (Heltec V3, T-LoRa Pager, T-Beam Supreme) | esp-radio BLE 5 via bt-hci | Yes (HCI) | Yes | Not used |
| ESP32 (Heltec V2) | BTDM 4.2 | No | No | Excluded |
| Heltec V3 bridge image | none | — | — | No BLE in the image (RAM); excluded |
| iOS | CoreBluetooth | Scan only | — | Excluded as participant |
| Android | `AdvertisingSet` | Yes | Most devices | Later work |

**nRF52840.**

- **Current SDC build** (`firmware/nrf52-tracker/src/main.rs:1695`):
  - It calls only `support_adv`, `support_peripheral`, `support_le_privacy`
    and `peripheral_count(1)`, with a fixed `sdc::Mem::<8192>`.
  - Builder calls exist for the missing pieces: `support_ext_adv`,
    `support_ext_scan`, `support_le_coded_phy`, `adv_count`,
    `adv_buffer_cfg` and `scan_buffer_cfg`.
  - The memory requirement is never logged today. The SDC reports it
    through `required_memory()`.
- **RAM headroom:** images use 134–145 KiB of 256 KiB.
- **TrouBLE** (fork rev `675edc8`):
  - Already present: `advertise_ext` over several sets, with `max_events`
    and anonymous non-connectable sets (`host/src/advertise.rs:87, 321`).
  - Legacy-PDU sets through the extended commands (`advertise.rs:245`).
  - Extended scanning (`host/src/scan.rs:43`).
  - Raw `on_ext_adv_reports`, delivered through `Runner::run_with_handler`
    (`host/src/host.rs:1123`).
  - Missing: reassembly of fragmented reports (data status "incomplete, more
    to come").
  - The firmware does not enable TrouBLE's `scan` feature.

**HCI rule: no mixing legacy and extended advertising commands.** Once a
host issues an extended advertising command, the controller rejects legacy
ones with *Command Disallowed* until reset. TrouBLE's `advertise()` uses the
legacy commands (`host/src/peripheral.rs:90`), and the ULCP advertiser uses
`advertise()`. So the ULCP advertiser must move to `advertise_ext` with a
legacy-PDU set before any bearer advertising starts. This changes the
reconnect path the iOS app depends on and needs its own hardware pass.

**ESP32-S3.**

- The controller's `ble_50_feat_supp` path provides extended advertising
  and scanning over HCI.
- **RAM:** the BLE controller allocates from the 72 KiB no-Wi-Fi heap. The
  Pager and T-Beam run BLE with Wi-Fi coexistence and only about 1 KB of
  stack-check slack, so they need measurement before the bearer is enabled.

**Power.**

- Continuous extended scanning keeps the 2.4 GHz receiver on: about 5 mA on
  the nRF52840, comparable to the SX1262's own receive current.
- Enabling the bearer therefore roughly doubles a tracker's receive draw.
  It is off by default and aimed at powered or large-battery devices.

## Architecture

```
  LoRa channel X                        BLE space                        LoRa channel Y
 ───────────────┐                                                   ┌───────────────
   SX1262 ── radio mux ── node MAC ──export──▶ bearer TX ··· bearer RX ──▶ radio mux ── node MAC ── SX1262
                    ▲           │                                         (RxOrigin::Bearer)  │
                    └─(RxOrigin::Bearer)── bearer RX ◀··· bearer TX ◀──export──────────────────┘
```

**The node's MAC gains a secondary egress, the bearer.** One forwarding
decision now produces up to two outputs:

- the existing LoRa enqueue, with contention, deferral, and confirmation;
- an immediate bearer export, with none of those.

**Bearer ingress uses the existing `BridgePort` hand-off.** It enters the
radio multiplexer and reaches the node as a reception tagged
`RxOrigin::Bearer`.

**The MAC exposes exported frames as a bounded queue.** The node driver
drains it into the bearer's transmit queue. This keeps a new associated type
off `Platform`, which every adapter would otherwise have to carry.

**One firmware-side task pair owns the BLE bearer:**

- **Advertiser.** It takes one frame at a time from the transmit queue. For
  each frame it writes the AdvData to a dedicated non-connectable,
  non-scannable set and enables the set for a fixed number of advertising
  events. When the set reports terminated, it moves to the next frame.
- **Scanner.** It runs continuous extended passive scanning. It filters on
  the bearer identifier, reassembles fragmented reports, and applies the
  ingress rate limit. Surviving frames go to the `BridgePort` queue.

The ULCP connectable advertiser becomes a second set beside the bearer set.
The ULCP peripheral connection continues unchanged; the controller schedules
scanning and advertising around its connection events.

### Forwarding matrix

Rows are where a frame came from; columns are where the node may send it.
"Decision" means the ordinary forwarding procedure passed: not a duplicate,
flood hops remain or the source route names this node, region policy allows
it, and not addressed to this node.

| Arrived from | To LoRa | To the bearer |
|---|---|---|
| LoRa (`Air`) | Decision, repeater enabled | Decision, bearer enabled |
| Bearer | Decision, bearer enabled; contended | Never (first increment) |
| Internet tunnel or host (`Backhaul`) | Decision, repeater enabled (unchanged) | Decision, bearer enabled |
| Local origination | Always (unchanged) | Bearer enabled |

**The second row makes the bearer switch alone sufficient for a device to
bridge.** It forwards across interfaces even when LoRa-to-LoRa repeating is
off. That is the "turn on this feature on each one" behavior, and whether to
keep it is a decision listed under [Risks and decisions](#risks-and-decisions).

**One decision, one dup-cache entry, one rewrite.** The frame exported to the
bearer is byte-for-byte the frame enqueued for LoRa:

- FHOPS decremented or source-route hint consumed;
- this node's router hint prepended to the trace;
- region code inserted.

Retransmissions of an already-exported frame (`TxPriority::Retry`) are not
exported again. A route retry carries a new forwarding identity and is
exported as a new frame.

### Worked crossings

**Two bridges, different channels.**

1. A node on X transmits.
2. Bridge A (on X) hears it, decides, enqueues a LoRa repeat on X and
   exports the same rewrite to BLE.
3. Bridge C (on Y) receives the BLE copy as `Bearer`. It passes the dup
   check, decrements the hop, and enqueues on Y after a contention window.
4. C does not export it back to BLE.

The crossing costs two flood hops, as in the internet bridge. If A's repeat
on X is suppressed because an ordinary repeater went first, the export has
already happened.

**Three bridges, same channel X, one on Y.**

- **X to Y:** A and B both hear the LoRa frame and both export to BLE. C
  takes the first copy; the second is a dup.
- **Y to X:**
  - C exports to BLE. A and B each receive it as `Bearer` and contend with
    windows drawn uniformly from the flood contention range.
  - A transmits first. B hears A's LoRa transmission over the air and defers
    as it would for any repeat it overheard.
  - B also receives nothing more from BLE: C exported once, and A does not
    re-export bearer input.

**A device's own traffic.** A node originating a message on X transmits on
X and exports to BLE in parallel. A bridge on Y carries it there without the
LoRa repeat on X having to happen first.

## Protocol changes

All of the following are protocol-level, are new, and need spec text before
code lands.

### P1. BLE bearer binding (new chapter `docs/protocol/src/ble-bearer.md`)

**Advertising set.**

- **Set type:** extended, non-connectable, non-scannable, undirected.
- **Address:** anonymous where the controller supports it. Otherwise a
  non-resolvable private address, regenerated at least every 15 minutes and
  never the ULCP set's address.
- **PHY:** one fixed primary and secondary PHY for the whole bearer, chosen
  in the spike (see [D2](#risks-and-decisions)). The recommendation is LE
  Coded S8 for range, with LE 1M as the fallback.
- **Events:** each frame is advertised for `N_EVENTS` advertising events
  (proposed 3) at an advertising interval of 30 ms. The controller's random
  `advDelay` decorrelates senders.
- **ADI:** the Advertising Data ID changes with each new frame. Scanners may
  use it for controller-level duplicate filtering; it carries no protocol
  meaning.
- **Ordering:** a sender advertises one bearer frame at a time per set, in
  queue order.

**AdvData encoding.**

- AdvData consists of one or more **bearer AD structures** and nothing
  else.
- Each bearer AD structure is `LENGTH, TYPE, IDENTIFIER, DATA`. TYPE and
  IDENTIFIER are fixed by the identifier decision ([D1](#risks-and-decisions)):
  - Manufacturer Specific Data (`0xFF`) with an assigned company ID, or
  - Service Data with a 16-bit UUID (`0x16`), or
  - Service Data with a 128-bit UUID (`0x21`) from the UMSH base, slot
    `0x0100` ([UUID Allocation](protocol/src/ulcp-ble.md#uuid-allocation)).
- IDENTIFIER uses BLE's own little-endian encoding; it is a Bluetooth field,
  not a UMSH one.
- The **bearer payload** is the concatenation of the DATA of every bearer AD
  structure, in order. A sender uses the fewest structures that fit.
- **Frames that need two structures:**

  | Identifier | Frame size that needs a second structure |
  |---|---|
  | 2-octet identifier | over 251 octets |
  | 16-octet identifier | over 237 octets |

- The bearer payload is one header octet followed by exactly one UMSH frame
  of 1–255 octets:

  ```
    0   1   2   3   4   5   6   7
  +---+---+---+---+---+---+---+---+
  |  VER  |       RESERVED        |
  +---+---+---+---+---+---+---+---+
  ```

  - **`VER`** is 0.
  - **`RESERVED`** is sent as zero and ignored on receipt.
  - A receiver drops a payload with a nonzero `VER`, an empty frame, or a
    frame over 255 octets, counting it as malformed.

**Single-PDU capacity.** These figures are the frames that fit in one
`AUX_ADV_IND` without chaining. Larger frames chain; senders and receivers
must support AdvData up to the two-structure maximum.

| Identifier | Anonymous | With AdvA |
|---|---|---|
| 2-octet | 246 | 240 |
| 16-octet | 232 | 226 |

**Scanning.**

- A node with the bearer enabled scans continuously: extended passive
  scanning on the bearer's primary PHY, with scan window equal to scan
  interval.
- It reassembles fragmented extended advertising reports per advertiser
  address and SID. An incomplete or truncated report ends the attempt with
  nothing delivered.
- It ignores reports without a leading bearer AD structure.

**Receive processing.**

- **Origin and measurements:** a bearer payload is handed to the node as a
  received frame with no LoRa measurements, origin *bearer*. BLE RSSI is not
  a measurement of the LoRa link and is not reported as one.
- **Ingress limit:** a node **MUST** bound the rate at which bearer frames
  enter its node, and **SHOULD** expose the bound (proposed default: 30
  frames per minute, token bucket). Frames over the limit are dropped and
  counted. This is the BLE analog of the mandatory host-interface rate limit
  ([internet-bridging.md](protocol/src/internet-bridging.md#host-interfaces)).
  BLE injection is cheap, and every admitted frame may cost LoRa airtime.
- **Duplicates:** repeated copies of one frame (other advertising events,
  other bridges' exports) are ordinary duplicates for the node's dup cache.
  A receiver may also drop them earlier.

### P2. Forwarding across a bearer (`repeater-operation.md`, `routing-overview.md`, `packet-processing.md`)

- **The model:** a node may have a *bearer* in addition to its radio. The
  [forwarding matrix](#forwarding-matrix) becomes normative.
- **Export at decision:** the frame offered to the bearer is the
  forwarding procedure's rewritten output, offered when the decision is
  made, whether or not the radio transmission later happens.
- **No re-export:** frames received from the bearer are not offered to the
  bearer.
- **Hop accounting:** a crossing between two radios costs two flood hops. A
  frame originated by the exporting node costs one, at the importing node.
- **Trace:** a bearer reception adds the existing "nothing was measured"
  trace signal entry, so no new trace semantics are needed. A distinct
  bearer marker is deferred until route scoring has a use for it.
- **Source routes:** a hint consumed by a bridge leads to forwarding on
  both egresses allowed by the matrix, because the bridge cannot know which
  side the next hint is on. Pruning by learned neighbor side is an
  optimization, deferred.
- **Duty ceiling:** bridged LoRa transmissions count against it like any
  other forward. Bearer exports spend no LoRa airtime.

### P3. Channel access (`channel-access.md`)

- **Contention for bearer receptions:** a frame received from the bearer
  carries no LoRa measurement but was heard by every bridge in range. Its
  flood contention window is drawn uniformly from `[W_min, W_max]`, plus the
  usual jitter.
- **Point-to-point backhaul:** keeps its zero window.
- **Only over-the-air receptions defer or confirm:**
  - Only a copy received over the air may defer or suppress a pending
    forward.
  - Only such a copy may satisfy
    [forwarding confirmation](protocol/src/repeater-operation.md#forwarding-confirmation).
  - Receptions from a bearer or a backhaul say nothing about who heard the
    radio channel.
  - This also corrects the internet bridge's co-located-client case. There,
    a tunnel copy currently reschedules a pending forward to fire
    immediately.
- **Ack-MIC cancellation is unchanged.** It stays origin-independent: an
  acknowledgment proves delivery whichever way it arrived.

### P4. ULCP surface

This section defines `CAP_BLE_BEARER`, its properties, and the bearer
counters.

**Capability.** `CAP_BLE_BEARER`, tentatively 63, the next free code after
`CAP_MESHCORE_FORWARDER` (62); recheck before allocating. It requires
`CAP_REPEATER` (the forwarding engine).

**Properties.** These are device-domain properties, saved by `CMD_SAVE` and
writable by authorized mesh administrators. Numbers are tentative, chosen
clear of the MeshCore block (4976–4978).

| Prop | Name | Type | Access | Notes |
|---|---|---|---|---|
| 4992 | `PROP_BLE_BEARER_ENABLED` | BOOL | Get/Set | Default false |
| 4993 | `PROP_BLE_BEARER_INGRESS_LIMIT` | UINT16 | Get/Set | Frames per minute; default 30; zero invalid |
| 4994 | `PROP_BLE_BEARER_STATE` | UINT8 | Get/Is | 0 Disabled, 1 Active, 2 BLE off (`PROP_BLE_ENABLED` false), 3 Controller fault |

**Counters** under `CAP_STATS`:

- bearer frames received;
- dropped by the ingress limit;
- malformed;
- exported;
- dropped from the bearer transmit queue.

**Independence from other switches.**

- Enabling the bearer does not change `PROP_MAC_REPEATER_ENABLED` or
  `PROP_BLE_ENABLED`.
- When BLE is off, the bearer reports state 2 and neither scans nor
  advertises.

### P5. Edits to existing text

- **`ulcp.md` § BLE As A Local Bearer:** correct the payload-size table for
  extended advertising and point at the new chapter.
- **`ulcp-ble.md`:** if Service Data with a 128-bit UUID is chosen, assign
  slot `0x0100`. Either way, state that the ULCP connectable set and the
  bearer set coexist.
- **`internet-bridging.md`:** note that an integrated client and a bearer
  may coexist on one node, and how frames move between them (see
  [Architecture](#architecture)).

## Implementation increments

### 0. Spike (no spec, no merge)

A throwaway branch with two nRF52840 boards and one ESP32-S3 to answer the
questions the spec depends on.

- **Delivery:** probability per frame against `N_EVENTS` (1–4) and
  interval, at LE 1M and LE Coded S8, at 1 m and at about 30 m indoors.
  Repeat with a ULCP connection active on the scanner.
- **Anonymous advertising:** does it work on the SDC and on the S3
  controller, and do both scanners report it?
- **TrouBLE sets:** can one ULCP legacy-PDU connectable set and one
  bearer set be enabled and disabled independently? `advertise_ext` takes
  all sets at once and owns the `Peripheral`. If per-set control is
  missing, patch the fork (and offer it upstream in parallel).
- **Fragmented reports:** behavior for 250–260-octet AdvData on both
  controllers.
- **SDC memory:** `required_memory()` with extended advertising (2 sets,
  max AdvData of at least 292) and extended scanning enabled. Size `Mem`
  from it.
- **Current draw:** continuous scanning on T1000-E and Heltec V3.
- **iOS reconnect:** ULCP advertising through `advertise_ext` with a
  legacy PDU on an nRF board. Check bonding, RPA, reconnect, and pairing
  mode.

Exit: decisions D1 and D2 settled, and the risks below either resolved or
promoted into the increments.

### 1. Spec

Write P1–P5. Add bearer encoding test vectors to `test-vectors.md`: one
single-structure frame, one two-structure 255-octet frame, and one
malformed `VER`.

### 2. MAC and HAL (host-tested)

**`umsh-hal`.**

- Add `RxOrigin::Bearer`, with `is_measured()` false.
- Audit every `match` on `RxOrigin` and every `is_measured()` call.
  Measurement gates stay as they are; the contention and confirmation sites
  change.

**`umsh-mac`.**

- **Configuration:** `BearerConfig { enabled }` beside `RepeaterConfig`.
- **Export queue:** a bounded queue of exported frames (8 × `FRAME`, about
  2 KB inside `Mac`), drained through `poll_bearer_export(cx)`. A full queue
  drops the oldest frame and counts it.
- **Entry gate:** `maybe_forward_received` runs when either the repeater or
  the bearer is enabled.
- **Planning:** `plan_forwarding` computes the egress set from the matrix,
  shares one dup-cache insert and one rewrite, enqueues for LoRa when that
  egress is allowed, and pushes the rewrite to the export queue when that
  egress is allowed.
- **Local origination:** the three `tx_queue` enqueue sites and the
  immediate-ACK path export frames from `TxPriority::Application` and
  `ImmediateAck`, not from `Retry`.
- **Contention:** `sample_flood_contention_delay_ms` draws uniformly over
  the window for `Bearer`.
- **Deferral and confirmation:** `defer_pending_forward` and
  `observe_forwarding_confirmation` act only on `Air`.
  - Check that this does not regress a backhaul leaf host. Its MAC sees the
    device's transmissions through ULCP, so trace the origin it reports on
    the host side.
  - Add a regression test for the co-located internet-bridge case.
- **Counters:** add the bearer counters.

**Tests in `umsh-mac`.** Cover every cell of the matrix and:

- export when the LoRa forward is later suppressed;
- no re-export of bearer input;
- two-hop crossing accounting;
- a nonzero window for `Bearer` and zero for `Backhaul`;
- no deferral from `Bearer` or `Backhaul` copies;
- no confirmation from them;
- `Retry` not exported.

**Topology test.** A three-node topology test with two simulated channels
and a shared bearer. Reuse whatever harness the `umsh-bridge` integration
tests use to stand up several nodes in one process.

### 3. `crates/umsh-ble-bearer` (`no_std`, host-tested)

- **Codec:** AdvData encode (one or two AD structures) and decode, using
  the test vectors from increment 1.
- **Reassembler:** for fragmented extended reports, keyed on address and
  SID, with a bounded number of concurrent advertisers and a timeout.
- **Ingress limiter:** a token bucket driven by a `Clock`.
- **Transmit queue:** eight frames, oldest dropped under pressure, frames
  older than ten seconds expired (matching the integrated client).
- **Runner (feature `trouble`):** an advertiser task and a scanner event
  handler over TrouBLE and bt-hci traits, so both firmware families share
  it.

### 4. Radio multiplexer

- `radio_mux_with_bridge` takes a slice of attachments, each with its own
  ingress `RxOrigin`. Hand-off is round-robin among them.
- `transmitted()` copies go only to attachments that want them; the BLE
  bearer does not, since its egress is the MAC export.
- The Pager and T-Beam then run the internet client and the bearer side
  by side.
- Extend the existing mux tests.

### 5. nRF52840 firmware

- **Features:** enable TrouBLE `scan`.
- **SDC builder:** `support_ext_adv`, `support_ext_scan`,
  `support_le_coded_phy` (if D2 picks Coded), `adv_count(2)`,
  `adv_buffer_cfg`, `scan_buffer_cfg`, and `Mem` sized from the spike.
- **Host runner:** run it with `run_with_handler`, feeding the scanner
  handler.
- **ULCP advertiser:** move it to `advertise_ext` with a legacy-PDU set,
  coordinated with the bearer set through whatever per-set control the
  spike settled.
- **Wiring:** spawn the bearer runner and attach its port to the mux. The
  device node driver drains `poll_bearer_export` into the bearer transmit
  queue.
- **BLE gate:** stop scanning and advertising when BLE is gated off.
- **Builds:** all five boards (`make` targets). Record flash and RAM in
  `docs/firmware-architecture.md`.

### 6. ESP32-S3 firmware

- The same wiring in `firmware/esp32-tracker`.
- The `esp_radio::ble::Config` keeps `scan` and `adv` on.
- Measure the heap and stack slack on the Pager and T-Beam with coexistence
  before enabling the bearer there.
- Heltec V2 and the V3 bridge image do not advertise `CAP_BLE_BEARER`.
- Keep `scripts/check_esp32_manifests.py` in step if a feature is added.

### 7. ULCP and clients

- **Device session:** P4 properties, persistence, the capability, `Is`
  notifications for state, and the counters.
- **`umshctl`:** get and set for the three properties, and the bearer
  counters in its stats output.
- **iOS:** one toggle in device settings, with the state shown beside it.
  Keep the copy terse: a label and at most a one-line footer, with no
  mechanism in it.
- **Web debugger and dissector:** property names only.

### 8. Hardware validation

- **Two-frequency bridge.** Two T1000-Es on different channels, side by
  side, bearer on, repeater off on both. Chat and node-management round
  trips between a phone on each side. Confirm the hop accounting and both
  bridge hints in traces.
- **Same-channel contention.** Three bridges on X and one on Y. Count
  LoRa transmissions on X per bridged frame (expected: one, plus ordinary
  repeats).
- **Mixed families.** An nRF52840 bridge with an ESP32-S3 bridge.
- **ULCP coexistence.** The iOS app attached to a bridge during traffic.
  Check reconnects, pairing mode, and that the BLE gate turns scanning off.
- **Power.** Current on a T1000-E with the bearer idle and with traffic.
- **Ingress limit.** An Android or Linux test injector flooding the bearer;
  the LoRa duty ledger and the ingress counters behave as specified.

## Acceptance criteria

- Two devices on different LoRa channels, each with only the bearer enabled,
  carry chat both ways with no other configuration.
- With several bridges on one channel in one BLE space, a frame bridged onto
  that channel is transmitted once by a bridge, not once per bridge.
- A bridge that loses LoRa contention still bridges the frame.
- No frame circulates. Each node forwards a given frame at most once per
  egress, and bearer input is never re-exported.
- Tethered ULCP over BLE behaves as before on iOS, including bonding and
  reconnect, with the bearer on and off.
- Bearer ingress is rate-limited; bridged LoRa transmissions stay under the
  duty ceiling.
- Boards without the capability report it absent. No image's RAM or stack
  headroom goes negative.

## Risks and decisions

**D1. Bearer identifier.**

| Option | Overhead | What it takes |
|---|---|---|
| Company ID (Manufacturer Data) | 4 octets | A Bluetooth SIG assignment. `0xFFFF`, used in ULCP pairing advertising today, is for testing only and must not ship on the bearer. |
| 16-bit service UUID | 4 octets | A Bluetooth SIG assignment |
| 128-bit Service Data, UUID base slot `0x0100` | 18 octets | Nothing. Many frames exceed one PDU. |

- Recommendation: obtain a company ID, which also fixes the ULCP pairing
  advertisement.
- Fallback: 128-bit Service Data until the assignment arrives. The `VER`
  field cannot paper over an identifier change, so whatever ships first is
  what interoperates.

**D2. PHY.**

- LE Coded S8 gives roughly four times the range of 1M. Airtime per frame
  is still negligible.
- 1M has the widest support (including Android scanners) and the lowest
  energy per frame.
- Scanning both halves the time spent on each, so the bearer fixes one.
- Recommendation: Coded S8, unless the spike shows ESP32-S3 coexistence or
  reception trouble.

**D3. Bearer switch alone makes a bridge.**

- The matrix lets a node with the repeater off forward across interfaces.
  That is the one-toggle experience, at a cost.
- **Cost 1, no LoRa ingress repeat.** Such a bridge never repeats on LoRa
  what it hears on LoRa. A LoRa repeater upstream whose only downstream
  listener is the bridge hears no repeat and retries up to its retry cap.
- **Cost 2, the `REP` advertisement bit.** The bit is driven by the
  repeater switch, so it understates what a bearer-only bridge does.
  Whether bridges advertise themselves distinctly is open.
- **The alternative:** require the repeater switch as well, with two
  toggles in the UI.

**D4. Integrated internet client parity.** The client exports on
transmission and so has the contention-loss weakness described above. Moving
it to the MAC export path would make both bridges behave the same. The change
alters [ulcp-bridge.md](protocol/src/ulcp-bridge.md#integrated-clients) and is
out of scope here unless chosen.

**Risks.**

- **Traffic merging.** Bridging two meshes puts each mesh's floods on the
  other's airtime. Region policy and the existing exit clamp are the tools.
  A per-bearer region list is a possible follow-up if field use shows the
  need.
- **Controller scheduling.** Scanning shares the 2.4 GHz radio with the
  ULCP connection. Missed events show up as lower delivery, which
  `N_EVENTS` absorbs; the spike sizes it.
- **ESP32-S3 RAM.** The Pager and T-Beam are already tight. If the bearer
  does not fit beside Wi-Fi coexistence, those images ship it only without
  the internet client, or not at all, decided per board.
- **Privacy.** Bearer frames are the same bytes LoRa already broadcasts.
  Anonymous advertising, or a rotating non-resolvable address, keeps the BLE
  layer from adding a stable device identifier beyond what ULCP advertising
  already exposes.

## Later work

- **Relaying inside the BLE space.** This lifts the "never re-export"
  rule:
  - bearer-to-bearer forwarding with its own contention window, scaled to
    the bearer's delivery window (about `N_EVENTS` × interval) rather than
    LoRa's T_frame;
  - plus Air-equivalent deferral for bearer copies when the egress is the
    bearer itself.
- **BLE-only nodes.** A device whose node uses the bearer as its only radio:
  LoRa off, or no LoRa hardware. Bearer receptions are its air, timing is
  the bearer's, and bridges carry its traffic onto LoRa in one hop.
- **Android and Linux participants.** These would join the bearer
  directly: the app through `AdvertisingSet`, `umshctl` through BlueZ. A
  Linux bearer capture would also be a natural sniffer for the dissector.
- **Pruning source-routed egress** by the side on which the next hint was
  last heard.
- **MeshCore frames on the bearer**, using a `VER` or reserved-bit
  extension.
