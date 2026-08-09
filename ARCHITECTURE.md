# gm65-scanner Architecture Document

## Purpose

This document describes the architecture of the gm65-scanner project — a dual
sync/async Rust driver for GM65/M3Y QR barcode scanner modules, with production
firmware for the STM32F469I-Discovery board. The project serves as:

1. **A publishable `no_std` library crate** (`gm65-scanner`) for any Rust embedded project using GM65 scanners
2. **A reference implementation** for VLS (Validating Lightning Signer) on STM32F469, demonstrating display, touch, scanner, and USB CDC integration
3. **A testbed for sync vs async embedded patterns** — both firmware variants run on identical hardware, enabling direct comparison

---

## 1. System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Host Computer (Linux)                      │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │ CDC Client  │  │ Diagnostic   │  │ HIL Test Runner   │  │
│  │ (Python)    │  │ Monitor      │  │ (probe-rs)        │  │
│  └──────┬──────┘  └──────┬───────┘  └────────┬──────────┘  │
│         │                │                    │              │
└─────────┼────────────────┼────────────────────┼──────────────┘
          │ USB             │ USB                │ SWD
          │ CDC-ACM         │ CDC-ACM            │ (ST-Link)
          │                 │                    │
┌─────────┼────────────────┼────────────────────┼──────────────┐
│         ▼                ▼                    ▼              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │              STM32F469NIHx (Cortex-M4 @ 180MHz)         │ │
│  │                                                          │ │
│  │  ┌────────────────────────────────────────────────────┐  │ │
│  │  │              Firmware (no_std, no_main)            │  │ │
│  │  │                                                     │  │ │
│  │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │  │ │
│  │  │  │ USB CDC  │  │ Display  │  │ Touch (FT6X06)   │  │  │ │
│  │  │  │ Protocol │  │ DSI/LTDC │  │ I2C1             │  │  │ │
│  │  │  └────┬─────┘  └────┬─────┘  └────────┬─────────┘  │  │ │
│  │  │       │              │                  │            │  │ │
│  │  │  ┌────▼──────────────▼──────────────────▼─────────┐ │  │ │
│  │  │  │           Main Loop (sync) / Executor (async)  │ │  │ │
│  │  │  │                                                    │ │  │ │
│  │  │  │  ┌─────────────────────────────────────────┐    │ │  │ │
│  │  │  │  │       gm65-scanner library crate        │    │ │  │ │
│  │  │  │  │                                          │    │ │  │ │
│  │  │  │  │  ┌─────────┐   ┌──────────────────┐    │    │ │  │ │
│  │  │  │  │  │ Scanner │   │  ScannerCore     │    │    │ │  │ │
│  │  │  │  │  │ Driver  │──▶│  (state machine) │    │    │ │  │ │
│  │  │  │  │  │ (I/O)   │   │                  │    │    │ │  │ │
│  │  │  │  │  └────┬────┘   └────────┬─────────┘    │    │ │  │ │
│  │  │  │  │       │                 │              │    │ │  │ │
│  │  │  │  │  ┌────▼────┐   ┌────────▼─────────┐   │    │ │  │ │
│  │  │  │  │  │ UART    │   │  Protocol +     │   │    │ │  │ │
│  │  │  │  │  │ Ring    │   │  Buffer +       │   │    │ │  │ │
│  │  │  │  │  │ Buffer  │   │  Decoder        │   │    │ │  │ │
│  │  │  │  │  │ (ISR)   │   │                 │   │    │ │  │ │
│  │  │  │  │  └────┬────┘   └─────────────────┘   │    │ │  │ │
│  │  │  │  └───────┼──────────────────────────────┘    │ │  │ │
│  │  │  └──────────┼────────────────────────────────────┘ │  │ │
│  │  └─────────────┼──────────────────────────────────────┘ │ │
│  └────────────────┼────────────────────────────────────────┘ │
│                   │ USART6 (PG14=TX, PG9=RX, 115200 baud)    │
│                   ▼                                            │
│          ┌────────────────┐                                    │
│          │  GM65 Scanner  │◄──── QR code input                 │
│          │  Module (M3Y)  │                                    │
│          └────────────────┘                                    │
└────────────────────────────────────────────────────────────────┘
```

---

## 2. Library Crate Architecture (`gm65-scanner`)

### 2.1 Module Structure

```
gm65-scanner/
├── src/
│   ├── lib.rs              (90 lines)   — crate root, public exports
│   ├── buffer.rs           (262 lines)  — ScanBuffer: EOL-terminated UART data accumulation
│   ├── decoder.rs          (488 lines)  — payload classification (Cashu, UR, plaintext)
│   ├── display_util.rs     (306 lines)  — text wrapping/visibility math (testable, no hardware)
│   ├── protocol.rs         (524 lines)  — GM65 command frame encoding/decoding
│   ├── scanner_core.rs     (1470 lines) — ScannerCore: sans-IO state machine + init sequence
│   ├── settings.rs         (91 lines)   — ScannerSettings + config constants
│   └── driver/
│       ├── mod.rs          (66 lines)   — module root, public re-exports
│       ├── types.rs        (222 lines)  — ScannerConfig, ScannerError, ScannerState, etc.
│       ├── traits.rs       (93 lines)   — ScannerDriverSync + ScannerDriver traits
│       ├── io.rs           (87 lines)   — ScannerIO abstraction (foundation for maybe-async)
│       ├── sync.rs         (981 lines)  — Gm65Scanner<UART>: blocking driver
│       ├── async_.rs       (1148 lines) — Gm65ScannerAsync<UART>: embassy async driver
│       └── test_helpers.rs (58 lines)   — mock UART for unit tests
├── tests/                  — 170 unit tests
└── Cargo.toml
```

### 2.2 Sans-IO Core Pattern

The library follows the **sans-IO core** pattern (recommended by the Rust Embedded WG):

```
┌─────────────────────────────────────────────────┐
│              ScannerCore (sans-IO)               │
│                                                   │
│  • State machine (Uninitialized → Ready →        │
│    Scanning → ScanComplete → Ready)              │
│  • Init sequence driver (InitAction enum)        │
│  • ScanBuffer (EOL-terminated accumulation)      │
│  • Settings management                           │
│  • Zero I/O dependencies                         │
│                                                   │
│  Testable without hardware (170 tests)           │
└──────────────────┬──────────────────────────────┘
                   │
          ┌────────┴────────┐
          │                 │
