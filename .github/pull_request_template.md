# Pull Request Template

## Description

Please describe the changes you made and why.

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] License/legal change

## Checklist

- [ ] cargo fmt --check passes
- [ ] cargo clippy --all-targets -- -D warnings passes
- [ ] cargo test passes
- [ ] License compatibility verified (GPL-3.0-or-later)
- [ ] No new network-capable crates added
- [ ] No secrets or private data added
- [ ] Documentation updated if needed

## Checklist for License Switch

- [ ] LICENSE file is GPL-3.0-or-later
- [ ] NOTICE file exists with correct copyright
- [ ] SPDX-License-Identifier headers added to source files
- [ ] Cargo.toml license field updated
- [ ] THIRD_PARTY_NOTICES.md updated
- [ ] TERMS.md and PRIVACY.md created

---  
Fill in the above template when opening a new pull request.