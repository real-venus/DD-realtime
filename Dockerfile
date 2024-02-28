# just install cargo chef
# FROM rust:1.72.1 AS chef
FROM rust:1.74 AS chef
RUN cargo install cargo-chef
WORKDIR /app

# copy in source files, cd into target create and prepare recipe
FROM chef AS planner
# only copy in required source code, otherwise recipe.json will have new hash and skip caching
COPY src src
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json # THIS SHOULD BE CACHED UNLESS DEP CHANGE!
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt install -y openssl
RUN apt-get install ca-certificates
WORKDIR /app
COPY --from=builder /app/target/release/dex-data-realtime-rs /user/local/bin/app
ENTRYPOINT ["/user/local/bin/app"]