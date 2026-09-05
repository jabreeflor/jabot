#!/usr/bin/env bash
#
# The live loop, on any machine: set up, bring the real app up in a browser,
# drive it, take pictures, tear it down. No macOS, no Tauri, no guessing.
#
#   ./scripts/live.sh setup            # system libs, npm deps, dev bins, browser — idempotent
#   ./scripts/live.sh up               # vite + a real jabot-hostd behind it; waits until live
#   ./scripts/live.sh status           # is the host up? (GET /__jabot/host)
#   ./scripts/live.sh shot --out docs/img/<feature>/after.png [steps…]
#                                      # screenshot the live renderer (scripts/dev/shot.mjs)
#   ./scripts/live.sh rpc '<json-rpc>' # one request to the host, from the shell
#   ./scripts/live.sh seed             # put Chief on the fake ACP agent (no claude needed)
#   ./scripts/live.sh smoke            # reset, up, seed, drive one turn, screenshot: the
#                                      # whole loop proven in one command (~10s warm)
#   ./scripts/live.sh log              # the dev server's log
#   ./scripts/live.sh down             # stop it
#   ./scripts/live.sh reset            # stop it and wipe the dev data directory
#
# What "live" means here: `npm run dev` is served with the `jabot-host` Vite
# plugin (scripts/dev/host-plugin.ts), which spawns `jabot-hostd` — the same
# host the e2e suite drives, the same SQLite, the same ACP adapters — and
# bridges it over Vite's HMR socket to the renderer (src/host/devTransport.ts).
# The page a Chromium sees is the product minus the window chrome, not a mock.
#
# Every step is deterministic and prints what it did; a failure names the
# thing that failed and what to run. Nothing here needs a person at a prompt:
# `.claude/hooks/session-start.sh` runs `setup` for Claude Code on the web.
#
# Environment:
#   JABOT_LIVE_PORT      default 1420 (vite.config.ts pins it; tauri dev uses it too)
#   JABOT_HOSTD_BIN      the host binary; default src-tauri/target/debug/jabot-hostd
#   JABOT_DEV_DATA_DIR   the host's --data-dir; default .jabot-dev/data
#   JABOT_SECRETS_BACKEND  default memory (Linux has no Keychain; see secrets.rs)
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

DEV_DIR=.jabot-dev
PORT=${JABOT_LIVE_PORT:-1420}
URL="http://127.0.0.1:${PORT}"
PIDFILE="$DEV_DIR/vite.pid"
LOG="$DEV_DIR/vite.log"
MANIFEST=(--manifest-path src-tauri/Cargo.toml)

say()  { printf '\033[1m> %s\033[0m\n' "$*"; }
ok()   { printf '\033[32m  %s\033[0m\n' "$*"; }
warn() { printf '\033[33m  !! %s\033[0m\n' "$*" >&2; }
die()  { printf '\033[31mlive: %s\033[0m\n' "$*" >&2; exit 1; }

