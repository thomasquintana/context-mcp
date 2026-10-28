FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS build
WORKDIR /build
COPY . .
RUN cargo build --release --locked -p context-process

FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

# Operator-owned integration image. Record the built digest with verification;
# native package contents are not claimed reproducible from the base pin alone.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ffmpeg poppler-utils ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/context-process /usr/local/bin/context-process

USER 65532:65532
WORKDIR /tmp
