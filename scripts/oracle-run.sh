#!/usr/bin/env bash
# Starts the original Go server (the oracle) for probing, on SQLite or PostgreSQL.
#
#   scripts/oracle-run.sh <name> <port> [single-user-token] [dbtype] [dbconfig] [filespath]
#
# The run directory is target/oracle/runs/<name>/ (config.json, server.log, files/, and
# the SQLite database unless a dbconfig is given; a filespath puts the files elsewhere). Prints the server's PID once it
# answers; the caller stops it with `kill <pid>`. With a single-user token the server
# runs in single-user mode and accepts that token as a bearer token.
set -euo pipefail

name=${1:?usage: oracle-run.sh <name> <port> [single-user-token] [dbtype] [dbconfig]}
port=${2:?missing port}
token=${3:-}
dbtype=${4:-sqlite3}

root=$(cd "$(dirname "$0")/.." && pwd)
run_dir="$root/target/oracle/runs/$name"
rm -rf "$run_dir"
mkdir -p "$run_dir/files"
dbconfig=${5:-$run_dir/oracle.db?_busy_timeout=5000}
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
# The environment variable below is the original server's own name for the token.
(cd "$run_dir" && FOCALBOARD_SINGLE_USER_TOKEN="$token" exec "$root/target/oracle/go-server" "${args[@]}") \
    >"$run_dir/server.log" 2>&1 &
pid=$!

for _ in $(seq 1 80); do
    if curl -fsS -o /dev/null "http://localhost:$port/hello"; then
        echo "$pid"
        exit 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "the oracle exited early; see $run_dir/server.log" >&2
        exit 2
    fi
    sleep 0.25
done
echo "the oracle did not answer on port $port; see $run_dir/server.log" >&2
kill "$pid" 2>/dev/null || true
exit 2
