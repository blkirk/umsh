#!/usr/bin/env python3
"""Interoperability vectors for the MeshCore v1 forwarder.

A model of the baseline repeater's receive path, transliterated from
MeshCore `repeater-v1.17.1` (commit d92964352441e53b93e8667b802e04f6e072b39e):

- src/Dispatcher.cpp   Dispatcher::tryParsePacket
- src/Packet.cpp       Packet::calculatePacketHash
- src/Mesh.cpp         Mesh::onRecvPacket, routeRecvPacket,
                       forwardMultipartDirect, routeDirectRecvAcks
- examples/simple_repeater/MyMesh.cpp
                       MyMesh::onRecvPacket (region lookup),
                       allowPacketForward, getRetransmitDelay,
                       getDirectRetransmitDelay
- src/helpers/RoutingPolicy.h   isFloodHopLimitExceeded
- src/helpers/RegionMap.cpp, src/helpers/TransportKeyStore.cpp
                       transport keys and codes

The model follows the C++ control flow statement by statement, so a
reviewer can hold the two side by side. Where the baseline reads past
the end of a malformed packet, the model declines it instead and says so.
MeshCore is MIT-licensed; see ../LICENSE-MESHCORE.

Local policy is not modeled: the vectors fix the airtime estimate at
four milliseconds per octet and the random draw at its maximum, so a
randomized delay is its window's upper bound, five times
trunc(airtime * factor), with the factor in integer thousandths.

Standard library only. Regenerate with:

    python3 crates/umsh-meshcore-forwarder/vectors/generate.py \
        > crates/umsh-meshcore-forwarder/vectors/vectors.json
"""

import hashlib
import hmac
import json
import sys

BASELINE_COMMIT = "d92964352441e53b93e8667b802e04f6e072b39e"

# MeshCore.h
MAX_HASH_SIZE = 8
MAX_ADVERT_DATA_SIZE = 32
CIPHER_MAC_SIZE = 2
PUB_KEY_SIZE = 32
SIGNATURE_SIZE = 64
MAX_PACKET_PAYLOAD = 184
MAX_PATH_SIZE = 64

# Packet.h
ROUTE_TYPE_TRANSPORT_FLOOD = 0
ROUTE_TYPE_FLOOD = 1
ROUTE_TYPE_DIRECT = 2
ROUTE_TYPE_TRANSPORT_DIRECT = 3

REQ, RESPONSE, TXT_MSG, ACK, ADVERT = 0, 1, 2, 3, 4
GRP_TXT, GRP_DATA, ANON_REQ, PATH, TRACE = 5, 6, 7, 8, 9
MULTIPART, CONTROL, RAW_CUSTOM = 10, 11, 15

# MyMesh.cpp defaults. The baseline multiplies by a float; the factors
# are kept here in the integer thousandths UMSH computes with.
TX_DELAY_FACTOR = 500
DIRECT_TX_DELAY_FACTOR = 300
FLOOD_MAX = 64
FLOOD_MAX_UNSCOPED = 64
FLOOD_MAX_ADVERT = 8

# Fixed local policy for the vectors.
AIRTIME_MS_PER_OCTET = 4


def airtime(length):
    return AIRTIME_MS_PER_OCTET * length


def max_window(t):
    # getRNG()->nextInt(0, 5*t + 1) at its largest draw.
    return 5 * t


# ─── Ed25519 (RFC 8032, section 6 reference) ───────────────────────────────

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)


def _add(a, b):
    x1, y1, z1, t1 = a
    x2, y2, z2, t2 = b
    A = (y1 - x1) * (y2 - x2) % P
    B = (y1 + x1) * (y2 + x2) % P
    C = t1 * 2 * D * t2 % P
    Dd = z1 * 2 * z2 % P
    E, F, G, H = B - A, Dd - C, Dd + C, B + A
    return (E * F % P, G * H % P, F * G % P, E * H % P)


def _mul(s, point):
    q = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            q = _add(q, point)
        point = _add(point, point)
        s >>= 1
    return q


def _equal(a, b):
    x1, y1, z1, _ = a
    x2, y2, z2, _ = b
    return (x1 * z2 - x2 * z1) % P == 0 and (y1 * z2 - y2 * z1) % P == 0


def _recover_x(y, sign):
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P)
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P != 0:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P != 0:
        return None
    if (x & 1) != sign:
        x = P - x
    return x


G_Y = 4 * pow(5, P - 2, P) % P
G_X = _recover_x(G_Y, 0)
G = (G_X, G_Y, 1, G_X * G_Y % P)


def _compress(point):
    x, y, z, _ = point
    zinv = pow(z, P - 2, P)
    x, y = x * zinv % P, y * zinv % P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decompress(data):
    y = int.from_bytes(data, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % P)


def _sha512_int(*parts):
    return int.from_bytes(hashlib.sha512(b"".join(parts)).digest(), "little")


def _secret_expand(secret):
    h = hashlib.sha512(secret).digest()
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def public_key(secret):
    a, _ = _secret_expand(secret)
    return _compress(_mul(a, G))


def sign(secret, message):
    a, prefix = _secret_expand(secret)
    A = _compress(_mul(a, G))
    r = _sha512_int(prefix, message) % L
    R = _compress(_mul(r, G))
    h = _sha512_int(R, A, message) % L
    s = (r + h * a) % L
    return R + int.to_bytes(s, 32, "little")


