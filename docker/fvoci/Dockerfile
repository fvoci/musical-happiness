# syntax=docker/dockerfile:1
#
# The fvoci board server.
#
#   docker build -f docker/fvoci/Dockerfile -t fvoci-server \
#       --build-arg FVOCI_BUILD_HASH=$(git rev-parse HEAD) .
#   docker run -p 8000:8000 -v fvoci-data:/data fvoci-server
#
# The build context is the repository root. TLS support is rustls throughout, so the
# image needs no OpenSSL.
#
# The web app is not built here yet. Put a built bundle (index.html and static/) into the
# directory named by WEB_BUNDLE, relative to the context, or mount one at /opt/fvoci/web;
# without one the server answers every non-API path with a placeholder page.
#
# Metrics are off. To serve them, set FVOCI_PROMETHEUSADDRESS=:9092, publish that port to
# the monitoring network only, and set FVOCI_PROMETHEUSTOKEN: the listener is plain HTTP.

ARG RUST_VERSION=1
ARG WEB_BUNDLE=docker/fvoci/web

# Debian 13 for both stages: a prebuilt part of the TLS library needs its C library
# (glibc 2.38 or newer); with Debian 12 the link step fails.
FROM rust:${RUST_VERSION}-trixie AS build
# The commit the server is built from, for ping and the metrics' fvoci_build_info:
#   --build-arg FVOCI_BUILD_HASH=$(git rev-parse HEAD)
ARG FVOCI_BUILD_HASH=
ENV FVOCI_BUILD_HASH=${FVOCI_BUILD_HASH}
WORKDIR /src
COPY Cargo.toml Cargo.lock clippy.toml ./
COPY crates ./crates
# A member of the workspace; cargo needs its manifest even though it is not built here.
COPY xtask ./xtask
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p fvoci-server \
    && cp target/release/fvoci-server /fvoci-server

FROM debian:trixie-slim
ARG WEB_BUNDLE
# Certificate authorities for outgoing HTTPS: webhooks and S3.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --home-dir /data --create-home fvoci \
    && mkdir -p /opt/fvoci/web /data/files \
    && chown -R fvoci:fvoci /data
COPY --from=build /fvoci-server /opt/fvoci/fvoci-server
COPY docker/fvoci/config.json /opt/fvoci/config.json
COPY ${WEB_BUNDLE}/ /opt/fvoci/web/
USER fvoci
WORKDIR /data
VOLUME /data
EXPOSE 8000
ENTRYPOINT ["/opt/fvoci/fvoci-server", "--config", "/opt/fvoci/config.json"]
