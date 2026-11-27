#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/servers.sh"

MODE_NAME=main
for arg in "$@"; do
  case "$arg" in
    --closed) MODE_NAME=closed ;;
    *) die "usage: dns-ip.sh [--closed]" ;;
  esac
done
use_mode "$MODE_NAME"
container_reusable "$DNS_CONTAINER" "$(servers_network)" || die "$DNS_CONTAINER is not running: run setup.sh"
server_ip "$DNS_CONTAINER"
