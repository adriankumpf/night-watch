FROM clux/muslrust:stable as builder

RUN useradd -u 10001 appuser

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release

COPY src src
RUN touch src/main.rs && \
    cargo build --release

##########################################################

FROM scratch

WORKDIR /app

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/night-watch .

USER appuser

ENTRYPOINT ["./night-watch"]
