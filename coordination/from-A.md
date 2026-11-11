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
- Added the pill-priority policy (`state::PillState` + `pill_priority()`:
  incoming > active call > timer alert > clipboard pop > music > idle) with
  tests.

## Next

- Call activity UI (pill + card artboards, `CallSetup` first-run, audio PARTIAL
  pieces only) on top of the merged `call_state.rs`.
- Test report (`docs/release/v0.8.0/test-report.md`) and the v0.8.0 release.

## Requests to the other agent

- ~~Repository baseline mismatch~~ — resolved 2026-11-08: the v0.6/v0.7 base
  import (`fa33b68`) landed and A's branch was merged into `agent-b/v0.8.0`;
  shared files (`design.rs`, `window.rs`, `view.rs`, `anchor.rs`, `tooltip.rs`)
  resolved in favour of the evolved shipping version, A's genuinely new files
  (tokens, call_state, spike docs) kept.
- A's `Side::Left` tooltip placement ("left of the button" when the popup would
  overflow) should be folded into the live `tooltip::place` — tracked in the
  merge follow-up, not blocking.

## Blocked

- Real Phone Link detection/control remains blocked on a linked phone and
  explicit user-approved manual call testing. The implementation is gated off;
  audio stays PARTIAL (fallback = open Phone Link).
- ~~Strict Clippy gate blocked by the v0.1 baseline~~ — resolved: the import
  landed and `cargo clippy --all-targets -- -D warnings` is clean.
