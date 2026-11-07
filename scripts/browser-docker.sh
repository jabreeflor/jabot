#!/usr/bin/env bash
#
# Run a Playwright command with the browsers inside the pinned Playwright
# Docker image, so screenshots render the same on a Mac, in a cloud agent
# container, and in CI.
#
#   ./scripts/browser-docker.sh                                  # npx playwright test
#   ./scripts/browser-docker.sh npx playwright test --project=chromium-visual
#   ./scripts/browser-docker.sh npm run test:browser
#
# Only the browsers run in the container (`playwright run-server`). The test
# runner, Vite, and jabot-hostd stay on this machine; the config's
# `connectOptions.exposeNetwork: "<loopback>"` tunnels the browser's
# localhost requests back here, so no host networking is needed (works on
# Docker Desktop for Mac too).
#
# The image tag is the "baseline environment" for visual snapshots
# (docs/browser-tests.md). It must match the `@playwright/test` version in
# package.json; this script refuses to run when they drift.
#
# If PLAYWRIGHT_WS_ENDPOINT is already set, the command runs as-is (so npm
# scripts that call this can be nested without starting a second server).
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $# -eq 0 ]]; then
  set -- npx playwright test
fi

if [[ -n "${PLAYWRIGHT_WS_ENDPOINT:-}" ]]; then
  exec "$@"
fi

pw_version="$(node -p 'require("@playwright/test/package.json").version' 2>/dev/null)" || {
  printf 'browser-docker: @playwright/test is not installed (npm ci)\n' >&2
  exit 1
}
# Keep in step with package.json; the distro suffix pins the system fonts
# and font rendering libraries the screenshots depend on.
PLAYWRIGHT_IMAGE="${PLAYWRIGHT_IMAGE:-mcr.microsoft.com/playwright:v${pw_version}-noble}"
case "$PLAYWRIGHT_IMAGE" in
  *":v${pw_version}-"*) ;;
  *)
    printf 'browser-docker: %s does not match @playwright/test %s\n' \
      "$PLAYWRIGHT_IMAGE" "$pw_version" >&2
    exit 1
    ;;
esac

if ! docker info >/dev/null 2>&1; then
  printf 'browser-docker: Docker is not running (start Docker Desktop, or `sudo dockerd &` in a cloud container)\n' >&2
  exit 1
fi

port="${PLAYWRIGHT_SERVER_PORT:-3123}"
name="jabot-playwright-$$"

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM

# Serve the browsers with the repo's own playwright-core (read-only mount),
# so the server is exactly the client's version and needs no npm download.
docker run -d --rm --init --ipc=host \
  --name "$name" \
  --user pwuser --workdir /home/pwuser \
  -p "127.0.0.1:${port}:${port}" \
  -v "$PWD/node_modules/playwright-core:/opt/playwright-core:ro" \
  "$PLAYWRIGHT_IMAGE" \
  node /opt/playwright-core/cli.js run-server --port "$port" --host 0.0.0.0 \
  >/dev/null

for _ in $(seq 1 60); do
  if docker logs "$name" 2>&1 | grep -q 'Listening on'; then
    break
  fi
  if ! docker inspect "$name" >/dev/null 2>&1; then
    printf 'browser-docker: Playwright server exited during startup\n' >&2
    exit 1
  fi
  sleep 0.5
done
if ! docker logs "$name" 2>&1 | grep -q 'Listening on'; then
  docker logs "$name" >&2 || true
  printf 'browser-docker: Playwright server did not start\n' >&2
  exit 1
fi

export PLAYWRIGHT_WS_ENDPOINT="ws://127.0.0.1:${port}/"
printf '> browsers: %s (%s)\n' "$PLAYWRIGHT_IMAGE" "$PLAYWRIGHT_WS_ENDPOINT"
set +e
"$@"
status=$?
set -e
exit "$status"
