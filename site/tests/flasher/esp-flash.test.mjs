/**
 * ESP32-S3 boards that arrive running Arduino firmware on TinyUSB.
 *
 *     node --test site/tests/flasher/
 *
 * Such firmware enters the ROM downloader by setting the ESP32-S3's
 * `FORCE_DOWNLOAD_BOOT` latch and restarting, rather than through the
 * strapping pins. The ROM comes up as a different USB device, and the chip
 * returns to the downloader on every reset until the latch is cleared. These
 * check that the new port is recognized, and that the latch is cleared before
 * EN is pulsed.
 */

import test from "node:test";
import assert from "node:assert/strict";

import { resetIntoFirmware, isUsbSerialJtag, ESPRESSIF_PORT_FILTERS } from "../../static/flasher/esp-flash.js";
import { findNewPort } from "../../static/flasher/serial.js";

const port = (usbVendorId, usbProductId) => ({ getInfo: () => ({ usbVendorId, usbProductId }) });

/** Web Serial's filter rule: a port matches a filter when every field the filter names agrees. */
const admits = (filters, candidate) =>
  filters.some((filter) => Object.entries(filter).every(([key, value]) => candidate.getInfo()[key] === value));

test("a factory XIAO's ROM is found, and a UMSH board already plugged in is not", async () => {
  const factory = port(0x2886, 0x0059);
  const tbeam = port(0x303a, 0x1001);
  const rom = port(0x303a, 0x1001);
  let call = 0;
  const getPorts = async () => (call++ === 0 ? [tbeam] : [tbeam, rom]);
  assert.equal(await findNewPort([factory, tbeam], isUsbSerialJtag, { getPorts, intervalMs: 0 }), rom);
});

test("only USB-Serial-JTAG counts as the ROM without asking", () => {
  assert.equal(isUsbSerialJtag(port(0x303a, 0x1001)), true);
  assert.equal(isUsbSerialJtag(port(0x2886, 0x0059)), false);
  assert.equal(isUsbSerialJtag(port(0x303a, 0x0002)), false);
  assert.equal(isUsbSerialJtag(port(0x10c4, 0xea60)), false);
});

test("the second picker shows the ROM in either USB mode and hides other devices", () => {
  assert.equal(admits(ESPRESSIF_PORT_FILTERS, port(0x303a, 0x1001)), true);
  assert.equal(admits(ESPRESSIF_PORT_FILTERS, port(0x303a, 0x0002)), true);
  assert.equal(admits(ESPRESSIF_PORT_FILTERS, port(0x2886, 0x0059)), false);
  assert.equal(admits(ESPRESSIF_PORT_FILTERS, port(0x10c4, 0xea60)), false);
});

/** An ESPLoader and Transport stand-in that log every call, in order, to one list. */
function fakes(chipName, { writeRegFails = false } = {}) {
  const calls = [];
  const loader = {
    chip: { CHIP_NAME: chipName },
    async writeReg(address, value, mask) {
      calls.push(["writeReg", address, value, mask]);
      if (writeRegFails) throw new Error("no response");
    },
  };
  const transport = {
    async setDTR(state) {
      calls.push(["DTR", state]);
    },
    async setRTS(state) {
      calls.push(["RTS", state]);
    },
  };
  return { calls, loader, transport };
}

const EN_PULSE = [
  ["DTR", false],
  ["RTS", true],
  ["RTS", false],
];

test("an ESP32-S3 has its forced-download latch cleared before EN is pulsed", async () => {
  const { calls, loader, transport } = fakes("ESP32-S3");
  await resetIntoFirmware(loader, transport);
  assert.deepEqual(calls, [["writeReg", 0x6000812c, 0, 0x1], ...EN_PULSE]);
});

test("a failed latch write still resets the board", async () => {
  const { calls, loader, transport } = fakes("ESP32-S3", { writeRegFails: true });
  const logged = [];
  await resetIntoFirmware(loader, transport, (line) => logged.push(line));
  assert.deepEqual(calls.slice(1), EN_PULSE);
  assert.equal(logged.length, 1);
});

test("other chips are only reset", async () => {
  const { calls, loader, transport } = fakes("ESP32");
  await resetIntoFirmware(loader, transport);
  assert.deepEqual(calls, EN_PULSE);
});
