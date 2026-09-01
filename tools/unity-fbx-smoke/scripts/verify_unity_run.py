#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--expected", type=Path)
    parser.add_argument("--exit-status", type=int, required=True)
    args = parser.parse_args()
    if args.exit_status != 0:
        raise SystemExit(f"Unity exited {args.exit_status}")
    if not args.log.is_file() or not args.report.is_file():
        raise SystemExit("Unity did not create both log and report")
    log = args.log.read_text(encoding="utf-8", errors="replace")
    anchors = re.findall(r"FCAD_FBX_SMOKE_EXECUTED checks=([0-9]+)", log)
    if len(anchors) != 1:
        raise SystemExit(f"Unity execution anchor count is {len(anchors)}, expected one")
    if "FCAD_FBX_SMOKE_FAILURE" in log:
        raise SystemExit("Unity log contains the failure anchor")
    report_bytes = args.report.read_bytes()
    report = json.loads(report_bytes.decode("utf-8"))
    checks = int(anchors[0])
    if checks <= 40 or report.get("checks") != checks:
        raise SystemExit("Unity report has zero/stale/mismatched check count")
    if report.get("unity_version") != "6000.4.10f1":
        raise SystemExit(f"wrong Unity version in report: {report.get('unity_version')}")
    if args.expected is not None and report_bytes != args.expected.read_bytes():
        raise SystemExit("Unity report differs byte-for-byte from committed measurement")
    print(f"FCAD_UNITY_RUN_PROVEN checks={checks}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
