# Taskbar modes

**Which pill mode shipped in v0.11:** v0.11.0 ships the **embedded** pill — the island's pill is a
`WS_CHILD` window re-parented into `Shell_TrayWnd` (§7 Stage A), so it lives inside the taskbar's
own child list, hides and shows with the bar, never fights the taskbar for z-order, and needs no
separate top-level window at rest. The embedded child is created once at startup and healed by the
2 s z-order guard (a dead pill after an Explorer restart is re-embedded on `TaskbarCreated`, a live
one is only re-raised when a taskbar sibling sits above it). If the taskbar refuses the embed
(`SetParent`/`SetWindowPos` sequence fails), the app falls back to the **separate** top-level pill —
the card window draws the pill itself exactly as before Stage A — and logs `pill.embed: refused`.
The card/flyout itself is and remains a separate top-level popup in both modes.

## Modes (v0.12.0)

_To be filled in by the v0.12.0 taskbar-modes work: `Taskbar mode` setting
(Auto / Always embedded / Always separate), safe mode under other taskbar tools, and the
top/bottom/left/right edge behavior._