def verify(public, message, signature):
    A = _decompress(public)
    if A is None:
        return False
    R = _decompress(signature[:32])
    if R is None:
        return False
    s = int.from_bytes(signature[32:], "little")
    if s >= L:
        return False
    h = _sha512_int(signature[:32], public, message) % L
    return _equal(_mul(s, G), _add(R, _mul(h, A)))


# ─── Packet ────────────────────────────────────────────────────────────────


class Packet:
    def __init__(self):
        self.header = 0
        self.transport_codes = [0, 0]
        self.path_len = 0
        self.path = b""
        self.payload = b""
        self.snr = 0

    def route_type(self):
        return self.header & 0x03

    def payload_type(self):
        return (self.header >> 2) & 0x0F

    def payload_ver(self):
        return (self.header >> 6) & 0x03

    def is_route_flood(self):
        return self.route_type() in (ROUTE_TYPE_FLOOD, ROUTE_TYPE_TRANSPORT_FLOOD)

    def is_route_direct(self):
        return self.route_type() in (ROUTE_TYPE_DIRECT, ROUTE_TYPE_TRANSPORT_DIRECT)

    def has_transport_codes(self):
        return self.route_type() in (ROUTE_TYPE_TRANSPORT_FLOOD, ROUTE_TYPE_TRANSPORT_DIRECT)

    def path_hash_size(self):
        return (self.path_len >> 6) + 1

    def path_hash_count(self):
        return self.path_len & 63

    def path_byte_len(self):
        return self.path_hash_count() * self.path_hash_size()

    def set_path_hash_count(self, n):
        self.path_len = (self.path_len & ~63) | n

    def calculate_hash(self):
        # Packet::calculatePacketHash
        sha = hashlib.sha256()
        t = self.payload_type()
        sha.update(bytes([t]))
        if t == TRACE:
            sha.update(int.to_bytes(self.path_len, 2, "little"))  # uint16_t in memory
        sha.update(self.payload)
        return sha.digest()[:MAX_HASH_SIZE]

    def write(self):
        # Dispatcher::checkSend
        raw = bytes([self.header])
        if self.has_transport_codes():
            raw += int.to_bytes(self.transport_codes[0], 2, "little")
            raw += int.to_bytes(self.transport_codes[1], 2, "little")
        raw += bytes([self.path_len]) + self.path[: self.path_byte_len()] + self.payload
        return raw

    def copy(self):
        other = Packet()
        other.header = self.header
        other.transport_codes = list(self.transport_codes)
        other.path_len = self.path_len
        other.path = self.path
        other.payload = self.payload
        other.snr = self.snr
        return other


def try_parse(raw):
    """Dispatcher::tryParsePacket, bounds-checked. None for a rejection."""
    if len(raw) == 0:
        return None, "foreign"
    pkt = Packet()
    i = 0
    pkt.header = raw[i]
    i += 1
    if pkt.payload_ver() > 0:  # PAYLOAD_VER_1
        return None, "foreign"
    if pkt.has_transport_codes():
        if i + 4 > len(raw):
            return None, "malformed"  # baseline reads past the end
        pkt.transport_codes = [
            int.from_bytes(raw[i : i + 2], "little"),
            int.from_bytes(raw[i + 2 : i + 4], "little"),
        ]
        i += 4
    if i >= len(raw):
        return None, "malformed"  # baseline reads past the end
    pkt.path_len = raw[i]
    i += 1
    if pkt.path_len >> 6 == 3:
        return None, "malformed"
    path_byte_len = (pkt.path_len & 63) * pkt.path_hash_size()
    if path_byte_len > MAX_PATH_SIZE or i + path_byte_len > len(raw):
        return None, "malformed"
    pkt.path = raw[i : i + path_byte_len]
    i += path_byte_len
    pkt.payload = raw[i:]
    if len(pkt.payload) > MAX_PACKET_PAYLOAD:
        return None, "malformed"
    return pkt, None


# ─── Regions ───────────────────────────────────────────────────────────────


def transport_key(name):
    """RegionMap::getTransportKeysFor; None for a private region with no keystore."""
    if name.startswith(b"$"):
        return None
    if not name.startswith(b"#"):
        name = b"#" + name
    return hashlib.sha256(name).digest()[:16]


def transport_code(key, payload_type, payload):
    """TransportKey::calcTransportCode, as the uint16_t it returns."""
    mac = hmac.new(key, bytes([payload_type]) + payload, hashlib.sha256).digest()
    code = int.from_bytes(mac[:2], "little")
    if code == 0:
        code += 1
    elif code == 0xFFFF:
        code -= 1
    return code


# ─── Repeater ──────────────────────────────────────────────────────────────


