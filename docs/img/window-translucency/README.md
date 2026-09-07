# Subtle Cursor-style window translucency (#250)

Evidence from the real app via `./scripts/live.sh` / `scripts/dev/shot.mjs`
against a live `jabot-hostd`. The live loop is Chromium, not a Tauri window,
so the shots inject a high-contrast “desktop” (`--backdrop`) behind the
chrome. That is how a 4% gap can be seen in a still; it is not a product
wallpaper.

`--opaque` forces the solid fallback the same way `prefers-reduced-transparency`
does, which is the before state and the reduced-transparency path.

| file | what it shows |
| --- | --- |
| `before-dark.png` | shipped opaque chrome over a dark desktop |
| `after-dark.png` | same scene with the 96% fill — a faint hint of the wallpaper |
| `before-light.png` | opaque Light palette over a light desktop |
| `after-light.png` | Light chrome at 95–96% over the same wallpaper |
| `after-dark-over-light.png` | Dark chrome over a light desktop — still readable |
| `after-light-over-dark.png` | Light chrome over a dark desktop — still readable |
| `settings-dark.png` | Settings → Appearance, Dark selected, fallback copy visible |
| `fallback-reduced.png` | `--opaque`: solid fills, no wallpaper bleed |

## Platform limits

| platform | native material | what you see |
| --- | --- | --- |
| macOS 10.14+ | `HudWindow` (Dark) / `HeaderView` (Light) on a transparent window | barely perceptible desktop through the chrome |
| macOS + Reduce Transparency | effects cleared; CSS fills go solid | identical to the pre-#250 window |
| Linux | `set_effects` is unsupported; `apply` is a compile-time no-op | opaque (4% CSS gap composites against an opaque backing) |
| Windows | not shipped; no Mica/Acrylic wired | opaque, same as Linux |
| live.sh / unit tests | no Tauri window | CSS fills only; `--backdrop` is how a screenshot shows the gap |

Foreground (text, icons, inputs, cards, modals) keeps the solid `--ink` /
`--raise` / `--side` / `--chat` tokens. Only `--*-fill` on the sidebar,
main pane, and onboarding ground go translucent.
