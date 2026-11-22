#!/usr/bin/env bash
# server.sh start|stop <run-dir> [port]
# Starts a built fvoci-server (FVOCI_SERVER_BIN, a release build) on the seeded database of
# <run-dir>, serving the built web app WEB (apps/web/dist). Stops it by its PID only.
set -euo pipefail
cmd=${1:?start or stop}
dir=$(cd "${2:?run directory}" && pwd)
port=${3:-20610}
case "$cmd" in
start)
  server=${FVOCI_SERVER_BIN:?the server binary}
  web=$(cd "${WEB:?the built web app}" && pwd)
  if ss -ltnH | awk '{print $4}' | grep -qE "[:.]${port}\$"; then echo "port ${port} is taken" >&2; exit 1; fi
  cd "$dir"
  env FVOCI_PORT="$port" \
    FVOCI_SERVER_URL="${SERVER_URL:-http://127.0.0.1:${port}}" \
    FVOCI_LISTEN_LOOPBACK_ONLY=true \
    FVOCI_DATABASE_KIND=sqlite \
    FVOCI_DATABASE_CONNECTION="$dir/fvoci.db" \
    FVOCI_FILE_STORAGE_PATH="$dir/files" \
    FVOCI_TMP_PATH="$dir/tmp" \
    FVOCI_WEB_APP_PATH="$web" \
    FVOCI_ORG_CREATION=first_user_only \
    FVOCI_LOG_LEVEL=warn \
    setsid "$server" --config "$dir/config.json" >"$dir/server.log" 2>&1 </dev/null &
  echo $! >"$dir/server.pid"
  for _ in $(seq 1 120); do
    if curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:${port}/api/v1/ready" | grep -q 200; then
      echo "ready on port ${port}, pid $(cat "$dir/server.pid")"; exit 0
    fi
    kill -0 "$(cat "$dir/server.pid")" 2>/dev/null || { echo "stopped; see $dir/server.log" >&2; exit 1; }
    sleep 0.5
  done
  echo "not ready" >&2; exit 1 ;;
stop)
  pid=$(cat "$dir/server.pid"); kill "$pid"
  for _ in $(seq 1 40); do kill -0 "$pid" 2>/dev/null || { echo "stopped pid $pid"; exit 0; }; sleep 0.25; done
  echo "pid $pid still runs" >&2; exit 1 ;;
esac
