# LAN Bearer

A device with a [LAN bearer](lan-bearer.md) joins its node to a multicast
group on the local network, and forwards between that group and its radio.
Configuration belongs to the device domain and is independent of the
attached host. Enabling the bearer does not enable Wi-Fi, change IP
configuration, or change `PROP_MAC_REPEATER_ENABLED`: the bearer forwards by
its own switch, as the [forwarding table](lan-bearer.md#the-forwarding-decision)
describes.

## Capabilities

`CAP_LAN_BEARER` (63) requires `CAP_REPEATER` and `CAP_IPV4`. It requires all
four properties below, and, with `CAP_STATS`, the five counters.

## PROP_LAN_BEARER_ENABLED {#prop-lan-bearer-enabled}

Property 4992; Get/Set; BOOL. Defaults to false. A write changes the desired
state immediately; it does not wait for the device to join the group.

## PROP_LAN_BEARER_GROUP {#prop-lan-bearer-group}

Property 4993; Get/Set; six octets: an IPv4 address in network order,
followed by a UINT16 port. Defaults to `239.255.134.121` port 15056. An
address in `224.0.0.0/4` is a multicast group; any other is the peer of a
[point-to-point](lan-bearer.md#point-to-point) link. A write of an empty value
returns it to the default; a read always reports the group in effect. A write
of `0.0.0.0`, `255.255.255.255`, a zero port, or any other length is refused
with `STATUS_INVALID_ARGUMENT`.

## PROP_LAN_BEARER_INGRESS_LIMIT {#prop-lan-bearer-ingress-limit}

Property 4994; Get/Set; UINT16. The most bearer receptions per minute the
device hands to its node; see [Ingress Limit](lan-bearer.md#ingress-limit).
Defaults to 120. A write of an empty value returns it to the default; a read
always reports the limit in effect. Zero is invalid. The device MAY admit a
short burst above the rate after a quiet interval.

## PROP_LAN_BEARER_STATE {#prop-lan-bearer-state}

Property 4995; Get/Is; UINT8. Read-only. Changes are announced to the
attached host.

Value | Meaning
------|--------
0 | Disabled
1 | Active: joined to the group and carrying frames
2 | No network: enabled, with no IPv4 network to join the group on

While the state is not Active, the node offers nothing to the bearer.

## Counters

Under `CAP_STATS`, with the semantics of the other
[statistics](ulcp-radio.md#statistics): cumulative `UINT32_LE` values, reset
by writing four zero octets.

Property | Name | Counts
---------|------|-------
4845 | `PROP_STAT_LAN_RX` | Datagrams handed to the node
4846 | `PROP_STAT_LAN_RX_LIMITED` | Datagrams dropped by the ingress limit, or because the node had not yet taken earlier ones
4847 | `PROP_STAT_LAN_RX_MALFORMED` | Datagrams dropped as empty, oversized, or of no recognized protocol
4848 | `PROP_STAT_LAN_EXPORTED` | Frames the node offered to the bearer
4849 | `PROP_STAT_LAN_EXPORT_DROPPED` | Offered frames discarded before they could be sent

## Persistence and lifecycle

The three configuration properties are saved by `CMD_SAVE` in the device
snapshot. A group or ingress limit at its default is saved as the default,
not as its value, so that the device follows the default of whatever firmware
it later runs. `CMD_CLEAR` erases saved configuration without changing live
configuration; `CMD_RST` follows the normal saved-state restore rules.
Attach and detach do not change these properties or interrupt the bearer.
Authorized mesh administrators may write them through the existing
device-management binding.

Changing the group leaves the old one and joins the new one. Losing the
network discards frames waiting to be sent, and the device rejoins the group
when the network returns.

## Virtual Radios

A [virtual radio](lan-bearer.md#participants) uses no identifiers of its own.
It declares the host-side capabilities a companion radio does—the raw
stream, host filters and keys, auto-acknowledgment, the inbound queue, and
saved state—and none of the repeater, Wi-Fi, or hardware ones, since it has
no node. Its radio properties describe a LoRa channel it does not itself
transmit on; their values set the frame time the host's MAC times against.
