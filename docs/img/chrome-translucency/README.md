# Chrome translucency (extends #250)

#250 mixed only `--side-fill` / `--chat-fill` and left raised chrome solid.
This pass keeps that enablement (`data-translucency`, Reduce Transparency,
macOS vibrancy / `?translucency=preview`) and extends the mix to `--raise`,
`--raise2`, `--sunk`, `--bub-me`, plus a light `backdrop-filter` frost on
panes and floating chrome.

Shots use `./scripts/live.sh shot --translucency preview --desktop …` against
a live host. The loud desktop behind the page is what a 5–12% mix has to
bleed; Reduce Transparency and a missing native material stay fully opaque.

| file | what it shows |
| --- | --- |
| `before-dark-sidebar-chat.png` | Dark, panes-only mix (#250): search, composer, selected row stay solid |
| `before-light-sidebar-chat.png` | Light, same |
| `before-dark-modal.png` | Add-a-bot overlay: opaque card on a solid dimmer |
| `after-dark-sidebar-chat.png` | Dark: sidebar, chat, search, composer, bubbles all slightly glass |
| `after-light-sidebar-chat.png` | Light: same tokens, readable ink |
| `after-dark-modal.png` | Frosted modal; BotMark tiles unchanged |
| `after-light-modal.png` | Light modal |

Cream buttons, ink, and hairlines stay solid. Bot icon artwork is untouched.
