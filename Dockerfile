# Tahap 1: compile binary Rust. Migrasi SQL ikut tertanam di binary lewat sqlx::migrate!.
FROM rust:1-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

# Tahap 2: image runtime kecil. Hanya binary dan folder static yang dibawa.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/target/release/sanimas-dokumentasi /usr/local/bin/sanimas-dokumentasi
COPY static ./static
RUN mkdir -p uploads

ENV BIND=0.0.0.0:8080
EXPOSE 8080
# Memanggil /health (cek koneksi database). Dipakai Docker dan Coolify untuk status kontainer.
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD ["sanimas-dokumentasi", "healthcheck"]
# Foto yang diunggah disimpan di /app/uploads. Folder ini dijadikan volume agar tidak hilang saat deploy ulang.
VOLUME ["/app/uploads"]

CMD ["sanimas-dokumentasi"]
