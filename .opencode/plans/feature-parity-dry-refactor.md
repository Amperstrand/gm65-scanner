# Feature Parity + DRY Refactoring Plan

**Status:** APPROVED

## Phase 1: Add futures dev-dep + async mock UART tests

### 1a. Add futures dev-dependency

File: `crates/gm65-scanner/Cargo.toml` line 42-43

Change:
```toml
[dev-dependencies]
```
to:
```toml
[dev-dependencies]
futures = "0.3"
```

### 1b. Add 28 async mock UART tests

File: `crates/gm65-scanner/src/driver/async_.rs`

Append `#[cfg(test)] mod tests` after the HIL tests module.

**MockAsyncUart:**
- `MockInner`: `read_queue: Vec<u8>`, `written: Vec<u8>`, `pending_responses: Vec<Vec<u8>>`
- `MockAsyncUart`: `inner: Rc<RefCell<MockInner>>`
- Implements `embedded_io_async::Read` + `embedded_io_async::Write`
- `read()` returns `Poll::Ready` immediately (data always available)
- `flush()` loads next pending response into read_queue
- Clone via `Rc::clone`

**Test runner:** `futures::executor::block_on()` wraps each async test.

** embassy-time risk:** `send_command()` uses `embassy_time::with_timeout()`. In std test mode without a time driver, this may panic. If so:
- Fallback: skip init_success/init_not_detected tests (which go through send_command with timeout)
- Test only non-timeout paths: ping, get_setting, set_setting, settings, release, into_parts, initial_state, cancel_scan (~20 tests)
- Gate init tests with `#[cfg(feature = "std")]` or similar if embassy-time works in std mode

**28 tests (26 ported from sync + 2 async-specific):**
1. test_mock_uart_write_read
2. test_mock_uart_flush_loads_response
3. test_mock_uart_empty_read_returns_pending
4. test_initial_state_uninitialized
5. test_ping_success
6. test_ping_failure_no_response
7. test_ping_command_bytes_on_wire
8. test_get_setting_returns_value
9. test_get_setting_invalid_response_returns_none
10. test_get_setting_no_response_returns_none
11. test_get_setting_command_bytes
12. test_set_setting_success
13. test_set_setting_command_bytes
14. test_get_scanner_settings_valid
15. test_get_scanner_settings_invalid_bits
16. test_set_scanner_settings
17. test_release_returns_uart
18. test_into_parts
19. test_cancel_scan (async-only)
20. test_init_success (may need embassy-time workaround)
21. test_init_not_detected (may need embassy-time workaround)
22. test_reinit_resets_state (may need embassy-time workaround)
23. test_trigger_scan_not_initialized
24. test_stop_scan_not_initialized
25. test_read_scan_not_initialized
26. test_trigger_and_stop_after_init (may need embassy-time workaround)

### 1c. Verify

```bash
cargo test -p gm65-scanner
cargo test -p gm65-scanner --features async,defmt
cargo clippy -p gm65-scanner -- -D warnings
cargo clippy -p gm65-scanner --features async,defmt -- -D warnings
```

---

## Phase 2: Extract init logic into ScannerCore

### 2a. Define InitAction enum in scanner_core.rs

```rust
pub enum InitAction {
    DrainAndRead(Register),
    ReadRegister(Register),
    WriteRegister(Register, u8),
    VerifyRegister(Register, u8),
    Complete(ScannerModel),
    Fail(ScannerError),
}
```

### 2b. Add init state machine methods to ScannerCore

New fields: `init_retry_count: u32`, `config_seq_index: usize`

Methods:
- `pub fn init_begin(&mut self) -> InitAction` — returns DrainAndRead(SerialOutput)
- `pub fn init_advance(&mut self, result: Option<u8>) -> InitAction` — state machine

State machine phases:
1. Probe: None -> Fail(NotDetected), Some(_) -> mark_detected -> ReadRegister(SerialOutput)
2. SerialOutput retry: None -> retry up to 3x then Fail, Some(val) -> check fix -> WriteRegister(SerialOutput, fixed) or WriteRegister(Settings, CMD_MODE)
3. SerialOutput fix: None -> Fail, Some(_) -> WriteRegister(Settings, CMD_MODE)
4. CMD_MODE: None -> Fail, Some(_) -> ReadRegister(config_seq[0].register)
5. Config read: None -> Fail, Some(val) -> if val != target: WriteRegister(reg, target), else: VerifyRegister(reg, target)
6. Config write: None -> Fail, Some(_) -> VerifyRegister(reg, target)
7. Config verify (defmt only): always advance. If more config: ReadRegister(next). Else: ReadRegister(Version)
8. Version check: None -> Complete(Gm65) (skip raw fix), Some(v) -> if needs_fix: ReadRegister(RawMode), else: Complete(Gm65)
9. RawMode read: None -> Complete, Some(val) -> if val != target: WriteRegister(RawMode, target), else: Complete
10. RawMode write: Complete(Gm65)

### 2c. Refactor sync do_init() to ~30-line InitAction loop

### 2d. Refactor async do_init() to ~30-line InitAction loop (with .await)

### 2e. Remove standalone probe_gm65() from both drivers

---

## Phase 3: Fix minor parity gaps

### 3a. Add defmt logging to async do_trigger_scan, do_stop_scan, save_settings

### 3b. Normalize imports: sync uses direct imports matching async style

---

## Phase 4: Update tests

### 4a. Add ~9 ScannerCore init state machine unit tests

### 4b. Update sync mock tests for InitAction-based init

### 4c. Async mock tests already done in Phase 1

---

## Phase 5: Full verification + commit

```bash
cargo test -p gm65-scanner
cargo test -p gm65-scanner --features async,defmt
cargo clippy -p gm65-scanner -- -D warnings
cargo clippy -p gm65-scanner --features async,defmt -- -D warnings
cargo build -p stm32f469i-disco-scanner --release --target thumbv7em-none-eabihf --bin stm32f469i-disco-scanner
cargo build -p stm32f469i-disco-scanner --release --target thumbv7em-none-eabihf --bin hil_test_sync
cargo clippy -p stm32f469i-disco-scanner --bin stm32f469i-disco-scanner --target thumbv7em-none-eabihf -- -D warnings
```

Commits:
1. test: add async mock UART tests (Phase 1)
2. refactor: extract init into ScannerCore InitAction state machine (Phase 2)
3. fix: defmt logging parity, normalize imports (Phase 3)
4. test: ScannerCore init state machine tests (Phase 4)
5. docs: update AGENTS.md test count (Phase 5)
