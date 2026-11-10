#!/usr/bin/env bash
# Database round trip between the original server and the port, in both directions.
#
#   scripts/differential/roundtrip.sh sqlite
#   scripts/differential/roundtrip.sh postgres 'postgres://user:pass@localhost:15433'
#
# For each direction a fresh database is written by one server and then read by the
# other (roundtrip.py), and finally read once more by the first, which must still open
# what the second one touched. For PostgreSQL the second argument is the server URL
# without a database; two databases named roundtrip_a and roundtrip_b are (re)created
# in it through the container `hafgufa-pg`.
set -uo pipefail

kind=${1:?usage: roundtrip.sh sqlite|postgres [postgres-url]}
root=$(cd "$(dirname "$0")/../.." && pwd)
work="$root/target/roundtrip/$kind"
rm -rf "$work"
mkdir -p "$work"
status=0

launch() { # launch <oracle|fvoci> <name> <port> <dbtype> <dbconfig> <filespath>
    "$root/scripts/$1-run.sh" "$2" "$3" "" "$4" "$5" "$6" | tail -1
}

run_direction() { # run_direction <label> <first> <second> <dbtype> <dbconfig> <dir>
    local label=$1 first=$2 second=$3 dbtype=$4 dbconfig=$5 dir=$6 pid
    mkdir -p "$dir/files"
    echo "== $label: written by $first, read by $second =="
    pid=$(launch "$first" "rt-$kind-$label-1" 18131 "$dbtype" "$dbconfig" "$dir/files") || { echo "  $first did not start"; status=1; return; }
    python3 "$root/scripts/differential/roundtrip.py" populate http://localhost:18131 "$dir/snapshot.json" || status=1
    kill "$pid"; wait "$pid" 2>/dev/null
    pid=$(launch "$second" "rt-$kind-$label-2" 18132 "$dbtype" "$dbconfig" "$dir/files") || { echo "  $second did not open the database of $first"; status=1; return; }
    python3 "$root/scripts/differential/roundtrip.py" verify http://localhost:18132 "$dir/snapshot.json" || status=1
    kill "$pid"; wait "$pid" 2>/dev/null
    echo "-- and back: $first opens the database again after $second wrote to it"
    pid=$(launch "$first" "rt-$kind-$label-3" 18133 "$dbtype" "$dbconfig" "$dir/files") || { echo "  $first did not open the database again"; status=1; return; }
    if curl -fsS -o /dev/null http://localhost:18133/ping; then echo "  $first answers on the shared database"; else status=1; fi
    kill "$pid"; wait "$pid" 2>/dev/null
}

case "$kind" in
sqlite)
    run_direction original-to-port oracle fvoci sqlite3 "$work/a/data.db?_busy_timeout=5000" "$work/a"
    run_direction port-to-original fvoci oracle sqlite3 "$work/b/data.db?_busy_timeout=5000" "$work/b"
    ;;
postgres)
    url=${2:?missing the PostgreSQL server URL}
    for db in roundtrip_a roundtrip_b; do
        docker exec hafgufa-pg psql -U fvoci -d fvoci -q -c "DROP DATABASE IF EXISTS $db" -c "CREATE DATABASE $db" || exit 2
    done
    run_direction original-to-port oracle fvoci postgres "$url/roundtrip_a?sslmode=disable&connect_timeout=10" "$work/a"
    run_direction port-to-original fvoci oracle postgres "$url/roundtrip_b?sslmode=disable&connect_timeout=10" "$work/b"
    ;;
*)
    echo "unknown database kind: $kind" >&2
    exit 2
    ;;
esac

if [ "$status" -eq 0 ]; then echo "round trip ($kind): no differences"; else echo "round trip ($kind): DIFFERENCES"; fi
exit "$status"
