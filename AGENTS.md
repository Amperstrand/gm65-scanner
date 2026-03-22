# Agent Reference

## ST-LINK Lockup / "Interface is Busy" — Causes & Recovery

### Root Causes

There are 3 distinct lockup states, each with different symptoms and fixes:

### 1. Stale probe-rs Process (most common)

**Symptom:** `interface is busy (errno 16)` or `Failed to open probe`

**Cause:** A background `probe-rs run` or `probe-rs attach` was killed (SIGTERM/SIGKILL) but the ST-LINK USB device wasn't properly released. The kernel still has the USB device claimed.

**Provokes it:** Any `probe-rs run & ... ; kill $PID` pattern. The `kill` doesn't let probe-rs gracefully disconnect from the ST-LINK.

**Fix:**
```
pkill -9 probe-rs; sleep 3-5
```
If that doesn't work, physically unplug and replug the ST-LINK USB cable.

### 2. JTAG DMA Error (SDRAM/FMC contention)

**Symptom:** `JtagDmaError` when trying to `probe-rs attach` or `probe-rs run` AFTER flashing/running firmware that uses SDRAM.

**Cause:** The STM32F469's FMC (Flexible Memory Controller) is active (SDRAM configured). Debug accesses that go through the AHB matrix conflict with the FMC's DMA-like access patterns. The ST-LINK's debug access port gets starved or the DAP returns errors.

**Provokes it:** Flashing and running the main firmware (which inits SDRAM via FMC), then trying to attach with probe-rs. Especially bad if probe-rs was previously running (state 1 compounds this).

**Fix:**
```
pkill -9 probe-rs; sleep 5
probe-rs reset --chip STM32F469II
sleep 2
# now safe to download/run/attach
```
The `reset` without `--connect-under-reset` does a simple system reset which reinitializes FMC but doesn't try to read memory. If that fails too, power cycle the board.

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

### Prevention Rules

1. **Always `pkill -9 probe-rs; sleep 3` before any probe-rs operation** — never assume the previous session cleaned up.

2. **Never do `probe-rs run & ... ; kill`** if you need to use CDC afterward — the kill leaves ST-LINK dirty. Use `probe-rs download` + `probe-rs reset` instead.

3. **After flashing SDRAM-using firmware, add a longer sleep** (5s) before the next probe-rs command. The SDRAM init sequence can leave the bus in a state that makes immediate debug access fail.

4. **Use `probe-rs download` for flashing, NOT `probe-rs run`** — `download` flashes and returns. `run` holds the probe forever and blocks RTT from being read separately.

5. **For RTT logging, use `probe-rs run` in background** with a timeout, accept that you can't use CDC simultaneously. For CDC testing, use `download` + `reset` + disconnect probe-rs.

6. **The defmt RTT sometimes doesn't produce output** on the main firmware binary (but works on hil_test_sync). Root cause unknown — possibly defmt version conflict (BSP uses 0.3.x, workspace uses 1.0.x). Don't waste time debugging this; use CDC for main firmware testing.

## USB CDC Testing Protocol

For reliable CDC testing:
```
pkill -9 probe-rs; sleep 2
probe-rs download --chip STM32F469II <binary>
probe-rs reset --chip STM32F469II
sleep 5
python3 tests/hil_test.py --port /dev/ttyACM0 protocol
```

If ttyACM0 doesn't respond, try ttyACM1 (probe-rs may still be partially connected).

## HIL Test Protocol

For on-device HIL tests (RTT output):
```
pkill -9 probe-rs; sleep 2
probe-rs run --chip STM32F469II <binary> &
PID=$!
sleep 25
kill $PID 2>/dev/null
wait $PID 2>/dev/null
```

## Why Not Specter-DIY USB Settings

The specter-diy project does NOT disable USB — it simply doesn't implement a USB device at all. It's a pure UART + LCD firmware. The GM65 scanner's settings register (0x0000) has no USB-related bits. All USB issues are STM32/ST-LINK debug probe issues, not scanner-related.
