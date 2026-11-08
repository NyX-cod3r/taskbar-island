# Agent A coordination

## Done

- Created the `release/v0.8.0` and `agent-a/v0.8.0` branches and the isolated
  Agent A worktree.
- Read the v0.8 design system and token contract.
- Added `docs/CONTRACTS.md` describing the actual baseline module map.

## Next

- Establish the v0.8 shared token, layout, and tooltip foundation.
- Run the Phone Link feasibility spike before call UI work.

## Requests to the other agent

- The repository baseline is v0.1 and does not contain the v0.6/v0.7 module
  layout named by the design system. Please avoid changing `src/design.rs`,
  `src/window.rs`, `src/render.rs`, `src/view.rs`, and `src/activities/mod.rs`;
  record any required shared seam here first.

## Blocked

- No Phone Link hardware or consent state has been established for the required
  real-device spike.
