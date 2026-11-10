# syntax=docker/dockerfile:1
#
# TriagePay API image.
#
# Stage 1 builds the Rust binaries (aps-api, aps-admin). The bundled SQLite
# in aps-store compiles from C, which the rust image's gcc handles.
# Stage 2 is a slim Python runtime: the scanner is stdlib-only Python run as
# a subprocess. The process runs as a non-root user; run the container with
# a read-only root filesystem and a writable /data volume (see
# docker-compose.yml and docs/operations/deploy.md).
#
# Base images are pinned by tag AND digest (looked up on Docker Hub
# 2026-11-06). Both stages use Debian trixie so the binaries link against
# the same glibc they run on.

ARG RUST_IMAGE=rust:1.98.0-slim-trixie@sha256:17d1ba895198f9934c6314ec5346a0d5115372f3243390c3d731e242f35c2f27
ARG PYTHON_IMAGE=python:3.12.15-slim-trixie@sha256:a0f57064cbdcd6285fa45f51237ab0b35ea8ebaed06a1f5973e89e19dfb86704

FROM ${RUST_IMAGE} AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p aps-api -p aps-auth --bins

FROM ${PYTHON_IMAGE} AS runtime
RUN groupadd --system --gid 10001 aps \
 && useradd --system --uid 10001 --gid aps --no-create-home --home-dir /nonexistent \
      --shell /usr/sbin/nologin aps \
 && mkdir -p /data \
 && chown aps:aps /data \
 && chmod 0700 /data

COPY --from=build /src/target/release/aps-api /usr/local/bin/aps-api
COPY --from=build /src/target/release/aps-admin /usr/local/bin/aps-admin
# The public image carries only the public rulesets in scanner/rulesets.
# A private ruleset is mounted at runtime from outside the repository.
COPY scanner/aps_scanner /app/scanner/aps_scanner
COPY scanner/rulesets /app/scanner/rulesets
COPY ui /app/ui
COPY deploy/healthcheck.py /app/healthcheck.py

# APS_BIND and APS_DB_PATH are read once the wave-2 API wiring lands; until
# then the API binds 127.0.0.1 inside the container and keeps state in
# memory, so only the in-container smoke test reaches it.
ENV APS_PORT=8080 \
    APS_BIND=0.0.0.0 \
    APS_DB_PATH=/data/aps.sqlite \
    APS_UI_DIR=/app/ui \
    APS_SCANNER_PYTHON=python3 \
    APS_SCANNER_DIR=/app/scanner \
    APS_SCANNER_TIMEOUT_SECS=30 \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

VOLUME ["/data"]
USER 10001:10001
WORKDIR /app
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["python3", "/app/healthcheck.py"]
ENTRYPOINT ["/usr/local/bin/aps-api"]
