# syntax=docker/dockerfile:1

FROM clux/muslrust:stable AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src src

RUN cargo build --release --locked && \
    cp "target/$CARGO_BUILD_TARGET/release/night-watch" /night-watch

##########################################################

# The runtime image is `scratch`, which only works because the binary is
# statically linked against musl and needs nothing from the filesystem:
#
#   * no CA certificates -- reqwest is built without a TLS backend, so the
#     binary can only speak plain HTTP
#   * no tzdata -- every timestamp is chrono's `Utc`
#   * no writable /tmp -- camera frames are decoded in memory
#
# Revisit this base image if any of those stop being true.
FROM scratch

COPY --from=builder /night-watch /night-watch

USER 10001:10001

ENTRYPOINT ["/night-watch"]
