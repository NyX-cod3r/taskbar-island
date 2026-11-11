# Contributing to TaskbarIsland

## Building

`ash
cargo build --release
`

## Testing

`ash
cargo test
`

## Code Style

Run clippy with warnings treated as errors:
`ash
cargo clippy --all-targets -- -D warnings
`

Run cargo deny for license and advisory checks:
`ash
cargo deny check licenses check advisories
`

## Version Rules

The project uses x.y.z version naming:
- **x** (major): Huge overhaul - stays 0 until the production-ready 1.0.0 release, after owner approval
- **y** (minor): Single feature upgrade or incremental upgrade with it - bump y, reset z to 0
- **z** (patch): Minor bug fixes and small tweaks - bump z only

## Contribution Process

1. Fork the repository
2. Create a feature branch from main
3. Make your changes, ensuring all tests pass
4. Run cargo fmt --check and cargo clippy --all-targets -- -D warnings
5. Submit a Pull Request with a clear description of changes
6. The maintainer will review and merge

## Contributions Accepted Under

All contributions must be submitted under the GPL-3.0-or-later license matching this project.

---  
This is a plain-language template, not legal advice.