┌─────────▼──────┐ ┌────────▼──────────┐
│  Sync Driver   │ │  Async Driver     │
│  (sync.rs)     │ │  (async_.rs)      │
│                │ │                   │
│  embedded-hal  │ │  embedded-io-async│
│  0.2 (nb)      │ │  (impl Future)    │
│                │ │                   │
│  Gm65Scanner   │ │  Gm65ScannerAsync │
│  <UART>        │ │  <UART>           │
└────────────────┘ └───────────────────┘
```

**Key principle**: All protocol logic, state transitions, and buffer management
live in `ScannerCore` with zero I/O dependencies. Both sync and async drivers
share the same core. Only the I/O layer differs.

### 2.3 Protocol Details

The GM65 protocol was reverse-engineered from [specter-diy](https://github.com/cryptoadvance/specter-diy)
because the official datasheet is incorrect in several places:

**Command format**:
```
[7E 00] [type] [len] [addr_hi addr_lo] [data...] [AB CD]
```

**Response format** (NOT as documented in datasheet):
```
[02 00 00 01] [value] [33 31]
```

**Key discovery**: The `AB CD` suffix is NOT a CRC — it's a constant sentinel.

### 2.4 Settings Register (0x0000) — Datasheet V1.7

Multi-bit field layout (NOT individual bitflags):

```
Bit  7:    Always-on (no sleep)
Bit  6:    Mute off (buzzer enabled)
Bits 5-4:  AIM/Collimation  (00=Off, 01=On-when-reading, 1x=Always)
Bits 3-2:  LIGHT/Headlamp    (00=Off, 01=On-when-reading, 1x=Always)
Bits 1-0:  Read mode         (00=Manual, 01=Command, 10=Continuous, 11=Induction)
```

Default: `0xD1` (Command mode, AIM on-when-reading, LIGHT off, buzzer on)
Production: `0xD2` after init (switch to Continuous mode for scanning)

---

## 3. Firmware Architecture

### 3.1 Sync Firmware (`stm32f469i-disco-scanner`)

**Architecture**: Single-threaded polling main loop.

```
┌─────────────────────────────────────────────┐
│              Main Loop (infinite)            │
│                                              │
│  1. Poll USB CDC → dispatch commands         │
│  2. Check auto-scan → trigger if Ready       │
│  3. Scanner watchdog → recover if stuck      │
│  4. Self-healing → re-init after 3 failures  │
│  5. Poll scanner UART (200 iterations)       │
│     → try_read_scan()                        │
│     → if data: render + LED blink            │
│  6. Poll touch I2C → handle tap              │
│                                              │
│  UART RX: USART6 interrupt → ring buffer     │
│  (cortex_m::interrupt::Mutex<RefCell<Queue>>)│
└─────────────────────────────────────────────┘
```

**Key components**:
- `scanner_uart.rs`: Interrupt-driven ring buffer (512-byte Queue, ISR fills from USART6)
- `cdc.rs`: 3-byte framed CDC protocol (Command + payload)
- `display.rs`: embedded-graphics rendering (home, settings, scan result)
- `main.rs`: Hardware init + main loop + CDC diagnostic system

**Boot sequence**:
```
1. Take peripherals, configure RCC (180MHz SYSCLK)
2. Clear SDRAM framebuffer (0xFF000000 = opaque black)
3. Init display (DSI + LTDC + NT35510)
4. Init USB CDC (OTG FS)
5. Init scanner UART (USART6, split TX/RX, enable RXNE interrupt)
6. Scanner init() — 15-step init sequence in Command mode
7. Switch to Continuous mode (fire-and-forget write + drain)
8. Init touch (I2C1, FT6X06)
9. Render home screen
10. Enter main loop
```

### 3.2 Async Firmware (`async_firmware`)

**Architecture**: Embassy executor with concurrent tasks.

```
┌─────────────────────────────────────────────┐
│           Embassy Executor (async)           │
│                                              │
│  Task: USB CDC (reads commands, writes resp) │
│  Task: Scanner (trigger/scan/poll cycle)     │
│  Task: Display (renders current state)       │
│  Task: Touch (EXTI interrupt on PJ5)         │
│  Task: LED (blink feedback)                  │
│  Task: Heartbeat (health monitoring)         │
│                                              │
│  Communication via heapless Channels         │
│  Shared state via Mutex<RefCell<Shared>>     │
└─────────────────────────────────────────────┘
```

**Key differences from sync**:
- Uses embassy-stm32 HAL (not stm32f4xx-hal)
- UART is blocking (`Uart::new_blocking`) wrapped in `AsyncUart` struct
- Touch is interrupt-driven (EXTI on PJ5, not polling I2C)
- Tasks communicate via embassy Channels (not shared variables)
- Timeouts use `embassy_time::with_timeout` (wall-clock, not spin-loop)

---

## 4. Sync vs Async Comparison

### 4.1 Feature Matrix

| Feature | Sync | Async (Embassy) |
|---------|------|-----------------|
| **HAL** | stm32f4xx-hal 0.23 | embassy-stm32 0.6 |
| **I/O model** | Polling + ISR ring buffer | Embassy tasks with `.await` |
| **Concurrency** | Sequential main loop | Cooperative multitasking |
| **UART RX** | USART6 interrupt → Mutex<Queue> | `Uart::new_blocking` + AsyncUart wrapper |
| **Touch** | I2C polling every loop iteration | EXTI interrupt (PJ5) |
| **Timeout** | Spin-loop (fixed iteration count) | `with_timeout` (wall-clock ms) |
| **State sharing** | Local variables in main loop | `Mutex<RefCell<Shared>>` + Channels |
| **Memory** | No heap needed for I/O | Requires heap allocator |
| **USB CDC** | `usb-device` polling | Embassy USB class |
| **Display** | Same (embedded-graphics) | Same (embedded-graphics) |
| **Code size** | ~60KB binary | ~2.7MB binary (with embassy) |
| **HIL tests** | 5 core + 1 QR (RTT) | 5 core + 3 extended + 1 QR (RTT) |
| **Lines of code** | ~4,200 (firmware) | ~4,300 (firmware) |

### 4.2 When Sync Wins

1. **Simplicity**: No executor, no tasks, no channels — just a loop
2. **Deterministic timing**: Every iteration takes the same time (no task scheduling)
3. **No heap**: Works without alloc — everything on stack or static
4. **Small binary**: ~60KB vs ~2.7MB (embassy framework overhead)
5. **Debugging**: Single call stack — no async state machine to trace
6. **USB CDC**: Direct polling — no task yield points to miss

### 4.3 When Async Wins

1. **Concurrency**: Scanner + USB + display + touch run simultaneously
2. **Wall-clock timeouts**: `with_timeout(5s)` is exact, not iteration-based
3. **Interrupt-driven touch**: No I2C polling — touch events wake the task
4. **Human-interaction scanning**: 5-second QR scan windows with precise timeout
5. **Composability**: Tasks can be added/removed without restructuring the main loop
6. **Future-proof**: Embassy is the emerging standard for Rust embedded async

### 4.4 Shared Vulnerability: UART Data Loss

**Both** sync and async share a critical limitation: the GM65 scanner sends data
asynchronously. If the firmware is busy (USB processing, display rendering, I2C
touch) when scan data arrives, bytes can be lost to UART overrun.

**Sync fix (implemented)**: Interrupt-driven ring buffer (USART6 ISR fills a
512-byte `heapless::spsc::Queue` via `Mutex<RefCell<Queue>>`). No data lost
during USB/display processing.

**Async fix (planned)**: Use Embassy's `RingBufferedUartRx` with DMA-backed
continuous reception. Eliminates the manual `AsyncUart` wrapper entirely.

### 4.5 The Continuous Mode Discovery

During testing we discovered that the GM65 in **Command mode** (bits 1-0 = 01)
ACKs `ScanEnable` commands but does not actually scan. The scanner hardware
appears to require **Continuous mode** (bits 1-0 = 10) to produce scan data.

**Pattern adopted** (matches specter-diy):
1. Init in Command mode (0xD1) for clean ACK/response communication
2. After init, switch to Continuous mode (0xD2) via fire-and-forget write
3. Scanner scans continuously; data flows through ring buffer automatically
4. No `trigger_scan()` needed — just read from the buffer

**Specter-diy** uses the identical pattern:
```python
# Configure in CMD_MODE
self.set_setting(SETTINGS_ADDR, self.CMD_MODE)
# Start scanning in CONT_MODE
self.set_setting(SCAN_ADDR, 1)  # ScanEnable
```

---

## 5. CDC Diagnostic Protocol

### 5.1 Command Set

| Code | Command | Purpose |
|------|---------|---------|
| 0x10 | ScannerStatus | Get scanner connection state |
| 0x11 | ScannerTrigger | Trigger a single scan |
| 0x12 | ScannerData | Read last scan data |
| 0x13 | GetSettings | Read scanner settings register |
| 0x14 | SetSettings | Write scanner settings + persist to NVRAM |
| 0x15 | DisplayQr | Render QR code on LCD |
| 0x16 | EnterSettings | Enter touch settings UI |
| **0x20** | **Diagnostic** | **Return all internal counters + scanner state** |
| **0x21** | **SelfTest** | **Quick health check (connected, settings, scans)** |

### 5.2 Diagnostic Response Format (0x20)

15-byte payload:
```
[0-1]   scan_count (u16 LE) — total QR codes captured
[2-3]   nak_count (u16 LE) — scanner timeout NAKs received
[4]     watchdog_count — state machine recovery events
[5]     reinit_count — self-healing re-init events
[6]     scanner_state — 0=Uninit, 1=Ready, 2=Scanning, 3=Complete, 4=Error
[7]     settings_raw — live readback from scanner (0xD2 = correct)
[8]     ring_buffer_len — bytes currently queued
[9-11]  isr_bytes (24-bit LE) — total bytes received by USART6 ISR
[12-13] isr_ore (u16 LE) — UART overrun errors
[14-15] isr_fires (u16 LE) — total USART6 interrupt fires
```

### 5.3 Why CDC Instead of RTT/defmt

- **probe-rs holds SWD** → blocks USB enumeration → can't test USB CDC
- **defmt-rtt** prevents USB enumeration → mutually exclusive with CDC testing
- **CDC diagnostic** has zero performance impact (counters are atomic, polled on demand)
- **No external tools needed** — any serial client can query diagnostics

---

## 6. Known Issues & Technical Debt

### 6.1 Sync/Async Driver Duplication (~800 lines)

`sync.rs` (981 lines) and `async_.rs` (1148 lines) implement nearly identical
protocol logic. Bugs must be fixed in both. The `ScannerIO` trait (`io.rs`)
is the foundation for unification via the `maybe-async` crate, but full
implementation requires making sync/async features mutually exclusive.

**Pattern from research**: The `maybe-async` crate (`#[maybe_async]` macro)
strips `async`/`await` in sync mode. Combined with `embedded-io` traits
(unified blocking/async byte stream interface), this would eliminate all
duplication. Demonstrated by [AP33772S-rs driver](https://github.com/scottgibb/AP33772S-rs).

### 6.2 `include!()` in Async Firmware

The async firmware uses `include!("../display.rs")` for cross-binary code
sharing. This prevents IDE support, proper module resolution, and test isolation.
The sync firmware was fixed (proper `mod display;`), but the async firmware
needs a shared library crate.

### 6.3 Scanner Hardware Quirks

- **BarType register (0x002C)**: Write ACKed but not persisted on fw 0x87
- **Command mode ScanEnable**: ACKs but doesn't actually scan
- **Settings persistence**: `save_settings` writes to EEPROM (~100K cycle limit)
- **Baud rate default**: Factory reset drops to 9600 (our firmware uses 115200)

### 6.4 No Display State Machine

Display rendering is ad-hoc: 6 render functions called directly from the main
loop. No formal `enum Screen` or navigation state. Touch handling uses
boolean flags (`in_settings`, `on_scan_result`).

**Recommended pattern** (from embedded-graphics research):
```rust
enum Screen { Home, Settings, ScanResult, Error }
impl Screen {
    fn render(&self, fb) { ... }
    fn handle_touch(&mut self, x, y) -> ScreenTransition { ... }
}
```

---

## 7. VLS Reference Implementation Notes

The gm65-scanner project serves as a reference for VLS on STM32F469. Key
patterns VLS can adopt:

### 7.1 What VLS Already Uses from This Ecosystem
- Same BSP (`stm32f469i-disc` @ `8942290`)
- Same HAL (`stm32f4xx-hal` @ `0c5bc3d`)
- Same NT35510 display driver
- Same FT6X06 touch driver
- Same PLLSAI display fix (`restore_pllsai_dividers`)

### 7.2 What VLS Can Learn from gm65-scanner

1. **Interrupt-driven UART ring buffer** (`scanner_uart.rs`): VLS uses
   `ScreenApprover` which displays invoice details and waits for touch.
   The ring buffer pattern prevents UART data loss during display rendering.

2. **CDC diagnostic protocol**: VLS could expose a similar diagnostic
   command for remote monitoring without probe-rs.

3. **Self-healing init**: If the scanner disconnects and reconnects,
   gm65-scanner re-initializes after 3 consecutive failures.

4. **Continuous mode scanning**: For VLS, the ScreenApprover needs the
   scanner to be ready when the user presents a QR code. Continuous mode
   ensures the scanner is always scanning.

5. **Settings register management**: The datasheet-correct multi-bit field
   interpretation (vs the incorrect individual bitflag pattern) is critical
   for controlling scanner behavior.

### 7.3 What VLS Does Differently

- VLS uses `ScreenApprover` which calls `wait_for_touch()` on row 9
  for approve/reject — a simpler touch pattern than gm65's settings UI
- VLS runs CLN (Core Lightning) node → more complex than gm65's scanner-only
- VLS uses the HAL's `Serial6` directly (no ring buffer) — this works
  because VLS doesn't have the same concurrent USB+scanner+touch pressure

---

## 8. Metrics

| Metric | Value |
|--------|-------|
| Library LOC | 5,798 |
| Firmware LOC | 4,268 |
| Unit tests | 170 (all passing) |
| HIL tests | Sync: 5/5, Async: 9/9 |
| Dependencies | 15 (workspace) |
| Binary size (sync) | ~60KB |
| Binary size (async) | ~2.7MB |
| Scanner UART baud | 115200 |
| Ring buffer size | 512 bytes |
| Display resolution | 480×800 (portrait, ARGB8888) |
| SDRAM framebuffer | 1.5MB (384,000 × u32) |

---

## 9. Areas for External Review

1. **Driver unification**: Is `maybe-async` the right approach? Should we
   migrate to `embedded-io` and drop `embedded-hal-02`?

2. **State machine design**: The watchdog/self-healing/cancel/reset flow
   has many edge cases. Is there a cleaner state machine pattern?

3. **CDC protocol**: Is the 3-byte framed format optimal? Should we use
   postcard/COBS serialization instead of hand-rolled framing?

4. **Display rendering**: Should we use `embedded-text` crate or keep
   manual `word_wrap`? The TextBox approach caused rendering issues.

5. **Error handling**: 80 `let _ =` in firmware. Is there a lightweight
   error logging pattern for `no_std` without defmt?

6. **Testing strategy**: 170 unit tests cover the library. Should we add
   `embedded-test` framework for on-device integration tests?

7. **VLS integration**: What's the minimal set of gm65-scanner code that
   VLS needs? Can we extract a `vls-scanner` crate?
