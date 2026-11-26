#!/usr/bin/env bash
set -euo pipefail

event="${EVENT_NAME:?}"
repository="${REPOSITORY:?}"
sha="${SHA:?}"
head_sha="${HEAD_SHA:-$sha}"
ref="${REF:-}"
head_repository="${HEAD_REPOSITORY:-$repository}"
pr_number="${PR_NUMBER:-}"
actor="${ACTOR:-}"
push_input="${PUSH_INPUT:-}"

push=false
reason="no rule allows a push"
case "$event" in
  push)
    if [ "$ref" = "refs/heads/main" ]; then
      push=true
      reason="push to main"
    fi
    ;;
  pull_request)
    if [ "$head_repository" != "$repository" ]; then
      reason="pull request from a fork"
    elif [ "$actor" = "dependabot[bot]" ]; then
      reason="pull request of Dependabot (read-only token)"
    else
      push=true
      reason="pull request from this repository"
    fi
    ;;
  workflow_dispatch | workflow_call)
    if [ "$push_input" = "true" ]; then
      push=true
      reason="${event} with push=true"
    else
      reason="${event} without push=true"
    fi
    ;;
esac

if [ "$push_input" = "false" ]; then
  push=false
  reason="push input is false"
fi

short="${head_sha:0:7}"
if [ "$event" = "pull_request" ]; then
  tag="pr-${pr_number:?}-${short}"
else
  tag="sha-${short}"
fi

owner="${repository%%/*}"
registry="ghcr.io/$(tr '[:upper:]' '[:lower:]' <<<"$owner")"

echo "push=${push}"
echo "tag=${tag}"
echo "revision=${head_sha}"
echo "registry=${registry}"
echo "reason=${reason}"
