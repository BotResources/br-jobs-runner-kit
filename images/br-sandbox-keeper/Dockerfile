FROM --platform=$BUILDPLATFORM rust:1.98.1-slim-bookworm@sha256:ff521445a372125ed4f76e1453a1f8098f2d05332d1601d30db1c1f62757e730 AS build

ARG TARGETARCH
ARG REVISION=unknown

RUN set -eu; \
    host="$(dpkg --print-architecture)"; \
    packages="ca-certificates gcc libc6-dev"; \
    if [ "$host" != "$TARGETARCH" ]; then \
      case "$TARGETARCH" in \
        arm64) packages="$packages gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
        amd64) packages="$packages gcc-x86-64-linux-gnu libc6-dev-amd64-cross" ;; \
        *) echo "unsupported architecture ${TARGETARCH}" >&2; exit 1 ;; \
      esac; \
    fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends $packages; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

ENV CARGO_PROFILE_RELEASE_STRIP=symbols

RUN set -eu; \
    host="$(dpkg --print-architecture)"; \
    case "$TARGETARCH" in \
      arm64) target=aarch64-unknown-linux-musl; variable=AARCH64_UNKNOWN_LINUX_MUSL; compiler=aarch64-linux-gnu-gcc ;; \
      amd64) target=x86_64-unknown-linux-musl; variable=X86_64_UNKNOWN_LINUX_MUSL; compiler=x86_64-linux-gnu-gcc ;; \
      *) echo "unsupported architecture ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    if [ "$host" = "$TARGETARCH" ]; then compiler=gcc; fi; \
    rustup target add "$target"; \
    export "CC_$(printf '%s' "$variable" | tr 'A-Z' 'a-z')=${compiler}"; \
    export "CARGO_TARGET_${variable}_LINKER=${compiler}"; \
    cargo build --locked --release -p br-sandbox-keeper --target "$target"; \
    install -D -m 0555 "target/${target}/release/br-sandbox-keeper" /out/br-sandbox-keeper; \
    test "$(wc -c </out/br-sandbox-keeper)" -gt 1000000

FROM scratch
LABEL org.opencontainers.image.title="br-sandbox-keeper" \
      org.opencontainers.image.description="The controller of the sandbox pools: it sizes the warm pools, expires pods and saves, and reports a failing pool" \
      org.opencontainers.image.licenses="Apache-2.0"
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /out/br-sandbox-keeper /br-sandbox-keeper
USER 65532:65532
ENTRYPOINT ["/br-sandbox-keeper"]
