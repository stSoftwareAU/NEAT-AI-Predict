# OSS Scanner build image (https://github.com/anthropics/oss-scanner).
# The scanner builds this with the repository as the build context, then runs
# its audit offline, so every crate the tool and its tests need (including the
# `neat-core` git dependency) is fetched here. The base image matches the
# channel pinned in rust-toolchain.toml.
FROM rust:1.99.0-trixie

# scanner contract: the checkout lives inside the image, at /src
COPY . /src
WORKDIR /src

# Full debug info and frame pointers. Setting RUSTFLAGS also replaces the
# `target-cpu=native` flag in .cargo/config.toml, so the binary does not depend
# on the CPU that happened to build the image.
ENV CARGO_PROFILE_DEV_DEBUG=true \
    CARGO_PROFILE_TEST_DEBUG=true \
    RUSTFLAGS="-C force-frame-pointers=yes"

# Install the pinned toolchain's components, fetch every crate against the
# lockfile, then build the binary and every test and bench target.
RUN rustup toolchain install \
    && rustc --version \
    && cargo fetch --locked \
    && cargo build --locked --workspace --all-features \
    && cargo test --locked --workspace --all-features --all-targets --no-run

# From here on the audit runs offline; keep cargo from reaching for the network.
ENV CARGO_NET_OFFLINE=true

# The suite runs in seconds, so smoke-run all of it; a failure warns rather
# than aborting the build.
RUN (timeout 900 cargo test --locked --workspace --all-features \
      || echo "WARNING: smoke tests reported failures")
