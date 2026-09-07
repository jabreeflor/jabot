# Devices as a Settings tab

Evidence for moving pairing out of the CODE rail. Captured by rendering the
real `Sidebar` and `SettingsView` in Chromium at 1180×860, deviceScaleFactor 2,
against the same fixtures the unit tests use. The throwaway Vite entry that
mounted them is not part of the tree.

| file | what it shows |
| --- | --- |
| `sidebar.png` | the CODE list after the move: Schedules, then folders — no Devices row. The gear in the profile footer is the way in. |
| `general.png` | Settings open on **General** — the idle-timeout and fold-policy knobs, with a **Devices** tab next to them. |
| `devices.png` | the **Devices** tab: console, a live pairing, and a revoked tombstone. |

Pairing is a fact about this Mac. A CODE row under Schedules made revoke look
like a daily surface; it is the answer to "my phone was stolen". The list,
the in-place confirm, and the tombstones are unchanged — only the way in
moved.
