# Copy response

Evidence for #267. Frames are the real `App` against a live `jabot-hostd`
(`scripts/live.sh` + `scripts/dev/shot.mjs`), Chief and a Code thread both
on `fake-acp`.

| file | what it shows |
| --- | --- |
| `bot-chat-copy.png` | Chief reply with the overlapping-squares action under the bubble |
| `bot-chat-tooltip.png` | hover: “Copy response” tooltip on the same control |
| `bot-chat-copied.png` | after click: check icon and “Copied” confirmation |
| `code-chat-copy.png` | the same row on a Code conversation |

Both surfaces render `Transcript`. The copied payload is that reply’s source
markdown, not the rendered bubble or the chrome around it.
