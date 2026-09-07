# Bot awareness and conversational drafts (#237)

Captured against the live host (`./scripts/live.sh up`) with Chief on
`fake-acp`. `draft_bot` was called over the real loopback MCP bridge, then
the renderer was driven with `scripts/dev/shot.mjs`.

| File | What it shows |
| --- | --- |
| `pending-proposals.png` | Crew view with a pending Researcher proposal |
| `proposal-editor.png` | Proposed-by-Chief editor (Close ≠ Dismiss) |
| `after-save.png` | Researcher saved as a crew member; no pending list |
