# v0.8 collaboration contracts

This repository is currently a flat Rust module tree; the v0.8 design-system
paths are target ownership boundaries, not existing filesystem paths.

| Design-system area | Current implementation | v0.8 owner |
| --- | --- | --- |
| `ui/tokens` | `src/design.rs` | A |
| `ui/layout` | `src/design.rs::RectD`; `src/anchor.rs` owns screen geometry | A |
| `ui/tooltip` | Not present in this baseline; new shared module required | A |
| Rail and pill | `src/render.rs`, `src/window.rs`, `src/view.rs` | A |
| Activity registry | `src/activities/mod.rs` | A |
| Calls | `src/activities/call.rs`, `src/activities/audio.rs` | A |
| Music/timer | `src/activities/media.rs`, `src/activities/timer.rs` | A |
| Shelf | `src/activities/shelf.rs` | B |
| Convert/clipboard/engines | Not present in this baseline; B creates them | B |
| Clipboard platform code | Not present in this baseline; B creates it | B |
| Installer/release | `scripts/installer.iss`, `scripts/build.ps1` | A |

## Frozen shared APIs for Phase 0

- Tokens: `crate::design`; Phase A1 will introduce the v0.8-compatible token
  surface and `card_content_rect()` there. Existing callers must not derive
  card content dimensions locally.
- Layout: `card_content_rect() -> RectD` returns the non-rail content area of
  the fixed `FLY_W × FLY_H` card after `CARD_PAD` on all content edges.
- Tooltip: no reusable implementation exists in the starting revision. The
  Phase A1 API will be owned by A and published before B adopts it.
- Activities: the baseline has no `Activity` trait. Activities are currently
  selected in `window.rs` and represented by `view.rs` view models. A will
  publish a trait/registry only after preserving current behavior.
- Pills: `view.rs` describes pill content and `render.rs` draws it; `window.rs`
  owns priority and click routing. Only A changes priority or pill-state rules.

## Compatibility note

The base revision predates the v0.6/v0.7 work referenced by the v0.8 brief.
Agent B should create new owned modules without editing A-owned rendering,
layout, token, or priority code; requests for those seams belong in
`coordination/from-B.md`.