class Repeater:
    def __init__(self, secret_pub_key, region_names, region_policy, min_rssi, min_snr):
        self.pub_key = secret_pub_key
        self.keys = [k for k in (transport_key(n) for n in region_names) if k is not None]
        self.policy = region_policy  # 0 open, 1 scoped only, 2 ignore
        self.min_rssi = min_rssi
        self.min_snr = min_snr
        self.seen = []
        self.recv_pkt_region = None

    # SimpleMeshTables
    def was_seen(self, pkt):
        return pkt.calculate_hash() in self.seen

    def mark_seen(self, pkt):
        self.seen.append(pkt.calculate_hash())

    def is_hash_match(self, data, length):
        return len(data) >= length and data[:length] == self.pub_key[:length]

    def retransmit_delay(self, pkt, factor):
        # MyMesh::getRetransmitDelay / getDirectRetransmitDelay
        t = airtime(pkt.path_byte_len() + len(pkt.payload) + 2) * factor // 1000
        return max_window(t)

    def allow_packet_forward(self, pkt):
        if pkt.is_route_flood():
            hops = pkt.path_hash_count()
            if hops >= FLOOD_MAX:
                return False, "hop_limit"
            if pkt.route_type() == ROUTE_TYPE_FLOOD and hops >= FLOOD_MAX_UNSCOPED:
                return False, "hop_limit"
            if pkt.payload_type() == ADVERT and hops >= FLOOD_MAX_ADVERT:
                return False, "hop_limit"
            if self.recv_pkt_region is None:
                return False, "region"
        return True, None

    def route_recv_packet(self, pkt):
        """Mesh::routeRecvPacket"""
        n = pkt.path_hash_count()
        if not pkt.is_route_flood():
            return ("declined", "zero_hop")
        if (n + 1) * pkt.path_hash_size() > MAX_PATH_SIZE:
            return ("declined", "path_full")
        if n + 1 > 63:
            # The baseline appends anyway, and setPathHashCount(64) carries
            # into the hash-size bits: the frame goes out claiming two-octet
            # entries and an empty path. Declined here instead.
            return ("declined", "path_full")
        allowed, why = self.allow_packet_forward(pkt)
        if not allowed:
            return ("declined", why)
        out = pkt.copy()
        out.path = pkt.path + self.pub_key[: pkt.path_hash_size()]
        out.set_path_hash_count(n + 1)
        return ("scheduled", out, out.path_hash_count(), self.retransmit_delay(out, TX_DELAY_FACTOR))

    def route_direct_recv_acks(self, pkt, delay):
        """Mesh::routeDirectRecvAcks, with no extra acknowledgments."""
        a2 = Packet()
        a2.header = ACK << 2
        a2.payload = pkt.payload
        a2.path_len = pkt.path_len
        a2.path = pkt.path
        a2.header = (a2.header & ~0x03) | ROUTE_TYPE_DIRECT
        return ("scheduled", a2, 0, delay)

    @staticmethod
    def remove_self_from_path(pkt):
        pkt.set_path_hash_count(pkt.path_hash_count() - 1)
        pkt.path = pkt.path[pkt.path_hash_size() :]

    def find_match(self, pkt):
        for key in self.keys:
            if transport_code(key, pkt.payload_type(), pkt.payload) == pkt.transport_codes[0]:
                return "region"
        return None

    def on_recv_packet(self, pkt, heard):
        """MyMesh::onRecvPacket, then Mesh::onRecvPacket."""
        if pkt.route_type() == ROUTE_TYPE_TRANSPORT_FLOOD:
            self.recv_pkt_region = "any" if self.policy == 2 else self.find_match(pkt)
        elif pkt.route_type() == ROUTE_TYPE_FLOOD:
            self.recv_pkt_region = None if self.policy == 1 else "wildcard"
        else:
            self.recv_pkt_region = None

        # The UMSH link-quality gate, ahead of everything for a measured flood.
        if pkt.is_route_flood() and heard[0] == "air":
            rssi, snr_centibels = heard[1], heard[2]
            if self.min_rssi is not None and rssi < self.min_rssi:
                return ("link_gated",)
            if self.min_snr is not None and snr_centibels < self.min_snr * 10:
                return ("link_gated",)

        ptype = pkt.payload_type()
        payload = pkt.payload

        if pkt.is_route_direct() and ptype == TRACE:
            # The baseline accepts a path length of 63 and increments it to
            # 64, which encodes as two-octet entries and an empty path.
            # Declined here instead.
            if pkt.path_len < MAX_PATH_SIZE - 1:
                if len(payload) < 9:
                    return ("declined", "incomplete")  # baseline reads past the end
                flags = payload[8]
                path_sz = flags & 0x03
                length = len(payload) - 9
                offset = pkt.path_len << path_sz
                if offset >= length:
                    return ("declined", "trace_complete")
                if self.is_hash_match(payload[9 + offset :], 1 << path_sz):
                    if not self.was_seen(pkt):
                        self.mark_seen(pkt)
                        out = pkt.copy()
                        out.path = pkt.path + bytes([pkt.snr & 0xFF])
                        out.path_len = pkt.path_len + 1
                        delay = self.retransmit_delay(out, DIRECT_TX_DELAY_FACTOR)
                        return ("scheduled", out, 5, delay)
                    return ("duplicate",)
                return ("declined", "not_next_hop")
            return ("declined", "path_full")

        if pkt.is_route_direct() and ptype == CONTROL and len(payload) > 0 and payload[0] & 0x80:
            return ("declined", "local_control")

        if pkt.is_route_direct() and pkt.path_hash_count() > 0:
            if self.is_hash_match(pkt.path, pkt.path_hash_size()):
                if ptype == MULTIPART:
                    return self.forward_multipart_direct(pkt)
                if ptype == ACK:
                    if not self.was_seen(pkt):
                        self.mark_seen(pkt)
                        out = pkt.copy()
                        self.remove_self_from_path(out)
                        return self.route_direct_recv_acks(out, 0)
                    return ("duplicate",)
                if not self.was_seen(pkt):
                    self.mark_seen(pkt)
                    out = pkt.copy()
                    self.remove_self_from_path(out)
                    return ("scheduled", out, 0, self.retransmit_delay(out, DIRECT_TX_DELAY_FACTOR))
                return ("duplicate",)
            return ("declined", "not_next_hop")

        if ptype == ACK:
            if 4 > len(payload):
                return ("declined", "incomplete")
            if self.was_seen(pkt):
                return ("duplicate",)
            self.mark_seen(pkt)
            return self.route_recv_packet(pkt)
        if ptype in (PATH, REQ, RESPONSE, TXT_MSG):
            if 2 + CIPHER_MAC_SIZE >= len(payload):
                return ("declined", "incomplete")
            if self.was_seen(pkt):
                return ("duplicate",)
            self.mark_seen(pkt)
            return self.route_recv_packet(pkt)
        if ptype == ANON_REQ:
            if 1 + PUB_KEY_SIZE + 2 >= len(payload):
                return ("declined", "incomplete")
            if self.was_seen(pkt):
                return ("duplicate",)
            self.mark_seen(pkt)
            return self.route_recv_packet(pkt)
        if ptype in (GRP_DATA, GRP_TXT):
            if 1 + 2 >= len(payload):
                return ("declined", "incomplete")
            if self.was_seen(pkt):
                return ("duplicate",)
            self.mark_seen(pkt)
            return self.route_recv_packet(pkt)
        if ptype == ADVERT:
            i = PUB_KEY_SIZE + 4 + SIGNATURE_SIZE
            if i > len(payload):
                return ("declined", "incomplete")
            if self.was_seen(pkt):
                return ("duplicate",)
            self.mark_seen(pkt)
            key = payload[:32]
            timestamp = payload[32:36]
            signature = payload[36:100]
            app_data = payload[100:][:MAX_ADVERT_DATA_SIZE]
            if not verify(key, key + timestamp + app_data, signature):
                return ("declined", "forged_advert")
            return self.route_recv_packet(pkt)
        if ptype == RAW_CUSTOM:
            if pkt.is_route_direct():
                if self.was_seen(pkt):
                    return ("duplicate",)
                self.mark_seen(pkt)
                return ("declined", "zero_hop")
            return ("declined", "payload_type")
        if ptype == MULTIPART:
            if len(payload) > 2 and payload[0] & 0x0F == ACK and len(payload) >= 5:
                tmp = pkt.copy()
                tmp.payload = payload[1:]
                if self.was_seen(tmp):
                    return ("duplicate",)
                self.mark_seen(tmp)
            return ("declined", "payload_type")
        return ("declined", "payload_type")

    def forward_multipart_direct(self, pkt):
        """Mesh::forwardMultipartDirect"""
        if len(pkt.payload) < 1:
            return ("declined", "incomplete")  # baseline reads past the end
        remaining = pkt.payload[0] >> 4
        if pkt.payload[0] & 0x0F == ACK and len(pkt.payload) >= 5:
            tmp = pkt.copy()
            tmp.payload = pkt.payload[1:]
            if not self.was_seen(tmp):
                self.mark_seen(tmp)
                self.remove_self_from_path(tmp)
                return self.route_direct_recv_acks(tmp, (remaining + 1) * 300)
            return ("duplicate",)
        return ("declined", "payload_type")

    def receive(self, raw, heard):
        pkt, rejection = try_parse(raw)
        if pkt is None:
            return (rejection,)
        if heard[0] == "local":
            self.mark_seen(pkt)
            return ("remembered",)
        if heard[0] == "air":
            pkt.snr = max(-128, min(127, quarter_db(heard[2])))
        else:
            pkt.snr = 127
        return self.on_recv_packet(pkt, heard)


