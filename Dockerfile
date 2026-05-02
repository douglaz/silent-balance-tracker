# Pre-built binary image. Build with `cargo build --release` first, or use the
# Nix-based image via `nix build .#dockerImage` which is the canonical path.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY target/release/silent-balance-tracker /usr/local/bin/silent-balance-tracker
RUN chmod +x /usr/local/bin/silent-balance-tracker

ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=8080

EXPOSE 8080

CMD ["silent-balance-tracker", "serve"]
