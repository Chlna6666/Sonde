FROM node:26-bookworm-slim AS web
WORKDIR /source/web
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM rust:1.95-bookworm AS server
WORKDIR /source
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY --from=web /source/web/dist ./web/dist
ENV SONDE_SKIP_WEB_BUILD=1
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home sonde
WORKDIR /app
COPY --from=server /source/target/release/sonde /usr/local/bin/sonde
RUN mkdir /app/data && chown sonde:sonde /app/data
USER sonde
ENV SONDE_BIND=0.0.0.0:8080 SONDE_DATA_DIR=/app/data
EXPOSE 8080
VOLUME ["/app/data"]
ENTRYPOINT ["sonde"]

