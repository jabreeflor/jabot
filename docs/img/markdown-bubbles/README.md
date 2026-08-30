# Markdown inside an agent bubble

Evidence for #88. Both frames are the real `App` in Chromium at
deviceScaleFactor 2 against a stubbed `host_rpc` transport, over the same
`thread/transcript` — one reply of the shape a coding agent really sends: a
sentence with bold and inline code, a bulleted list, a fenced block, and a
closing line.

| file | what it shows |
| --- | --- |
| `before.png` | the raw characters. Backticks, asterisks and hyphens, as text |
| `after.png` | the same reply, drawn |

Two things in the frames are worth pointing at.

**The fence.** In `before.png` the code is prose — proportional, unindented,
wrapping, with ```` ```ts ```` and ```` ``` ```` as visible lines. In
`after.png` it is a block: monospaced, its own ground, its indentation intact,
and scrolling rather than widening the bubble when a line is long.

**The last line.** "Running the suite now — 2 * 3 files left to check." is
identical in both. That is the parser's flanking rule doing its job in the
render rather than only in a test: an agent writing about multiplication must
come out as typed. Most of what an agent says is prose, not markup, and a
renderer that ate a stray asterisk would be worse than the one that shipped.

The user bubble is untouched and stays literal throughout. A person's asterisks
are their own.

Everything reaches the DOM as a React text child — no `dangerouslySetInnerHTML`
anywhere, so there is no markup path to sanitise. An agent that echoes a user's
`<script>` back is drawing eight characters.
