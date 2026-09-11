# Library API Audit — what consumers should NOT need to know

Date: 2026-09-11 · Trigger: the micronuts QR-rig integration session
(Amperstrand/micronuts `tools/hil/` + firmware), which is the first
"micronuts-class consumer" the README promises this crate serves.

## The evidence: what micronuts had to reimplement today

Integrating `Gm65ScannerAsync` into micronuts firmware required the
consumer to know and re-implement four pieces of module lore that belong
behind the crate's API:

| # | Consumer-side reimplementation | Root knowledge | Where it lives today |
|---|---|---|---|
| 1 | Continuous-mode entry dance: `stop_scan()` → set `Settings` read-mode=Continuous → set `ScanEnable=1` | issue #75: Command mode ACKs ScanEnable writes but **never scans** | sync driver `enter_continuous_mode()` (examples only); NOT on the async driver |
| 2 | Silent-buzzer policy: build settings from `ScannerSettings::default()` + explicit `buzzer=false` because the pinned rev predates the silent default | fa6dc93: buzzer is **bit 6**; 0xD1 beeps, 0x91/0x92 don't | crate default at HEAD only; consumers on older revs or composing raw values get beeps (this session beeped the lab before the fix) |
| 3 | ACK-frame leak sanitization: strip leading `02 00 00 01 <val> 00 33 31` frames from scan data | bench lesson 2026-09-10: register responses race decodes into the scan stream | `tools/hil/gm65qr.py` (host-side regex) — consumers without that file get corrupted payloads classified as Binary |
| 4 | Quiet-cadence capture contract: host must not poll during the firmware's capture window | firmware-side UART shim limitation (see #88/#71) | campaign LIMITATIONS.md prose |

A fifth piece is queued but not shipped: the boot-heal policy (#100 —
factory-reset/deep-sleep on boot) is explicitly desired to be a
consumer-opt-in crate feature.

## Proposed API (the DRY boundary)

```rust
// One call replaces dance #1 + policy #2. Failures return the underlying
// driver error; no save_settings (NVRAM corruption risk, #82).
pub enum ScanPolicy {
    /// Continuous scanning, aim LED while reading, decode buzzer OFF.
    /// The proven bench configuration (settings 0x92 + ScanEnable=1).
    SilentContinuous,
    /// Same, with the decode buzzer armed (0xD2) for pose-finding (#99).
    BuzzingContinuous,
    /// Command-triggered, silent (0x91) — for hosts that poll triggers.
    SilentCommand,
}

impl Gm65ScannerAsync<UART> {
    pub async fn start_scanning(&mut self, policy: ScanPolicy)
        -> Result<(), ScannerError>;
}
```

- `read_scan()` (both drivers) strips leading leaked ACK frames (#3) before
  returning — the leak is a module property, not a consumer concern.
- The blessed transport for new integrations is embassy `BufferedUart`
  (#88 research: interrupt-based, no byte loss between reads — removes the
  quiet-cadence contract #4 at the source).
- Boot heal (#100 Tier A/B) lands as `Gm65ScannerAsync::boot_heal(policy)`
  once hardware-validated; firmware policy consts opt in.

## Reference patterns (researched 2026-09-11)

- **embedded-hal philosophy** (fdi.sk "Adventures in abstraction"): driver
  crates own device details; apps compose transports behind traits. This
  crate's sans-IO core + sync/async drivers already matches; the gap is
  *policy*, not I/O.
- **specter-diy PR #335** (the upstream this crate reverse-engineered
  from): model detection across GM65 + M3Y protocols, EOL-validated reads
  for complete QR capture, keep-alive during animated QR sequences (the
  exact stall class our UR cycling hit), 4096-byte read buffers for large
  single QRs. Worth porting: the animated-QR keep-alive and larger buffers.
- **embassy `BufferedUartRx`** docs.rs guidance: interrupt-driven, stores
  data between reads, implements `embedded_io::Read` safely — chosen over
  `RingBufferedUartRx` (DMA) for this crate's command/response + moderate
  streaming profile (#88).

## Consumer contract after this lands

```rust
let mut scanner = Gm65ScannerAsync::with_default_config(uart); // BufferedUart
scanner.init().await?;
scanner.start_scanning(ScanPolicy::SilentContinuous).await?;
// read_scan() returns sanitized payloads; no module lore required.
```

## Filing

Track as crate issues: `start_scanning(ScanPolicy)`, sanitize-in-read,
BufferedUart example, boot-heal validation (#100 dependency). The micronuts
firmware keeps its inline dance (documented) until the crate ships the API
— then micronuts deletes ~30 lines of lore and pins the new rev.
