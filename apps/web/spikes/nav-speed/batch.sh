#!/usr/bin/env bash
# batch.sh: the measurements of the nav-speed spike, one at a time, each line with the load.
# Needs a server started by server.sh with SERVER_URL set to the proxy's address
# (http://127.0.0.1:$PROXY_PORT): the rooms' WebSocket checks its origin.
#   RUN=<seeded run dir> OUT=<dir> SERVER_PORT=… PROXY_PORT=… PW_WS=ws://127.0.0.1:<port>/ bash batch.sh
set -uo pipefail
cd "$(dirname "$0")"
run() { echo "== $* | $(cat /proc/loadavg)"; timeout 3600 node "$@" 2>&1 | grep -E '^\{|Error' | cut -c1-1500; }
run card.mjs card 0,150 3 chromium
run card.mjs card 0 3 webkit
run board.mjs big 0,150 3
run nav.mjs b2b 0,150 3
run nav.mjs b2w 0,150 3
run card.mjs page 0,150 3 chromium
echo "== all done | $(cat /proc/loadavg)"