usage() { sed -n '3,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

# ---------------------------------------------------------------------------
# setup
# ---------------------------------------------------------------------------

# The pkg-config names Tauri's Linux build asks for. `verify.sh` needs the same
# set, which is why CI installs them (.github/workflows/ci.yml) and why a web
# session used to fail clippy: the crate links against them even for
# jabot-hostd, because the lib depends on `tauri` unconditionally.
LINUX_PKGS=(gtk+-3.0 webkit2gtk-4.1 libsoup-3.0 librsvg-2.0 ayatana-appindicator3-0.1)
LINUX_APT=(libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev)

setup_system() {
  case "$(uname -s)" in
    Linux)
      local missing=()
      for pkg in "${LINUX_PKGS[@]}"; do
        pkg-config --exists "$pkg" 2>/dev/null || missing+=("$pkg")
      done
      if (( ${#missing[@]} == 0 )); then
        ok "system libraries present (${LINUX_PKGS[*]})"
        return 0
      fi
      printf '  missing: %s\n' "${missing[*]}"
      command -v apt-get >/dev/null 2>&1 || die "no apt-get; install the equivalents of: ${LINUX_APT[*]}"
      local sudo=()
      if [[ $(id -u) -ne 0 ]]; then
        command -v sudo >/dev/null 2>&1 || die "not root and no sudo; install: ${LINUX_APT[*]}"
        sudo=(sudo)
      fi
      printf '  installing with apt-get: %s\n' "${LINUX_APT[*]}"
      # PPAs that have gone stale on the image make `update` exit non-zero
      # while the main archive still refreshed; the install below is the
      # verdict that matters.
      "${sudo[@]}" env DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null 2>&1 || true
      "${sudo[@]}" env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
        "${LINUX_APT[@]}" >/dev/null || die "apt-get install failed for: ${LINUX_APT[*]}"
      for pkg in "${LINUX_PKGS[@]}"; do
        pkg-config --exists "$pkg" 2>/dev/null || die "$pkg still missing after apt-get install"
      done
      ok "system libraries installed"
      ;;
    Darwin)
      xcode-select -p >/dev/null 2>&1 || die "Xcode command line tools missing: xcode-select --install"
      ok "macOS: command line tools present"
      ;;
    *)
      warn "unknown platform $(uname -s); assuming the Tauri build prerequisites are installed"
      ;;
  esac
}

setup_toolchain() {
  command -v cargo >/dev/null 2>&1 || die "cargo not on PATH — install rustup (https://rustup.rs); rust-toolchain.toml picks the channel"
  command -v node >/dev/null 2>&1 || die "node not on PATH (CI uses node 22)"
  ok "$(rustc --version) / node $(node --version)"
}

setup_node() {
  if [[ -d node_modules && node_modules/.package-lock.json -nt package-lock.json ]]; then
    ok "node_modules up to date"
    return 0
  fi
  printf '  npm install\n'
  npm install --no-audit --no-fund --loglevel=error >/dev/null || die "npm install failed"
  ok "node_modules installed"
}

setup_host() {
  # Incremental: a warm tree is a no-op in about a second, a cold one is a
  # few minutes. `--bins` with `dev-bins` is what verify.sh builds for e2e.
  printf '  cargo build --features dev-bins --bins\n'
  cargo build "${MANIFEST[@]}" --features dev-bins --bins 2>&1 | sed 's/^/    /' | grep -E "Compiling|Finished|error|warning: unused" | tail -3
  [[ -x "$(hostd_bin)" ]] || die "$(hostd_bin) not built"
  ok "host: $(hostd_bin)"
}

setup_browser() {
  local exe
  if exe=$(node -e '
    const { chromium } = require("playwright-core");
    const p = chromium.executablePath();
    require("fs").accessSync(p);
    console.log(p);' 2>/dev/null); then
    ok "browser: $exe"
    return 0
  fi
  printf '  no Chromium for playwright-core; downloading (network)\n'
  npx --no-install playwright-core install chromium >/dev/null 2>&1 || die "playwright-core install chromium failed"
  ok "browser installed"
}

hostd_bin() {
  printf '%s' "${JABOT_HOSTD_BIN:-src-tauri/target/debug/jabot-hostd}"
}

setup() {
  say "setup: system libraries"; setup_system
  say "setup: toolchain";        setup_toolchain
  say "setup: node modules";     setup_node
  say "setup: host binary";      setup_host
  say "setup: browser";          setup_browser
}

# ---------------------------------------------------------------------------
# up / down / status
# ---------------------------------------------------------------------------

status_json() { curl -sf --max-time 2 "$URL/__jabot/host" 2>/dev/null; }

is_live() {
  local json
  json=$(status_json) || return 1
  [[ "$json" == *'"running":true'* && "$json" == *'"hello":{'* ]]
}

server_pid() {
  [[ -f "$PIDFILE" ]] || return 1
  local pid
  pid=$(cat "$PIDFILE")
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s' "$pid"
}

up() {
  if is_live; then
    ok "already live at $URL"
    status
    return 0
  fi
  setup
  say "up: starting vite on $URL"
  mkdir -p "$DEV_DIR"
  if server_pid >/dev/null; then
    warn "a dev server (pid $(cat "$PIDFILE")) is running but not live yet; waiting on it"
  else
    # Its own session, so `down` can take the whole tree (vite, esbuild,
    # jabot-hostd, adapters) with one signal to the group.
    if command -v setsid >/dev/null 2>&1; then
      setsid npx vite --port "$PORT" --strictPort >"$LOG" 2>&1 < /dev/null &
    else
      npx vite --port "$PORT" --strictPort >"$LOG" 2>&1 < /dev/null &
    fi
    echo $! >"$PIDFILE"
  fi
  local waited=0
  until is_live; do
    if ! server_pid >/dev/null; then
      printf '%s\n' "--- $LOG ---" >&2; tail -20 "$LOG" >&2
      die "vite exited before the host came up"
    fi
    (( waited >= 60 )) && {
      printf '%s\n' "--- $LOG ---" >&2; tail -20 "$LOG" >&2
      printf '%s\n' "--- $URL/__jabot/host ---" >&2; status_json >&2; echo >&2
      die "host not live after 60s"
    }
    sleep 1; waited=$((waited + 1))
  done
  ok "live at $URL after ${waited}s"
  status
}

down() {
  local pid
  if pid=$(server_pid); then
    # The group first (setsid), the pid alone as the fallback.
    kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null
    local waited=0
    while kill -0 "$pid" 2>/dev/null && (( waited < 10 )); do sleep 1; waited=$((waited + 1)); done
    kill -0 "$pid" 2>/dev/null && { kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null; }
    ok "stopped dev server (pid $pid)"
  else
    ok "no dev server running"
  fi
  rm -f "$PIDFILE"
}

status() {
  local json
  if ! json=$(status_json); then
    printf '  not serving at %s\n' "$URL"
    return 1
  fi
  node -e 'const s=JSON.parse(process.argv[1]);
    console.log(`  host ${s.running ? "running" : "NOT running"} pid=${s.pid} requests=${s.requests}`);
    if (s.hello) console.log(`  hello  ${s.hello.hostName} v${s.hello.version}`);
    console.log(`  binary ${s.binary}`);
    console.log(`  data   ${s.dataDir}`);
    if (s.exit) console.log(`  last exit ${JSON.stringify(s.exit)}`);
    for (const l of s.stderr) console.log(`  stderr ${l}`);' "$json"
  [[ "$json" == *'"running":true'* ]]
}

rpc() {
  [[ $# -ge 1 ]] || die "rpc wants one JSON-RPC request, e.g. '{\"id\":1,\"method\":\"host/health\"}'"
  is_live || die "not live; run: $0 up"
  node -e '
    const body = JSON.parse(process.argv[1]);
    if (body.jsonrpc === undefined) body.jsonrpc = "2.0";
    if (body.id === undefined) body.id = "cli";
    fetch(process.argv[2] + "/__jabot/rpc", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json()).then((j) => { console.log(JSON.stringify(j, null, 2)); if (j.error) process.exit(1); })
      .catch((e) => { console.error(e.message); process.exit(1); });' "$1" "$URL"
}

shot() {
  is_live || die "not live; run: $0 up"
  JABOT_LIVE_URL="$URL" node scripts/dev/shot.mjs "$@"
}

reset() {
  down
  rm -rf "${JABOT_DEV_DATA_DIR:-$DEV_DIR/data}"
  ok "wiped ${JABOT_DEV_DATA_DIR:-$DEV_DIR/data}"
}

# Chief on `fake-acp`, the scriptable agent from src-tauri/src/bin/fake_acp_agent.rs
# that the dev host registers as a harness (scripts/dev/host-plugin.ts). A bot's
# standing thread is opened with the bot's harness *at the time it is first
# opened* (host/crew/standing.rs), so this has to run before Chief is clicked
# on a fresh data directory — which is why `smoke` resets first.
seed() {
  is_live || die "not live; run: $0 up"
  local out
  out=$(rpc '{"method":"crew/update","params":{"botId":"chief","harnessId":"fake-acp"}}') \
    || { printf '%s\n' "$out" >&2; die "could not put Chief on fake-acp (is fake-acp-agent built? scripts/live.sh setup)"; }
  ok "Chief is on the fake-acp harness"
}

# The acceptance test for everything above: one command that proves a fresh
# machine can bring the real app up and drive a real agent turn through it.
smoke() {
  local out=${1:-$DEV_DIR/smoke.png}
  say "smoke: reset";  reset >/dev/null
  say "smoke: up";     up | tail -3
  say "smoke: seed";   seed
  say "smoke: drive Chief through fake-acp and screenshot"
  shot --out "$out" \
    --click 'text=Chief' \
    --fill '[aria-label="Message Chief"]' 'hello from the live loop' \
    --press Enter \
    --wait-text 'hello from fake-acp' \
    --sleep 300 || die "smoke failed: the drive did not reach the agent's reply"
  printf '\033[32mPASS smoke — %s\033[0m\n' "$out"
}

log() {
  [[ -f "$LOG" ]] || die "no $LOG yet"
  tail -n "${1:-40}" "$LOG"
}

cmd=${1:-}
[[ -n "$cmd" ]] || { usage; exit 2; }
shift
case "$cmd" in
  setup)  setup ;;
  up)     up ;;
  down)   down ;;
  status) status ;;
  shot)   shot "$@" ;;
  rpc)    rpc "$@" ;;
  seed)   seed ;;
  smoke)  smoke "$@" ;;
  reset)  reset ;;
  log)    log "$@" ;;
  -h|--help|help) usage ;;
  *) die "unknown command: $cmd (try --help)" ;;
esac
