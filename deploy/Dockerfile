FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS application
ENV CARGO_BUILD_JOBS=2
RUN rustup toolchain install 1.98.0 --profile minimal
WORKDIR /build
COPY . .
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo +1.98.0 build --release --locked -p context-mcp && cp target/release/context-mcp /context-mcp

FROM rust:1.98-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS model
ENV CARGO_BUILD_JOBS=2
WORKDIR /source
RUN git clone https://github.com/thomasquintana/llm-mcp.git . \
    && git checkout --detach a9b6d2fe3fed686dd2ca2fb67591613f0cf44cb8 \
    && cargo +1.98.0 build --release --locked

FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8
ARG TARGETARCH
RUN apt-get update && apt-get install --yes --no-install-recommends ca-certificates curl tar \
    && rm -rf /var/lib/apt/lists/*
RUN case "$TARGETARCH" in \
      arm64) checksum=9bae9bc7bf3ac571e48e06b3446c9441dad7fe6529640319e67b8a347b65b8eb ;; \
      amd64) checksum=70dc2eb04dbe8ef8b7288beafd7ea11d5d36d8d9d24e739c13c959d2000cf31c ;; \
      *) exit 1 ;; esac \
    && curl --fail --location --proto '=https' --tlsv1.2 "https://github.com/teran/mcp-sandbox/releases/download/v0.2.0/mcp-sandbox_0.2.0_linux_${TARGETARCH}.tar.gz" --output /tmp/sandbox.tar.gz \
    && printf '%s  /tmp/sandbox.tar.gz\n' "$checksum" | sha256sum --check \
    && tar -xzf /tmp/sandbox.tar.gz -C /usr/local/bin mcp-sandbox \
    && mkdir -p /usr/share/doc/mcp-sandbox \
    && tar -xzf /tmp/sandbox.tar.gz -C /usr/share/doc/mcp-sandbox LICENSE \
    && rm /tmp/sandbox.tar.gz
COPY --from=application /context-mcp /usr/local/bin/context-mcp
COPY --from=model /source/target/release/llm-mcp /usr/local/bin/llm-mcp
COPY --from=model /source/LICENSE /usr/share/doc/llm-mcp/LICENSE
COPY examples/operator /etc/context/operator
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/doc/context-mcp/
RUN dpkg-query -W > /usr/share/doc/context-mcp/runtime-packages.txt
USER 65532:65532
EXPOSE 8089
ENTRYPOINT ["/usr/local/bin/context-mcp"]
CMD ["serve", "--config", "/configuration/runtime.json", "--transport", "http"]
