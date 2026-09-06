# Sidebar sparkle

Evidence that thread rows no longer print a coloured pip and a status word.
Both frames are the real `App` in Chromium at deviceScaleFactor 2 against a
live `jabot-hostd` (`./scripts/live.sh up`), seeded with two code threads in
one folder: `Auth migration` held open on `fake-acp-agent hang`, and
`Sidebar overflow fix` finished on the default echo agent.

| file | what it shows |
| --- | --- |
| `after.png` | the shell: no "running" / "done" labels, no amber/green dots |
| `folder.png` | the two rows together, 3×3 marks where the pips were |
| `auth-migration-running.png` | a live row. Cells at mixed brightness — the loop in flight |
| `sidebar-overflow-fix-done.png` | a settled row. A still constellation, not a green pip |
| `live-a.png` `live-b.png` `live-c.png` | three frames of the same live mark, a few hundred ms apart |

The status word is the row's accessible name (`Auth migration, running`) and
the sparkle's tooltip. It is not drawn.
