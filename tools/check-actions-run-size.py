#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Check run scalar size and real packed Bash argv without executing run bodies.

These workflows use plain/quoted one-line scalars or literal | scalars. Refuse
other run formats so a new YAML representation cannot silently evade the limit.
Actionlint remains the independent full YAML/workflow/shell validator.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys

LIMIT = 21000


def scripts(path):
    lines = path.read_text().splitlines(keepends=True)
    shell = "bash"
    for index, line in enumerate(lines):
        if line.startswith("      - "):
            shell = "bash"
        if line.strip().startswith("shell:"):
            shell = line.strip().split(":", 1)[1].strip()
        match = re.match(r"^(\s*)(?:- )?run:\s*(.*?)\s*$", line)
        if not match:
            continue
        indent, scalar = match.groups()
        body_indent = len(indent) + (2 if line.lstrip().startswith("- ") else 0)
        if not scalar and index + 1 < len(lines) and lines[index + 1].strip().startswith(("#", "shell:")):
            continue  # defaults.run is a mapping, not a command
        if scalar in ("|", "|-", "|+"):
            body = []
            for following in lines[index + 1:]:
                if following.strip() and len(following) - len(following.lstrip()) <= body_indent:
                    break
                body.append(following[body_indent + 2:] if following.strip() else following)
            command = "".join(body)
        elif scalar.startswith('"'):
            command = json.loads(scalar)
        elif scalar.startswith("'"):
            assert scalar.endswith("'"), (path, index + 1, "bad quoted scalar")
            command = scalar[1:-1].replace("''", "'")
        else:
            assert scalar and not scalar.startswith(('>', '&', '*')), (path, index + 1, "unsupported run scalar")
            command = scalar
        yield index + 1, command, shell


def main():
    count, maximum, packed_max = 0, (0, ""), 0
    for path in sorted(Path(".github/workflows").glob("*.yml")):
        for line, command, shell in scripts(path):
            size = len(command.encode())
            assert size <= LIMIT, f"{path}:{line}: run is {size} bytes > {LIMIT}"
            # The real argv is passed to exec via subprocess; -n parses but never
            # executes commands. Keep raw script size above (including expressions).
            checked = re.sub(r"\$\{\{.*?\}\}", "CHECK", command)
            argv = ["bash", "--noprofile", "--norc", "-n", "-c", command]
            if shell in ("pwsh", "powershell", "cmd"):
                # PowerShell/cmd syntax is platform-owned; the same script is passed
                # as an actual argv value to a length probe on this host.
                argv = [sys.executable, "-c", "import sys; assert len(sys.argv[1].encode()) <= 21000", command]
            packed = sum(len(os.fsencode(arg)) + 1 for arg in argv)
            assert packed <= LIMIT, f"{path}:{line}: packed argv is {packed} bytes"
            # Pass the original scalar, including unreplaced GitHub expressions,
            # as a real argv value too; the syntax-only Bash call below needs
            # those expressions resolved to a harmless literal.
            probe = subprocess.run(
                [sys.executable, "-c", "import sys; assert len(sys.argv[1].encode()) <= 21000", command],
                capture_output=True, text=True,
            )
            assert probe.returncode == 0, (path, line, probe.stderr)
            if shell not in ("pwsh", "powershell", "cmd"):
                argv[-1] = checked
            result = subprocess.run(argv, capture_output=True, text=True)
            assert result.returncode == 0, (path, line, result.stderr)
            count += 1
            maximum = max(maximum, (size, f"{path}:{line}"))
            packed_max = max(packed_max, packed)
    print(f"FCAD_ACTIONS_RUN_SIZE_OK blocks={count} max_bytes={maximum[0]} packed_argv_max={packed_max} limit={LIMIT} largest={maximum[1]}")


if __name__ == "__main__":
    main()
