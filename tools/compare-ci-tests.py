#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Compare passed tests in old named steps with new whole-workspace steps.

Example:
  python3 tools/compare-ci-tests.py --before 38931197204 38931197196 \
      --after <ci-run> <runtime-run> --cache /tmp/ferritecad-ci-logs

Download immutable logs and job metadata with gh. Exclude echoed shell scripts,
non-suite steps and ignored statuses. Compare each platform separately.
"""
import argparse
from datetime import datetime
import io
import json
from pathlib import Path
import re
import subprocess
import zipfile

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TIMESTAMP = re.compile(r"^\ufeff?(\d{4}-\d\d-\d\dT\S+Z) (.*)$")
PASSED = re.compile(r"^test ([\w:]+) \.\.\. ok$")


def gh(*args):
    return subprocess.check_output(["gh", *args])


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def platform(name):
    for value, aliases in [("linux", ("linux", "ubuntu")),
                           ("macos", ("macos",)),
                           ("windows", ("windows",))]:
        if any(alias in name.lower() for alias in aliases):
            return value
    return None


def named_steps(ref, workflow):
    source = subprocess.check_output(
        ["git", "show", f"{ref}:.github/workflows/{workflow}"], text=True)
    result = set()
    for step in re.split(r"(?=^      - )", source, flags=re.M):
        if "cargo test" not in step or "--no-run" in step:
            continue
        if not re.search(r"--exact|--test |--bin ", step):
            continue
        match = re.match(r"      - name: (.+)", step)
        if match:
            result.add(match[1])
    return result


def collect(run, before, cache, repository):
    metadata_path, log_path = cache / f"{run}.json", cache / f"{run}.zip"
    if not metadata_path.exists():
        metadata_path.write_bytes(gh("run", "view", str(run), "--repo", repository,
                                    "--json", "jobs,headSha,workflowName,event,conclusion"))
    if not log_path.exists():
        log_path.write_bytes(gh("api", f"repos/{repository}/actions/runs/{run}/logs"))
    metadata = json.loads(metadata_path.read_text())
    if metadata["conclusion"] != "success":
        raise SystemExit(f"run {run} did not succeed: {metadata['conclusion']}")
    if before:
        workflow = "ci.yml" if metadata["workflowName"] == "CI" else "runtime-layout.yml"
        selected = named_steps(metadata["headSha"], workflow)
    else:
        selected = {step["name"] for job in metadata["jobs"] for step in job["steps"]
                    if step["name"].startswith("Run the whole workspace with OCCT")
                    or step["name"] == "Run cargo test --workspace"}
    results = {os: set() for os in ("linux", "macos", "windows")}
    archive = zipfile.ZipFile(io.BytesIO(log_path.read_bytes()))
    for job in metadata["jobs"]:
        os = platform(job["name"])
        if os is None:
            continue
        files = [name for name in archive.namelist()
                 if "/" not in name and re.sub(r"^\d+_", "", name) == job["name"] + ".txt"]
        if len(files) != 1:
            raise SystemExit(f"run {run}: cannot identify log for {job['name']}")
        steps = [step for step in job["steps"] if step["startedAt"] != "0001-01-01T00:00:00Z"]
        for line in archive.read(files[0]).decode("utf-8-sig").splitlines():
            match = TIMESTAMP.match(ANSI.sub("", line))
            if not match:
                continue
            time, content = timestamp(match[1]), match[2]
            passed = PASSED.fullmatch(content)
            if not passed:
                continue
            active = next((step for step in reversed(steps) if timestamp(step["startedAt"]) <= time), None)
            if active and active["name"] in selected:
                results[os].add(passed[1])
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", nargs="+", type=int, required=True)
    parser.add_argument("--after", nargs="+", type=int, required=True)
    parser.add_argument("--repo", default="gesriot/ferrite-cad")
    parser.add_argument("--cache", type=Path, required=True)
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    sets = []
    for runs, before in [(args.before, True), (args.after, False)]:
        combined = {os: set() for os in ("linux", "macos", "windows")}
        for run in runs:
            for os, names in collect(run, before, args.cache, args.repo).items():
                combined[os].update(names)
        sets.append(combined)
    report = {os: {"before": len(sets[0][os]), "after": len(sets[1][os]),
                   "missing": sorted(sets[0][os] - sets[1][os])}
              for os in sets[0]}
    print(json.dumps({"before_runs": args.before, "after_runs": args.after,
                      "platforms": report}, indent=2))
    if any(not value["before"] or not value["after"] or value["missing"] for value in report.values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
