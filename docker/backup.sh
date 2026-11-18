#!/bin/sh
# Backs up the installation of docker/compose.yaml: its database (pg_dump's custom
# format), its files when they are kept in the volume (not those in S3: the bucket is
# the operator's to back up), and copies of its .env and compose files, into a folder
# of their own under --dir that only its owner can read. Then removes this project's
# backups older than --keep-days. compose.yaml's header says more.
#
#   docker/backup.sh [-p project] [-f compose.yaml]... [--env-file .env]
#                    [--dir folder] [--keep-days 14]
set -eu
script=backup
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=docker/ops.sh
. "$here/ops.sh"

while [ $# -gt 0 ]; do
    shared_option "$@"
    [ "$shift_by" -gt 0 ] || fail "unknown option $1"
    shift "$shift_by"
done
check_shared_options

started=$(now_ms)
running postgres || fail "PostgreSQL of the project $project does not run (docker compose up -d postgres)"
storage=$(file_storage)

umask 077
mkdir -p -- "$backup_dir"
chmod 700 -- "$backup_dir"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
name="$project-$stamp"
[ ! -e "$backup_dir/$name" ] || fail "$backup_dir/$name is there already"
# Made under another name and renamed once whole: a folder of the final name is a
# whole backup.
work="$backup_dir/.$name.partial"
rm -rf -- "$work"
mkdir -- "$work"
trap 'rm -rf -- "$work"' EXIT
trap 'exit 130' INT TERM

# The database first, then the files: a file uploaded in between is kept without a
# record (a comparison of the files reports it), never a record without its file.
compose exec -T postgres pg_dump --username=postgres --dbname=fvoci --format=custom \
    >"$work/db.dump"
if [ "$storage" = local ]; then
    # The files' volume through fvoci's image and user, whether fvoci runs or not.
    compose run --rm --no-deps -T --entrypoint tar fvoci \
        --create --file=- --directory=/data --exclude=files/.tmp --exclude=files/.fvoci-ready \
        files >"$work/files.tar"
    files_said="its files"
else
    files_said="no files: they are in S3, whose bucket is yours to back up"
fi
# What it was made with; the .env holds every secret, as the folder's mode says.
cp -- "$env_file" "$work/env"
n=0
old_ifs=$IFS
IFS=:
for file in $COMPOSE_FILE; do
    n=$((n + 1))
    cp -- "$file" "$work/compose-$n-$(basename -- "$file")"
done
IFS=$old_ifs
commit=unknown
if running fvoci; then
    commit=$(fvoci_request http GET /api/v1/ping | sed -n 's/.*"commit":"\([^"]*\)".*/\1/p')
    commit=${commit:-unknown}
fi
{
    echo "project=$project"
    echo "made=$stamp"
    echo "file_storage=$storage"
    echo "schema=$(schema_version)"
    echo "commit=$commit"
} >"$work/made"
chmod 600 -- "$work"/*
mv -- "$work" "$backup_dir/$name"
trap - EXIT

size=$(du -sk -- "$backup_dir/$name" | cut -f1)
say "made $backup_dir/$name (${size} KiB: the database, $files_said) in $(seconds $(($(now_ms) - started)))"

if [ "$keep_days" -gt 0 ]; then
    old=$(find "$backup_dir" -mindepth 1 -maxdepth 1 -type d \
        -name "$project-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]T[0-9][0-9][0-9][0-9][0-9][0-9]Z" \
        -mmin +$((keep_days * 1440)))
    while IFS= read -r gone; do
        [ -n "$gone" ] || continue
        rm -rf -- "$gone"
        say "removed $gone, older than $keep_days days"
    done <<END
$old
END
fi
