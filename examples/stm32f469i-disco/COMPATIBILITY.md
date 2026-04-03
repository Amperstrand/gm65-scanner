# DS2208 Compatibility Profile

This firmware aims to match **Zebra DS2208 host interaction and operator UX as closely as practical** while keeping a **project-owned USB identity**. It does **not** impersonate Zebra USB identifiers, manufacturer/product strings, or serial schemes.

## Scope

### Async firmware (`async_firmware`)

This is the DS2208-compatible image.

- **USB modes**
  - `DS2208 Keyboard HID` (default)
  - `DS2208 HID POS`
  - `Admin CDC`
- **Persistence**
  - active USB mode
  - suffix: none / Enter / Tab
  - keystroke delay: 0 / 20 / 40 ms
  - case handling: preserve / upper / lower
  - fast HID: on / off
  - caps-lock override: on / off
  - simulated caps lock: on / off
  - scanner settings byte
  - optional prefix/suffix byte sequences (CDC/admin path)
- **UI**
  - touch-accessible compatibility page
  - settings saved in internal flash (bank 2 sector region)
  - USB mode / fast HID changes trigger reboot for clean re-enumeration

### Sync firmware (`stm32f469i-disco-scanner`)

This remains the legacy/reference CDC firmware.

- keeps the existing screen + CDC + scanner relay behavior
- rejects the new DS2208 profile CDC commands with `InvalidCommand`
- documented here so the scope difference is explicit

## DS2208 behaviors selected

Authoritative references:

- Zebra DS2208 Product Reference Guide  
  <https://www.zebra.com/content/dam/support-dam/en/documentation/unrestricted/guide/product/ds2208-prg-en.pdf>
- Zebra DS2208 Quick Start Guide  
  <https://www.zebra.com/content/dam/support-dam/en/documentation/unrestricted/guide/product/ds2208-qsg-en.pdf>
- Microsoft barcode scanner configuration guidance  
  <https://learn.microsoft.com/en-us/windows/uwp/devices-sensors/pos-barcodescanner-configure>

Implemented DS2208-like defaults:

- default USB profile: **Keyboard HID**
- optional HID POS profile
- optional Admin CDC profile for diagnostics/configuration
- suffix options: none / Enter / Tab
- keystroke delay options: 0 / 20 / 40 ms
- case handling: preserve / upper / lower
- caps-lock override tracking in keyboard mode
- simulated caps lock toggle
- fast HID toggle (changes HID poll interval)

## Operator feedback

The DS2208 documentation describes:

- power-up: low / medium / high
- successful decode: short high
- transmission error: 4 long low
- programming/config success and error patterns

This board does **not** have a programmable buzzer that can faithfully synthesize those tones. The current implementation therefore uses:

- GM65 scanner `SOUND` setting for module-side audible feedback when available
- LED pulse patterns to approximate DS2208 event classes
- display status messages

Implemented event mappings:

- power-up → 3 rising-duration LED pulses
- decode success → 1 short LED pulse
- transmission error → 4 long LED pulses
- config success → 2 short LED pulses
- config error → 2 uneven LED pulses

## HID mode details

### Keyboard HID

- Boot keyboard descriptor / 8-byte reports
- report stream generated from `gm65_scanner::hid::keyboard`
- deterministic unsupported-character policy: **skip unmappable bytes**
- optional prefix/suffix raw byte sequences are applied before/after barcode data
- suffix key option (`Enter` / `Tab`) is applied after payload bytes
- no per-character delay by default

### HID POS

- uses the library HID POS descriptor and report layout
- sends decoded data + explicit length + symbology field
- current firmware uses `SYMBOLOGY_UNKNOWN` when the GM65 transport does not provide a reliable AIM code
- payloads over 256 bytes are truncated explicitly and surfaced on-screen

## CDC/Admin protocol additions

The existing frame format remains unchanged.

### New commands

| Command | Code | Payload | Response |
|---|---:|---|---|
| `GetCompatibilityProfile` | `0x20` | none | active USB mode byte |
| `SetCompatibilityProfile` | `0x21` | 1 byte mode | `RebootRequired` |
| `RebootUsb` | `0x22` | none | `RebootRequired` |
| `GetHostOptions` | `0x23` | none | serialized 64-byte profile |
| `SetHostOptions` | `0x24` | serialized 64-byte profile | updated profile or `RebootRequired` |

### Mode bytes

| Value | Meaning |
|---:|---|
| `0x01` | DS2208 Keyboard HID |
| `0x02` | DS2208 HID POS |
| `0x03` | Admin CDC |

### Status additions

| Status | Code | Meaning |
|---|---:|---|
| `RebootRequired` | `0x20` | settings saved; reboot/re-enumeration required |

## Host test matrix

### Keyboard HID

- [ ] Linux: text input field / terminal
- [ ] Windows: Notepad / generic text field
- [ ] macOS: text input field
- [ ] caps-lock override on/off
- [ ] case conversion preserve/upper/lower
- [ ] suffix none/Enter/Tab
- [ ] key delay 0 / 20 / 40 ms

### HID POS

- [ ] Windows Device Manager enumerates HID scanner-oriented interface
- [ ] Windows POS / UWP barcode path smoke test
- [ ] Linux hidraw / generic HID read
- [ ] payload truncation behavior over 256 bytes
- [ ] unknown symbology behavior accepted by host tooling

### Admin CDC

- [ ] query active profile
- [ ] set active profile
- [ ] query/set host options
- [ ] reboot/re-enumerate command

## Known deviations from a real DS2208

- USB identity is project-owned, not Zebra
- sync firmware is not a DS2208 profile image; async firmware is the compatibility target
- audible tones are approximated with LED/display plus whatever the GM65 module itself emits
- HID POS Windows/POS-driver behavior is not yet hardware-validated on this branch
- HID POS currently sends `unknown` symbology when the scanner transport does not expose AIM IDs
- unsupported keyboard characters are skipped rather than converted through vendor-specific fallback schemes

## Quick checklist

- [x] Async firmware builds with DS2208 compatibility modes
- [x] Sync firmware still builds as legacy/reference CDC image
- [x] Library tests pass
- [ ] Validate Keyboard HID on Linux
- [ ] Validate Keyboard HID on Windows
- [ ] Validate Keyboard HID on macOS
- [ ] Validate HID POS on Windows
