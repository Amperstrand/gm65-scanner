# QR Loopback HIL — CYD → GM65 → F469

Hands-off scan testing: a CYD (ESP32 + ST7796 320x480) renders QR codes that
the GM65 (on the F469's USART6) scans; the harness drives both ends and
asserts payload round-trips over USB CDC.

## Rig

| Element | Identity | Access |
|---------|----------|--------|
| CYD (QR source) | ST7796 320x480, CH340 `1a86:7523` | `/dev/serial/by-path/pci-0000:02:00.0-usb-0:1:1.0-port0` |
| F469 + GM65 | ST-Link `066FFF515786534867184152`, CDC `c0de:cafe` (async fw) | st-flash / CDC by-id |
| Wallet image | micronuts CDC `16c0:27dd`, product `Micronuts Cashu Hardware Wallet`, serial `F4691` | backup + restore every run |

The F469 board is SHARED with the micronuts wallet: flash sessions MUST
backup the 2 MiB image first (the session fixture does this and restores in
`finally`). The gm65 sync firmware shares the wallet's VID:PID/serial —
identify CDCs by `/dev/serial/by-id` product string, never VID:PID alone.

## Bench discipline (fips-lab playbook)

- `tollgate_lab.acquire_bench_lock("amperstrand-bench")` FIRST (conftest),
  then the labgrid place `gm65-qr-loopback` when the coordinator is up.
- Flash gates: `../fips-lab/fips_lab/boards.toml` (`cyd-ch340`,
  `stm32f469i-disco`).
- `st-flash write` alone leaves the target's USB wedged — the harness always
  follows writes with `st-flash --connect-under-reset reset`.

## CYD firmware (../cyd-qr)

esp-hal + mipidsi ST7796 build (config provenance: the slint demo in
`~/src/cyk/embassy-hello`). UART line protocol at 115200 on the CH340 port:

- `QR <hex>` — render max-size centered QR → `RENDERED <modules> <px> <len>`
- `QRS <cap-px> <hex>` — size-capped centered render
- `QRP <x> <y> <hex>` — 160px QR anchored at (x, y) for position sweeps
- `CLR` — white screen; `ID` — firmware id; `DIAG` — panel-ID/touch probe

Build: `. ~/export-esp.sh && cd ../cyd-qr && cargo +esp build --release`

## Workflow

Two layers, two jobs:

| Layer | Command | Purpose |
|-------|---------|---------|
| **Regression gate** | `make test-qr-loopback` | 6/6 pytest suite: CYD up, scanner connected, byte-exact roundtrips (10/45/127B), negative control, backup/restore. ~3.5 min. Run before merges. |
| **Characterization campaign** | `make test-qr-campaign` | `campaign.py --firmwares async,sync`: reliability soak, QR envelope ladder, scan-speed/cadence, settings A/B (issue #11), negative controls, settings-wedge repro + auto-recovery, randomized jitter net. Artifacts: `results/campaign-*/`. ~45 min unattended. |

Both take the bench flock FIRST, then the labgrid place; both backup the
2 MiB flash and restore whatever image was present (verified by by-id
product string). Fault isolation: one experiment wedging records and the
campaign continues; the xHCI port self-heals (PCI remove/rescan) inside
every CDC wait.

## Reference module for other Amperstrand projects

`gm65qr.py` is the bring-along client (no gm65-scanner repo coupling beyond
`rig.py` — copy both):

```python
from gm65qr import arm_winning_config, scan_roundtrip, unique_payload, WINNING_CAP
arm_winning_config(cyd)                      # inverted + ECC-H + 224px cap
r = scan_roundtrip(cdc, cyd, b"any-payload") # {ok, latency_s, modules, ...}
```

It encodes every bench lesson: winning render config, no-drain polling
(clipped-payload fix), ACK-frame sanitizing (`02 00 00 01 xx 33 31` leaks),
stale-buffer consumption before negative windows, unique sequential
payloads to defeat the module's 5s same-barcode delay.

**Known limits (campaign-measured, see results/campaign-*/summary.md).**

## Labgrid deployment (cross-project reservation)

The place `gm65-qr-loopback` binds BOTH bench tokens
(`ai-legion-small-microfips/cyd-serial` + `.../stm32-stlink`). Other
projects (micronuts-class) reserve the pair:

    labgrid-client -x 192.168.13.221:20408 -p gm65-qr-loopback acquire
    # ... drive via tools/hil/rig.py + gm65qr.py (direct I/O, flock honored)
    labgrid-client -x 192.168.13.221:20408 -p gm65-qr-loopback release

Or via the labgrid pytest plugin: `pytest --lg-env tools/hil/labgrid-env.yaml`.
Place tags carry live state (`firmware=`, `test=`, `owner=`, `ts=`) —
`make hil-place` recreates the place after coordinator restarts.

## Open physics — RESOLVED

Inverted rendering (white modules on black) + ECC-H + ~203px QR is the only
decoding configuration on the GM65+ST7796 pair (matrix experiment
2026-09-10); root analysis in `crates/gm65-scanner/docs/GM65-OPTICS-FINDINGS.md`.
Phone/hand-held QRs decode in any polarity; the CYD rig needs the winning
config armed (gm65qr does it automatically).

## Open physics issue (2026-09-09 → 2026-09-10)

The GM65 decodes hand-held phone QRs — verified repeatedly (logged instance
2026-09-08 21:21 `page.link/naxz` on April-async; user-observed buzzer + LCD
feedback 2026-09-10) — but has never decoded a CYD-screen QR, even with the
screen physically moved/tilted by hand, across sizes 40-288px, 18 positions,
both mirror parities, with/without module illumination, normal AND inverted
(film-negative) rendering, on three firmware builds (April-sync/async
known-good + HEAD). Root causes documented in
`crates/gm65-scanner/docs/GM65-OPTICS-FINDINGS.md`: ST7796 ~110 PPI grid
pose-locks moiré into the 648x488 sensor, no screen/exposure register on
GM65, 4cm DoF floor. Best next hardware: a ~285+ PPI display (LilyGo
T-Display-S3 class) or e-ink/paper.
