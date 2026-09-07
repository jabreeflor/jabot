# Restore animated bot avatars in the vertical tab list (#207)

Evidence from the real app via `./scripts/live.sh` / `scripts/dev/shot.mjs`
against a live `jabot-hostd`. The sidebar stayed a vertical chat list; the
faces are the earlier monochrome silhouettes again, not solid color circles.

| file | what it shows |
| --- | --- |
| `vertical-rows.png` | Dark theme. Chief, Recruiter, Writer, Talent Scout, Relay, and Crew as rows: distinct outline characters, name, last-line/persona. No color blocks. |
| `vertical-rows-light.png` | Same list on the cream-paper light theme. Stroke follows `--ink`. |
| `selected-writer.png` | Writer selected: circular ring on the row’s own avatar only; labels still one line. |
| `picker.png` | Crew → Add a bot. The eight characters (Classic…Visor) with their idle motions named. |
| `walkthrough.html` | PR artifact — how color IDs become drawings, and why the list layout is untouched. |
