# LysineOS Phase 0 Dev Environment
# Ubuntu 22.04 + Rust 1.90 + LFS build tools

FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
ENV TZ=UTC

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates build-essential pkg-config \
    libarchive-dev libsodium-dev libbtrfs-dev \
    binutils bison gawk m4 texinfo \
    pandoc \
    git vim less jq ripgrep \
    && rm -rf /var/lib/apt/lists/*

ENV RUST_VERSION=1.90
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
    -y --default-toolchain ${RUST_VERSION} \
    --profile default \
    --component rustfmt clippy rust-src rust-analyzer

ENV PATH="/root/.cargo/bin:${PATH}"

RUN rustc --version && cargo --version

WORKDIR /workspace

CMD ["/bin/bash"]
