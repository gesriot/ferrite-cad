# Expressions and parameters

Set a length, angle or scalar from a literal or from parameters in the same document.
For example, a 10 mm stock plate with a 2 mm allowance can use `stock - allowance`
as its Extrude distance. An edit that changes either parameter recomputes the distance
in the same transaction and Undo step.

## Grammar

```text
sum      = product (("+" | "-") product)*
product  = unary (("*" | "/") unary)*
unary    = "-" unary | number [unit] | name | "(" sum ")"
number   = (digits ["." [digits]] | "." digits) [("e" | "E") ["+" | "-"] digits]
name     = [A-Za-z_][A-Za-z0-9_]*
unit     = "mm" | "cm" | "m" | "in" | "ft" | "rad" | "deg"
```

Whitespace may separate tokens. Names and units are case-sensitive. Reserve the unit
names for number suffixes. A single number with an optional leading minus uses the
slot's base unit, even when its written unit is omitted. Inside a compound expression
a bare number is scalar. For a length `h`, use `h + 5 mm` or `2 * h`. Multiplication
adds the length and angle exponents, division subtracts them, and addition and
subtraction require matching exponents. An intermediate area in `h * h / 1 mm` is valid.
The result must have the declared dimension. Binary operators at each level associate
left to right. Unit conversion uses the same factors as numeric CLI requests.

Opening reports `input` for syntax errors, unknown names and missing or extra dependency
rows. Syntax positions are one-based byte offsets. Rebuild reports dimension errors,
division by zero, non-finite results and stored-value mismatches on the owning object.
Independent objects still build. A consumer of a failed parameter identifies that parameter.
Numeric window forms accept literals and direct compound expressions to
**Parameters → Values**. The CLI keeps its existing text-expression refusal.

## In the window

Open **Parameters…** in Actions or **Edit…** on a parameter in Model. Add a length,
angle or scalar, edit its expression, or rename it; the document rewrites name references.
The table sorts names bytewise and shows each stored expression, accepted value and
consumer count. Select a row to see its consumers by name, kind and input slot. Lengths
use millimetres, angles use degrees, and scalars have no unit. Errors come from the
accepted rebuild, without evaluating again in the window.

Select another Model object to edit its expression text in **Values**. Constraint slots
use the same rule words and UUIDs as Sketch solves. Each Apply is one transaction and
one Undo step. Kernel refusals retain the typed text and appear in full at the field and
in the status. Parameters stays open after an accepted change; Close discards remaining
unsubmitted text and permits document Undo/Redo. Parameter deletion uses the shared
Model catalogue and its dependent-closure confirmation. See [the session contract](document-session.md#parameters).

## Executable CLI recipe

Set `FERRITECAD` to the CLI executable. The recipe creates a sample, drives its existing
height slot, and checks that refused edits leave the source and destination intact.

```python
# EXPRESSIONS_RECIPE
import json
import os
from pathlib import Path
import subprocess
import tempfile
import uuid

cli = os.environ["FERRITECAD"]
root = Path(tempfile.mkdtemp(prefix="expressions-"))
original = root / "plate.fcad"


def call(*args):
    return subprocess.run([cli, *map(str, args)], capture_output=True, text=True)


def inspect(path):
    result = call("inspect", path, "--json")
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)["result"]


def edit(path, filename, changes):
    request = root / "request.json"
    request.write_text(json.dumps({"request_version": 1, "changes": changes}), encoding="utf-8")
    return call("edit", path, "--request", request, "--expect-version",
                inspect(path)["content_version"], "-o", root / filename, "--json")


def identity():
    # A document object uses a UUIDv7.
    import time
    data = bytearray(int(time.time() * 1000).to_bytes(6, "big") + os.urandom(10))
    data[6] = (data[6] & 15) | 112
    data[8] = (data[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(data)))


assert call("create", original, "--sample").returncode == 0
feature = inspect(original)["features"][0]["feature_id"]
height, offset = identity(), identity()
first = root / "driven.fcad"
create = edit(original, first.name, [
    {"kind": "add_parameter", "object": height, "name": "h", "dimension": "length",
     "expression": "1 cm"},
    {"kind": "add_parameter", "object": offset, "name": "allowance", "dimension": "length",
     "expression": "h / 5"},
    {"kind": "set_expression", "object": feature, "slot": {"kind": "extrude_distance"},
     "expression": "h - allowance"},
])
if create.returncode:
    error = json.loads(create.stdout)["error"]
    assert error["kind"] == "unsupported" and "Open CASCADE" in error["message"], error
    assert not first.exists()
    print("EXPRESSIONS_STUB edit reports unavailable geometry")
    raise SystemExit(0)
assert inspect(first)["expression_slots"][0]["expression"]["value"] == 8
final = root / "renamed.fcad"
changed = edit(first, final.name, [
    {"kind": "set_parameter", "object": height, "expression": "25 mm"},
    {"kind": "rename_parameter", "object": height, "name": "stock"},
])
assert changed.returncode == 0, changed.stderr
facts = inspect(final)
assert facts["expression_slots"][0]["expression"] == {"source": "stock - allowance", "value": 20}
stock = next(p for p in facts["parameters"] if p["name"] == "stock")
assert stock["value"] == 25 and set(stock["dependents"]) == {feature, offset}
assert "stock = 25 mm" in call("inspect", final).stdout
for change, words in [
    ({"kind": "remove_object", "object": height}, [feature, offset]),
    ({"kind": "set_parameter", "object": height, "expression": "allowance"},
     ["cycle", "stock", "allowance"]),
]:
    before = final.read_bytes()
    rejected = edit(final, "refused.fcad", [change])
    assert rejected.returncode == 2, rejected.stdout
    error = json.loads(rejected.stdout)["error"]
    assert error["kind"] == "input" and all(w in error["message"] for w in words), error
    assert final.read_bytes() == before and not (root / "refused.fcad").exists()
mode = "SOLVER" if os.environ.get("FCAD_EXPECT_SOLVER") == "1" else "NO_SOLVER"
print("EXPRESSIONS_" + mode + " values, dependencies, rename and atomic refusals")
```
