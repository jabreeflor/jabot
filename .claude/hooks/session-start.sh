#!/bin/bash
#
# SessionStart hook for Claude Code on the web.
#
# A web session starts in a fresh Linux container with none of what the live
# loop needs: the GTK/WebKit libraries the crate links against, node_modules,
# the dev binaries, a browser. `scripts/live.sh setup` installs every one of
# them, idempotently, and this hook is nothing but that call — so a session
# can run `./scripts/live.sh up` and `./scripts/verify.sh` from its first
# turn without an agent rediscovering the recipe.
#
# Synchronous on purpose: the container state is cached once the hook has
# finished, and a session that starts before the host is built would only
# start by failing its first `up`.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
./scripts/live.sh setup
