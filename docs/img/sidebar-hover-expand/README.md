# Hover-to-expand sidebar

Evidence for the collapsed-rail flyout (#209). App frames are the real
`App` via `./scripts/live.sh shot` against a live `jabot-hostd`. The
walkthrough frames are the standalone explainer in this directory.

| file | what it shows |
| --- | --- |
| `collapsed.png` | pinned shut: 78px strip, list unmounted, chat keeps the rest |
| `peeked.png` | after the toggle is armed, pointer in the rail slides the list over the chat |
| `pinned.png` | click / ⌘B still pins the rail in flow |
| `walkthrough.html` | mechanism, files, how to verify |
| `artifact-full.png` | the rendered walkthrough |
| `artifact-section-1.png` | the collapsed → armed → peek sequence |

Peek is not a pin. `jabot.sidebarOpen`, the toggle, and Ctrl/⌘B still own
the remembered open/closed state. The hide click does not immediately
re-open just because the pointer is still on the button.
