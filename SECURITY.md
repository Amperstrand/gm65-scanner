# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do not** open a public issue.
2. Email the maintainers or use [GitHub Security Advisories](https://github.com/Amperstrand/gm65-scanner/security/advisories/new) to report privately.
3. Include:
   - A description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

We will acknowledge receipt within 48 hours and aim to release a fix
within 7 days for critical issues.

## Scope

This policy covers:

- The `gm65-scanner` driver crate (`crates/gm65-scanner/`)
- Firmware examples (`examples/stm32f469i-disco/`)
- CI/CD configuration

## USB Identity

This project uses **placeholder** USB VID/PID values for development.
Production deployments should use a legitimately obtained Vendor ID.
See the README for guidance on USB identity configuration.
