# Rustiio — jedan binarni fajl, bez runtime ovisnosti osim ffmpeg-a.
FROM rust:1-slim-bookworm AS builder
WORKDIR /src

# Prvo samo manifesti — Docker cache preživi izmjene u kodu.
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY crates ./crates
COPY apps ./apps
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg ca-certificates tini \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/rustiio /usr/local/bin/rustiio

# Konfiguracija i mediji dolaze kao volume-i; ništa se ne piše u sliku.
ENV RUSTIIO_CONFIG_DIR=/config
ENV RUST_LOG=info
VOLUME ["/config", "/media"]

# SSDP traži host mrežu; ovdje su samo dokumentacijski.
EXPOSE 8200/tcp 1900/udp

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
  CMD ["rustiio", "health", "--quiet"]

ENTRYPOINT ["/usr/bin/tini", "--", "rustiio"]
CMD ["run"]
