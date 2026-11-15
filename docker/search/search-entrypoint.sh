#!/bin/bash
# The entry of the fvoci search image: certificates, the user `fvoci`, then OpenSearch.
#
#   FVOCI_SEARCH_PASSWORD_FILE  a file (a Docker secret) with the password of `fvoci`;
#                               required, and never printed
#   FVOCI_SEARCH_INDEX          the index fvoci writes (fvoci's search_index; `fvoci`):
#                               the only one the user may touch
set -euo pipefail

home=/usr/share/opensearch
tls=$home/config/tls
security=$home/config/opensearch-security

fail() {
    echo "fvoci-search: $*" >&2
    exit 1
}

file=${FVOCI_SEARCH_PASSWORD_FILE:-}
[ -n "$file" ] || fail "FVOCI_SEARCH_PASSWORD_FILE is not set: it names the file of the password of the user fvoci"
[ -r "$file" ] || fail "FVOCI_SEARCH_PASSWORD_FILE names $file, which cannot be read"
password=$(cat "$file")
[ -n "$password" ] || fail "FVOCI_SEARCH_PASSWORD_FILE names $file, which is empty"
index=${FVOCI_SEARCH_INDEX:-fvoci}
[[ $index =~ ^[a-z0-9][a-z0-9_-]{0,99}$ ]] || fail "FVOCI_SEARCH_INDEX is not a name fvoci takes"

# A certificate authority that signs one certificate, for the name fvoci reaches the
# server by, and is then thrown away: nothing else can be signed by it. Made once; a
# new volume makes new ones.
if [ ! -s "$tls/node.pem" ]; then
    work=$(mktemp -d)
    (
        umask 077
        cd "$work"
        openssl req -x509 -newkey rsa:3072 -nodes -days 3650 -subj "/CN=fvoci search authority" \
            -keyout ca-key.pem -out ca.pem 2>/dev/null
        openssl req -newkey rsa:3072 -nodes -subj "/CN=search" \
            -keyout node-key.pem -out node.csr 2>/dev/null
        printf '%s\n' "basicConstraints=CA:FALSE" \
            "keyUsage=digitalSignature,keyEncipherment" \
            "extendedKeyUsage=serverAuth,clientAuth" \
            "subjectAltName=DNS:search,DNS:localhost,IP:127.0.0.1" >node.ext
        openssl x509 -req -days 3650 -in node.csr -CA ca.pem -CAkey ca-key.pem \
            -CAcreateserial -extfile node.ext -out node.pem 2>/dev/null
        chmod 644 ca.pem node.pem
        mv node-key.pem ca.pem "$tls/"
        mv node.pem "$tls/"
    )
    rm -rf "$work"
    echo "fvoci-search: made the certificate of the name search"
fi
cp "$tls/ca.pem" "$home/ca-out/ca.pem"
chmod 644 "$home/ca-out/ca.pem"

# The one user, allowed the one index and what it needs of the cluster: bulk writes
# and its health, which the health check reads.
hash=$(FVOCI_SEARCH_PASSWORD="$password" "$home/plugins/opensearch-security/tools/hash.sh" \
    -env FVOCI_SEARCH_PASSWORD | grep '^\$2' | tail -n 1)
[ -n "$hash" ] || fail "the password could not be hashed"
cat >"$security/internal_users.yml" <<YAML
_meta:
  type: "internalusers"
  config_version: 2
fvoci:
  hash: "$hash"
  description: "The fvoci server"
YAML
cat >"$security/roles.yml" <<YAML
_meta:
  type: "roles"
  config_version: 2
fvoci:
  cluster_permissions:
    - "cluster_monitor"
    - "cluster_composite_ops"
  index_permissions:
    - index_patterns:
        - "$index"
      allowed_actions:
        - "indices_all"
YAML
cat >"$security/roles_mapping.yml" <<YAML
_meta:
  type: "rolesmapping"
  config_version: 2
fvoci:
  users:
    - "fvoci"
YAML

# The health check signs in as fvoci from a file, so that the password is on no
# command line.
escaped=${password//\\/\\\\}
escaped=${escaped//\"/\\\"}
(
    umask 077
    printf 'user = "fvoci:%s"\ncacert = "%s"\n' "$escaped" "$tls/ca.pem" \
        >"$home/config/healthcheck.curlrc"
)
unset password escaped hash

exec "$home/opensearch-docker-entrypoint.sh" "$@"
