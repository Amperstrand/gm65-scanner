# Agent Reference

## Hardware Test Evidence

All testing performed on STM32F469I-Discovery board with GM65 module connected via USART6 (PG14=TX, PG9=RX) through shield-lite Arduino headers. GM65 firmware version: 0x87.

### Known-Good Pin

| Commit | Notes |
|--------|-------|
| `f1d694d` (main HEAD) | Full HIL verification: sync 6/6, async 9/9, QR scans on both. 118 unit tests, 28 mock UART tests. |

### Test Date: 2026-03-26

Testing performed by the **micronuts** firmware (Amperstrand/micronuts), which depends on this crate for QR scanner communication via async USART6.

| Subsystem | Status | Evidence | Notes |
|-----------|--------|----------|-------|
| **Scanner init** | PASS | Gm65 detected, firmware 0x87, settings 0x81 | USART6 at 115200 baud, auto-detect works |
| **Trigger scan** | PASS | 23 bytes received from QR code scan | Aim laser enables/disables correctly |
| **Ping** | PASS | ACK received after init | |
| **Settings read** | PASS | 0x81 (ALWAYS_ON\|COMMAND mode) | |
| **Settings write** | PASS | 0x05→0x01, 0x00→0x01 accepted | Aim laser toggle also works |
| **HIL core tests (5/5)** | PASS | init_detects_scanner, ping, trigger_and_stop, read_scan_timeout, state_transitions | |
| **HIL extended (2/3)** | 2 PASS, 1 known test bug | cancel_then_rescan PASS, rapid_triggers FAIL (test expectation wrong — trigger is idempotent), read_idle_no_trigger PASS |

### Test Date: 2026-03-28

Native HIL test binaries (not via micronuts). Both sync and async drivers verified end-to-end with real QR code scans.

#### Async HIL: 9/9 PASS

| Test | Status | Evidence |
|------|--------|----------|
| init_detects_scanner | PASS | GM65 detected, fw 0x87, settings 0x81 |
| ping_after_init | PASS | ACK received |
| trigger_and_stop | PASS | Trigger ACK, stop ACK |
| read_scan_timeout | PASS | Ambient barcode tolerated (scanner working correctly) |
| state_transitions | PASS | Re-init resets to Ready |
| cancel_then_rescan | PASS | Cancel + re-trigger, 25 bytes from rescan |
| rapid_triggers | PASS | 5 rapid trigger/stop cycles, state Error(Timeout) |
| read_idle_no_trigger | PASS | Correctly times out without trigger |
| **run_hil_test_with_qr** | **PASS** | **25 bytes scanned with aim laser + PG6 LED blink** |

#### Sync HIL: 6/6 PASS

| Test | Status | Evidence |
|------|--------|----------|
| init_detects_scanner | PASS | GM65 detected, fw 0x87, settings 0x81 |
| ping_after_init | PASS | ACK received |
| trigger_and_stop | PASS | Trigger ACK, stop ACK |
| read_scan_timeout | PASS | Ambient barcode tolerated |
| state_transitions | PASS | Re-init resets to Ready |
| **run_hil_test_with_qr** | **PASS** | **Scanned with aim laser, 50-retry loop (5s window)** |

### Known Issues

#### drain_uart() Data Loss (#12) — FIXED

`send_command()` now skips `drain_uart()` when the scanner is in `Scanning` state.

#### BarType Register Not Persisted (#10)

Register 0x002C (BarType) write is accepted but not persisted across GM65 reboots on firmware 0.87. GM65 hardware quirk — no fix possible.

#### Settings Mode Comparison (#11)

0x81 (ALWAYS_ON|COMMAND) vs 0xD1 (ALWAYS_ON|SOUND|AIM|COMMAND) — not yet compared. Current micronuts firmware uses 0x81.

#### LCD GRAM Retention (#5)

LCD retains previous frame briefly after power-cycle. Likely GRAM vs SDRAM behavior — not a scanner issue.

#### Double-Buffering Breaks USB (#4)

Using `set_layer_buffer_address()` to implement double-buffering breaks USB composite device on the old sync BSP. Using single-buffer workaround on the embassy BSP. Not a scanner issue.

