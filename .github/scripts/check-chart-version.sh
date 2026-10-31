#!/usr/bin/env bash
set -euo pipefail

chart_dir="charts/br-runner-toolkit-chart"
chart_yaml="${chart_dir}/Chart.yaml"

chart_version() {
  grep -m1 '^version:' | sed -E 's/^version:[[:space:]]*"?([^"[:space:]]+)"?.*/\1/'
}

base_ref="${GITHUB_BASE_REF:-}"
if [ -n "$base_ref" ]; then
  base="origin/${base_ref}"
else
  base="origin/main"
fi

if ! git rev-parse --verify "$base" >/dev/null 2>&1; then
  echo "::notice::base ${base} not found, skipping chart-version check (not a PR context)"
  exit 0
fi

merge_base=$(git merge-base HEAD "$base")

changed=$(git diff --name-only "$merge_base" HEAD -- "$chart_dir" || true)
if [ -z "$changed" ]; then
  echo "✓ no change in ${chart_dir}"
  exit 0
fi

current=$(chart_version <"$chart_yaml")
if [ -z "$current" ]; then
  echo "::error file=${chart_yaml}::could not extract the chart version" >&2
  exit 1
fi

if ! git cat-file -e "${merge_base}:${chart_yaml}" 2>/dev/null; then
  echo "✓ ${chart_dir} is new since ${base} (version ${current})"
  exit 0
fi

baseline=$(git show "${merge_base}:${chart_yaml}" | chart_version)

if [ "$current" = "$baseline" ]; then
  echo "::error file=${chart_yaml}::${chart_dir} changed but the Chart.yaml version is still ${current}. A chart change is a chart release: bump the version." >&2
  exit 1
fi

highest=$(printf '%s\n%s\n' "$baseline" "$current" | sort -V | tail -n1)
if [ "$highest" != "$current" ]; then
  echo "::error file=${chart_yaml}::Chart.yaml version ${current} is below ${baseline} on ${base}. A chart release moves the version line forward." >&2
  exit 1
fi

echo "✓ chart version bumped ${baseline} -> ${current}"
