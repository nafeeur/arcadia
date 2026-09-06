# ---- Frontend build ----
FROM node:20-slim AS web
WORKDIR /src
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ---- Backend build ----
# The build distro must be pinned to match the run stage: rust:1-slim tracks upstream
# (now trixie/glibc 2.41), so the resulting binary ends up requiring GLIBC_2.38, while
# bookworm only has 2.36 — the image still builds fine, but the container fails on
# startup with "version `GLIBC_2.38' not found".
FROM rust:1-slim-bookworm AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/
# docs_corpus.rs uses include_str! to compile the in-product docs into the binary, so it's
# a backend source dependency, not just a frontend asset — omit this line and cargo build
# fails outright with "couldn't read".
COPY web/src/docs/ web/src/docs/
RUN cargo build --release -p utopia-server

# ---- Runtime ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /src/target/release/utopia-server /usr/local/bin/utopia-server
COPY --from=web /src/dist /app/web-dist
ENV UTOPIA_WEB_DIST=/app/web-dist
EXPOSE 1516
CMD ["utopia-server"]
