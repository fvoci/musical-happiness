#!/bin/sh
# The entry of the fvoci image: reads secrets from files into the variables the server
# reads, then becomes the server (its arguments are passed on).
#
# For each NAME below, NAME_FILE may name a file (a Docker secret, /run/secrets/...)
# whose content, without the final newline, becomes NAME. A file that is named but
# missing or empty stops the start, saying which variable named it; the value is never
# printed. NAME and NAME_FILE together are refused, so that it is clear which one holds.
#
#   PGPASSWORD                 the password of the PostgreSQL role in FVOCI_DBCONFIG
#                              (the address then carries no password)
#   FVOCI_SECRET_KEY           the key the server seals stored secrets with
#   FVOCI_SEARCH_PASSWORD      the password of the search server's user
#   FVOCI_SEARCH_API_KEY       an Elasticsearch API key, instead of a user
#   FVOCI_PROMETHEUSTOKEN      the bearer token of the metrics listener
#   FVOCI_SMTP_PASSWORD        the password of the mail relay
#   FVOCI_CLOUDFLARE_API_TOKEN, FVOCI_SES_SECRET_ACCESS_KEY  mail providers' secrets
#   AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY                 S3 credentials
set -eu

for name in PGPASSWORD FVOCI_SECRET_KEY FVOCI_SEARCH_PASSWORD FVOCI_SEARCH_API_KEY \
    FVOCI_PROMETHEUSTOKEN FVOCI_SMTP_PASSWORD FVOCI_CLOUDFLARE_API_TOKEN \
    FVOCI_SES_SECRET_ACCESS_KEY AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY; do
    eval "file=\${${name}_FILE:-}"
    [ -n "$file" ] || continue
    eval "given=\${${name}+set}"
    if [ -n "$given" ]; then
        echo "fvoci: both $name and ${name}_FILE are set; give one" >&2
        exit 1
    fi
    if [ ! -r "$file" ]; then
        echo "fvoci: ${name}_FILE names $file, which cannot be read" >&2
        exit 1
    fi
    value=$(cat "$file")
    if [ -z "$value" ]; then
        echo "fvoci: ${name}_FILE names $file, which is empty" >&2
        exit 1
    fi
    export "$name=$value"
    unset "${name}_FILE"
done
unset file given value

exec /opt/fvoci/fvoci-server --config /opt/fvoci/config.json "$@"