#### sdram_pins! Macro Requires `alt` Import

The `sdram::sdram_pins!` macro from `stm32f469i-disc` expands to paths like `alt::A0`, `alt::D0`, etc. Callers must import `stm32f469i_disc::sdram::alt` (or `hal::gpio::alt::fmc as alt`) for the macro to resolve. This is a `#[doc(hidden)]` re-export in the BSP's sdram module.

## ST-LINK Lockup / "Interface is Busy" — Causes & Recovery

### Root Causes

There are 4 distinct lockup states, each with different symptoms and fixes:

### 1. Stale probe-rs Process (most common)

**Symptom:** `interface is busy (errno 16)` or `Failed to open probe`

**Cause:** A background `probe-rs run` or `probe-rs attach` was killed (SIGTERM/SIGKILL) but the ST-LINK USB device wasn't properly released. The kernel still has the USB device claimed.

**Provokes it:** Any `probe-rs run & ... ; kill $PID` pattern. The `kill` doesn't let probe-rs gracefully disconnect from the ST-LINK.

**Fix:**
```
pkill -9 probe-rs; sleep 3-5
```
If that doesn't work, try the USB bus unbind/rebind (State 4) or physically unplug and replug the ST-LINK USB cable.

### 2. JTAG DMA Error (SDRAM/FMC contention)

**Symptom:** `JtagDmaError` when trying to `probe-rs attach` or `probe-rs run` AFTER flashing/running firmware that uses SDRAM.

**Cause:** The STM32F469's FMC (Flexible Memory Controller) is active (SDRAM configured). Debug accesses that go through the AHB matrix conflict with the FMC's DMA-like access patterns. The ST-LINK's debug access port gets starved or the DAP returns errors.

**Provokes it:** Flashing and running the main firmware (which inits SDRAM via FMC), then trying to attach with probe-rs. Especially bad if probe-rs was previously running (state 1 compounds this).

**Fix:**
```
pkill -9 probe-rs; sleep 5
probe-rs reset --chip STM32F469NIHx
sleep 2
# now safe to download/run/attach
```
The `reset` without `--connect-under-reset` does a simple system reset which reinitializes FMC but doesn't try to read memory. If that fails too, try xHCI PCI rescan (State 4) or power cycle the board.

### 3. CDC Port Confusion (not really a lockup, but wastes time)

**Symptom:** `ser.read()` returns empty bytes, `No such file or directory` on ttyACM1

**Cause:** When probe-rs is connected to the ST-LINK, it may create its own CDC endpoint or hold the debug interface in a way that affects USB enumeration. The firmware's CDC device appears on an unpredictable ACM port:
- Standalone (no probe-rs): `/dev/ttyACM0`
- With probe-rs attached: `/dev/ttyACM1` (probe-rs takes ttyACM0)

**Provokes it:** Using `probe-rs download` + `probe-rs reset` leaves probe-rs connected. Using `probe-rs run` holds the probe.

**Fix:** Always check BOTH ports:
```python
for port in ['/dev/ttyACM0', '/dev/ttyACM1']:
    # try serial open + test command
```
Or use VID/PID matching (`0x16c0:0x27dd`) to find the right port.

**Better approach for CDC testing:** Use `probe-rs download` then `probe-rs reset`, then `pkill -9 probe-rs; sleep 2`. This disconnects probe-rs and the firmware's CDC will be on `/dev/ttyACM0`.

### 4. xHCI Host Controller Died (severe — software fix available)

**Symptom:** `lsusb` shows only root hubs (no devices at all). `probe-rs list` shows nothing. `dmesg` shows `xHCI host controller not responding, assume dead` and `HC died; cleaning up`.

**Cause:** Repeated `probe-rs run` + kill cycles, or abrupt USB device disconnects, can crash the xHCI host controller on the machine. The controller enters a dead state and all USB devices on all buses disappear. On this machine (AMD 500 Series chipset), the xHCI controller is at PCI address `0000:02:00.0`.

**Provokes it:** Multiple `probe-rs run & ... ; kill` cycles without proper cleanup. Compounds State 1 when `pkill -9` alone doesn't recover.

