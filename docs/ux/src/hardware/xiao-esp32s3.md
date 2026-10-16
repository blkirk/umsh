# Seeed XIAO ESP32S3 + Wio-SX1262

The XIAO ESP32S3 kit is a headless node: one user button, one green LED, and
no display, sounder, or battery sense. It is the smallest board in the family
that carries Wi-Fi alongside Bluetooth, so the same image is a phone
companion, a repeater, or an internet bridge, depending only on configuration.

Its interaction model is the [Solar P1](sensecap-solar-p1.md)'s, reduced to
one button and one LED: the USR button does everything a single button can,
and the green LED carries every status and acknowledgment.

## Buttons

| Button | While running | From deep sleep |
|---|---|---|
| USR | Press → beacon, or cancel a running locate alert. Hold 4 s → power off | Press → powers the node on. Held through boot → force pairing |

USR, on the carrier, is the only button that can be pressed. The XIAO's own
BOOT and RESET buttons sit under the carrier.

A press asks the device node to beacon, and the LED confirms only once the
MAC accepts the send, as on the other boards' primary-action slot (see
[status and feedback](../interaction-model/status-and-feedback.md)). While a
locate alert is running, the first press silences it and does nothing else.

The press that wakes the node from deep sleep is not read as a beacon request.
Holding USR for a second past boot opens the pairing window over existing
bonds, and switches Bluetooth on if it was off.

USR shares its pin with the XIAO's yellow LED, so the yellow LED lights
whenever the button is held. That is the wiring, not a firmware signal.

## LED

The green LED on the carrier carries everything. In priority order:

| Sequence | Meaning |
|---|---|
| Triple blink, 1.5 s period | Locate alert (`PROP_ALERT`). With no sounder, the blink is the entire alert |
| Two blinks every 2 s | Pairing window open |
| One short flash | An action was accepted (a beacon the MAC took) |
| Two flashes | Force-pairing hold accepted at boot |
| Three flashes | Power-off hold accepted; deep sleep follows |
| Brief flick every 4 s | Heartbeat |

The XIAO's own red LED is the charger's and is not under firmware control.

## USB

The board cannot see VBUS, so it watches the USB bus instead. The wired port
exists while a host is on the bus. When the host goes away, whether unplugged
or suspended, the port closes within about a second and the board may
light-sleep. A host that arrives is picked up within about a second.

Plugging in USB does not wake the board from deep sleep: press USR.

## Power off

Holding USR for four seconds plays the three-flash acknowledgment, puts the
radio to sleep, waits for the button to be released, and enters deep sleep
with USR armed as the wake source.

## Notes and limitations

- No battery level. The kit can charge a cell but nothing measures it, so the
  node reports that it cannot.
- No display and no sounder. Anything richer than a beacon and an alert
  happens over BLE, USB, or Wi-Fi.
- The carrier's D4/D5 I²C pins are open for host access (`ulcp-i2c`), with
  nothing fitted.

Implemented in [`firmware-esp32/firmware/esp32-tracker/`][src], selected by the
`board-xiao-esp32s3` feature, with the board's pins in
[`umsh-bsp-xiao-esp32s3`][bsp]. Electrical detail lives in
`docs/hardware/seeed-xiao-esp32s3-wio-sx1262-kit-hardware.md`.

[src]: https://github.com/darconeous/umsh/tree/main/firmware-esp32/firmware/esp32-tracker
[bsp]: https://github.com/darconeous/umsh/tree/main/firmware-esp32/crates/umsh-bsp-xiao-esp32s3
