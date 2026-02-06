FROM rust:1.89 AS builder
WORKDIR /usr/src

RUN USER=root cargo new --bin meshexec-temp

WORKDIR /usr/src/meshexec-temp
COPY Cargo.* .
RUN cargo build --release
RUN rm -r src
COPY src ./src
RUN rm ./target/release/deps/meshexec*

RUN --mount=type=cache,target=/volume/target \
    --mount=type=cache,target=/root/.cargo/registry \
    cargo build --release --bin meshexec
RUN mv target/release/meshexec .

FROM debian:stable-slim

COPY --from=builder --chown=nonroot:nonroot /usr/src/meshexec-temp/meshexec /usr/local/bin

ENTRYPOINT [ "/usr/local/bin/meshexec", "serve" ]
