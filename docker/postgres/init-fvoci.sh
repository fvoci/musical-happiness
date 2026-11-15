#!/bin/sh
# Run once by the postgres image, on a new volume, as its superuser (POSTGRES_USER,
# `postgres`), in the database POSTGRES_DB (`fvoci`): makes the role the fvoci server
# connects as.
#
# The role `fvoci` logs in and is no superuser, has no BYPASSRLS, and makes no databases
# or roles. It owns nothing: the schema `public` and the tables in it are the
# superuser's, who brings them up to date (the compose service `migrate`, running
# `fvoci-server --migrate`). The role may only use the tables: SELECT, INSERT, UPDATE
# and DELETE, and the sequences, also on the tables the migrations make later. So the
# row-level security policies that keep organizations apart hold it, and it cannot turn
# them off: only an owner may alter a table.
#
#   FVOCI_DB_PASSWORD_FILE  a file (a Docker secret) with the role's password
set -eu

file=${FVOCI_DB_PASSWORD_FILE:?FVOCI_DB_PASSWORD_FILE names the file of the password of the role fvoci connects as}
[ -s "$file" ] || { echo "init-fvoci: $file is empty or cannot be read" >&2; exit 1; }

# The password is read by psql itself (\set with a backquote), so it is on no command
# line, and quoted by psql (:'password').
psql --no-psqlrc --set ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
    --set role=fvoci --set owner="$POSTGRES_USER" --set db="$POSTGRES_DB" \
    --set file="$file" <<'SQL'
\set password `tr -d '\r' < :'file'`
CREATE ROLE :"role" LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION
    PASSWORD :'password';
\unset password
REVOKE ALL ON DATABASE :"db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"db" TO :"role";
REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO :"role";
ALTER DEFAULT PRIVILEGES FOR ROLE :"owner" IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO :"role";
ALTER DEFAULT PRIVILEGES FOR ROLE :"owner" IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO :"role";
SQL
