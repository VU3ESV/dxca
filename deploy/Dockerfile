# DXCA container image — one prebuilt binary on Debian slim.
#
# Built by deploy/docker-deploy.sh, which cross-compiles dxca on the Mac and
# runs `docker build` ON THE TARGET HOST with this file and the binary as the
# whole build context. There is no Rust toolchain in here and none is needed:
# the web UI is compiled into the binary (include_dir), SQLite is bundled, and
# ureq carries its own TLS roots (webpki-roots), so not even ca-certificates.
#
# Debian slim rather than distroless for bash: `docker exec dxca bash` is how
# you look inside, and the deploy script's smoke test talks to /api/status
# over bash's /dev/tcp because there is no curl.
FROM debian:trixie-slim

# config/dxca.toml and data/ resolve relative to the working directory, exactly
# as under systemd (WorkingDirectory=/opt/dxca). Bind-mount the host's own
# /opt/dxca/config and /opt/dxca/data over these two; nothing else is written.
WORKDIR /opt/dxca
COPY dxca /opt/dxca/dxca
RUN mkdir -p /opt/dxca/config /opt/dxca/data

# Documentation only: the container runs with --network host (see
# docker-deploy.sh for why), so nothing is published.
EXPOSE 7580/tcp 7575/tcp

ENTRYPOINT ["/opt/dxca/dxca"]
