# Emoji reactions (#265)

Evidence from the real renderer via `./scripts/live.sh` / `scripts/dev/shot.mjs`
against a live `jabot-hostd`. Chief is on `fake-acp`; the turn is a real
`session/prompt`.

| file | what it shows |
| --- | --- |
| `before.png` | Agent reply with the Add reaction control, no marks yet |
| `after-reacted.png` | 👍 badge under the reply, picker closed |
| `picker.png` | The palette open next to the existing mark |
| `walkthrough.html` | PR artifact — toggle path, store overlay, verify steps |
| `artifact-full.png` | The walkthrough as rendered |

The add control is a real button named **Add reaction**. Badges the user
added are named **Remove {emoji} reaction**.
