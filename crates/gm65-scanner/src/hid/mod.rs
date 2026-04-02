//! HID (Human Interface Device) primitives for barcode scanner output.
//!
//! This module provides reusable, no_std building blocks for converting
//! scanned barcode data into USB HID reports. These are **library primitives**
//! — they generate report data structures but do not perform USB I/O.
//!
//! Firmware is responsible for instantiating USB HID class interfaces
//! (e.g., via `usbd-human-interface-device` or `embassy_usb::class::hid`)
//! and writing the reports generated here.
//!
//! # Submodules
//!
//! - [`keyboard`] — HID keyboard wedge: maps barcode bytes to boot keyboard
//!   reports (Usage Page 0x07). Compatible with any application that accepts
//!   keyboard input.
//!
//! - [`pos`] — **Experimental**: HID POS barcode scanner reports (Usage Page
//!   0x8C) per USB-IF HID POS Usage Tables 1.02. Not yet wired into firmware.

pub mod keyboard;
pub mod pos;
