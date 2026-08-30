# Base image for JaBot Cloud Agents.
#
# JaBot is a Tauri 2 app: a Rust host (`src-tauri/`) and a React + Vite renderer
# (`src/`). The one gate, `./scripts/verify.sh`, compiles the whole Rust crate
# (which links GTK/WebKitGTK through Tauri), runs the Rust + TypeScript test
# suites, and builds the renderer — so the base image needs the Tauri system
# libraries, a Rust stable toolchain with clippy + rustfmt, and Node.
#
# These are the same libraries CI installs for its Linux `verify` job
# (.github/workflows/ci.yml). Slow, stable, system-level dependencies live here;
# the repository's own dependency refresh (`npm ci`) is the `install` step.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# System toolchain + the Tauri/WebKitGTK build dependencies. `git` and `curl`
# are required for a fully custom image; the rest are what `tauri` and the
# WebKitGTK stack link against when the crate is compiled.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        build-essential \
        pkg-config \
        file \
        libssl-dev \
        libgtk-3-dev \
        libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libsoup-3.0-dev \
    && rm -rf /var/lib/apt/lists/*

# Node 22.x (current), matching CI's `node-version: 22`. NodeSource ships a
# 22.x newer than 22.22, which is the floor a couple of dev dependencies
# (jsdom, undici) declare in their `engines`.
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# The image ships an `ubuntu` user (uid 1000); the environment runs as it.
USER ubuntu
WORKDIR /home/ubuntu

# Rust via rustup, defaulting to the `stable` channel that rust-toolchain.toml
# tracks, with the clippy + rustfmt components verify.sh runs. CI installs
# current stable on every run, so the toolchain is intentionally not pinned to
# an old release here (issue #69 / D-014).
ENV RUSTUP_HOME=/home/ubuntu/.rustup \
    CARGO_HOME=/home/ubuntu/.cargo \
    PATH=/home/ubuntu/.cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal --component clippy --component rustfmt \
    && rustc --version \
    && cargo --version \
    && cargo clippy --version \
    && cargo fmt --version
