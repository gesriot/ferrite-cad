# §25F — сохраняемая длина Line

[Точный request/result и UI-контракт](sketch-constraints-copy.md),
[проверки и ограничения](line-length-constraints-verification.md).
Длина — положительное конечное евклидово расстояние Start/End одного Line в mm.
Stored координаты остаются inputs solver. Translation under-constrained модели
может изменяться; измерять нужно размеры/объём, а не фиксированное положение.
Нет arbitrary point-to-point dimensions, formula/Parameter, solver preview,
in-place Save или coordinate drag constrained Sketch.

Ниже запускаемый рецепт через публичный CLI. Нужны OCCT + PlaneGCS, `FERRITECAD`
и `FCAD_UFBX_READER` — путь к существующему pinned reader, собранному из
`tools/unity-fbx-smoke/scripts/read_production.c` с pin из `fetch_ufbx.sh`.
На macOS native library paths экспортируются в том же Bash. Рецепт создаёт свой
временный каталог, не меняет fixtures и не требует UI или заранее известных UUID.
Новые constraint UUID независимы между отдельными публикациями.

```python
# FCAD_25F_AGENT_RECIPE
import hashlib, json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-line-length-"))
def run(operation, *args):
    p = subprocess.run([cli, operation, *map(str, args), "--json"], capture_output=True)
    if p.returncode == 7:
        raise RuntimeError("Report lost; inspect publication. Do not retry automatically.")
    data = json.loads(p.stdout)
    assert data["schema_version"] == 1 and data["operation"] == operation
    if p.returncode != 0 or not data["ok"]:
        raise RuntimeError((p.returncode, data))  # includes STEP 4/5 and partial FBX 6
    result = data["result"]
    if operation == "validate" and (not result["valid"] or result["diagnostics"]):
        raise RuntimeError(("Validation requires a decision", result))
    if operation == "export-fbx" and (not result["complete"] or result["omissions"]):
        raise RuntimeError(("Partial model requires a decision", result))
    return result

source = root / "stored-80x40.fcad"
profile = root / "profile.json"
profile.write_text(json.dumps({"request_version":1,
    "points_mm":[[-40,-20],[40,-20],[40,20],[-40,20]], "height_mm":10}))
run("create-sketch-extrude", profile, "-o", source)
before = (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns)
catalog = run("inspect", source)
choices = [s for s in catalog["sketches"] if s["constraint_edit"]["available"]]
assert len(choices) == 1, "Choose Sketch explicitly in an ambiguous document"
sketch = choices[0]
curves = sketch["constraint_edit"]["curves"]
assert len(curves) == 4 and not sketch["constraint_edit"]["constraints"]
horizontal = [c for c in curves if c["start_mm"][1] == c["end_mm"][1]]
vertical = [c for c in curves if c["start_mm"][0] == c["end_mm"][0]]
assert len(horizontal) == len(vertical) == 2
assert all(abs(c["end_mm"][0]-c["start_mm"][0]) == 80 for c in horizontal)
assert all(abs(c["end_mm"][1]-c["start_mm"][1]) == 40 for c in vertical)
bottom = min(horizontal, key=lambda c: c["start_mm"][1])
right = max(vertical, key=lambda c: c["start_mm"][0])
add = [{"curve_id": c["curve_id"], "rule": "horizontal" if c in horizontal else "vertical"}
       for c in curves]
add += [{"curve_id": bottom["curve_id"], "rule": "distance", "distance_mm": 60},
        {"curve_id": right["curve_id"], "rule": "distance", "distance_mm": 30}]
request = root / "constraints.json"
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": add}))
copy = root / "solved-60x30.fcad"
published = run("edit-sketch-constraints-copy", source, "--sketch", sketch["sketch_id"],
                "--expect-version", catalog["content_version"], "--request", request, "-o", copy)
assert published["destination"] == str(copy)
assert published["document_id"] == catalog["document_id"]
run("validate", copy)
after = run("inspect", copy)
saved = next(s for s in after["sketches"] if s["sketch_id"] == sketch["sketch_id"])
assert saved["constraint_edit"]["curves"] == curves  # stored inputs did not move
assert saved["constraint_edit"]["constraints"] == published["added_constraints"]
assert all(c["rule"]["kind"] in {"coincident", "horizontal", "vertical", "distance"}
           for c in published["added_constraints"]), "Unknown rule: stop editing"
lengths = [c for c in published["added_constraints"] if c["rule"]["kind"] == "distance"]
assert [c["rule"]["distance"] for c in lengths] == [60, 30]  # response field is distance
assert len({c["constraint_id"] for c in published["added_constraints"]}) == 10
for c in lengths:
    r = c["rule"]
    assert r["a"]["curve_id"] == r["b"]["curve_id"]
    assert (r["a"]["at"], r["b"]["at"]) == ("start", "end")
assert len(after["bodies"]) == 1, "Choose Body explicitly if ambiguous"
stl = root / "measured.stl"
report = run("export-stl", copy, "--solid", after["bodies"][0]["body_id"], "-o", stl)
data = stl.read_bytes()
n = struct.unpack_from("<I", data, 80)[0]
assert n == report["triangles"] and len(data) == report["bytes"] == 84 + 50*n
points, volume6 = [], 0.0
for i in range(n):
    f = struct.unpack_from("<12fH", data, 84 + 50*i)
    a, b, c = f[3:6], f[6:9], f[9:12]
    points.extend((a, b, c))
    volume6 += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2])
                + a[2]*(b[0]*c[1]-b[1]*c[0]))
dimensions = [max(p[j] for p in points)-min(p[j] for p in points) for j in range(3)]
assert all(math.isclose(a,b,rel_tol=1e-6,abs_tol=1e-5)
           for a,b in zip(dimensions, [60,30,10])), dimensions
volume = abs(volume6)/6
assert math.isclose(volume,18000,rel_tol=1e-6), volume
fbx = root / "measured.fbx"
run("export-fbx", copy, "-o", fbx)
subprocess.run([reader, "--identity", str(fbx)], check=True)  # independent FBX reader
assert (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns) == before
print(json.dumps({"artifacts": str(root), "dimensions_mm": dimensions, "volume_mm3": volume,
                  "solve": published["solve"]}, ensure_ascii=False))
```

`solve.degrees_of_freedom` — измеренный результат, не обещание fully constrained.
Отдельные inspect/export читают отдельные snapshots. Constraint-copy защищён
полным `--expect-version`; экспорты такого guard не имеют. При отказе/конфликте
нет новой копии. Exit 7 после publish оставляет копию целой, повтор запрещён без
проверки назначения. UI Undo/Redo меняет request до публикации; CLI воспроизводит
финальный ordered request без команд для отдельных UI-жестов.
