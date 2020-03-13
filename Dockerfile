FROM clux/muslrust:stable as builder

RUN useradd -u 10001 appuser

WORKDIR /home/rust/

COPY Cargo.toml .
COPY Cargo.lock .
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release

COPY src src
RUN touch src/main.rs
RUN cargo build --release

RUN strip target/x86_64-unknown-linux-musl/release/night-watch

##########################################################

FROM scratch

WORKDIR /home/rust/

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /home/rust/target/x86_64-unknown-linux-musl/release/night-watch .

USER appuser

ENTRYPOINT ["./night-watch"]
