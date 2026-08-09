# Human Review & Testing Session Guide

## Session Date: August 2026
## HEAD: `a488d33` on `Amperstrand/gm65-scanner` main

---

## 1. What Changed (Commit Summary)

### Bug Fixes
| Commit | Issue | Description |
|--------|-------|-------------|
| `1702137` | #60 | auto_scan fix + scanner watchdog + touch debounce |
| `342ffca` | #61 | ScannerSettings: datasheet multi-bit fields (LIGHT/AIM/ReadMode) |
| `e567d5f` | — | Interrupt-driven UART ring buffer (512-byte Queue) |
| `57b2db0` | — | State machine: reset_to_ready after ScanComplete |
| `501caa3` | — | Remove QR mirror overwrite of text result |
| `5ad57d3` | #70 | Ring buffer: Mutex<RefCell<Queue>> (removed unsafe) |
| `f162f4a` | — | Fix async firmware duplicate DISPLAY_CENTER_X constants |

### Improvements
| Commit | Issue | Description |
|--------|-------|-------------|
| `a28f71a` | #62 | embedded-text TextBox replaces manual text wrapping |
| `962dcb8` | #66 | MockDisplay tests (5 new tests) |
| `41902f3` | #67 | include!() → proper mod declarations |
| `1888fc8` | #64 | Configurable ScannerConfig (timeout, interval, delay) |
| `52b3204` | #69 | Dead code cleanup |
| `a257612` | #69 | ScannerSettings extracted to settings.rs module |
| `a488d33` | #72 | ScannerIO trait (foundation for maybe-async unification) |
| `a655b88` | — | README bit layout corrected to datasheet V1.7 |
| `db25fc0` | #65 | Error handling audit (80 `let _ =` documented) |

### Test Results
- **170/170** library tests pass
- **4/4** binaries build (sync fw, async fw, sync HIL, async HIL)
- **Scanner**: connected=1, settings 0xD1 (LIGHT off, AIM reading, buzzer on)

---

## 2. Pre-Test Setup

### On ai-legion (Linux machine):
```bash
# Pull latest
cd ~/gm65-scanner
git fetch origin && git reset --hard origin/main

# Build sync firmware
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin stm32f469i-disco-scanner --no-default-features --features sync-mode

# Convert and flash
arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/stm32f469i-disco-scanner /tmp/sync_fw.bin
st-flash --connect-under-reset write /tmp/sync_fw.bin 0x08000000
st-flash --connect-under-reset reset
sleep 12
```

### Verify firmware booted:
```bash
# Check USB
lsusb | grep 16c0

# Check CDC port (may be ACM0 or ACM1)
for p in /dev/ttyACM0 /dev/ttyACM1; do
  echo -n "$p: "; udevadm info $p 2>/dev/null | grep ID_MODEL= | cut -d= -f2
done

# Use the QR_Barcode_Scanner port for CDC commands
python3 -c "
import serial, time
s = serial.Serial('/dev/ttyACM1', 115200, timeout=3)
time.sleep(1)
s.write(b'\x10\x00\x00')
time.sleep(2)
r = s.read(64)
print('Scanner connected=%d' % (r[3] if len(r) > 3 else 0))
s.close()
"
```

**Expected**: `Scanner connected=1`

---

## 3. Display Tests

### Test 1: Home Screen
**Action**: Look at the LCD after boot
**Expected**: Home screen with scanner status, settings button at bottom
**Pass criteria**: Text readable, colors correct (dark background, cyan/white text)

### Test 2: Settings Navigation
**Action**: Tap "Settings" button (bottom of home screen)
**Expected**: Settings page with toggle rows (Sound, Aim, Light, Mode)
**Pass criteria**: Screen changes on tap, no multi-toggle (debounce working)

### Test 3: Toggle Settings
**Action**: Tap each toggle on settings page
**Expected**: Each toggle visually changes state
**Pass criteria**: 
- Sound toggle: switches ON/OFF
- Aim toggle: switches ON/OFF  
- Light toggle: switches ON/OFF
- Mode toggle: switches Command/Continuous

### Test 4: Back Navigation
**Action**: Tap "Back" button on settings page
**Expected**: Returns to home screen
**Pass criteria**: Home screen displayed, auto-scan resumes

### Test 5: Touch Debounce
**Action**: Rapidly tap a toggle 5 times in 1 second
**Expected**: Toggle changes ONCE (not 5 times)
**Pass criteria**: No multi-toggle from single touch

---

## 4. Scanner Tests

### Test 6: Scanner Status
```bash
python3 -c "
import serial, time
s = serial.Serial('/dev/ttyACM1', 115200, timeout=3)
time.sleep(1)
s.write(b'\x10\x00\x00')
time.sleep(2)
r = s.read(64)
print('Status: %s connected=%d' % (r.hex(), r[3] if len(r) > 3 else 0))
s.close()
"
```
**Expected**: `connected=1`

