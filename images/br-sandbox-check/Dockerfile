ARG KUBECTL_VERSION=1.34.13
ARG ALPINE=alpine:3.24.4@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf

FROM scratch AS kubectl-amd64
ARG KUBECTL_VERSION
ADD --checksum=sha256:b4bb494e9466a7a94bb851414548bc0d3e2f8da90757e6eccc3371e648648591 --chmod=0555 \
    https://dl.k8s.io/release/v${KUBECTL_VERSION}/bin/linux/amd64/kubectl /kubectl

FROM scratch AS kubectl-arm64
ARG KUBECTL_VERSION
ADD --checksum=sha256:66e8283d52d9bc1f9c0e9daade7c971cd20db5a379306862897dfb25395f3e03 --chmod=0555 \
    https://dl.k8s.io/release/v${KUBECTL_VERSION}/bin/linux/arm64/kubectl /kubectl

FROM kubectl-${TARGETARCH} AS kubectl

FROM ${ALPINE}
ARG KUBECTL_VERSION
LABEL org.opencontainers.image.title="br-sandbox-check" \
      org.opencontainers.image.description="The programs of the network check Job of br-sandbox-chart: sh, kubectl, nc, cat, sed, tr, mktemp, awk, sleep, date" \
      org.opencontainers.image.licenses="Apache-2.0 AND GPL-2.0-only AND GPL-2.0-or-later AND MIT AND BSD-2-Clause AND MPL-2.0 AND Zlib"
COPY --from=kubectl /kubectl /usr/local/bin/kubectl
ADD --checksum=sha256:cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30 --chmod=0444 \
    https://raw.githubusercontent.com/kubernetes/kubernetes/v${KUBECTL_VERSION}/LICENSE /usr/share/kubectl-LICENSE
