FROM ekidd/rust-musl-builder:beta-openssl11 as builder

RUN sudo useradd -u 10001 appuser

WORKDIR /home/rust/

COPY Cargo.toml .
COPY Cargo.lock .
RUN sudo chown -R rust:rust Cargo.*
RUN echo "fn main() {}" > src/main.rs
RUN cargo build --release

COPY . .
RUN sudo chown -R rust:rust src/ Cargo.* target/
RUN touch src/main.rs
RUN cargo build --release

RUN strip target/x86_64-unknown-linux-musl/release/sun-events

##########################################################

FROM scratch

WORKDIR /home/rust/

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /home/rust/target/x86_64-unknown-linux-musl/release/sun-events .

USER appuser

ENTRYPOINT ["./sun-events"]