def quarter_db(centibels):
    """Snr::as_quarter_db_steps: round to the nearest quarter decibel."""
    scaled = centibels * 2
    return (scaled + 2) // 5 if scaled >= 0 else -((-scaled + 2) // 5)


# ─── Builders ──────────────────────────────────────────────────────────────


def header(ptype, route, version=0):
    return (version << 6) | (ptype << 2) | route


def frame(ptype, route, path=b"", hash_size=1, payload=b"", codes=None, count=None):
    if count is None:
        count = len(path) // hash_size
    raw = bytes([header(ptype, route)])
    if route in (ROUTE_TYPE_TRANSPORT_FLOOD, ROUTE_TYPE_TRANSPORT_DIRECT):
        raw += codes if codes is not None else b"\x11\x22\x33\x44"
    return raw + bytes([((hash_size - 1) << 6) | count]) + path + payload


def secret(tag):
    return hashlib.sha256(b"umsh-meshcore-vector " + tag).digest()


def first_secret_with_prefix(first):
    n = 0
    while True:
        s = secret(b"device %d" % n)
        if public_key(s)[0] == first:
            return s
        n += 1


DEVICE = public_key(first_secret_with_prefix(0x5A))
OTHER = bytes([0xA5]) + bytes(31)
NEIGHBOR = bytes([0x31, 0x32, 0x33])

AIR = ("air", -80, 50)


