FROM rust:1.89-bookworm AS build
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples
COPY tests ./tests
ARG CARGO_BUILD_JOBS=2
RUN cargo build --locked --release --no-default-features --bin desklink-relay

FROM debian:bookworm-slim
RUN mkdir -p /data && chown 65532:65532 /data
COPY --from=build /source/target/release/desklink-relay /usr/local/bin/desklink-relay
USER 65532:65532
ENV DESKLINK_RELAY_BIND=0.0.0.0:8080
ENV DESKLINK_RELAY_DIRECTORY=/data/devices.sqlite3
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/desklink-relay"]
