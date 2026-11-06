FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p kova-server

FROM debian:bookworm-slim
COPY --from=build /src/target/release/kova-server /usr/local/bin/kova-server
USER 65534:65534
ENTRYPOINT ["kova-server"]
