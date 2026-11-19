#!/bin/sh
# Restores a folder that backup.sh made into an installation of docker/compose.yaml
# that has no fvoci container and whose database and files' volume are empty (new
# volumes): refuses otherwise, changing nothing. Starts PostgreSQL alone, restores the
# database in one transaction and the files, then starts everything (`up`, which
# migrates a backup of an older version), makes the search index anew and compares the
# files with the records. compose.yaml's header says more.
#
#   docker/restore.sh [-p project] [-f compose.yaml]... [--env-file .env]
#                     [--profile name]... <backup folder>
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
elif [ -f "$backup/files.tar" ]; then
    say "the files of $backup/files.tar are not restored: this installation keeps its files in S3"
fi
# The key that seals the sign-in providers' secrets must be the backup's, or they do
# not open: the backup's .env against the value compose gives fvoci (the shell's wins
# over the .env's). Compared here, printed nowhere.
current_key=$(compose config --environment) ||
    fail "compose cannot list the variables it sees (config --environment needs a newer compose). Nothing was changed"
current_key=$(printf '%s\n' "$current_key" | sed -n 's/^FVOCI_SECRET_KEY=//p' | tail -n 1)
if [ -f "$backup/env" ] &&
    grep -q '^[[:space:]]*\(export[[:space:]][[:space:]]*\)\{0,1\}FVOCI_SECRET_KEY[[:space:]]*=' "$backup/env"; then
    backup_key=$(env_value "$backup/env" FVOCI_SECRET_KEY)
    [ "$backup_key" = "$current_key" ] ||
        fail "FVOCI_SECRET_KEY is not the backup's ($backup/env has it): the sign-in providers' secrets would not open. Nothing was changed"
else
    say "warning: $backup/env does not say FVOCI_SECRET_KEY: the .env's must be the one the backup was made with"
fi
unset backup_key current_key
fvoci_containers=$(compose ps --all --quiet fvoci) || fail "compose cannot list the containers of $project"
[ -z "$fvoci_containers" ] ||
    fail "fvoci has a container: a restore goes into an installation without one (docker compose down, and remove its volumes postgres-data and fvoci-data). Nothing was changed"

stage="starting PostgreSQL"
said_where_it_stopped() {
    status=$?
    [ "$status" -eq 0 ] || printf '%s\n' "restore: stopped while $stage." >&2
}
trap said_where_it_stopped EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
say "starting PostgreSQL alone"
compose up --detach --wait --wait-timeout 300 postgres

# Empty: no table, view or sequence of the database's own, no file in the volume.
stage="looking whether the database and the volume are empty (nothing was changed)"
relations=$(sql "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg\_%'
      AND c.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')")
[ "$relations" = 0 ] ||
    fail "the database holds $relations tables and the like: a restore goes into an empty one (docker compose down, then remove the project's volume postgres-data). Nothing was changed"
if [ "$storage" = local ]; then
    present=$(compose run --rm --no-deps -T --pull never --entrypoint find fvoci /data/files \
        -mindepth 1 -not -path '/data/files/.tmp*' -not -name .fvoci-ready -print -quit)
    [ -z "$present" ] ||
        fail "the files' volume holds files: a restore goes into an empty one (remove the project's volume fvoci-data). Nothing was changed"
fi

stage="setting the role fvoci (the database is still empty)"
compose run --rm --no-deps -T roles >/dev/null
say "restoring the database"
# One transaction: a restore that fails leaves the database empty, to be tried again.
stage="restoring the database (in one transaction: it is still empty)"
compose exec -T postgres pg_restore --username=postgres --dbname=fvoci \
    --exit-on-error --single-transaction <"$backup/db.dump"
if [ "$storage" = local ]; then
    say "restoring the files"
    stage="restoring the files: the database is restored; remove the volumes before trying again"
    compose run --rm --no-deps -T --pull never --entrypoint tar fvoci \
        --extract --file=- --directory=/data <"$backup/files.tar"
fi

say "starting the installation (migrate brings an older backup's schema up to date)"
stage="starting the installation: the database and files are restored (docker compose logs migrate fvoci)"
compose up --detach --no-build --wait --wait-timeout 300
say "schema $(schema_version), $(row_counts)"
check_trusted_edge

# The search server's index is not in the backup: it is made anew from the database.
stage="making the search index anew: all else is restored (POST /api/v1/admin/search/reindex on the admin socket)"
answer=$(fvoci_request admin POST /api/v1/admin/search/reindex)
case $answer in
200*) say "the search index is being made anew from $(json_number containers "$answer") boards and wikis" ;;
404*search_not_configured*) say "no search server: nothing to index" ;;
*) fail "the search index was not made anew ($(first_line "$answer"))" ;;
esac
# A comparison of the files with the records, which changes nothing.
stage="comparing the files with the records: all else is restored (POST /api/v1/admin/files/reconcile)"
answer=$(fvoci_request admin POST /api/v1/admin/files/reconcile)
case $answer in
200*) ;;
*) fail "the files were not compared with the records ($(first_line "$answer"))" ;;
esac
missing=$(printf '%s' "$answer" | sed -n 's/.*"missing":{"count":\([0-9]*\).*/\1/p')
[ -n "$missing" ] || fail "the comparison of the files answered what this script does not read: $(printf '%s' "$answer" | tail -n +2)"
if [ "$storage" = s3 ]; then where="the bucket"; else where="the volume"; fi
[ "$missing" = 0 ] ||
    fail "restored $backup, but $missing files the records name are not in $where (the admin socket's files/reconcile lists them)"
say "every file the records name is in $where"
trap - EXIT
say "restored $backup in $(seconds $(($(now_ms) - started)))"
