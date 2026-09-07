# Collapsible sidebar

Evidence for the rail toggle. Both frames are the real `App` in Chromium at
deviceScaleFactor 2 against the Vite preview (no Tauri host — the fixture
crew and threads, and the host-line error, are that build).

| file | what it shows |
| --- | --- |
| `open.png` | the full rail, with the split-window toggle to the left of Search |
| `closed.png` | the rail collapsed to the traffic-lights strip; the chat keeps the rest |
| `toggle.png` | the control itself, cropped from the open frame |

The glyph is a rounded window with a left partition — the same picture the
shell is. It sits in a raised 32px square so it reads as a control next to
the search field rather than as part of it.

Closed is not gone. Overlay traffic lights still need a dark strip to sit
on, and the button that brings the list back has to stay where the hand
already is. Width is `--traffic-lights-w` (78px) so the lights never spill
onto the chat. The list and the me-row unmount; Tab cannot wander into a
column that is not on screen.

The preference is `jabot.sidebarOpen` in localStorage. Only an explicit `0`
hides the rail on the next launch — a missing key or garbage opens, which is
the same default as a first run. Ctrl/⌘B is the chord; it is silent while a
modal has the keyboard.
