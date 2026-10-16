//! Board BSP for the Seeed XIAO ESP32S3 + Wio-SX1262 kit: an ESP32-S3R8
//! XIAO on Seeed's Wio-SX1262 for XIAO carrier.
//!
//! See `docs/hardware/seeed-xiao-esp32s3-wio-sx1262-kit-hardware.md` for
//! the authoritative hardware reference. Pin numbers here are GPIO
//! numbers.
//!
//! Shared-resource constraints:
//! - GPIO21 is both the carrier's USR button and the XIAO's active-low
//!   yellow LED. It is only ever an input: driving it would fight the
//!   button.
//! - GPIO33–37 belong to the in-package octal PSRAM.
//! - GPIO0 is the XIAO's BOOT button and a strapping pin; nothing here
//!   claims it.
//! - GPIO19/20 carry native USB, the only wired host link. There is no
//!   USB-UART bridge and no VBUS sense, and nothing senses the battery
//!   the carrier can charge.
#![no_std]

pub mod radio;

pub const BOARD_NAME: &str = "Seeed XIAO ESP32S3 + Wio-SX1262 Kit";

// SX1262 radio on the carrier. DIO3 powers the 1.8 V TCXO and DIO2
// drives the RF switch's transmit side; the receive side is RF_SW1,
// driven from GPIO38 (hardware doc §4).
pub const LORA_SCK: u8 = 7;
pub const LORA_MISO: u8 = 8;
pub const LORA_MOSI: u8 = 9;
pub const LORA_NSS: u8 = 41;
/// Pulled up on the carrier.
pub const LORA_RESET: u8 = 42;
pub const LORA_BUSY: u8 = 40;
pub const LORA_DIO1: u8 = 39;
/// RF switch receive enable (RF_SW1), active high.
pub const LORA_RX_ENABLE: u8 = 38;

/// USR button on the carrier: active low, 10 kΩ pull-up.
pub const USER_BUTTON: u8 = 21;
/// The USR button's pin, which the shutdown path borrows to await its
/// release and arm the deep-sleep wake (it is an RTC GPIO).
pub type UserButtonPin = esp_hal::peripherals::GPIO21<'static>;

/// Green status LED on the carrier, active high.
pub const STATUS_LED: u8 = 48;
/// Heartbeat pulse, in milliseconds: the usual status-LED flash.
pub const STATUS_LED_PULSE_MS: u64 = 20;

// I²C on the XIAO header's D4/D5, with nothing fitted.
pub const I2C_SDA: u8 = 5;
pub const I2C_SCL: u8 = 6;

/// 8 MB of PSRAM inside the ESP32-S3R8 package, wired octal.
pub const PSRAM_MODE: esp_hal::psram::PsramMode = esp_hal::psram::PsramMode::OctalSpi;

#[cfg(feature = "ulcp-i2c")]
pub mod i2c;

/// Shared controller and per-transaction device handle for host access.
pub type I2cBus = embassy_sync::mutex::Mutex<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    esp_hal::i2c::master::I2c<'static, esp_hal::Async>,
>;
pub type I2cHandle = embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice<
    'static,
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    esp_hal::i2c::master::I2c<'static, esp_hal::Async>,
>;
