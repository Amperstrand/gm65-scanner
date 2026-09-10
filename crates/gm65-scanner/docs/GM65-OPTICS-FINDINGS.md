# GM65 Optics Findings — screens, distance, moiré

> Compiled 2026-09-10 from datasheet mining + ecosystem research during the
> CYD loopback bring-up. Answers "why does the module read hand-held phone
> QRs but never LCD-screen QRs" with citations. Complements
> `GM65-PROTOCOL-FINDINGS.md` (wire protocol) — this file is the optics.

## Datasheet facts (GROW GM65)

| Parameter | Value | Source |
|-----------|-------|--------|
| Sensor | 648x488 CMOS | DFRobot DFR0660, hzgrow GM65 |
| FOV | 34°H × 26°V (some sheets 35°/28°) | DFRobot, sunrom manual |
| Min working distance | **4.0 cm (hard DoF floor, all densities)** | sunrom 60-page manual scan-area table |
| Max distance | scales with module size: 5mil → 9cm, 15mil → 25cm | same table (250 lux office) |
| Min print contrast | 25–30% | hzgrow / DFRobot |
| Illumination | 6500K white LED (aim: 617nm red) | DFRobot |
| Screen mode / exposure regs | **NONE** — only illumination on/off + aim | DFRobot 12MB config manual, full-text grep |

## Why phone screens work and bench LCDs don't

1. **Pixel pitch.** A phone at ~400 PPI presents modules finer than the
   sensor's optical resolution — the grid averages into smooth gray, like
   paper. The CYD's ST7796 (~110 PPI, ~0.23 mm pitch) sits *at* the sensor's
   resolvable band: the pixel grid beats against the 648x488 sensor grid
   (grid resonance at ~25 cm for these numbers) and corrupts module edges.
   Moiré is pose-sensitive — shifts of 0.5 mm / 0.1° change it (INFOCOM
   "Moiré QR" paper) — so a fixed mount locks in one (bad) moiré condition
   while a hand-held phone's micro-motion escapes it.
2. **No screen mode.** Zebra imagers ship a dedicated "LCD mode" "to
   improve imager performance reading barcodes from LCD displays"
   (Zebra DataWedge docs). The GM65 has no equivalent register — there is
   nothing to tune electronically.
3. **Specular glint.** Glossy polarizer + fixed parallel mount reflects
   ambient light straight back into the lens. Specter-DIY (GM65 airgap
   wallet) ships illumination OFF by default with the comment *"Flashlight —
   can create blicks on the screen"*.
4. **Backlight PWM banding.** Cheap TFT backlights can strobe at frequencies
   that alias into a rolling-shutter capture (horizontal bands). Check with
   a phone slow-mo camera pointed at the screen.

## Project history corroborates (see AGENTS.md rig section)

- Every verified scan in this repo's history (17/18/23/25/47-byte passes,
  Mar–May 2026 HIL tables + Aug 2026 session) was a **hand-presented**
  target at ~5–10 cm.
- The `hil_test.py` e2e mode — QR rendered on the host PC screen, held up by
  a human — has **zero logged passes** in the entire git history.
- The CYD rig (2026-09-08/09): 0 scans across sizes 40–288 px, 18 positions,
  both mirror parities (both readable classes covered), ±illumination,
  three firmware builds — while hand-held phone QRs decode in the same
  session.

## Mitigations, ranked by cost

**RESOLVED 2026-09-10 (matrix experiment, self-identifying sequential QRs):**
on this exact GM65 + ST7796 rig the winning configuration is **inverted
rendering (white modules on black) + ECC-H + ~203px QR (224px cap)** —
only inverted cells decoded; normal polarity never did, at any size or
position. Film-negative works because the black background cuts screen
emission/bloom and the engine (fw 0x87) reads inverse codes. The 261px
full-size QR never decoded — 203px is the FOV sweet spot. The full
loopback harness runs 6/6 green in this configuration (byte-exact
roundtrips at 10/45/127 bytes). Rig constant: `QR_WINNING_CAP = 224`
(tools/hil/rig.py).

1. Distance ≥ 4 cm (DoF floor); sweet spot 8–15 cm for large modules.
2. Tilt the screen 10–20° (kills specular glint; breaks moiré pose-lock).
   Beware TN contrast inversion beyond ~30° viewing angle.
3. Matte anti-glare film (diffuses glint and softens the pixel grid).
4. Render once and freeze (no animation) — refresh-vs-shutter tearing is
   only a factor for redrawn content (HPRT/cnscanpay vendor guides).
5. Highest-density display available (OLED > high-PPI TFT > ST7796), or
   e-ink / paper.

## References

- sunrom GM65 manual (scan-area DoF table): sunrom.com/download/767.pdf
- DFRobot DFR0660 + 12MB config manual: dfrobot.com.cn/goods-2648.html
- hzgrow GM65 spec: hzgrow.com/product/35.html
- Zebra DataWedge "LCD mode": techdocs.zebra.com/datawedge (barcode input)
- Specter-DIY qr.py (light-off default for screens):
  github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py
- HPRT "Can barcode scanners scan screens" manufacturer guide (hprt.com)
- INFOCOM moiré-QR paper (pose sensitivity of moiré)
