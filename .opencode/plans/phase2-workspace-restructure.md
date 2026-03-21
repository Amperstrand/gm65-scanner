# Phase 2: gm65-scanner Workspace Restructuring

## Goal

Transform gm65-scanner from a single library crate into a workspace containing:
1. The library crate (current code, stays at root)
2. An example firmware binary at `examples/stm32f469i-disco/` that demonstrates the library

The example firmware is a **standalone QR/barcode scanner** — NOT a Cashu wallet. It knows nothing about Cashu. It acts as both a standalone display device AND a USB-connected scanner emulator.

Micronuts stays focused on being a Cashu wallet that depends on gm65-scanner as a library. The scanner firmware serves as inspiration/reference for micronuts but they share no code.

## Approach

- gm65-scanner repo becomes a **Cargo workspace** with 2 members
- The existing root `Cargo.toml` moves to `crates/gm65-scanner/` (the library)
- Root `Cargo.toml` becomes workspace manifest
- `examples/stm32f469i-disco/` is a new binary crate
- All library source stays at `crates/gm65-scanner/src/`
- Root re-exports from `src/lib.rs` → `crates/gm65-scanner/src/lib.rs`

## Detailed Steps

### 1. Restructure gm65-scanner repo
- Create `crates/gm65-scanner/` directory
- Move `src/`, `Cargo.toml` (library), `AGENTS.md`, `docs/`, `README.md` into `crates/gm65-scanner/`
- Create workspace root `Cargo.toml` with members: `["crates/gm65-scanner", "examples/stm32f469i-disco"]`
- Update `crates/gm65-scanner/Cargo.toml` package name stays `gm65-scanner`
- Create `examples/stm32f469i-disco/Cargo.toml` as binary crate
- Create `examples/stm32f469i-disco/.cargo/config.toml` (build target, runner, linker flags)
- Create `examples/stm32f469i-disco/build.rs` (memory.x copy)
- Create `examples/stm32f469i-disco/memory.x` (linker memory layout, copy from micronuts)
- Create `examples/stm32f469i-disco/src/main.rs` — scanner-only firmware
- Copy needed board init code from micronuts (SDRAM, display, USB, USART6 pins)

### 2. Example firmware (scanner-only, no Cashu)
The example firmware at `examples/stm32f469i-disco/` will:
- Init STM32F469I-Discovery board (clocks, SDRAM, LCD, USB CDC, USART6)
- Probe and init GM65 scanner via USART6 (multi-baud probing)
- Continuous scan polling in main loop
- Display scan results on LCD (type label + data, using gm65-scanner's `classify_payload`)
- Send scan results via USB CDC with type prefix byte (`[type:1][data:N]`)
- USB CDC also accepts commands: status query, trigger scan, get scan data
- No Cashu dependency whatsoever

### 3. Example firmware dependencies
- `gm65-scanner` (workspace path dep, features: embedded-hal, defmt)
- `stm32f469i-disc` (BSP @ fa6dc86, features: usb_fs, framebuffer, defmt)
- `embedded-hal`, `embedded-hal-02`, `embedded-graphics`
- `usb-device`, `usbd-serial`
- `cortex-m`, `cortex-m-rt`, `defmt`, `defmt-rtt`, `panic-probe`
- `heapless`, `static_cell`, `linked_list_allocator`, `nb`
- NO `cashu-core-lite`, NO `k256`, NO `sha2`

### 4. Shared code between example and micronuts
The example firmware will duplicate some board init code from micronuts (USB CDC protocol, display rendering). This is intentional — they are independent projects. The scanner SDK (gm65-scanner) is the shared dependency.

### 5. Update micronuts
- Pin gm65-scanner to new commit after restructuring
- `cargo build` to verify workspace dep resolution works
- No changes to micronuts firmware code needed (it depends on gm65-scanner via git)

### 6. Verify
- `cargo test` in gm65-scanner (library tests still pass)
- `cargo build` in gm65-scanner example (cross-compile succeeds)
- `cargo build` in micronuts firmware (still compiles with new gm65-scanner dep)

## Files to Create/Move in gm65-scanner

```
gm65-scanner/
├── Cargo.toml              # NEW: workspace root
├── Cargo.lock              # updated
├── crates/
│   └── gm65-scanner/
│       ├── Cargo.toml      # MOVED: library crate
│       ├── src/            # MOVED: all existing source
│       ├── AGENTS.md       # MOVED
│       ├── docs/           # MOVED
│       └── README.md       # MOVED
└── examples/
    └── stm32f469i-disco/
        ├── Cargo.toml      # NEW: binary crate
        ├── .cargo/
        │   └── config.toml # NEW
        ├── build.rs        # NEW
        ├── memory.x        # NEW
        └── src/
            ├── main.rs     # NEW: scanner firmware
            ├── usb.rs      # NEW: CDC protocol (adapted from micronuts)
            └── display.rs  # NEW: scan result display (adapted from micronuts)
```
