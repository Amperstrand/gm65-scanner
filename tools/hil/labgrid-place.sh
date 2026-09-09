#!/usr/bin/env bash
# Idempotently create the gm65 QR-loopback place with its resource matches.
# The tokens come from the microfips bench exporter (one exporter per bench
# machine — this rig reuses the same physical CYD + ST-Link).
set -euo pipefail

COORDINATOR="${LABGRID_COORDINATOR:-192.168.13.221:20408}"
EXPORTER_NAME="${LABGRID_EXPORTER_NAME:-ai-legion-small-microfips}"
PLACE="gm65-qr-loopback"

lg() { labgrid-client -x "$COORDINATOR" -p "$PLACE" "$@"; }

if lg show >/dev/null 2>&1; then
    echo "place $PLACE exists"
else
    lg create
    echo "place $PLACE created"
fi

lg add-match "${EXPORTER_NAME}/cyd-serial/BenchSerialToken"
lg add-match "${EXPORTER_NAME}/stm32-stlink/BenchSerialToken"
lg set-comment "CYD QR source -> GM65 -> F469 async firmware CDC loopback (gm65-scanner tools/hil)"
lg set-tags "firmware=-" "test=qr-loopback" "owner=gm65-scanner" "ts=$(date +%Y%m%dT%H%M%S)"
lg show | sed -n '1,8p'
