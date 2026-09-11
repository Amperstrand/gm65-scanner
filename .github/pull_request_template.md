## Summary

<!-- Brief description of the change and why it's needed -->

## Changes

<!-- List the key changes made -->

-

## Testing

<!-- How was this tested? Check all that apply -->

- [ ] `cargo test -p gm65-scanner --lib` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy -p gm65-scanner --all-features -- -D warnings` passes
- [ ] Cross-compile: `cargo build --release --target thumbv7em-none-eabihf -p stm32f469i-disco-scanner --no-default-features --features sync-mode`
- [ ] Cross-compile: `cargo build --release --target thumbv7em-none-eabihf -p stm32f469i-disco-scanner --no-default-features --features scanner-async`
- [ ] HIL tested on hardware (describe results)

## Breaking Changes

<!-- Does this change break any existing functionality or API? -->

None / Yes (describe):

## Checklist

- [ ] I have read [CONTRIBUTING.md](../CONTRIBUTING.md)
- [ ] My code follows the project's style guidelines
- [ ] I have added tests for new functionality
- [ ] Documentation has been updated