def advert(secret_bytes, app_data, signed_app_data=None):
    key = public_key(secret_bytes)
    timestamp = (1_760_000_000).to_bytes(4, "little")
    signed = app_data[:MAX_ADVERT_DATA_SIZE] if signed_app_data is None else signed_app_data
    signature = sign(secret_bytes, key + timestamp + signed)
    return key + timestamp + signature + app_data


def ciphertext(n, tag=0):
    return bytes((tag + i * 7) & 0xFF for i in range(n))


# ─── Scenarios ─────────────────────────────────────────────────────────────


def heard_json(heard):
    if heard[0] == "air":
        return {"air": {"rssi": heard[1], "snr_centibels": heard[2]}}
    return heard[0]


def expectation(outcome, now_ms):
    kind = outcome[0]
    if kind == "scheduled":
        _, pkt, priority, delay = outcome
        return {
            "verdict": "scheduled",
            "forwarded": pkt.write().hex(),
            "priority": priority,
            "due_ms": now_ms + delay,
        }
    if kind == "declined":
        return {"verdict": "declined", "reason": outcome[1]}
    return {"verdict": kind}


def scenario(name, steps, regions=(), policy=0, min_rssi=None, min_snr=None, key=DEVICE):
    repeater = Repeater(key, list(regions), policy, min_rssi, min_snr)
    out = []
    for index, (raw, heard) in enumerate(steps):
        now_ms = 1_000 * (index + 1)
        outcome = repeater.receive(raw, heard)
        out.append(
            {"frame": raw.hex(), "heard": heard_json(heard), "now_ms": now_ms}
            | expectation(outcome, now_ms)
        )
    return {
        "name": name,
        "device_key": key.hex(),
        "regions": [r.decode() for r in regions],
        "region_policy": policy,
        "min_rssi": min_rssi,
        "min_snr": min_snr,
        "steps": out,
    }


