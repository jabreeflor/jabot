# Folder settings

Evidence for #94. Both are the real `App` in Chromium at deviceScaleFactor 2
against a stubbed `host_rpc` transport, over one registered folder with a setup
command and a files-to-copy list.

| file | what it shows |
| --- | --- |
| `sidebar-row.png` | the folder row, with the settings control beside the ＋ |
| `modal.png` | the card, seeded from what the host holds |

`folder/update` has been routed and handled since #16 — name, setup command,
files-to-copy, and a `refresh` that asks git again — and `client.updateFolder`
had no caller anywhere in `src/`. Everything a folder knew was typed once at
registration and then frozen, so a wrong setup command silently produced a
half-built worktree on every thread and there was no way to correct it short of
forgetting the folder and adding it again.

Two things the screenshots settle.

**The control is sliders, not a gear.** The first cut was a gear and the
screenshot is what caught it: at 14px the teeth close up and it reads as a sun.
Three tracks with a knob each stay legible at the size the row actually draws
them, which is the only size that matters.

**The path is shown and not editable.** The host has no move method, and a
folder pointed somewhere else is a different folder — the honest gesture is to
forget this one and add that one.

The line under the fields is what git currently says, because that is the fact
"Ask git again" exists to correct. And the action area takes a third button
without moving anything, which is where #23's deferred Repair lands when the
boot sweep gets one.
