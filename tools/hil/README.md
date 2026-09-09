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

## Run

    make test-qr-loopback     # from repo root (pytest: flash + matrix + restore)
    make hil-place            # (re)create the labgrid place after coordinator restarts

Results land in `results/run-*/` (verdicts JSON, flash backup, ledger
append in `results/history.jsonl`).

## Open physics issue (2026-09-09)

The GM65 decodes hand-held phone QRs (proven, logged) but has never decoded
a CYD-screen QR — across sizes 40-288px, 18 screen positions, both mirror
parities, with/without module illumination, on three firmware builds
(April-sync/async known-good + HEAD). Suspects: specular ambient glint on
the parallel-mounted screen, LCD moiré at close range, or working-distance
mismatch. See the bench session notes in the repo AGENTS.md.
