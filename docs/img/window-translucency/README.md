# Subtle window translucency (#250)

Evidence from the real renderer via `./scripts/live.sh` / `scripts/dev/shot.mjs`
against a live `jabot-hostd`. Native under-window vibrancy is macOS-only
(Tauri `Effect::UnderWindowBackground`); Linux has no window effects, so
these shots enable the CSS fill with `?translucency=preview` and paint a
fake desktop behind the page. Reduce Transparency and a missing native
material keep the same surfaces fully opaque — that is the before column.

| file | what it shows |
| --- | --- |
| `before-dark-over-light.png` | Dark theme over a light desktop, opaque fallback |
| `before-light-over-dark.png` | Light theme over a dark desktop, opaque fallback |
| `after-dark-over-light.png` | Dark chrome at ~96–97% over the same light desktop |
| `after-light-over-dark.png` | Light chrome at ~96–97% over the same dark desktop |
| `after-dark-over-dark.png` | Dark over dark — the hint stays restrained |
| `after-settings.png` | Settings / Appearance, dark: text and radios stay solid |
| `after-settings-light.png` | the same pane on Light |
| `walkthrough.html` | PR artifact — native material, fill tokens, fallbacks |

Foreground (search field, composer, selected row, Settings controls)
stays on the solid `--raise` / `--cream` tokens. Only `--side-fill` and
`--chat-fill` mix.
