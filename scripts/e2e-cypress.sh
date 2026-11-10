#!/usr/bin/env bash
# Runs the original Cypress specs (webapp/cypress) against a server binary.
#
#   scripts/e2e-cypress.sh <label> <server-binary> [extra server args...]
#
# The server is started with a generated config (in-memory SQLite, the original
# React bundle in webapp/pack as web root) in target/e2e/<label>/, so nothing is
# written outside target/. The Cypress log lands in target/e2e/<label>/cypress.log
# and the script exits with Cypress's exit code.
set -euo pipefail

label=${1:?usage: e2e-cypress.sh <label> <server-binary> [args...]}
server=$(realpath "${2:?missing server binary}")
shift 2

root=$(cd "$(dirname "$0")/.." && pwd)
run_dir="$root/target/e2e/$label"
port=${E2E_PORT:-18088}

rm -rf "$run_dir"
mkdir -p "$run_dir/files"

cat >"$run_dir/config.json" <<EOF
{
    "serverRoot": "http://localhost:$port",
    "port": $port,
    "dbtype": "sqlite3",
    "dbconfig": "file::memory:?cache=shared&_busy_timeout=5000",
    "useSSL": false,
    "webpath": "$root/webapp/pack",
    "filespath": "$run_dir/files",
    "telemetry": false,
    "webhook_update": [],
    "session_expire_time": 2592000,
    "session_refresh_time": 18000
}
EOF

(cd "$run_dir" && exec "$server" --config "$run_dir/config.json" "$@") >"$run_dir/server.log" 2>&1 &
server_pid=$!
trap 'kill "$server_pid" 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
    if curl -fsS -o /dev/null "http://localhost:$port/"; then
        break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then
        echo "server exited early; see $run_dir/server.log" >&2
        exit 2
    fi
    sleep 0.5
done

set +e
(cd "$root/webapp" && node_modules/.bin/cypress run --config "baseUrl=http://localhost:$port") \
    >"$run_dir/cypress.log" 2>&1
status=$?
set -e

grep -E '✔|✖|passing|failing|pending|All specs|Spec  ' "$run_dir/cypress.log" | tail -40 || true
exit "$status"
