# Design (v0.9)

The current design material for TaskbarIsland v0.9:

- `Taskbar Island Design v0.9.0.pdf` — the v0.9 artboards (Music, Timer, Call, Shelf,
  Clipboard, Convert, pill and call pill states).
- `TaskbarIsland-v0.9-Design-System-Addendum.md` — the v0.9 design-system addendum.
- `design-tokens.json` — the token file the code compiles in
  (`src/tokens.rs` → `include_str!`); the design-drift unit tests compare `src/design.rs`
  against it, so any size change updates both.
- `design-tokens-v0.9.0.json` — the v0.9 token set as authored (reference source while
  `design-tokens.json` is migrated).
- `icons/` — the four activity identity SVGs (`call`, `music`, `shelf`, `timer`); the
  shipped glyphs are baked into `src/icons.rs` as constants, the SVGs are the design source.

**Precedence:** where any document here disagrees with the current prompt in
`../prompts/`, the prompt wins — in particular the outer sizes. The shell stays at the
repo's values (`design.rs`: pill 224 × 40 DIP, card 400 × 192 DIP at 100 %; i.e.
280 × 50 px and 500 × 240 px at 125 %, which is what the v0.9 design measurements were
taken from) and the inner layout is scaled to fit; the shell is never resized.
