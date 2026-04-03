# DS2208-Compatible Host Validation Checklist

This checklist is for the project-owned DS2208-compatible async firmware image.
It intentionally validates **behavior** rather than Zebra identity strings.

Authoritative references:

- Zebra DS2208 Product Reference Guide  
  <https://www.zebra.com/content/dam/support-dam/en/documentation/unrestricted/guide/product/ds2208-prg-en.pdf>
- Zebra DS2208 Quick Start Guide  
  <https://www.zebra.com/content/dam/support-dam/en/documentation/unrestricted/guide/product/ds2208-qsg-en.pdf>
- Microsoft barcode scanner configuration guidance  
  <https://learn.microsoft.com/en-us/windows/uwp/devices-sensors/pos-barcodescanner-configure>

## Keyboard HID

- [ ] Linux text editor / terminal receives scan text
- [ ] Windows Notepad / generic text field receives scan text
- [ ] macOS text field receives scan text
- [ ] Suffix `None` emits only payload bytes
- [ ] Suffix `Enter` submits a line / activates default text-field behavior
- [ ] Suffix `Tab` advances focus as expected
- [ ] Prefix bytes appear before payload
- [ ] Raw suffix bytes appear before the suffix key option
- [ ] Case mode `Preserve` leaves ASCII case untouched
- [ ] Case mode `Upper` forces ASCII letters uppercase
- [ ] Case mode `Lower` forces ASCII letters lowercase
- [ ] Caps-lock override ON preserves intended letter case while host Caps Lock LED is on
- [ ] Simulated caps lock ON toggles host-visible caps behavior only when needed for alphabetic payloads
- [ ] Key delay `0 / 20 / 40 ms` feels like expected inter-key spacing

## HID POS

- [ ] Windows Device Manager shows scanner-oriented HID interface
- [ ] Windows POS / UWP smoke test can open the scanner path
- [ ] Linux `hidraw` can read 261-byte reports
- [ ] Host honors little-endian length field and ignores zero padding
- [ ] Payloads at 255 / 256 / 257 bytes match expected truncation semantics
- [ ] `SYMBOLOGY_UNKNOWN` is accepted when no reliable AIM code is available

## Admin CDC

- [ ] `GetCompatibilityProfile` returns the active personality
- [ ] `SetCompatibilityProfile` returns `RebootRequired`
- [ ] `SetHostOptions` only returns `RebootRequired` when USB mode / fast HID changes
- [ ] `RebootUsb` forces clean re-enumeration

## Lightweight Host Tools

- POSIX CDC admin tool:  
  `/home/runner/work/gm65-scanner/gm65-scanner/examples/stm32f469i-disco/tools/cdc_admin.py`
- Linux HID POS dump tool:  
  `/home/runner/work/gm65-scanner/gm65-scanner/examples/stm32f469i-disco/tools/hid_pos_dump.py`

## Suggested Execution Notes

- Record OS version, application used, and whether the board was in Keyboard HID / HID POS / Admin CDC mode.
- When testing Keyboard HID delays, compare `fast_hid` on/off separately from `key_delay_ms`; `fast_hid` changes USB polling interval, while `key_delay_ms` spaces out release reports between emitted keystrokes.
- When testing HID POS, capture at least one short barcode and one payload over 256 bytes so truncation behavior is explicit.
