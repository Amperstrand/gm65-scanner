# Agent Reference

## Hardware

- Board: STM32F469I-Discovery (STM32F469NIHx)
- Scanner: GM65/M3Y, firmware 0x87
- UART: USART6, PG14 (TX) / PG9 (RX), 115200 baud
- USB: USB OTG FS, PA12 (DP) / PA11 (DM)

## Known-Good Pins

| Commit | Notes |
|--------|-------|
| `070e387` (main HEAD) | Sync 5/5, async 8/8. BSP `56a0bc8`, embassy `84444a19`. InitAction refactor + new BSP verified. |
| `f1d694d` | Full HIL verification: sync 6/6, async 9/9, QR scans on both (old BSP `9f52a58`) |

## HIL Test Results

### 2026-03-30 — InitAction refactor + new BSP (`56a0bc8`)

Both drivers verified end-to-end with InitAction state machine, defmt logging parity, and updated BSP (HAL 0.5 migration, embedded-hal 1.0).

**Sync 5/5 PASS**: init, ping, trigger/stop, timeout, state transitions

**Async 8/8 PASS**: init, ping, trigger/stop, timeout, state transitions, cancel+rescan, rapid triggers, idle no-trigger

Note: BarType VERIFY FAIL observed (wrote 0x01, read 0x05) — expected per known issue #10. No QR scan test (no QR presented).

### 2026-03-28 — Native binaries, both drivers, real QR scans

**Async 9/9 PASS**: init, ping, trigger/stop, timeout, state transitions, cancel+rescan, rapid triggers, idle no-trigger, QR scan (25 bytes, aim laser + LED)

**Sync 6/6 PASS**: init, ping, trigger/stop, timeout, state transitions, QR scan (aim laser, 50-retry loop)

### 2026-03-29 — Sync binary from feat/async-firmware branch

**Sync 5/5 PASS**: init, ping, trigger/stop, timeout, state transitions

Note: BarType VERIFY FAIL observed (wrote 0x01, read 0x05) — expected per known issue #10.

## Known Issues

- **BarType register not persisted (#10)**: Register 0x002C write accepted but not persisted across GM65 reboots on firmware 0.87. Hardware quirk.
- **Settings mode comparison (#11)**: 0x81 vs 0xD1 not yet compared. Current firmware uses 0x81.
- **drain_uart data loss (#12)**: FIXED. `send_command()` skips drain when in `Scanning` state.

## Upstream Interaction Policy

**NEVER file PRs or issues on upstream projects without human review.** See [Amperstrand/micronuts#19](https://github.com/Amperstrand/micronuts/issues/19) for retrospective.

## ST-LINK Recovery

Kill stale processes before any probe-rs operation:
```
pkill -9 probe-rs; sleep 3
```

If `interface is busy` persists or xHCI controller dies (all USB devices vanish):
```
echo 1 | sudo tee /sys/bus/pci/devices/0000:02:00.0/remove
sleep 2
echo 1 | sudo tee /sys/bus/pci/rescan
sleep 3
probe-rs list
```

Find PCI address on other machines: `sudo lspci -nn | grep -i "xHCI"`

Use `probe-rs download` + `probe-rs reset` (not `probe-rs run`) for CDC testing so probe-rs releases the ST-LINK.
