/*
  Fallback linker script for non-defmt firmware builds.

  Some firmware targets in this workspace still pass `-Tdefmt.x` via
  `.cargo/config.toml`. Production USB builds intentionally omit the
  defmt runtime, so we provide an empty linker script to keep those
  targets linkable without pulling RTT back in.
*/
