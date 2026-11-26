#!/usr/bin/env bash
set -euo pipefail

event="${EVENT_NAME:?}"
repository="${REPOSITORY:?}"
sha="${SHA:?}"
head_sha="${HEAD_SHA:-$sha}"
ref="${REF:-}"
head_repository="${HEAD_REPOSITORY:-}"
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
    if [ -z "$head_repository" ]; then
      reason="pull request whose head repository is unknown"
    elif [ "$head_repository" != "$repository" ]; then
      reason="pull request from a fork"
    elif [ "$actor" = "dependabot[bot]" ]; then
      reason="pull request of Dependabot (read-only token)"
    else
      push=true
      reason="pull request from this repository"
    fi
    ;;
  workflow_dispatch)
    if [ "$push_input" = "true" ]; then
      push=true
      reason="dispatch with push=true"
    else
      reason="dispatch without push=true"
    fi
    ;;
  *)
    echo "::error::images.yml does not run on the event '${event}'" >&2
    exit 1
    ;;
esac

short="${head_sha:0:7}"
if [ "$event" = "pull_request" ]; then
  tag="pr-${pr_number:?}-${short}"
else
  tag="sha-${short}"
fi

echo "push=${push}"
echo "tag=${tag}"
echo "revision=${head_sha}"
echo "reason=${reason}"
