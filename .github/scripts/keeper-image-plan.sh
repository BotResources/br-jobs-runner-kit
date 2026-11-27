#!/usr/bin/env bash
set -euo pipefail

event="${EVENT_NAME:?}"
version="${VERSION:?}"
previous="${PREVIOUS_VERSION:-}"
published="${PUBLISHED:-}"
head_sha="${HEAD_SHA:?}"
ref="${REF:-}"
push_input="${PUSH_INPUT:-}"

if [ "$published" != "true" ] && [ "$published" != "false" ]; then
  echo "::error::PUBLISHED must be exactly 'true' or 'false', got '${published}'" >&2
  exit 1
fi

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::the crate version '${version}' is not a plain X.Y.Z version" >&2
  exit 1
fi

candidate=false
reason="no rule allows a push"
case "$event" in
  push)
    if [ "$ref" != "refs/heads/main" ]; then
      reason="push to a branch other than main"
    elif [ "$version" = "$previous" ]; then
      reason="the crate version did not change"
    else
      candidate=true
      reason="the crate version changed on main"
    fi
    ;;
  workflow_dispatch)
    if [ "$push_input" != "true" ]; then
      reason="dispatch without push=true"
    elif [ "$ref" != "refs/heads/main" ]; then
      reason="dispatch on a ref other than main"
    else
      candidate=true
      reason="dispatch with push=true on main"
    fi
    ;;
  pull_request)
    reason="a pull request builds and tests only"
    ;;
  *)
    echo "::error::keeper-image.yml does not run on the event '${event}'" >&2
    exit 1
    ;;
esac

push=false
if [ "$candidate" = "true" ]; then
  if [ "$published" = "true" ]; then
    reason="the tag ${version} is already published and is never replaced"
  else
    push=true
  fi
fi

echo "push=${push}"
echo "tag=${version}"
echo "revision=${head_sha}"
echo "reason=${reason}"
