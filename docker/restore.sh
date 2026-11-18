#!/bin/sh
# Restores a folder that backup.sh made into an installation of docker/compose.yaml
# whose database is empty (new volumes): refuses when its database has a table, or its
# files' volume a file. Starts PostgreSQL alone, restores the database in one
# transaction and the files, then starts everything (`up`, which migrates a backup of
# an older version), makes the search index anew and compares the files with the
# records. compose.yaml's header says more.
#
#   docker/restore.sh [-p project] [-f compose.yaml]... [--env-file .env] <backup folder>
set -eu
script=restore
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=docker/ops.sh
. "$here/ops.sh"

backup=""
while [ $# -gt 0 ]; do
    shared_option "$@"
    if [ "$shift_by" -gt 0 ]; then
        shift "$shift_by"
        continue
    fi
    case $1 in
    -*) fail "unknown option $1" ;;
    *)
        [ -z "$backup" ] || fail "one backup folder, not two"
        backup=$1
        shift
        ;;
    esac
done
check_shared_options
[ -n "$backup" ] || fail "which backup folder? (one that backup.sh made)"
[ -f "$backup/db.dump" ] || fail "$backup has no db.dump: not a folder backup.sh made"

started=$(now_ms)
storage=$(file_storage)
if [ "$storage" = local ]; then
    [ -f "$backup/files.tar" ] ||
        fail "$backup has no files.tar (its files were in S3) while this installation keeps them in its volume"
fi
running fvoci && fail "fvoci runs: a restore goes into an installation that does not (docker compose down, and remove its volumes)"

say "starting PostgreSQL alone, and setting the role fvoci"
compose up --detach --wait --wait-timeout 300 postgres
compose run --rm --no-deps -T roles >/dev/null

# Empty: no table, view or sequence of the database's own, no file in the volume.
relations=$(sql "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg\_%'
      AND c.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')")
[ "$relations" = 0 ] ||
    fail "the database holds $relations tables and the like: a restore goes into an empty one (docker compose down, then remove the project's volume postgres-data); nothing was changed"
if [ "$storage" = local ]; then
    present=$(compose run --rm --no-deps -T --entrypoint find fvoci /data/files -mindepth 1 \
        -not -path '/data/files/.tmp*' -not -name .fvoci-ready -print -quit)
    [ -z "$present" ] ||
        fail "the files' volume holds files: a restore goes into an empty one (remove the project's volume fvoci-data); nothing was changed"
fi

say "restoring the database"
# One transaction: a restore that fails leaves the database empty, to be tried again.
compose exec -T postgres pg_restore --username=postgres --dbname=fvoci \
    --exit-on-error --single-transaction <"$backup/db.dump"
if [ "$storage" = local ]; then
    say "restoring the files"
    compose run --rm --no-deps -T --entrypoint tar fvoci \
        --extract --file=- --directory=/data <"$backup/files.tar" ||
        fail "the files were not all restored; the database was: remove the volumes before trying again"
fi

say "starting the installation (migrate brings an older backup's schema up to date)"
compose up --detach --no-build --wait --wait-timeout 300
say "schema $(schema_version), $(row_counts)"

# The search server's index is not in the backup: it is made anew from the database.
answer=$(fvoci_request admin POST /api/v1/admin/search/reindex)
case $answer in
200*) say "the search index is being made anew from $(json_number containers "$answer") boards and wikis" ;;
404*) say "no search server: nothing to index" ;;
*) fail "the search index was not made anew (${answer%%
*}); try again: docker compose exec fvoci ... POST /api/v1/admin/search/reindex on the admin socket" ;;
esac
# A comparison of the files with the records, which changes nothing.
answer=$(fvoci_request admin POST /api/v1/admin/files/reconcile)
case $answer in
200*)
    missing=$(printf '%s' "$answer" | sed -n 's/.*"missing":{"count":\([0-9]*\).*/\1/p')
    if [ "$storage" = s3 ]; then where="the bucket"; else where="the volume"; fi
    if [ "${missing:-0}" = 0 ]; then
        say "every file the records name is in $where"
    else
        lost="$missing files the records name are not in $where (the admin socket's files/reconcile lists them)"
    fi
    ;;
*) say "the files were not compared with the records (${answer%%
*})" ;;
esac
took=$(seconds $(($(now_ms) - started)))
[ -z "${lost:-}" ] || fail "restored $backup in $took, but $lost"
say "restored $backup in $took"
