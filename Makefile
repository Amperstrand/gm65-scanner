CHIP       ?= STM32F469NIHx
VID_PID    ?= 16c0:27dd
TARGET     ?= thumbv7em-none-eabihf
BUILD_DIR  ?= target/$(TARGET)/release
EXAMPLE_DIR = examples/stm32f469i-disco

SYNC_BINARY  = $(BUILD_DIR)/stm32f469i-disco-scanner
ASYNC_BINARY = $(BUILD_DIR)/async_firmware

SYNC_FEATURES  = sync-mode
ASYNC_FEATURES = scanner-async
HIL_SYNC_FEATURES  = hil-tests,defmt
HIL_ASYNC_FEATURES = scanner-async,defmt,gm65-scanner/hil-tests

# --manifest-path keeps us at workspace root while cargo still picks up
# .cargo/config.toml (target, rustflags) from the workspace .cargo/ dir.
CARGO_FLAGS    = --release --target $(TARGET) --manifest-path $(EXAMPLE_DIR)/Cargo.toml --no-default-features
FLASH_HELPERS  = scripts/flash-helpers.sh
SHELL          = /bin/bash

.PHONY: build-sync build-async build-all \
        run-sync run-async \
        flash-sync flash-async \
        test-sync test-async test-cdc \
        recover reset monitor \
        clean

# ── Build ──────────────────────────────────────────────────────────────────

build-sync:
	cargo build $(CARGO_FLAGS) --bin stm32f469i-disco-scanner --features $(SYNC_FEATURES)

build-async:
	cargo build $(CARGO_FLAGS) --bin async_firmware --features $(ASYNC_FEATURES)

build-all: build-sync build-async

# ── Run with RTT capture (fastest edit-run cycle) ─────────────────────────

run-sync: build-sync
	@source $(FLASH_HELPERS); BINARY=$(SYNC_BINARY) RTT_TIMEOUT=30 run_rtt

run-async: build-async
	@source $(FLASH_HELPERS); BINARY=$(ASYNC_BINARY) RTT_TIMEOUT=30 run_rtt

# ── Flash + auto-recover + detect port ─────────────────────────────────────

flash-sync: build-sync
	@source $(FLASH_HELPERS); BINARY=$(SYNC_BINARY) ensure_flash_ready

flash-async: build-async
	@source $(FLASH_HELPERS); BINARY=$(ASYNC_BINARY) ensure_flash_ready

# ── Flash + CDC test (one-shot) ────────────────────────────────────────────

test-sync: build-sync
	@export BINARY=$(SYNC_BINARY); \
	source $(FLASH_HELPERS); \
	PORT=$$(ensure_flash_ready); \
	echo "---"; \
	python3 $(EXAMPLE_DIR)/tests/hil_test.py --port "$$PORT" protocol

test-async: build-async
	@export BINARY=$(ASYNC_BINARY); \
	source $(FLASH_HELPERS); \
	PORT=$$(ensure_flash_ready); \
	echo "---"; \
	python3 $(EXAMPLE_DIR)/tests/hil_test.py --port "$$PORT" protocol

# ── Test CDC against already-running firmware ──────────────────────────────

test-cdc:
	@source $(FLASH_HELPERS); \
	PORT=$$(detect_cdc_port); \
	if [[ -z "$$PORT" ]]; then \
		echo "No CDC device found"; exit 1; \
	fi; \
	python3 $(EXAMPLE_DIR)/tests/hil_test.py --port "$$PORT" protocol

# ── Recovery / utility ─────────────────────────────────────────────────────

recover:
	@source $(FLASH_HELPERS); recover_xhci

reset:
	@source $(FLASH_HELPERS); reset_device

monitor:
	@source $(FLASH_HELPERS); \
	PORT=$$(detect_cdc_port); \
	if [[ -z "$$PORT" ]]; then \
		echo "No CDC device found"; exit 1; \
	fi; \
	python3 -m serial.tools.miniterm "$$PORT" 115200 --dtr

clean:
	cargo clean
