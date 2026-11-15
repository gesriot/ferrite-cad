# FerriteCAD

FerriteCAD is a local parametric mechanical CAD application, with no cloud or
account. This early application provides sketch and solid modelling, a command
line with STEP import, and STL/FBX export. The `.fcad` format is not stable; a
document from another build may not open.

Platforms: Windows, Linux and macOS on Apple Silicon; see [platform support](docs/platform-support.md).

## Build for use

Use a **release** build to work with models. Debug builds are for tests and
debugging: Open CASCADE is built separately in Release, but Rust rebuilding,
scene preparation, meshing and the window run much more slowly in debug.

Install the pinned Rust toolchain and build the shared native libraries first:
[Open CASCADE](docs/build-occt.md) and [PlaneGCS](docs/build-planegcs.md).
The commands below run from the repository root and use the macOS checkout
locations `vendor/install` and `vendor/planegcs`; the build guides describe the
other platforms' compiler and library paths.

```sh
FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1 \
FCAD_PLANEGCS_DIR="$PWD/vendor/planegcs" \
OpenCASCADE_DIR="$PWD/vendor/install/lib/cmake/opencascade" \
cargo build --release --features planegcs \
  -p ferritecad-app --bin ferritecad-viewer \
  -p ferritecad-cli --bin ferritecad
```

Without Open CASCADE, the program builds with a **stub kernel**: operations that
build bodies refuse. `FERRITECAD_REQUIRE_OCCT=1` makes that fallback a build error,
so it is always set for a build intended for use. PlaneGCS is off by default
because it is LGPL; without it constrained sketches refuse to solve. See its
[build and replacement instructions](docs/build-planegcs.md).

Unpackaged binaries need the native libraries on the loader's search path. On
macOS, set this in the same shell that launches the binaries:

```sh
export DYLD_LIBRARY_PATH="$PWD/vendor/install/lib:$PWD/vendor/planegcs"
target/release/ferritecad-viewer
target/release/ferritecad --help
```

Close the window to return to the shell. Linux uses `LD_LIBRARY_PATH`; Windows
needs the native DLL directories on `PATH`, as described in the build guides.
For a movable application with its own libraries, follow the
[application bundle and archive recipe](docs/runtime-layout.md#build-a-local-application-package).

## Build for development

The default Cargo profile is debug:

```sh
cargo build
cargo test
```

CI exercises three configurations: the stub kernel, OCCT without PlaneGCS, and
OCCT with PlaneGCS. The stub is only for testing paths without a geometry kernel;
native tests may skip there. Set the native paths and required-dependency flags
for the configuration being tested; see [contributing](CONTRIBUTING.md#build-and-run)
and the two native build guides above. On a developer Mac, GUI and GPU runs,
including tests that exercise them, use the memory watchdog described in
[CONTRIBUTING.md](CONTRIBUTING.md#tests).

## First steps in the window

1. Choose **Open…** for an existing `.fcad`, or **New…**, **Empty document**, then
   **Create document** for a new one.
2. Choose **New sketch…** above the **Model** tree and choose the XY plane.
3. With **Line**, click four corners of a rectangle, then click its snapped start
   to close it. Choose **Finish** to accept the sketch.
4. Select the sketch in **Model**, choose **Extrude…**, keep **New body**, enter a
   distance in millimetres and choose **Apply**. **Save** writes the accepted model
   and asks for a path when the document has none.

See [sketch tools and draft history](docs/sketch-mode.md) and
[Extrude and Revolve](docs/extrude-and-revolve.md) for the controls and refusals.

## First steps on the command line

Run this Python 3 block from the repository root, with the release build and
loader environment above. It calls `create --sample`, `inspect --json`,
`validate`, `rebuild --cold` and `export-stl`, then reads the binary STL to check
its size and dimensions. Results remain in the printed temporary directory.
Set `FERRITECAD` to an absolute binary path to use another build.

```python
# README_PLATE_RECIPE
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

cli = Path(os.environ.get("FERRITECAD", "target/release/ferritecad")).resolve()
root = Path(tempfile.mkdtemp(prefix="ferritecad-first-steps-"))
source, output = root / "plate.fcad", root / "plate.stl"

def run(*args):
    result = subprocess.run([str(cli), *map(str, args)], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout

run("create", source, "--sample", "--size", 60, 40, 10)
before = source.read_bytes()
reading = json.loads(run("inspect", source, "--json"))
assert reading["schema_version"] == 1 and reading["operation"] == "inspect"
assert reading["ok"] and len(reading["result"]["bodies"]) == 1
body = reading["result"]["bodies"][0]["body_id"]
print(run("validate", source), end="")
print(run("rebuild", source, "--cold"), end="")
print(run("export-stl", source, "--solid", body, "--output", output), end="")
data = output.read_bytes()
triangles, = struct.unpack_from("<I", data, 80)
assert triangles == 12 and len(data) == 84 + 50 * triangles == 684
vertices = [struct.unpack_from("<3f", data, 84 + 50*t + 12 + 12*v)
            for t in range(triangles) for v in range(3)]
assert [max(p[a] for p in vertices) - min(p[a] for p in vertices)
        for a in range(3)] == [60, 40, 10]
assert source.read_bytes() == before
print("README_PLATE_OK 12 triangles; 60 x 40 x 10 mm; files:", root)
```

The same result is checked with and without PlaneGCS: this sample has no sketch
constraints. A stub build cannot complete the rebuild or export. Validation
checks stored consistency; it does not replace rebuilding geometry.

## Read further

- [CLI capabilities](docs/cli-capabilities.md) and [JSON v1](docs/cli-json-v1.md)
- [Model edit transactions](docs/edit-transactions.md) and [expressions](docs/expressions.md)
- [Document format](docs/file-format.md)
- [Window navigation](docs/workspace-viewport-layout.md#viewing-a-document)
- [Runtime layout and packaging](docs/runtime-layout.md)
- [OCCT build](docs/build-occt.md) and [PlaneGCS build](docs/build-planegcs.md)
- [Architecture decisions](docs/decisions/) and [contributing](CONTRIBUTING.md)

## Licence

FerriteCAD is MIT licensed; see [LICENSE](LICENSE). Open CASCADE is LGPL-2.1 with
the Open CASCADE exception, and PlaneGCS is LGPL-2.0-or-later. Both native libraries
stay dynamically linked and replaceable; see [third-party licences](THIRD_PARTY_LICENSES.md).
