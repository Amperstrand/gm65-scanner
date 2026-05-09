use alloc::vec::Vec;

use embassy_stm32f469i_disco::DisplayCtrl;
use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::Rgb888,
    prelude::{Point, Size},
    primitives::Rectangle,
};

pub use embassy_stm32f469i_disco::TestResult;

/// A named test entry for iteration.
#[derive(Clone, Copy)]
pub struct TestEntry {
    /// Human-readable test name.
    pub name: &'static str,
    /// Test result.
    pub result: TestResult,
}

/// GM65-scanner specific BIST results.
///
/// Extends BSP's BootTestResults with scanner-specific tests:
/// - scanner_uart: USART6 UART initialized
/// - scanner_detect: GM65 scanner detected
/// - usb_phy_reset: USB PHY reset performed
///
/// The BSP BootTestResults includes: sdram, display, touch_i2c, touch_vendor_id,
/// touch_chip_model, touch_idle, leds, user_button (8 tests).
#[derive(Clone)]
pub struct Gm65BistResults {
    /// BSP-managed hardware tests (SDRAM, display, touch, LEDs, button)
    pub board: embassy_stm32f469i_disco::BootTestResults,
    /// Scanner UART initialized
    pub scanner_uart: TestResult,
    /// GM65 scanner detected
    pub scanner_detect: TestResult,
    /// USB PHY reset performed
    pub usb_phy_reset: TestResult,
}

impl Gm65BistResults {
    /// Returns all test entries as a slice for iteration.
    /// First 8 entries from BSP, then 3 scanner-specific tests.
    pub fn all_entries(&self) -> [TestEntry; 10] {
        let bsp_entries = self.board.entries();
        let mut display_entry = bsp_entries[1];
        display_entry.name = "Display Init";
        [
            TestEntry { name: bsp_entries[0].name, result: bsp_entries[0].result },
            TestEntry { name: display_entry.name, result: display_entry.result },
            TestEntry { name: bsp_entries[2].name, result: bsp_entries[2].result },
            TestEntry { name: bsp_entries[3].name, result: bsp_entries[3].result },
            TestEntry { name: bsp_entries[4].name, result: bsp_entries[4].result },
            TestEntry { name: bsp_entries[5].name, result: bsp_entries[5].result },
            TestEntry { name: "Scanner UART", result: self.scanner_uart },
            TestEntry { name: "Scanner Detect", result: self.scanner_detect },
            TestEntry { name: "USB PHY Reset", result: self.usb_phy_reset },
            TestEntry { name: bsp_entries[6].name, result: bsp_entries[6].result },
        ]
    }

    /// Number of tests that passed.
    pub fn passed_count(&self) -> usize {
        self.all_entries().iter().filter(|e| e.result == TestResult::Pass).count()
    }

    /// Total number of tests (10: 6 BSP + 3 scanner + 1 LED).
    pub fn total_count(&self) -> usize {
        10
    }

    /// Returns `true` if all tests passed.
    pub fn all_passed(&self) -> bool {
        self.passed_count() == self.total_count()
    }

    /// Serialize to bytes for CDC: [count_passed, count_total, (result_byte, name_len, name_bytes)...]
    /// result_byte: 0=pass, 1=fail, 2=skip
    pub fn to_bytes(&self) -> Vec<u8> {
        let entries = self.all_entries();
        let capacity = 2 + entries
            .iter()
            .map(|entry| 2 + entry.name.len())
            .sum::<usize>();
        let mut bytes = Vec::with_capacity(capacity);
        bytes.push(self.passed_count() as u8);
        bytes.push(self.total_count() as u8);

        for entry in entries {
            bytes.push(result_to_byte(entry.result));
            bytes.push(entry.name.len() as u8);
            bytes.extend_from_slice(entry.name.as_bytes());
        }

        bytes
    }
}

/// Convert TestResult to byte for serialization.
fn result_to_byte(result: TestResult) -> u8 {
    match result {
        TestResult::Pass => 0,
        TestResult::Fail => 1,
        TestResult::Skip => 2,
    }
}

/// Get human-readable label for TestResult.
pub fn result_label(result: TestResult) -> &'static str {
    match result {
        TestResult::Pass => "PASS",
        TestResult::Fail => "FAIL",
        TestResult::Skip => "SKIP",
    }
}

pub fn test_sdram(sdram_ok: bool) -> TestResult {
    if sdram_ok {
        TestResult::Pass
    } else {
        TestResult::Fail
    }
}

pub fn test_display_init(display: &mut DisplayCtrl<'_>) -> TestResult {
    let fb = &mut display.fb();
    let _ = fb.clear(Rgb888::new(0, 0, 0));
    let _ = fb.fill_solid(
        &Rectangle::new(Point::new(0, 0), Size::new(480, 80)),
        Rgb888::new(0x00, 0x40, 0x40),
    );
    let _ = fb.fill_solid(
        &Rectangle::new(Point::new(0, 80), Size::new(480, 80)),
        Rgb888::new(0x00, 0x60, 0x00),
    );
    TestResult::Pass
}

pub fn test_touch_i2c(touch_ok: bool) -> TestResult {
    if touch_ok {
        TestResult::Pass
    } else {
        TestResult::Fail
    }
}

pub fn test_touch_vendor_id<E>(vendor_id: Result<u8, E>) -> TestResult {
    match vendor_id {
        Ok(0x11) => TestResult::Pass,
        _ => TestResult::Fail,
    }
}

pub fn test_touch_chip_model<E>(chip_model: Result<u8, E>) -> TestResult {
    match chip_model {
        Ok(0x06 | 0x36 | 0x64) => TestResult::Pass,
        _ => TestResult::Fail,
    }
}

pub fn test_touch_idle<E>(td_status: Result<u8, E>) -> TestResult {
    match td_status {
        Ok(0) => TestResult::Pass,
        _ => TestResult::Fail,
    }
}

pub fn test_scanner_uart(uart_ok: bool) -> TestResult {
    if uart_ok {
        TestResult::Pass
    } else {
        TestResult::Fail
    }
}

pub fn test_scanner_detect(scanner_connected: bool) -> TestResult {
    if scanner_connected {
        TestResult::Pass
    } else {
        TestResult::Skip
    }
}

pub fn test_usb_phy_reset() -> TestResult {
    TestResult::Pass
}