def scenarios():
    s = []
    one = lambda f, heard=AIR, **kw: [(f, heard)]

    # Parsing.
    s.append(scenario("empty frame", one(b"")))
    for version in (1, 2, 3):
        s.append(scenario(f"version {version}", one(bytes([version << 6 | 0x09, 0, 1, 2, 3, 4, 5]))))
    s.append(scenario("umsh frame", one(bytes([0xC0, 0x01, 0x02, 0x03]))))
    s.append(scenario("reserved path mode", one(bytes([header(TXT_MSG, ROUTE_TYPE_FLOOD), 0xC1, 1, 2, 3, 4, 5, 6]))))
    s.append(scenario("path over 64 octets", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, bytes(66), 3, ciphertext(8)))))
    s.append(scenario("path past the end", one(bytes([header(TXT_MSG, ROUTE_TYPE_FLOOD), 0x05, 1, 2]))))
    s.append(scenario("codes past the end", one(bytes([header(TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD), 1, 2]))))
    s.append(scenario("no path length", one(bytes([header(TXT_MSG, ROUTE_TYPE_FLOOD)]))))
    s.append(scenario("payload over 184", one(frame(GRP_TXT, ROUTE_TYPE_FLOOD, payload=ciphertext(185)))))
    s.append(scenario("payload of 184", one(frame(GRP_TXT, ROUTE_TYPE_FLOOD, payload=ciphertext(184)))))

    # Flood forwarding for each hash size.
    for size in (1, 2, 3):
        path = NEIGHBOR[:size] + bytes(range(0x40, 0x40 + size))
        for route, label in ((ROUTE_TYPE_FLOOD, "flood"), (ROUTE_TYPE_TRANSPORT_FLOOD, "transport flood")):
            s.append(
                scenario(
                    f"{label} txt, {size}-octet hashes",
                    one(frame(TXT_MSG, route, path, size, ciphertext(20))),
                    policy=2,
                )
            )

    # Minimum lengths by payload type.
    for ptype, shortest in (
        (ACK, 4),
        (PATH, 5),
        (REQ, 5),
        (RESPONSE, 5),
        (TXT_MSG, 5),
        (ANON_REQ, 36),
        (GRP_TXT, 4),
        (GRP_DATA, 4),
    ):
        s.append(scenario(f"flood type {ptype} at {shortest}", one(frame(ptype, ROUTE_TYPE_FLOOD, payload=ciphertext(shortest)))))
        s.append(
            scenario(f"flood type {ptype} at {shortest - 1}", one(frame(ptype, ROUTE_TYPE_FLOOD, payload=ciphertext(shortest - 1))))
        )

    # Types that are never flooded.
    for ptype in (TRACE, MULTIPART, CONTROL, RAW_CUSTOM, 0x0C, 0x0D, 0x0E):
        s.append(scenario(f"flood type {ptype}", one(frame(ptype, ROUTE_TYPE_FLOOD, payload=bytes([0x03]) + ciphertext(12)))))

    # Advertisements.
    signer = secret(b"advertiser")
    s.append(scenario("advert", one(frame(ADVERT, ROUTE_TYPE_FLOOD, NEIGHBOR[:1], 1, advert(signer, b"\x81name")))))
    forged = bytearray(advert(signer, b"\x81name"))
    forged[40] ^= 0x01
    s.append(scenario("forged advert", one(frame(ADVERT, ROUTE_TYPE_FLOOD, payload=bytes(forged)))))
    s.append(scenario("advert at 99 octets", one(frame(ADVERT, ROUTE_TYPE_FLOOD, payload=advert(signer, b"")[:99]))))
    s.append(scenario("advert with no app data", one(frame(ADVERT, ROUTE_TYPE_FLOOD, payload=advert(signer, b"")))))
    long_app = ciphertext(40, 3)
    s.append(scenario("advert signed over 32 of 40", one(frame(ADVERT, ROUTE_TYPE_FLOOD, payload=advert(signer, long_app)))))
    s.append(
        scenario(
            "advert signed over all 40",
            one(frame(ADVERT, ROUTE_TYPE_FLOOD, payload=advert(signer, long_app, signed_app_data=long_app))),
        )
    )
    s.append(scenario("advert at 7 hops", one(frame(ADVERT, ROUTE_TYPE_FLOOD, bytes(range(7)), 1, advert(signer, b"\x81")))))
    s.append(scenario("advert at 8 hops", one(frame(ADVERT, ROUTE_TYPE_FLOOD, bytes(range(8)), 1, advert(signer, b"\x81")))))

    # Path capacity and flood limits.
    s.append(scenario("62 one-octet entries", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, bytes(62), 1, ciphertext(8)))))
    s.append(scenario("63 one-octet entries", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, bytes(63), 1, ciphertext(8)))))
    s.append(scenario("32 two-octet entries", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, bytes(64), 2, ciphertext(8)))))
    s.append(scenario("21 three-octet entries", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, bytes(63), 3, ciphertext(8)))))
    s.append(scenario("20 three-octet entries", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, bytes(60), 3, ciphertext(8)))))

    # Region policy, for unscoped, matching, and unmatched floods.
    region_payload = ciphertext(24, 9)
    code = transport_code(transport_key(b"sf-bay"), TXT_MSG, region_payload).to_bytes(2, "little")
    matching = frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD, payload=region_payload, codes=code + b"\x00\x00")
    unmatched = frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD, payload=region_payload, codes=b"\x00\x01\x00\x00")
    second_code = frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD, payload=region_payload, codes=b"\x00\x01" + code)
    unscoped = frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=region_payload)
    for policy in (0, 1, 2):
        for label, f in (("unscoped", unscoped), ("matching", matching), ("unmatched", unmatched), ("second code", second_code)):
            s.append(scenario(f"policy {policy}, {label}", one(f), regions=[b"sf-bay"], policy=policy))
    for policy in (0, 1):
        s.append(scenario(f"policy {policy}, no regions, unscoped", one(unscoped), policy=policy))
        s.append(scenario(f"policy {policy}, no regions, scoped", one(matching), policy=policy))
    s.append(scenario("hashtag spelled out", one(matching), regions=[b"#sf-bay"]))
    s.append(scenario("region case differs", one(matching), regions=[b"SF-Bay"]))
    private_code = transport_code(hashlib.sha256(b"#$sf-bay").digest()[:16], TXT_MSG, region_payload)
    private = frame(
        TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD, payload=region_payload, codes=private_code.to_bytes(2, "little") + b"\x00\x00"
    )
    s.append(scenario("private region", one(private), regions=[b"$sf-bay"]))
    s.append(scenario("direct ignores scope", one(frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_DIRECT, DEVICE[:1] + b"\x77", 1, region_payload, codes=b"\x00\x01\x00\x00")), regions=[b"sf-bay"], policy=1))

    # Direct routes for each hash size.
    for size in (1, 2, 3):
        route = DEVICE[:size] + NEIGHBOR[:size]
        s.append(scenario(f"direct txt, {size}-octet hashes", one(frame(TXT_MSG, ROUTE_TYPE_DIRECT, route, size, ciphertext(16)))))
        s.append(
            scenario(
                f"transport direct txt, {size}-octet hashes",
                one(frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_DIRECT, route, size, ciphertext(16), codes=b"\x01\x02\x03\x04")),
            )
        )
        s.append(scenario(f"direct ack, {size}-octet hashes", one(frame(ACK, ROUTE_TYPE_DIRECT, route, size, ciphertext(4)))))
        s.append(
            scenario(
                f"transport direct ack, {size}-octet hashes",
                one(frame(ACK, ROUTE_TYPE_TRANSPORT_DIRECT, route, size, ciphertext(4), codes=b"\x01\x02\x03\x04")),
            )
        )
        s.append(scenario(f"direct, other next hop, {size}", one(frame(TXT_MSG, ROUTE_TYPE_DIRECT, OTHER[:size] + DEVICE[:size], size, ciphertext(16)))))
    s.append(scenario("direct, last hop", one(frame(TXT_MSG, ROUTE_TYPE_DIRECT, DEVICE[:1], 1, ciphertext(16)))))
    s.append(scenario("direct ack, empty payload", one(frame(ACK, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x09", 1, b""))))
    for remaining in (0, 2):
        s.append(
            scenario(
                f"direct multipart ack, {remaining} remaining",
                one(frame(MULTIPART, ROUTE_TYPE_DIRECT, DEVICE[:2] + b"\x09\x09", 2, bytes([remaining << 4 | ACK]) + ciphertext(4))),
            )
        )
    s.append(scenario("direct multipart, short", one(frame(MULTIPART, ROUTE_TYPE_DIRECT, DEVICE[:1], 1, bytes([ACK]) + ciphertext(3)))))
    s.append(scenario("direct multipart, not an ack", one(frame(MULTIPART, ROUTE_TYPE_DIRECT, DEVICE[:1], 1, bytes([TXT_MSG]) + ciphertext(8)))))
    s.append(scenario("direct control, high bit", one(frame(CONTROL, ROUTE_TYPE_DIRECT, DEVICE[:1], 1, b"\x80" + ciphertext(8)))))
    s.append(scenario("direct control, high bit, zero hop", one(frame(CONTROL, ROUTE_TYPE_DIRECT, payload=b"\x81" + ciphertext(8)))))
    s.append(scenario("direct control, low bit", one(frame(CONTROL, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x02", 1, b"\x10" + ciphertext(8)))))
    s.append(scenario("direct raw custom", one(frame(RAW_CUSTOM, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x02", 1, ciphertext(8)))))
    s.append(scenario("zero-hop direct txt", one(frame(TXT_MSG, ROUTE_TYPE_DIRECT, payload=ciphertext(16)))))

    # TRACE.
    def trace_payload(hash_shift, route):
        return b"\x01\x02\x03\x04" + b"\x05\x06\x07\x08" + bytes([hash_shift]) + route

    for shift in (0, 1, 2, 3):
        size = 1 << shift
        route = OTHER[:size] + DEVICE[:size] + NEIGHBOR[:1] * size
        snrs = bytes([0x14])
        s.append(
            scenario(
                f"trace, {size}-octet hashes",
                one(frame(TRACE, ROUTE_TYPE_DIRECT, snrs, 1, trace_payload(shift, route))),
            )
        )
    route = DEVICE[:1] + NEIGHBOR[:1]
    s.append(scenario("trace, first hop", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(0, route)))))
    s.append(scenario("trace, negative snr", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(0, route)), ("air", -100, -72))))
    s.append(scenario("trace, strongest snr", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(0, route)), ("air", -30, 400))))
    s.append(scenario("trace over the backhaul", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(0, route)), ("backhaul",))))
    s.append(scenario("trace, transport direct", one(frame(TRACE, ROUTE_TYPE_TRANSPORT_DIRECT, payload=trace_payload(0, route), codes=b"\x01\x02\x03\x04"))))
    s.append(scenario("trace, complete", one(frame(TRACE, ROUTE_TYPE_DIRECT, b"\x01\x02", 1, trace_payload(0, route)))))
    s.append(scenario("trace, other next hop", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(0, OTHER[:1] + DEVICE[:1])))))
    s.append(scenario("trace, entry cut short", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(1, DEVICE[:1])))))
    s.append(scenario("trace, short payload", one(frame(TRACE, ROUTE_TYPE_DIRECT, payload=b"\x01\x02\x03"))))
    s.append(scenario("trace, two-octet path mode", one(frame(TRACE, ROUTE_TYPE_DIRECT, b"\x01\x02", 2, trace_payload(0, route)))))
    s.append(scenario("trace, 62 hops", one(frame(TRACE, ROUTE_TYPE_DIRECT, bytes(62), 1, trace_payload(0, bytes(62) + DEVICE[:1])))))
    s.append(scenario("trace, 63 hops", one(frame(TRACE, ROUTE_TYPE_DIRECT, bytes(63), 1, trace_payload(0, bytes(63) + DEVICE[:1])))))
    s.append(scenario("trace, flooded", one(frame(TRACE, ROUTE_TYPE_FLOOD, payload=trace_payload(0, route)))))

    # Duplicates.
    txt = ciphertext(20, 1)
    s.append(
        scenario(
            "a repeat is a duplicate",
            [(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), AIR), (frame(TXT_MSG, ROUTE_TYPE_FLOOD, b"\x01", 1, txt), AIR)],
        )
    )
    s.append(
        scenario(
            "a direct copy of a flood is a duplicate",
            [
                (frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), AIR),
                (frame(TXT_MSG, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x01", 1, txt), AIR),
            ],
        )
    )
    s.append(
        scenario(
            "a zero-hop copy blocks the flood",
            [(frame(TXT_MSG, ROUTE_TYPE_DIRECT, payload=txt), AIR), (frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), AIR)],
        )
    )
    s.append(
        scenario(
            "a declined flood is still remembered",
            [
                (frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD, payload=txt, codes=b"\x00\x01\x00\x00"), AIR),
                (frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), AIR),
            ],
            policy=0,
        )
    )
    s.append(
        scenario(
            "a forged advert is remembered",
            [(frame(ADVERT, ROUTE_TYPE_FLOOD, payload=bytes(forged)), AIR), (frame(ADVERT, ROUTE_TYPE_FLOOD, payload=bytes(forged)), AIR)],
        )
    )
    s.append(
        scenario(
            "an incomplete packet is not remembered",
            [(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt[:4]), AIR), (frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt[:4]), AIR)],
        )
    )
    s.append(
        scenario(
            "an unrouted direct packet is not remembered",
            [
                (frame(TXT_MSG, ROUTE_TYPE_DIRECT, OTHER[:1] + b"\x01", 1, txt), AIR),
                (frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), AIR),
            ],
        )
    )
    s.append(
        scenario(
            "a trace revisits a node on its way back",
            [
                (frame(TRACE, ROUTE_TYPE_DIRECT, payload=trace_payload(0, DEVICE[:1] + b"\x33" + DEVICE[:1])), AIR),
                (frame(TRACE, ROUTE_TYPE_DIRECT, b"\x10\x11", 1, trace_payload(0, DEVICE[:1] + b"\x33" + DEVICE[:1])), AIR),
                (frame(TRACE, ROUTE_TYPE_DIRECT, b"\x10\x11", 1, trace_payload(0, DEVICE[:1] + b"\x33" + DEVICE[:1])), AIR),
            ],
        )
    )
    multi = bytes([0x10 | ACK]) + ciphertext(4, 5)
    s.append(
        scenario(
            "a multipart ack is remembered as itself",
            [
                (frame(MULTIPART, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x01", 1, multi), AIR),
                (frame(MULTIPART, ROUTE_TYPE_FLOOD, payload=multi), AIR),
                (frame(ACK, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x01", 1, multi[1:]), AIR),
            ],
        )
    )
    s.append(
        scenario(
            "a local transmission is never forwarded",
            [(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), ("local",)), (frame(TXT_MSG, ROUTE_TYPE_FLOOD, b"\x01", 1, txt), AIR)],
        )
    )

    # Link-quality gates.
    weak = ("air", -120, 50)
    noisy = ("air", -80, -130)
    s.append(
        scenario(
            "a weak copy is gated and not remembered",
            [(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), weak), (frame(TXT_MSG, ROUTE_TYPE_FLOOD, b"\x07", 1, txt), AIR)],
            min_rssi=-100,
        )
    )
    s.append(scenario("a noisy flood is gated", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), noisy), min_snr=-12))
    s.append(scenario("the snr threshold is inclusive", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), ("air", -80, -120)), min_snr=-12))
    s.append(scenario("the rssi threshold is inclusive", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), ("air", -100, 0)), min_rssi=-100))
    s.append(scenario("a scoped flood is gated", one(frame(TXT_MSG, ROUTE_TYPE_TRANSPORT_FLOOD, payload=txt), weak), min_rssi=-100, policy=2))
    s.append(scenario("direct traffic is exempt", one(frame(TXT_MSG, ROUTE_TYPE_DIRECT, DEVICE[:1] + b"\x02", 1, txt), weak), min_rssi=-100))
    s.append(scenario("a backhaul flood is exempt", one(frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=txt), ("backhaul",)), min_rssi=-100, min_snr=0))
    return s


