//! The identity a board's application image presents on USB.

/// What the application image puts in its USB device descriptor.
///
/// `vid` and `pid` are the board vendor's own application IDs: the pair the
/// vendor's stock firmware reports. Never the bootloader's pair, which the
/// web flasher uses to recognize update mode, and never a pair in a
/// namespace the project does not control. See "Adding a board" in
/// `docs/firmware-architecture.md`.
pub struct UsbIdentity {
    pub vid: u16,
    pub pid: u16,
    pub product: &'static str,
    pub serial_number: &'static str,
}
