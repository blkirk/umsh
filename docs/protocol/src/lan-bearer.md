# LAN Bearer

> [!NOTE]
> This section is an early work in progress and this protocol may change significantly.

The LAN bearer carries UMSH frames between the participants on one local
network, using IPv4 multicast. Repeaters on the network join their LoRa
meshes to it, so that two meshes on different LoRa settings, or two radios out of
range of each other, are joined by the network they share. Hosts with no
radio of their own join it too, and reach every mesh a radio on the network
carries.

A multicast group is used because every participant hears every other with
no server, no configuration beyond the group, and no state about who else is
present.

## Participants

- A **bridging radio** has a LoRa radio and a node whose repeater is enabled,
  and forwards between the two by the rules in
  [Forwarding Across Bearers](#forwarding-across-bearers). A radio whose
  repeater is disabled sends only its own traffic and its host's to the group,
  as an endpoint does.
- An **endpoint** has a node and no LoRa radio. It sends its own traffic to
  the group and receives what is sent there. A desktop tool or a phone on the
  network is an endpoint.
- A **virtual radio** is a [ULCP](ulcp.md) device whose physical layer is the
  group: what it transmits is sent to the group, and what it receives is
  every datagram from the group, delivered to its host with no measurements.
  It has no node and no repeater of its own. Its host is an endpoint that
  reaches the group through it, exactly as a host reaches the air through a
  LoRa radio.

## Transport

The bearer is UDP over IPv4 multicast. The default group is
`239.255.134.121`, in the administratively scoped range, and the default port
is 15056. A deployment MAY configure another IPv4 multicast group and another
nonzero port; every participant that is to hear the others uses the same
pair.

Every participant sends from the group's port as well as to it, and listens
on it; several participants on one host share it.

A participant joins the group on the interface it uses for the local network
and sends to the group from that interface, with a multicast TTL of 16 unless
configured otherwise. How far the group reaches is the network's decision:
routers forward a multicast group only where they are configured to, and
`239.255.0.0/16` is scoped to the organization. A network that routes the
group between its subnets joins their participants into one bearer.

### Point-to-Point

A unicast address MAY be configured in place of the group, naming the one
peer of a point-to-point link. The participant joins nothing, sends to the
peer at the group's port, and drops every datagram whose source address and
port are not the peer's. Each end names the other, on the same port.

### Echoes

A participant in a multicast group hears its own datagrams when multicast
loopback, or an access point repeating a station's multicast, hands them
back. A participant MAY drop a datagram whose source address and port are its
own, which on a host with several participants also drops the others'. A
participant that is alone on its host MAY instead disable multicast loopback.

## Datagram Format

A datagram carries exactly one frame, with no header. The frame is the
packet as it would be transmitted over the air: UMSH frames and
[MeshCore](meshcore-comparison.md) v1 frames are told apart by the version
bits in their first octet, as on LoRa.

- A sender **MUST NOT** send a datagram that is empty or larger than 255
  octets.
- A receiver drops, with accounting, a datagram that is empty, larger than
  255 octets, or whose version bits name no protocol it recognizes.
- A participant that does not carry MeshCore v1 frames ignores them.

The sender of a frame is not identified in the datagram, and nothing about
the datagram's source address is carried onward.

A datagram is a **bearer reception**. It was heard by every participant on
the network, and measured by none of them: it carries no RSSI or SNR, and
the [signal-quality thresholds](repeater-operation.md#forwarding-procedure)
do not apply to it.

## Discovery

Participants advertise themselves with DNS-SD over multicast DNS, under the
service type `_umsh._udp`. A bridging radio **SHOULD** advertise; other
participants MAY.

- The instance name is the participant's device name.
- The SRV port is the group's port.
- The TXT record carries the keys below.

Key | Value | Present
----|-------|--------
`v` | TXT schema version: `1` | Always
`grp` | The group address, dotted decimal with no leading zeros | Always
`id` | The node's public key, as 44 base58 characters | Always
`f` | LoRa center frequency, in Hz | When the participant has a LoRa radio with its PHY enabled
`bw` | LoRa bandwidth, in Hz | With `f`
`sf` | LoRa spreading factor | With `f`
`cr` | LoRa coding rate denominator, 5 to 8 | With `f`
`rep` | `1` if the node repeats LoRa to LoRa, else `0` | With `f`
`mc` | `1` if the node forwards MeshCore v1 | Optional
`ib` | `1` if an [internet bridge](internet-bridging.md) link is up | Optional

Numbers are decimal with no leading zeros. A participant updates its record
when any value changes.

A client ignores keys it does not recognize, treats a missing optional key
as false, and ignores a record whose `v` it does not recognize. Discovery is
only discovery: frames travel over the group whatever the records say, and a
client learns from them which LoRa meshes the network reaches and on which
settings, before it sends anything.

## Forwarding Across Bearers

A node MAY have one or more **bearers** beside its radio: shared media, such
as a LAN, on which every other participant hears what the node sends. A
bearer works as [backhaul](ulcp-host.md#prop-mac-backhaul) does:

- **What arrives on a bearer is a reception.** The node handles it as it
  handles a packet heard on its radio, recording which bearer it arrived
  on, and applies the
  [forwarding procedure](repeater-operation.md#forwarding-procedure) to it
  once. With its repeater enabled the node may repeat it on its radio; with
  its repeater disabled it forwards nothing, whatever the packet arrived on.
- **What the radio transmits goes to every bearer.** Each frame the node's
  radio puts on the air is sent to each bearer the node participates in:
  its own packets and acknowledgments, its repeats, and the transmissions
  of any other stack sharing the radio. A frame the radio does not
  transmit—a repeat abandoned to a better-placed repeater, or cancelled by
  an overheard acknowledgment—does not reach the bearer.

Enabling a bearer therefore makes the node a participant on it, not a
forwarder: a node whose repeater is disabled sends only its own traffic,
and its host's, to the bearer.

A crossing between two bridging radios spends two flood hops: one at the
radio that hears the packet and repeats it, which is what reaches the
bearer, and one at the radio that receives it from the bearer and repeats
it. A packet a bridging radio originates spends one. A repeat returns to
the bearer it came from, where every participant has already seen the
packet and suppresses it as a duplicate.

A bearer reception adds a [Trace Signal](packet-options.md#trace-signal-option-10)
entry recording that nothing was measured, as a point-to-point reception
does.

The [duty cycle](channel-access.md) a radio spends carrying bearer
receptions counts against its limit like any other forward. Copies sent to
a bearer spend no radio airtime.

### Ingress Limit

A bridging radio **MUST** bound the rate at which it hands bearer receptions
to its node, so that a busy network cannot spend the radio's airtime faster
than its operator allows. A token bucket of frames per minute is
recommended. Receptions over the limit are dropped with accounting.

## Channel Access

Every bridging radio on a network receives each datagram at once, so radios
on the same LoRa channel would carry it onto the air together. Bearer
receptions therefore contend, and only what is heard over the air says
anything about the air.

- **Bearer receptions contend.** A bearer reception carries no measurement
  to rank radios by. Its
  [flood contention window](channel-access.md#flood-forwarding-contention-window)
  is drawn uniformly from `[W_min, W_max]`, and the jitter and `D_ack` are
  added as for any other forward. A packet received over a point-to-point
  link keeps its zero window.
- **Only the air defers.** Only a copy received over the radio defers or
  abandons a pending forward. A copy received from a bearer or a
  point-to-point link says nothing about who has transmitted on the radio's
  channel, and is a duplicate and nothing more.
- **A bearer copy confirms.** A copy received from a bearer is a frame some
  radio transmitted, and satisfies
  [forwarding confirmation](repeater-operation.md#forwarding-confirmation)
  as a copy overheard on the air does.
- **Ack cancellation is unchanged.** An overheard acknowledgment
  [cancels](repeater-operation.md#ack-cancellation) a matching pending forward
  whatever it arrived on: it proves delivery either way.

Two radios on one LoRa channel that both hear a bearer reception draw
different windows. The first to transmit is overheard by the second, which
defers as it would for any repeat it overheard.

## Security Considerations

- **The bearer adds no protection and removes none.** UMSH frames are
  authenticated, and encrypted where their payload type calls for it,
  before they reach any medium. What crosses the LAN is what crosses the
  air.
- **The bearer reaches as far as the network routes the group.** A network
  that routes it between subnets has extended the bearer, and the radios on
  it, to everyone the routing reaches.
- **Anyone on the network can send to the group.** A participant on the LAN
  can inject frames into every mesh a bridging radio on it carries, as anyone
  with a LoRa radio can inject frames into the mesh around them, at a higher
  offered rate. The ingress limit and the duty-cycle limit bound what that
  costs the air.
- **Anyone on the network hears the group.** The LAN carries what the radios
  on it hear and transmit; a listener on the LAN learns what a LoRa receiver
  near each of those radios would.
- **DNS-SD records** reveal a radio's public key and LoRa settings, which its
  transmissions already reveal to anyone in range, to everyone on the
  network.
- **Bridging joins meshes.** The cautions in
  [Routing Overview § Bridging](routing-overview.md#bridging) apply: a
  bridging radio puts every mesh's flood traffic on the others' airtime.
  Region policy and the flood hop budget are the tools for keeping that
  local.
