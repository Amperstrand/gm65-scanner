# Issue draft — #100 Tier A (0xA5 ModuleReboot) validation: NEGATIVE (2026-09-11)

Owner: copy into gm65-scanner#100 when reviewed. Data:
`tools/hil/results/healval-20260911-131*/report.json`, soak
`tools/hil/results/soakdiag-20260911-125*/`, this session's log.

## Verdict

**Tier A (0xA5 deep-sleep reboot on boot) is not viable as implemented.**
Bench A/B (pose in-pocket, loopback 6/6 the same morning):

| Step | Result |
|---|---|
| async gate pre-heal | FAIL ×2 (uart_errors=19, scans_delivered=12 — the post-sustained-load choke class) |
| sync baseline gate | OK 5.61s |
| CDC 0x23 ModuleReboot | `[sent=1, responsive=0, model=1]` — module took the reboot, never re-answered ping in the firmware window |
| sync gate post-0x23 | FAIL (delivery went OK → FAIL after the reboot) |
| async gate post-0x23 | FAIL (choke unchanged) |
| CDC 0x22 fallback | accepted, but reinit_ok=0; module left undetectable (model=0) until a second 0x22 from a fresh boot — which healed it ([0,1,1], connected=1) |

## Interpretation

1. The "wedge" class reproduced today is carryover UART-conversation state
   after sustained sync-firmware load. It survives F469 re-flashes and the
   0xA5 reboot. 0x22's baud dance heals it, but only from a fresh-boot
   firmware state (first attempt from the desynced state failed).
2. 0xA5's non-responsiveness needs a driver-side look before Tier A is
   retried: the sync handler pings once after ~1s
   (`cortex_m::asm::delay(180_000_000)`). The module may need longer, or
   the 0xA5 wake handshake may differ from a plain ping. Not characterized.
3. Boot policy remains blocked on #92's root cause: today's soak shows the
   dominant delivery killer is firmware-side staging under load (see
   LIMITATIONS.md), which no module-side boot heal can fix.

## Suggested next steps (for the issue)

- Retitle Tier A as "needs wake-handshake characterization" or drop.
- If a boot policy is still wanted, Tier B (0x22 on boot) must first gain
  retry-from-fresh-boot semantics (today: second attempt healed).
- #92 root cause (firmware staging race) is the unblocking work for both
  #100 acceptance ("campaign green ×3") and E5's recovery_ok.
