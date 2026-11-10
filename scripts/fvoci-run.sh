#!/usr/bin/env bash
# Starts the Rust server for probing, on SQLite or PostgreSQL; the counterpart of
# oracle-run.sh with the same arguments.
#
#   scripts/fvoci-run.sh <name> <port> [single-user-token] [dbtype] [dbconfig] [filespath]
#
# The run directory is target/fvoci/runs/<name>/ (config.json, server.log, files/, and
# the SQLite database unless a dbconfig is given). Serves the original's web bundle from
# webapp/pack, so the same browser tests can run against both servers. Prints the
# server's PID once it answers; the caller stops it with `kill <pid>`.
# FVOCI_SERVER_BIN names the binary (default: target/debug/fvoci-server).
set -euo pipefail

name=${1:?usage: fvoci-run.sh <name> <port> [single-user-token] [dbtype] [dbconfig] [filespath]}
port=${2:?missing port}
token=${3:-}
dbtype=${4:-sqlite3}

root=$(cd "$(dirname "$0")/.." && pwd)
binary=${FVOCI_SERVER_BIN:-$root/target/debug/fvoci-server}
run_dir="$root/target/fvoci/runs/$name"
rm -rf "$run_dir"
mkdir -p "$run_dir/files"
dbconfig=${5:-$run_dir/fvoci.db?_busy_timeout=5000}
filespath=${6:-$run_dir/files}

cat >"$run_dir/config.json" <<JSON
{
    "serverRoot": "http://localhost:$port",
    "port": $port,
    "dbtype": "$dbtype",
    "dbconfig": "$dbconfig",
    "useSSL": false,
    "webpath": "$root/webapp/pack",
    "filespath": "$filespath",
    "telemetry": false,
    "session_expire_time": 2592000,
    "session_refresh_time": 18000,
    "localOnly": true,
    "enablePublicSharedBoards": true
}
JSON

args=(--config "$run_dir/config.json")
if [ -n "$token" ]; then
    args+=(--single-user)
fi
(cd "$run_dir" && FVOCI_SINGLE_USER_TOKEN="$token" exec "$binary" "${args[@]}") \
    >"$run_dir/server.log" 2>&1 &
pid=$!

for _ in $(seq 1 80); do
    if curl -fsS -o /dev/null "http://localhost:$port/hello" 2>/dev/null; then
        echo "$pid"
        exit 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "the server exited early; see $run_dir/server.log" >&2
        exit 2
    fi
    sleep 0.25
done
echo "the server did not answer on port $port; see $run_dir/server.log" >&2
kill "$pid" 2>/dev/null || true
exit 3
