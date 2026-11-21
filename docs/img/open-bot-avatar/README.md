# Bot icons are open-bot-avatar's avatars

Evidence for drawing every built-in bot icon with
[open-bot-avatar](https://github.com/DominikScholz/open-bot-avatar): all 23 of
its avatars (7 starters + 16 gallery), each in upstream's default color and
name. Every frame is `./scripts/live.sh shot` against a real `jabot-hostd`.

| file | what it shows |
| --- | --- |
| `picker-dark.png` | The editor's "Choose a bot" grid: every upstream avatar, starters first, then the gallery by name. |
| `picker-light.png` | The same grid in the light theme (white Mug/Egg/Ghost keep their shading on the light tiles). |
| `crew.png` | Bot Recruiter saved as Rocket (`oba-rocket`, a gallery avatar) and reloaded — the host accepts and persists the new IDs. |
| `sidebar-dark.png` | Sidebar rows, header pills and the Crew cluster after that reload. |
| `sidebar-light.png` | The same in the light theme. |
