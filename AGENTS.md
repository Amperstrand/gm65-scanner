# Agent Reference

## Hardware

- Board: STM32F469I-Discovery (STM32F469NIHx)
- Scanner: GM65/M3Y, firmware 0x87
- UART: USART6, PG14 (TX) / PG9 (RX), 115200 baud
- USB: USB OTG FS, PA12 (DP) / PA11 (DM)

## Known-Good Pins

| Commit | Notes |
|--------|-------|
| Pending (main HEAD) | Sync 6/6, async 9/9, QR scans on both. BSP `799df39`, embassy BSP `e202e9a`. |
| `1360469` | Sync 6/6, async 9/9, QR scans on both. BSP `56a0bc8`, embassy BSP `890a4d1`. |

## HIL Test Results

### 2026-03-31 — Full end-to-end with QR scans

Both drivers verified with InitAction state machine, defmt logging parity, BSP `56a0bc8` (HAL 0.5, embedded-hal 1.0).

**Sync 6/6 PASS**: init, ping, trigger/stop, timeout, state transitions, QR scan (18 bytes, aim laser)

**Async 9/9 PASS**: init, ping, trigger/stop, timeout, state transitions, cancel+rescan (24 bytes ambient QR), rapid triggers, idle no-trigger, QR scan (23 bytes, aim laser + LED)

Note: BarType VERIFY FAIL observed (wrote 0x01, read 0x05) — expected per known issue #10. cancel_then_rescan picked up ambient QR codes — expected behavior.

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
