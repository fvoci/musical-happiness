# shellcheck shell=sh disable=SC2034,SC2154 # set here, read by the scripts that source it; and the reverse.
# Sourced by backup.sh, restore.sh and update.sh (docker/compose.yaml's header says how
# to use them): the options they share, compose on the installation they name, and the
# helpers they share. Nothing here prints a secret.

# The installation: compose's project, its files and its .env, as the options say.
project=""
compose_files=""
env_file=""
profiles=""
# Where backups go, and how many days they are kept.
backup_dir=${FVOCI_BACKUP_DIR:-${HOME:-.}/fvoci-backups}
keep_days=14

say() { printf '%s: %s\n' "$script" "$*"; }
fail() {
    printf '%s: %s\n' "$script" "$*" >&2
    exit 1
}

# Reads one of the shared options from "$1" and "$2", and sets `shift_by` to how many
# it took (0 when "$1" is none of them).
shared_option() {
    shift_by=2
    case $1 in
    -p | --project-name) project=${2:?$1 needs a value} ;;
    -f | --file) compose_files="$compose_files${compose_files:+:}${2:?$1 needs a value}" ;;
    --env-file) env_file=${2:?$1 needs a value} ;;
    --profile) profiles="$profiles${profiles:+,}${2:?$1 needs a value}" ;;
    --dir) backup_dir=${2:?$1 needs a value} ;;
    --keep-days) keep_days=${2:?$1 needs a value} ;;
    *) shift_by=0 ;;
    esac
}

# Checks the shared options once they are read, and fixes compose's files.
check_shared_options() {
    case $keep_days in
    '' | *[!0-9]*) fail "--keep-days takes a number of days (0 keeps every backup)" ;;
    esac
    # compose.yaml beside these scripts unless -f (or COMPOSE_FILE, as compose reads it)
    # names others; .env beside the first.
    [ -n "$compose_files" ] || compose_files=${COMPOSE_FILE:-$here/compose.yaml}
    first_file=${compose_files%%:*}
    [ -n "$env_file" ] || env_file="$(dirname -- "$first_file")/.env"
    [ -r "$env_file" ] || fail "$env_file cannot be read (give --env-file)"
    # The project as compose names it: -p, the environment's, the .env's, or fvoci.
    if [ -z "$project" ]; then
        project=${COMPOSE_PROJECT_NAME:-$(sed -n 's/^COMPOSE_PROJECT_NAME=//p' "$env_file" | tail -n 1)}
        project=${project:-fvoci}
    fi
    case $project in
    '' | *[!a-z0-9_-]* | [_-]*) fail "the project name \"$project\" is not one compose takes" ;;
    esac
    COMPOSE_FILE=$compose_files
    COMPOSE_PROJECT_NAME=$project
    # --profile, else COMPOSE_PROFILES of the environment or the .env, as compose reads.
    if [ -n "$profiles" ]; then
        COMPOSE_PROFILES=$profiles
        export COMPOSE_PROFILES
    fi
    # compose's own separator of COMPOSE_FILE is ':' here: a file whose name holds one
    # cannot be given.
    COMPOSE_PATH_SEPARATOR=:
    export COMPOSE_FILE COMPOSE_PROJECT_NAME COMPOSE_PATH_SEPARATOR
}

compose() { docker compose --env-file "$env_file" "$@"; }

# Milliseconds since the epoch (whole seconds where date has no %N).
now_ms() {
    ns=$(date +%s%N)
    case $ns in
    *N) echo $(($(date +%s) * 1000)) ;;
    *) echo $((ns / 1000000)) ;;
    esac
}

# "12.3 s" from a number of milliseconds.
seconds() { printf '%d.%d s' $(($1 / 1000)) $(($1 % 1000 / 100)); }

# Whether the container of a service runs. Fails, saying so, if compose cannot tell.
running() {
    containers=$(compose ps --quiet --status running "$1") || fail "compose cannot list the containers of $project"
    [ -n "$containers" ]
}

# Prints `compose config "$@"`, or fails showing compose's complaint with its quoted
# words hidden: what it quotes of a value pasted wrong (a key's lines) may be secret.
checked_config() {
    complaint=$(mktemp)
    if ! compose config "$@" 2>"$complaint"; then
        sed 's/"[^"]*"/"..."/g' "$complaint" >&2
        rm -f -- "$complaint"
        fail "compose refuses its files or .env (above, its quoted words hidden). Nothing was changed"
    fi
    rm -f -- "$complaint"
}

