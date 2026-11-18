#!/bin/sh
# Brings the installation of docker/compose.yaml to the version its files and .env now
# name (the new docker/ in place, FVOCI_IMAGE set): checks them before anything
# changes, backs up (backup.sh), pulls the images, stops fvoci, starts everything
# (`up`, whose migrate brings the schema up), and checks that fvoci is ready and runs
# the commit expected. Says how long fvoci was down, and, when it fails after the
# stop, what is left and how to go back. compose.yaml's header says more.
#
#   docker/update.sh [-p project] [-f compose.yaml]... [--env-file .env]
#                    [--dir folder] [--keep-days 14] [--no-pull] [--commit sha]
#
# --no-pull: the images are on this machine already (built here, or loaded).
# --commit: the commit fvoci must say it is; else the image's label says it.
set -eu
script=update
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=docker/ops.sh
. "$here/ops.sh"

pull=yes
expected=""
while [ $# -gt 0 ]; do
    shared_option "$@"
    if [ "$shift_by" -gt 0 ]; then
        shift "$shift_by"
        continue
    fi
    case $1 in
    --no-pull) pull=no ;;
    --commit) expected=${2:?--commit needs a value} && shift ;;
    *) fail "unknown option $1" ;;
    esac
    shift
done
check_shared_options

# The new files and .env, before anything changes: compose reads the variable of an
# environment secret only when it starts the service, so `config` passes a .env that
# lacks one, and `up` would stop half-way.
compose config --quiet || fail "compose refuses its files or .env (above); nothing was changed"
config=$(compose config)
needed=$(printf '%s\n' "$config" | awk '
    /^[^ ]/ { secrets = ($1 == "secrets:") }
    secrets && $1 == "environment:" { print $2 }')
unset config
# The variables compose sees (.env and the shell's), names only: the values are
# printed nowhere.
defined=$(compose config --environment) ||
    fail "compose cannot list the variables it sees (config --environment needs a newer compose). Nothing was changed"
defined=$(printf '%s\n' "$defined" | sed 's/=.*//')
lacking=""
for name in $needed; do
    printf '%s\n' "$defined" | grep -qx -- "$name" || lacking="$lacking $name"
done
unset defined
[ -z "$lacking" ] ||
    fail "the .env ($env_file) lacks$lacking: add the line (empty when unused; .env.example says what each is). Nothing was changed"

running fvoci || fail "fvoci does not run: start it first (docker compose up -d), or restore. Nothing was changed"
image=$(fvoci_setting image)
old_container=$(compose ps --quiet fvoci)
old_image=$(docker inspect --format '{{.Image}}' "$old_container")
old_tags=$(docker image inspect --format '{{join .RepoTags " "}}' "$old_image")
schema_before=$(schema_version)
rows_before=$(row_counts)

backup_said=$("$here/backup.sh" --env-file "$env_file" --dir "$backup_dir" --keep-days "$keep_days") ||
    fail "the backup failed: nothing was changed"
printf '%s\n' "$backup_said"
backup=$(printf '%s\n' "$backup_said" | sed -n 's/^backup: made \(.*\) ([0-9]* KiB: .*/\1/p')

if [ "$pull" = yes ]; then
    compose pull ||
        fail "the images were not pulled (--no-pull if they are on this machine): fvoci still runs, nothing was changed"
fi
docker image inspect "$image" >/dev/null 2>&1 ||
    fail "the image $image is not on this machine: fvoci still runs, nothing was changed"
if [ -z "$expected" ]; then
    expected=$(docker image inspect --format \
        '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$image")
fi

# From here on fvoci is down until it is ready again.
since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
stopped=$(now_ms)
after_the_stop() {
    status=$?
    [ "$status" -ne 0 ] || return 0
    printf '%s\n' \
        "update: failed after fvoci was stopped ($since). What is left:" \
        "  - the backup $backup, made before the stop" \
        "  - the image that ran before: $old_image (${old_tags:-untagged})" \
        "  - the schema: $(schema_version) (it was $schema_before)" \
        "  docker compose logs migrate fvoci says why. There are no migrations down: if the" \
        "  schema moved, going back is restoring the backup into new volumes with the image" \
        "  that ran before (its tag in FVOCI_IMAGE; docker compose down; remove the volumes" \
        "  postgres-data and fvoci-data; docker/restore.sh <backup>). If it did not move," \
        "  that tag in FVOCI_IMAGE and docker compose up -d bring the old version back." >&2
    exit "$status"
}
trap after_the_stop EXIT
trap 'exit 130' INT TERM

say "stopping fvoci, then starting $image"
compose stop fvoci
compose up --detach --no-build --wait --wait-timeout 300
ready=$(fvoci_request http GET /api/v1/ready)
case $ready in
200*) ;;
*) fail "fvoci is up but not ready (${ready%%
*})" ;;
esac
down=$(($(now_ms) - stopped))

ping=$(fvoci_request http GET /api/v1/ping)
commit=$(printf '%s' "$ping" | sed -n 's/.*"commit":"\([^"]*\)".*/\1/p')
logs=$(compose logs --no-color --since "$since" migrate fvoci 2>&1 || true)
errors=$(printf '%s\n' "$logs" | grep -cw ERROR || true)
warnings=$(printf '%s\n' "$logs" | grep -cw WARN || true)
unset logs
say "fvoci was down $(seconds "$down") and is ready"
say "schema $schema_before -> $(schema_version)"
say "rows before: $rows_before; now: $(row_counts)"
say "since the stop fvoci and migrate logged $errors ERROR and $warnings WARN lines"
if [ -z "$expected" ] || [ -z "$commit" ]; then
    say "commit unknown: the image was built without its commit (ping says \"${commit}\", the image \"${expected}\")"
elif [ "$commit" != "$expected" ]; then
    fail "fvoci runs the commit $commit, not $expected"
else
    say "fvoci runs the commit $commit"
fi
trap - EXIT
