#!/usr/bin/env bash
set -euo pipefail

workflow="${1:?usage: images-check-gates.sh <workflow file>}"

python3 -I - "$workflow" <<'PY'
import re, sys

path = sys.argv[1]
lines = open(path).read().split("\n")
errors = []

jobs = {}
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
        jobs[current] = []
    elif current is not None:
        jobs[current].append(line)

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
