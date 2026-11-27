FROM alpine@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf AS nc
FROM rancher/k3s:v1.33.6-k3s1
COPY --from=nc /bin/busybox /bin/nc
COPY --from=nc /lib/ld-musl-aarch64.so.1 /lib/ld-musl-aarch64.so.1
