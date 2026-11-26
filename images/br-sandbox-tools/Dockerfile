FROM alpine:3.24.4@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf AS build

ARG LIBARCHIVE_VERSION=3.8.11
ARG MUSL_VERSION=1.2.6
ARG PAUSE_VERSION=1

RUN apk add --no-cache build-base

ADD --checksum=sha256:900fe9fc7237c2b04f090f3c3648de8fe597525771f5035016537ce2d667d12b \
    https://github.com/libarchive/libarchive/releases/download/v${LIBARCHIVE_VERSION}/libarchive-${LIBARCHIVE_VERSION}.tar.gz \
    /src/libarchive.tar.gz
ADD --checksum=sha256:d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a \
    https://musl.libc.org/releases/musl-${MUSL_VERSION}.tar.gz \
    /src/musl.tar.gz

WORKDIR /src
RUN tar -xzf libarchive.tar.gz \
 && cd libarchive-${LIBARCHIVE_VERSION} \
 && CFLAGS="-Os -fno-pie" LDFLAGS="-no-pie" ./configure \
      --disable-shared --enable-static \
      --enable-bsdtar=static --disable-bsdcpio --disable-bsdcat --disable-bsdunzip \
      --disable-acl \
      --without-zlib --without-bz2lib --without-lz4 --without-lzma --without-zstd \
      --without-openssl --without-nettle --without-mbedtls --without-libb2 \
      --without-xml2 --without-expat \
 && make -j"$(nproc)" bsdtar LDFLAGS="-all-static -no-pie" \
 && install -D -m 0555 bsdtar /stage/bin/bsdtar \
 && install -D -m 0444 COPYING /stage/licenses/libarchive-COPYING \
 && strip /stage/bin/bsdtar

RUN tar -xzf musl.tar.gz musl-${MUSL_VERSION}/COPYRIGHT \
 && install -D -m 0444 musl-${MUSL_VERSION}/COPYRIGHT /stage/licenses/musl-COPYRIGHT \
 && test "$(apk info musl | head -n 1 | cut -d- -f2)" = "${MUSL_VERSION}"

COPY images/br-sandbox-tools/pause.c /src/pause.c
RUN gcc -Os -static -no-pie -Wall -Wextra -Werror -o /stage/bin/pause pause.c \
 && strip /stage/bin/pause \
 && chmod 0555 /stage/bin/pause

ARG REVISION=unknown
RUN printf 'libarchive %s; musl %s; pause %s; rev %s\n' \
      "${LIBARCHIVE_VERSION}" "$(apk info musl | head -n 1 | cut -d' ' -f1 | sed 's/^musl-//')" "${PAUSE_VERSION}" "${REVISION}" \
      > /stage/VERSION \
 && chmod 0444 /stage/VERSION \
 && chmod 0755 /stage/bin \
 && find /stage -exec touch -h -d @1 {} + \
 && /stage/bin/bsdtar --create --format pax --numeric-owner --uid 0 --gid 0 --no-xattrs --no-acls --no-fflags \
      --file /stage/tools.tar -C /stage --no-recursion bin bin/bsdtar bin/pause VERSION

FROM scratch
LABEL org.opencontainers.image.title="br-sandbox-tools" \
      org.opencontainers.image.licenses="Apache-2.0 AND BSD-2-Clause AND MIT"
COPY --from=build /stage/bin/bsdtar /stage/bin/pause /sbx/bin/
COPY --from=build /stage/tools.tar /stage/VERSION /sbx/
COPY --from=build /stage/licenses/ /sbx/LICENSES/
COPY LICENSE /sbx/LICENSES/br-sandbox-tools-Apache-2.0
