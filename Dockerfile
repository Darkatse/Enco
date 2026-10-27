# syntax=docker/dockerfile:1.7
# A published Linux binary plus the pinned Rust toolchain, so Enco can build its own plugins in the
# container. The release workflow places the binaries in dist/ as enco-amd64 and enco-arm64.
FROM debian:bookworm-slim

ARG TARGETARCH
ENV RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

# Plugin builds link build scripts for the host, which needs a C toolchain.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl gcc git libc6-dev \
    && rm -rf /var/lib/apt/lists/*

# The repository's toolchain file is the one source of the version.
COPY rust-toolchain.toml /tmp/toolchain/
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | CARGO_HOME=/usr/local/cargo sh -s -- -y --no-modify-path --profile minimal --default-toolchain none \
    && cd /tmp/toolchain \
    && CARGO_HOME=/usr/local/cargo rustup toolchain install \
    && rustup default "$(rustup show active-toolchain | cut -d ' ' -f 1)" \
    && rm -rf /tmp/toolchain

COPY dist/enco-${TARGETARCH} /usr/local/bin/enco

RUN useradd --create-home --shell /bin/bash enco
USER enco
WORKDIR /home/enco
# The home holds Enco's home (~/.enco) and Cargo's download cache. HOME is explicit so the image
# also runs under another user ID, such as the owner of a mounted directory.
ENV HOME=/home/enco \
    CARGO_HOME=/home/enco/.cargo
VOLUME /home/enco

ENTRYPOINT ["enco"]
CMD ["serve"]