def hash_vectors():
    out = []
    cases = [
        frame(TXT_MSG, ROUTE_TYPE_FLOOD, payload=ciphertext(20)),
        frame(TXT_MSG, ROUTE_TYPE_DIRECT, b"\x01\x02", 1, ciphertext(20)),
        frame(ACK, ROUTE_TYPE_FLOOD, payload=ciphertext(4)),
        frame(ADVERT, ROUTE_TYPE_TRANSPORT_FLOOD, payload=ciphertext(100)),
        frame(TRACE, ROUTE_TYPE_DIRECT, payload=ciphertext(11)),
        frame(TRACE, ROUTE_TYPE_DIRECT, b"\x01\x02\x03", 1, ciphertext(11)),
        frame(MULTIPART, ROUTE_TYPE_DIRECT, payload=ciphertext(5)),
        frame(RAW_CUSTOM, ROUTE_TYPE_DIRECT, payload=b""),
    ]
    for ptype in range(16):
        cases.append(frame(ptype, ptype % 4, payload=ciphertext(ptype + 1, ptype), codes=b"\xaa\xbb\xcc\xdd"))
    for raw in cases:
        pkt, _ = try_parse(raw)
        out.append({"frame": raw.hex(), "hash": pkt.calculate_hash().hex()})
    return out


