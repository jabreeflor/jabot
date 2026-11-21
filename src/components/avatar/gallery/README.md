# open-bot-avatar gallery (vendored)

Copied verbatim from
[DominikScholz/open-bot-avatar](https://github.com/DominikScholz/open-bot-avatar)
`gallery/` at commit `5e64c081971eb296f562544641bad37d40fb36b6`, MIT licensed
(see `LICENSE`). The npm package ships the starters (`examples`) but not the
gallery, so the files live here.

- `<id>.json` — one avatar definition each. Do not edit; re-copy from upstream.
- `colors.json` — each avatar's default body color, starters included.

To update: copy `gallery/*.json` from a newer upstream commit, bump the hash
above, and run `npm test` — `bots.test.ts` validates every definition and
checks the catalog against the host's `BOT_COLORS`.
