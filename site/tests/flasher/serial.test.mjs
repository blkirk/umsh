/**
 * Finding a board again after it restarts as a different USB device.
 *
 *     node --test site/tests/flasher/
 *
 * Both flashing paths do this: an nRF52840 radio touched into its bootloader,
 * and an ESP32-S3 whose factory firmware restarts into the ROM. A port the
 * browser already trusts is used without asking, but only when it is new and
 * the only one.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { findNewPort } from "../../static/flasher/serial.js";

const port = (usbVendorId, usbProductId) => ({ getInfo: () => ({ usbVendorId, usbProductId }) });
const isBootloader = (candidate) => candidate.getInfo().usbProductId === 0x0029;

/** A getPorts() that answers with each list in turn, then keeps giving the last. */
function enumerations(...lists) {
  let call = 0;
  return async () => lists[Math.min(call++, lists.length - 1)];
}

const fast = { intervalMs: 0 };

test("the board's new port is found once it enumerates", async () => {
  const app = port(0x239a, 0x8029);
  const bootloader = port(0x239a, 0x0029);
  const getPorts = enumerations([app], [], [bootloader]);
  assert.equal(await findNewPort([app], isBootloader, { ...fast, getPorts }), bootloader);
});

test("a matching port that was already there is another board", async () => {
  const other = port(0x239a, 0x0029);
  const app = port(0x239a, 0x8029);
  const getPorts = enumerations([other]);
  assert.equal(await findNewPort([other, app], isBootloader, { ...fast, getPorts }), null);
});

test("two new matching ports are left to the user", async () => {
  const getPorts = enumerations([port(0x239a, 0x0029), port(0x239a, 0x0029)]);
  assert.equal(await findNewPort([], isBootloader, { ...fast, getPorts }), null);
});

test("nothing turning up gives up after the polls run out", async () => {
  let calls = 0;
  const getPorts = async () => {
    calls++;
    return [];
  };
  assert.equal(await findNewPort([], isBootloader, { ...fast, getPorts, polls: 3 }), null);
  assert.equal(calls, 3);
});