def code_vectors():
    out = []
    for name in (b"sf-bay", b"#sf-bay", b"SF-Bay", b"0x1234", b"au-nsw", b"#"):
        key = transport_key(name)
        payload = ciphertext(16, len(name))
        out.append(
            {
                "name": name.decode(),
                "key": key.hex(),
                "payload_type": TXT_MSG,
                "payload": payload.hex(),
                "code": transport_code(key, TXT_MSG, payload).to_bytes(2, "little").hex(),
            }
        )
    # The two reserved values: find payloads whose raw code lands on each.
    key = transport_key(b"sf-bay")
    found = {}
    n = 0
    while len(found) < 2:
        payload = n.to_bytes(4, "big")
        mac = hmac.new(key, bytes([GRP_TXT]) + payload, hashlib.sha256).digest()[:2]
        if mac in (b"\x00\x00", b"\xff\xff") and mac not in found:
            found[mac] = payload
        n += 1
    for raw, payload in sorted(found.items()):
        out.append(
            {
                "name": "sf-bay",
                "key": key.hex(),
                "payload_type": GRP_TXT,
                "payload": payload.hex(),
                "raw_code": raw.hex(),
                "code": transport_code(key, GRP_TXT, payload).to_bytes(2, "little").hex(),
            }
        )
    return out


def main():
    document = {
        "baseline_commit": BASELINE_COMMIT,
        "generator": "crates/umsh-meshcore-forwarder/vectors/generate.py",
        "airtime_ms_per_octet": AIRTIME_MS_PER_OCTET,
        "random_draw": "maximum",
        "hashes": hash_vectors(),
        "transport_codes": code_vectors(),
        "scenarios": scenarios(),
    }
    json.dump(document, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
