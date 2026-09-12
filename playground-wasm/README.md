# gm65-scanner playground (wasm)

**Live demo: <https://amperstrand.github.io/gm65-scanner/>** — deployed by
CI on every push to main, gated by a post-deploy Playwright e2e.

Browser playground that runs the **real** `Gm65ScannerAsync` driver in
wasm32 against a virtual GM65 module, presented as the bench device: an
STM32F469I-DISCO board whose 480x800 LCD renders the firmware example's
screens (same theme, layout constants, fonts, payload classification and
QR mirror), and a GM65 module mock below it whose window IS the camera
viewfinder, with the aim-laser animation while scanning. Driver controls,
UART monitors and event log live in the side wing.

This is the wasm rehearsal for a browser micronuts wallet
(`micronuts-web`): every seam a browser wallet needs is exercised here
first —

| Seam | Exercised by |
|---|---|
| `embassy-time` JS timer driver (`wasm` feature) | every `init()`/`read_scan()` timeout, `start_scanning` config rounds |
| wasm-bindgen executor driving embassy futures | all `pg_*` async exports |
| virtual `embedded-io-async` UART with waker plumbing | `VirtualUart` (writes answered synchronously; scan data wakes pending reads) |
| JS QR decode → protocol-faithful GM65 frames | camera (`BarcodeDetector`) or paste → `payload CRLF` scan stream |

The driver runs **unmodified** — scanner logic stays gm65-scanner's
(modularity rule). The `Gm65ModuleSim` answers with genuine protocol bytes
(`7E 00 08 … AB CD` commands, `02 00 00 01 vv 33 31` responses), including
the firmware-0.87 `BarType` write-not-persisted quirk and the
stop-discards-pending-decode behavior. Long-term home for
`Gm65ModuleSim`/`VirtualUart` is a `mock` feature on the crate itself once
`micronuts-web` becomes the second consumer.

## Build & run

```bash
cargo build -p playground-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web \
  ~/.cargo-target/wasm32-unknown-unknown/release/playground_wasm.wasm \
  --out-dir playground-wasm/web
python3 -m http.server 8901 --directory playground-wasm/web
```

Wasm-only crate: on every other target it compiles to an empty shell, so
the host CI battery (`fmt`, `test -p gm65-scanner`, `clippy -p …`) is
unaffected.

## Gotcha: embassy-time timer queue

`embassy-time` defaults to the embassy-executor-integrated timer queue
(`__embassy_time_queue_item_from_waker`), which fails at **link time** when
no embassy-executor is in the graph. The playground drives futures with the
wasm-bindgen executor, so it selects the self-contained queue:
`embassy-time = { features = ["wasm", "generic-queue-64"] }`. See the
comment in `Cargo.toml`.

## Verified session (2026-09-12, headless Chromium)

Init → `model=GM65`, state `Ready`, full ~20-round register config visible
in the UART monitor → `start_scanning(SilentContinuous)` → inject payload
mid-read → `read_scan ✓ 38 chars` in the same second (waker path), payload
byte-exact, state `ScanComplete`, subsequent reads time out cleanly.