### Test 7: Settings Verification
```bash
python3 -c "
import serial, time
s = serial.Serial('/dev/ttyACDM1', 115200, timeout=3)
time.sleep(1)
s.write(b'\x13\x00\x00')
time.sleep(2)
r = s.read(64)
raw = r[3] if len(r) > 3 else 0
print('Settings: 0x%02x' % raw)
print('  LIGHT=%d (0=off)' % ((raw>>2)&3))
print('  AIM=%d (1=reading)' % ((raw>>4)&3))
print('  Buzzer=%d (1=on)' % ((raw>>6)&1))
s.close()
"
```
**Expected**: `0xD1` — LIGHT=0, AIM=1, Buzzer=1

### Test 8: QR Code Scan (CRITICAL)
**Action**: Hold a QR code 5-10cm from the scanner lens
**Expected**:
- AIM laser pattern activates (red targeting lines)
- Buzzer beeps on successful scan
- LCD shows scan result with full decoded text
- Green LED blinks 3 times
- Text wraps at word boundaries (embedded-text)

**Pass criteria**: Full QR code content displayed on LCD, not truncated

### Test 9: Scanner Auto-Scan Recovery
**Action**: Let scanner run for 30 seconds without QR code
**Expected**: Scanner continuously triggers (every ~2 seconds), no freeze
**Pass criteria**: Scanner doesn't get stuck, CDC commands still respond

### Test 10: CDC Trigger
```bash
python3 -c "
import serial, time
s = serial.Serial('/dev/ttyACM1', 115200, timeout=5)
time.sleep(1)
s.write(b'\x11\x00\x00')
time.sleep(2)
r = s.read(64)
print('Trigger: %s' % r.hex())
s.write(b'\x12\x00\x00')
time.sleep(2)
r = s.read(256)
if len(r) > 2 and r[2] > 0:
    print('Scan data: %s' % r[3:3+r[2]])
else:
    print('No scan data (expected if no QR code)')
s.close()
"
```

---

## 5. Sync HIL Test

```bash
# Flash HIL test binary
st-flash --connect-under-reset write /tmp/hil_sync.bin 0x08000000
st-flash --connect-under-reset reset

# If probe-rs is available:
probe-rs run --probe 0483:374b:066FFF515786534867184152 --chip STM32F469NIHx \
  target/thumbv7em-none-eabihf/release/hil_test_sync
```

**Expected**: 5/5 PASS (init, ping, trigger/stop, timeout, state transitions)

---

## 6. Known Issues & Risks

### Confirmed Working
- ✅ Display renders correctly (user confirmed)
- ✅ Touch navigation works (user confirmed)
- ✅ Settings correct (LIGHT off, AIM reading, buzzer on)
- ✅ Scanner detection via CDC
- ✅ HIL 5/5 PASS
- ✅ QR code captured (18 bytes via HIL test)
- ✅ Auto-scan state machine cycles (trigger → scan → reset → re-trigger)

### Needs Human Verification
- ⚠️ Production firmware QR capture via auto-scan (interrupt-driven ring buffer)
- ⚠️ Full text display on LCD after QR scan (embedded-text wrapping)
- ⚠️ Touch debounce prevents multi-toggle
- ⚠️ Scanner watchdog recovery from stuck state
- ⚠️ Buzzer beep on successful scan

### Known Limitations
- Production QR capture requires scanner to be in clean state (run HIL test first to clean)
- CDC port may swap between ACM0 and ACM1 after USB reset
- probe-rs was wiped from ai-legion (needs reinstall for RTT/async HIL)

---

## 7. Open Issues (After This Session)

| # | Title | Priority | Status |
|---|-------|----------|--------|
| #71 | Embassy RingBufferedUartRx | High | Planned — needs probe-rs |
| #72 | embedded-test framework | Medium | Planned — needs probe-rs |
| #73 | Migrate to embedded-io | High | ScannerIO foundation done, full migration needs focused session |
| #22 | nt35510 init improvements | Low | Needs human review of init sequence |
| #21 | RGB565 dual pixel format | Low | RGB888 works, lower priority |

---

## 8. Recommended Next Steps

### Immediate (this session)
1. Power-cycle scanner (unplug/replug GM65 USB cable)
2. Flash sync firmware
3. Run tests 1-10 above
4. Report results

### Next Session
1. If QR capture works → undraft VLS MR !835, submit HAL PR to stm32-rs
2. Install probe-rs → run async HIL 9/9
3. Implement #71 (RingBufferedUartRx) for async firmware
4. Begin #73 (embedded-io migration + maybe-async unification)

### Long Term
1. Full maybe-async driver unification (#72/#73)
2. embedded-test framework (#72)
3. VLS BSP pin bump
4. HAL upstream PR
