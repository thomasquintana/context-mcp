FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

# Operator-owned integration image. Record the built digest with verification;
# native package contents are not claimed reproducible from the base pin alone.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*

USER 65532:65532
WORKDIR /tmp
