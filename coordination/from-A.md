# Agent A coordination

## Done

- Created the `release/v0.8.0` and `agent-a/v0.8.0` branches and the isolated
  Agent A worktree.
- Read the v0.8 design system and token contract.
- Added `docs/CONTRACTS.md` describing the actual baseline module map.
- Added the checked-in v0.8 token source, token validation, shared content
  geometry, and anchor tests for the 4-DIP pill inset / 6-DIP card gap.
- Completed the read-only Phone Link spike. Detection, banner suppression, and
  call controls are NO-GO without a linked real-phone validation; audio is
  PARTIAL. See `docs/spikes/phone-link.md`.
- Added pure call lifecycle/provider seams with a simulator and privacy-safe
  number masking. The release Phone Link provider reports unsupported controls
  instead of inventing success.

## Next

- Finish tooltip and pill-priority integration on the actual renderer.
- Integrate only Phone Link capabilities that pass a real-device validation.

## Requests to the other agent

- The repository baseline is v0.1 and does not contain the v0.6/v0.7 module
  layout named by the design system. Please avoid changing `src/design.rs`,
  `src/window.rs`, `src/render.rs`, `src/view.rs`, and `src/activities/mod.rs`;
  record any required shared seam here first.

## Blocked

- Real Phone Link detection/control remains blocked on a linked phone and
  explicit user-approved manual call testing. The implementation is gated off.
- The requested strict Clippy gate is also blocked by the v0.1 baseline: it
  currently reports 161 errors across shared and Agent-B-owned files. This is
  recorded for release triage; no `ready-A-*` marker has been issued.
