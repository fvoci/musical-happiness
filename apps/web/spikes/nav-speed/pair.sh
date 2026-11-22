#!/usr/bin/env bash
# pair.sh <rounds> <script and its arguments…>: the same measurement against two servers in turn,
# round after round, so that both meet the same load. The servers (started by server.sh with
# SERVER_URL set to their proxy's address) are on A_PORT and B_PORT, their proxies one port up.
#   RUN=… OUT=… A_PORT=… B_PORT=… bash pair.sh 3 card.mjs card 0,150 1 chromium
set -uo pipefail
cd "$(dirname "$0")"
rounds=${1:?rounds}; shift
base=$OUT
for round in $(seq 1 "$rounds"); do
  for side in a b; do
    port=$([ "$side" = a ] && echo "$A_PORT" || echo "$B_PORT")
    echo "== round $round $side $* | $(cat /proc/loadavg)"
    OUT=$base/$side-$round SERVER_PORT=$port PROXY_PORT=$((port + 1)) timeout 1800 node "$@" 2>&1 | grep -E '^\{|Error' | cut -c1-1500
  done
done
