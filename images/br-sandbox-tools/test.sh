#!/usr/bin/env bash
set -euo pipefail

exec bash "$(dirname "${BASH_SOURCE[0]}")/../../.github/scripts/test-tools-image.sh" "$@"