# A setting of the service fvoci as compose reads its files and .env: "image", or a
# variable of its environment. Empty when it has none.
fvoci_setting() {
    config=$(checked_config --format yaml) || exit 1
    printf '%s\n' "$config" | awk -v key="$1:" '
        /^[^ ]/ { top = $1 }
        /^  [^ ]/ { service = $1 }
        top == "services:" && service == "fvoci:" && $1 == key {
            value = $0; sub(/^[^:]*:[ ]*/, "", value); gsub(/["\047]/, "", value)
            print value; exit
        }'
    unset config
}

# The value the .env file "$1" gives the variable "$2" (the last line of it; `export`,
# spaces and quotes taken off). Empty when it has none.
env_value() {
    sed -n "s/^[[:space:]]*\(export[[:space:]][[:space:]]*\)\{0,1\}$2[[:space:]]*=[[:space:]]*//p" "$1" |
        tail -n 1 | sed -e 's/[[:space:]]*$//' -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/"
}

# Warns when the project runs cloudflared and the range of its network edge is not
# among FVOCI_TRUSTED_PROXIES: a new network (after `down`) may have another, and every
# person behind the tunnel would then be counted as cloudflared's one address.
check_trusted_edge() {
    services=$(compose config --services) || return 0
    printf '%s\n' "$services" | grep -qx cloudflared || return 0
    ranges=$(docker network inspect "${project}_edge" \
        --format '{{range .IPAM.Config}}{{.Subnet}} {{end}}' 2>/dev/null) || return 0
    trusted=$(fvoci_setting FVOCI_TRUSTED_PROXIES | tr -d ' ')
    for range in $ranges; do
        case ",$trusted," in
        *",$range,"*) ;;
        *) say "warning: the network edge is $range, which FVOCI_TRUSTED_PROXIES lacks: set it to $range in the .env and docker compose up -d" ;;
        esac
    done
}

# Which storage fvoci's files are in (local or s3), as compose gives it fvoci.
file_storage() {
    storage=$(fvoci_setting FVOCI_FILE_STORAGE) || return 1
    if [ "$storage" = s3 ]; then echo s3; else echo local; fi
}

# Runs SQL as PostgreSQL's superuser in the database fvoci and prints the rows, fields
# apart by '|'.
sql() {
    compose exec -T postgres psql --username=postgres --dbname=fvoci --no-psqlrc \
        --no-align --tuples-only --set ON_ERROR_STOP=1 --command "$1"
}

# The newest migration the database has had: "none" before the first, "unknown" when
# PostgreSQL does not answer.
schema_version() {
    sql "SELECT COALESCE((SELECT max(version) FROM _sqlx_migrations WHERE success)::text, 'none')" \
        2>/dev/null || echo "unknown (PostgreSQL did not answer)"
}

# Rows of the users and the blocks (cards, pages, boards), "users N, blocks M".
row_counts() {
    sql "SELECT 'users ' || (SELECT count(*) FROM users) || ', blocks ' || (SELECT count(*) FROM blocks)" \
        2>/dev/null || echo "not counted"
}

# Sends "$2 $3" (a method and a path) inside fvoci's container, to its admin socket when
# "$1" is admin and to its HTTP port (under FVOCI_BASE_PATH) when it is http. Prints the
# status code on a line, then the body. fvoci's image has no curl; perl is in every
# Debian.
fvoci_request() {
    # shellcheck disable=SC2016 # perl's variables, not the shell's.
    compose exec -T fvoci perl -MIO::Socket::UNIX -MIO::Socket::INET -e '
        my ($to, $method, $path) = @ARGV;
        my $socket = $to eq "admin"
            ? IO::Socket::UNIX->new(Peer => $ENV{FVOCI_ADMIN_SOCKET_PATH} || "/var/tmp/fvoci_local.socket")
            : IO::Socket::INET->new(PeerAddr => "127.0.0.1", PeerPort => $ENV{FVOCI_PORT} || 8000, Timeout => 10);
        $socket or die "cannot reach fvoci ($to): $!\n";
        $path = ($ENV{FVOCI_BASE_PATH} || "") . $path if $to eq "http";
        print $socket "$method $path HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n";
        local $/;
        my ($head, $body) = split /\r\n\r\n/, <$socket> // "", 2;
        my ($status) = ($head // "") =~ m{^HTTP/\S+ (\d{3})};
        print $status // "000", "\n", $body // "";
    ' "$@"
}

# The value of a number member "$1" in the JSON "$2" (the first of that name).
json_number() { printf '%s' "$2" | sed -n "s/.*\"$1\":\([0-9][0-9]*\).*/\1/p" | head -n 1; }

# The first line of "$1".
first_line() { printf '%s\n' "$1" | head -n 1; }
