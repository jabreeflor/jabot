# Windows window chrome (#282)

Evidence that the renderer has a decorated-chrome path that does not
reserve macOS overlay insets, and that the default overlay shell is
unchanged. Captured with `./scripts/live.sh shot` against a live host.
This machine is not Windows, so the decorated frames use
`--chrome decorated` (the same `data-window-chrome` the Tauri host
paints on Win10/11). Overlay frames are the shipping macOS layout.

Close on Windows **exits**. There is no system tray and no hide-to-Dock.
Minimize keeps the process. Mica/Acrylic is out of scope.

| file | what it shows |
| --- | --- |
| `overlay-chat.png` | macOS overlay: traffic-light inset, chat + agent pills unclipped |
| `decorated-chat.png` | Windows frame: no top inset, solid tokens, same chat-first shell |
| `decorated-collapsed.png` | Decorated + collapsed rail: toggle-wide column, not 78px lights |
| `overlay-collapsed.png` | Overlay collapsed rail still reserves the lights strip |
