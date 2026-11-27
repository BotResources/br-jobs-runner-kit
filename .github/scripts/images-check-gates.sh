#!/usr/bin/env bash
set -euo pipefail

workflow="${1:?usage: images-check-gates.sh <images-build.yml> <images.yml>}"
caller="${2:?usage: images-check-gates.sh <images-build.yml> <images.yml>}"

python3 -I - "$workflow" "$caller" <<'PY'
import re, sys

path = sys.argv[1]
caller_path = sys.argv[2]
errors = []

def parse_jobs(file):
    lines = open(file).read().split("\n")
    found = {}
    current = None
    in_jobs = False
    for line in lines:
        if re.match(r"^jobs:\s*$", line):
            in_jobs = True
            continue
        if in_jobs and re.match(r"^\S", line):
            in_jobs = False
        if not in_jobs:
            continue
        match = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if match:
            current = match.group(1)
            found[current] = []
        elif current is not None:
            found[current].append(line)
    return found

jobs = parse_jobs(path)
caller_jobs = parse_jobs(caller_path)

def needs_of(body):
    for index, line in enumerate(body):
        match = re.match(r"^    needs:\s*(.*)$", line)
        if not match:
            continue
        value = match.group(1).strip()
        if value.startswith("["):
            return [item.strip() for item in value.strip("[]").split(",") if item.strip()]
        if value:
            return [value]
        found = []
        for follow in body[index + 1:]:
            item = re.match(r"^      - (\S+)\s*$", follow)
            if not item:
                break
            found.append(item.group(1))
        return found
    return []

def job_conditions(body):
    found = []
    for index, line in enumerate(body):
        match = re.match(r"^    if:(.*)$", line)
        if not match:
            continue
        parts = [match.group(1)]
        for follow in body[index + 1:]:
            if follow.strip() and not re.match(r"^     ", follow):
                break
            parts.append(follow)
        found.append(" ".join(part.strip() for part in parts if part.strip()))
    return found

status_function = re.compile(r"\b(always|cancelled|failure|success)\s*\(", re.IGNORECASE)

for name in ("build", "manifest", "digests"):
    if name not in jobs:
        errors.append("the job '%s' does not exist" % name)

if "manifest" in jobs and "build" not in needs_of(jobs["manifest"]):
    errors.append("the job 'manifest' does not need the job 'build'")
if "digests" in jobs and "manifest" not in needs_of(jobs["digests"]):
    errors.append("the job 'digests' does not need the job 'manifest'")
for name in ("manifest", "digests"):
    for condition in job_conditions(jobs.get(name, [])):
        if status_function.search(condition):
            errors.append("the job '%s' has a status function in its condition: %s" % (name, condition.strip()))
def steps_of(body):
    steps = []
    for line in body:
        if re.match(r"^      - ", line):
            steps.append([line])
        elif steps:
            steps[-1].append(line)
    return steps

def has_line(body, text):
    return any(line.strip() == text for line in body)

def require_in_job(file, job_set, job, expected):
    for text in expected:
        if not has_line(job_set.get(job, []), text):
            errors.append("%s: the job '%s' lacks the line: %s" % (file, job, text))

def require_in_step(file, job_set, job, step_id, expected):
    step = None
    for candidate in steps_of(job_set.get(job, [])):
        if any(line.strip() == "id: " + step_id for line in candidate):
            step = candidate
    if step is None:
        errors.append("%s: the job '%s' has no step with the id '%s'" % (file, job, step_id))
        return
    for text in expected:
        if not has_line(step, text):
            errors.append("%s: the step '%s' of the job '%s' lacks the line: %s" % (file, step_id, job, text))

require_in_job(path, jobs, "plan", ["pushed: ${{ steps.pushed.outputs.pushed }}"])
require_in_step(path, jobs, "plan", "pushed", [
    "IMAGES: ${{ steps.discover.outputs.images }}",
    "PUSH: ${{ inputs.push }}",
    "HOLD_MARKED: ${{ inputs.hold_marked }}",
    'run: echo "pushed=$(bash .github/scripts/images-select-push.sh "$IMAGES")" >>"$GITHUB_OUTPUT"',
])
require_in_step(path, jobs, "build", "names", [
    "PUSHED: ${{ contains(fromJSON(needs.plan.outputs.pushed), matrix.image) }}",
    'echo "push=${PUSHED}"',
])
require_in_job(caller_path, caller_jobs, "plan", ["hold_marked: ${{ steps.plan.outputs.hold_marked }}"])
require_in_step(caller_path, caller_jobs, "plan", "plan", [
    "grep -E '^(push|hold_marked|tag|revision)=' plan.txt >>\"$GITHUB_OUTPUT\"",
])
require_in_job(caller_path, caller_jobs, "build", [
    "needs: plan",
    "uses: ./.github/workflows/images-build.yml",
    "push: ${{ needs.plan.outputs.push == 'true' }}",
    "hold_marked: ${{ needs.plan.outputs.hold_marked != 'false' }}",
])

per_image_push = "steps.names.outputs.push == 'true'"
for line in jobs.get("build", []):
    if "inputs.push" in line:
        errors.append("the job 'build' reads inputs.push: the push is decided for each image (steps.names.outputs.push)")
logins = [step for step in steps_of(jobs.get("build", [])) if any("docker/login-action" in line for line in step)]
if not logins:
    errors.append("the job 'build' has no login step")
for step in logins:
    if not any(line.strip() == "if: " + per_image_push for line in step):
        errors.append("the login step of the job 'build' does not run only when " + per_image_push)
for name, body in jobs.items():
    for line in body:
        if "continue-on-error" in line:
            errors.append("the job '%s' sets continue-on-error" % name)

for error in errors:
    print("::error::" + path + ": " + error, file=sys.stderr)
sys.exit(1 if errors else 0)
PY
