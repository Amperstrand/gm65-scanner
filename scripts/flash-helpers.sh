#!/usr/bin/env bash
# flash-helpers.sh — Reusable STM32 flash/test/recovery functions
#
# Designed to be sourced by a Makefile or other scripts.
# Override CHIP, VID_PID, FLASH_TIMEOUT, RTT_TIMEOUT as needed.
#
# Usage: source scripts/flash-helpers.sh; probe_healthy; flash_binary ...
#
# Functions:
#   probe_healthy       — kill stale probe-rs, check xHCI, auto-recover
#   recover_xhci        — auto-detect xHCI PCI address, remove/rescan
#   reset_device        — probe-rs reset with FMC-aware delay
#   flash_binary        — download --verify, expects $BINARY
#   detect_cdc_port     — find /dev/ttyACM* by VID/PID symlink or lsusb
#   wait_for_cdc        — poll for CDC device, timeout
#   ensure_flash_ready  — probe_healthy → reset → flash → reset → wait → print port

set -euo pipefail

CHIP="${CHIP:-STM32F469NIHx}"
VID_PID="${VID_PID:-16c0:27dd}"
FLASH_TIMEOUT="${FLASH_TIMEOUT:-60}"
RTT_TIMEOUT="${RTT_TIMEOUT:-25}"
CDC_WAIT_TIMEOUT="${CDC_WAIT_TIMEOUT:-15}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info()  { echo -e "${CYAN}[INFO]${NC}  $*"; }
log_ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
log_err()   { echo -e "${RED}[FAIL]${NC}  $*"; }

probe_healthy() {
    log_info "Cleaning up probe-rs processes..."
    if pkill -9 probe-rs 2>/dev/null; then
        sleep 3
        log_info "Killed stale probe-rs processes"
    fi

    if ! command -v probe-rs &>/dev/null; then
        log_err "probe-rs not found in PATH"
        return 1
    fi

    if ! probe-rs list &>/dev/null; then
        log_warn "No probe-rs devices found"
        log_info "Checking xHCI health..."
        recover_xhci
    fi

    log_ok "Probe ready"
}

recover_xhci() {
    local pci_addr
    pci_addr=$(sudo lspci -nn 2>/dev/null | grep -i "xHCI" | head -1 | awk '{print $1}')

    if [[ -z "$pci_addr" ]]; then
        log_err "No xHCI controller found via lspci"
        log_info "Try: sudo dmesg | grep -i xhci"
        return 1
    fi

    log_info "xHCI controller at PCI $pci_addr — removing and rescanning..."

    echo 1 | sudo tee "/sys/bus/pci/devices/$pci_addr/remove" >/dev/null
    sleep 2
    echo 1 | sudo tee /sys/bus/pci/rescan >/dev/null
    sleep 3

    if probe-rs list &>/dev/null; then
        log_ok "xHCI recovered, probe-rs devices found"
    else
        log_warn "xHCI rescan done but probe-rs still sees no devices"
        log_info "Physical USB replug may be needed"
    fi
}

reset_device() {
    log_info "Resetting device ($CHIP)..."
    probe-rs reset --chip "$CHIP" 2>&1 || {
        log_warn "probe-rs reset failed — trying xHCI recovery first"
        recover_xhci
        sleep 2
        probe-rs reset --chip "$CHIP" 2>&1 || {
            log_err "probe-rs reset failed after recovery"
            return 1
        }
    }
    sleep 2
    log_ok "Device reset"
}

flash_binary() {
    local binary="${BINARY:?BINARY not set}"
    if [[ ! -f "$binary" ]]; then
        log_err "Binary not found: $binary"
        return 1
    fi

    log_info "Flashing $binary ($CHIP)..."
    local start
    start=$(date +%s)

    if probe-rs download --chip "$CHIP" --verify "$binary" 2>&1; then
        local elapsed=$(( $(date +%s) - start ))
        log_ok "Flashed in ${elapsed}s"
    else
        log_err "Flash failed — trying reset + retry..."
        reset_device
        if probe-rs download --chip "$CHIP" --verify "$binary" 2>&1; then
            log_ok "Flash succeeded on retry"
        else
            log_err "Flash failed permanently"
            return 1
        fi
    fi
}

detect_cdc_port() {
    local vid="${VID_PID%%:*}"
    local pid="${VID_PID##*:}"

    if [[ -n "$vid" && -n "$pid" ]]; then
        local symlink
        symlink=$(find /dev/serial/by-id/ -name "*vid_${vid}*pid_${pid}*" 2>/dev/null | head -1)
        if [[ -n "$symlink" ]]; then
            readlink -f "$symlink"
            return 0
        fi
    fi

    local port
    for port in /dev/ttyACM*; do
        [[ -e "$port" ]] || continue
        if udevadm info --query=property --name="$port" 2>/dev/null | grep -qi "idVendor=$vid\|idProduct=$pid"; then
            echo "$port"
            return 0
        fi
    done

    for port in /dev/ttyACM*; do
        [[ -e "$port" ]] || continue
        echo "$port"
        return 0
    done

    return 1
}

wait_for_cdc() {
    local timeout="${1:-$CDC_WAIT_TIMEOUT}"
    local elapsed=0
    log_info "Waiting for CDC device (VID:PID=$VID_PID, timeout=${timeout}s)..."

    while [[ $elapsed -lt $timeout ]]; do
        local port
        if port=$(detect_cdc_port 2>/dev/null); then
            echo "$port"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done

    log_err "CDC device not found after ${timeout}s"
    return 1
}

ensure_flash_ready() {
    probe_healthy || return 1
    reset_device || return 1
    flash_binary || return 1

    log_info "Releasing probe..."
    pkill -9 probe-rs 2>/dev/null
    sleep 2
    reset_device || return 1

    local port
    if port=$(wait_for_cdc); then
        log_ok "Firmware running — CDC on $port"
        echo "$port"
    else
        log_err "Device flashed but CDC not detected"
        return 1
    fi
}

run_rtt() {
    local binary="${BINARY:?BINARY not set}"
    if [[ ! -f "$binary" ]]; then
        log_err "Binary not found: $binary"
        return 1
    fi

    probe_healthy || return 1

    log_info "Running $binary with RTT capture (${RTT_TIMEOUT}s)..."
    timeout "$RTT_TIMEOUT" probe-rs run --chip "$CHIP" "$binary" 2>&1 || true
    log_info "RTT capture complete"
}