**Fix (PCI remove/rescan — no physical access needed):**
```bash
# 1. Kill any remaining probe-rs
pkill -9 probe-rs; sleep 2

# 2. Remove the xHCI controller from PCI bus
echo 1 | sudo tee /sys/bus/pci/devices/0000:02:00.0/remove

# 3. Wait 2 seconds
sleep 2

# 4. Trigger PCI bus rescan — controller and all devices re-enumerate
echo 1 | sudo tee /sys/bus/pci/rescan

# 5. Verify ST-LINK is back
sleep 3
probe-rs list
```

**If PCI address changes** (different machine), find it with:
```bash
sudo lspci -nn | grep -i "USB controller.*xHCI"
```
Use the `XXXX:XX:XX.X` address from that output.

**If PCI rescan doesn't work**, physically unplug the main USB cable from the Discovery board, wait 10 seconds, and replug.

### Prevention Rules

1. **Always `pkill -9 probe-rs; sleep 3` before any probe-rs operation** — never assume the previous session cleaned up.

2. **Never do `probe-rs run & ... ; kill`** if you need to use CDC afterward — the kill leaves ST-LINK dirty. Use `probe-rs download` + `probe-rs reset` instead.

3. **After flashing SDRAM-using firmware, add a longer sleep** (5s) before the next probe-rs command. The SDRAM init sequence can leave the bus in a state that makes immediate debug access fail.

4. **Use `probe-rs download` for flashing, NOT `probe-rs run`** — `download` flashes and returns. `run` holds the probe forever and blocks RTT from being read separately.

5. **For RTT logging, use `probe-rs run` in background** with a timeout, accept that you can't use CDC simultaneously. For CDC testing, use `download` + `reset` + disconnect probe-rs.

6. **The defmt RTT sometimes doesn't produce output** on the main firmware binary (but works on hil_test_sync). Root cause unknown — possibly defmt version conflict (BSP uses 0.3.x, workspace uses 1.0.x). Don't waste time debugging this; use CDC for main firmware testing.

7. **If xHCI controller dies**, use PCI remove/rescan before reaching for the physical cable. It saves a trip to the hardware.

## USB CDC Testing Protocol

For reliable CDC testing:
```
pkill -9 probe-rs; sleep 2
probe-rs download --chip STM32F469NIHx <binary>
probe-rs reset --chip STM32F469NIHx
sleep 5
python3 tests/hil_test.py --port /dev/ttyACM0 protocol
```

If ttyACM0 doesn't respond, try ttyACM1 (probe-rs may still be partially connected).

## HIL Test Protocol

For on-device HIL tests (RTT output):
```
pkill -9 probe-rs; sleep 2
probe-rs run --chip STM32F469NIHx <binary> &
PID=$!
sleep 25
kill $PID 2>/dev/null
wait $PID 2>/dev/null
```

## xHCI Recovery Quick Reference

```bash
# One-liner to recover from dead xHCI controller
pkill -9 probe-rs; sleep 2
echo 1 | sudo tee /sys/bus/pci/devices/0000:02:00.0/remove
sleep 2
echo 1 | sudo tee /sys/bus/pci/rescan
sleep 3
probe-rs list
```

To find the PCI address on a different machine:
```bash
sudo lspci -nn | grep -i "xHCI"
```

## Why Not Specter-DIY USB Settings

The specter-diy project does NOT disable USB — it simply doesn't implement a USB device at all. It's a pure UART + LCD firmware. The GM65 scanner's settings register (0x0000) has no USB-related bits. All USB issues are STM32/ST-LINK debug probe issues, not scanner-related.

## Upstream Interaction Policy

**NEVER file PRs or issues on upstream projects (embassy-rs, stm32-rs, etc.) without human review and approval.** AI-generated bug diagnoses can be confidently wrong. If you find a potential upstream bug:
1. Document your findings in an Amperstrand repo issue first
2. Include all evidence (register dumps, test results, methodology)
3. Let a human decide whether to escalate

See [Amperstrand/micronuts#19](https://github.com/Amperstrand/micronuts/issues/19) for a retrospective on how a confident misdiagnosis wasted upstream maintainer time.
