# §25G — закреплённая вершина собственного Line-профиля

[Точный request/result и UI-контракт](sketch-constraints-copy.md),
[проверки и ограничения](fixed-sketch-vertex-verification.md).
Fixed закрепляет **один** endpoint одного сохранённого Line профиля в заданных
конечных X/Y миллиметрах через существующий `SketchConstraintRule::Fixed`,
существующий перевод в solver и прежнюю операцию `edit-sketch-constraints-copy`.
Новой команды, нового writer, rebuild или publish здесь нет.

Три разные вещи, которые этот срез намеренно не смешивает:

* **Stored coordinates** — то, что записано в `.fcad` и что discovery отдаёт в
  `constraint_edit.curves`. Они остаются исходными данными solver и **не**
  перезаписываются его решением ни при одной публикации.
* **Solved coordinates** — presentation одного rebuild. Их измеряют по телу
  (STL/FBX), а не читают из stored curves.
* **Degrees of freedom** — измеренный результат solve. H/V плюс две длины дают
  полностью размерный прямоугольник с DOF 2 (две трансляции); один Fixed
  endpoint снимает ровно эти две степени и даёт DOF 0. Снятие закрепления
  возвращает DOF 2.

Ограничения среза: не более одного Fixed на профиль, только собственные XY Line
polygons с одним positive literal Blind/NewBody, прежние H/V + Line length +
adjacent Coincident closure. Нет отверстий, новых форм, нескольких закреплений,
произвольных ограничений, live solver drag, in-place Save и persistent undo.
Coordinate edit, drag и Snap constrained Sketch по-прежнему недоступны.

Ниже запускаемый рецепт через публичный CLI: create → H/V + размеры →
inspect UUID/version → Fixed copy → cold rebuild → независимый STL/FBX →
замена → удаление. Нужны OCCT + PlaneGCS, `FERRITECAD` и `FCAD_UFBX_READER` —
путь к существующему pinned reader, собранному из
`tools/unity-fbx-smoke/scripts/read_production.c` с pin из `fetch_ufbx.sh`.
На macOS native library paths экспортируются в том же Bash. Рецепт создаёт свой
временный каталог, не меняет fixtures и не требует UI или заранее известных UUID.

```python
# FCAD_25G_AGENT_RECIPE
import hashlib, json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-fixed-vertex-"))
def run(operation, *args):
    p = subprocess.run([cli, operation, *map(str, args), "--json"], capture_output=True)
    if p.returncode == 7:
        raise RuntimeError("Report lost; inspect publication. Do not retry automatically.")
    data = json.loads(p.stdout)
    assert data["schema_version"] == 1 and data["operation"] == operation
    if p.returncode != 0 or not data["ok"]:
        raise RuntimeError((p.returncode, data))
    result = data["result"]
    if operation == "validate" and (not result["valid"] or result["diagnostics"]):
        raise RuntimeError(("Validation requires a decision", result))
    if operation == "export-fbx" and (not result["complete"] or result["omissions"]):
        raise RuntimeError(("Partial model requires a decision", result))
    return result
def edit_args(source, catalog, request, out):
    path = root / "request.json"
    path.write_text(json.dumps(request))
    return ["edit-sketch-constraints-copy", str(source), "--sketch",
            catalog["sketches"][0]["sketch_id"], "--expect-version",
            catalog["content_version"], "--request", str(path), "-o", str(out)]
def edit(source, catalog, request, out):
    return run(*edit_args(source, catalog, request, out))
def refused_edit(source, catalog, request, out):
    p = subprocess.run([cli, *edit_args(source, catalog, request, out), "--json"],
                       capture_output=True)
    assert p.returncode == 2, p
    data = json.loads(p.stdout)
    assert data["ok"] is False and not out.exists()
    return data["error"]
def measured(path):
    """Independent STL integration: extents, volume and the XY footprint."""
    stl = path.with_suffix(".stl")
    body = run("inspect", path)["bodies"][0]["body_id"]
    report = run("export-stl", path, "--solid", body, "-o", stl)
    data = stl.read_bytes()
    n = struct.unpack_from("<I", data, 80)[0]
    assert n == report["triangles"] and len(data) == report["bytes"] == 84 + 50 * n
    points, volume6 = [], 0.0
    for i in range(n):
        f = struct.unpack_from("<12fH", data, 84 + 50 * i)
        a, b, c = f[3:6], f[6:9], f[9:12]
        points.extend((a, b, c))
        volume6 += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2])
                    + a[2]*(b[0]*c[1]-b[1]*c[0]))
    lo = [min(p[j] for p in points) for j in range(3)]
    hi = [max(p[j] for p in points) for j in range(3)]
    fbx = path.with_suffix(".fbx")
    run("export-fbx", path, "-o", fbx)
    subprocess.run([reader, "--identity", str(fbx)], check=True)  # independent FBX reader
    return lo, hi, abs(volume6) / 6

source = root / "stored-80x40.fcad"
profile = root / "profile.json"
profile.write_text(json.dumps({"request_version": 1,
    "points_mm": [[-40,-20],[40,-20],[40,20],[-40,20]], "height_mm": 10}))
run("create-sketch-extrude", profile, "-o", source)
before = (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns)
catalog = run("inspect", source)
sketch = catalog["sketches"][0]
curves = sketch["constraint_edit"]["curves"]
assert sketch["constraint_edit"]["available"] and len(curves) == 4
assert not sketch["constraint_edit"]["constraints"]

# 1. H/V + two lengths: fully dimensioned, and still free to translate.
add = [{"curve_id": c["curve_id"], "rule": "horizontal" if i % 2 == 0 else "vertical"}
       for i, c in enumerate(curves)]
add += [{"curve_id": curves[0]["curve_id"], "rule": "distance", "distance_mm": 60},
        {"curve_id": curves[1]["curve_id"], "rule": "distance", "distance_mm": 30}]
sized = root / "dimensioned.fcad"
published = edit(source, catalog, {"request_version": 1, "remove": [], "add": add}, sized)
assert published["solve"]["degrees_of_freedom"] == 2, "a dimensioned rectangle still floats"
run("validate", sized)
sized_catalog = run("inspect", sized)
sized_ids = [c["constraint_id"]
             for c in sized_catalog["sketches"][0]["constraint_edit"]["constraints"]]
assert len(sized_ids) == 10
_, _, free_volume = measured(sized)  # size is fixed; its place in XY is not
assert math.isclose(free_volume, 18000, rel_tol=1e-6), free_volume

# 2. One Fixed endpoint at explicit millimetres.
pin_curve = sized_catalog["sketches"][0]["constraint_edit"]["curves"][0]["curve_id"]
pinned = root / "pinned.fcad"
r = edit(sized, sized_catalog,
         {"request_version": 1, "remove": [],
          "add": [{"rule": "fixed", "curve_id": pin_curve, "at": "start",
                   "x_mm": 10, "y_mm": -5}]}, pinned)
assert len(r["added_constraints"]) == 1, "closure already persisted"
rule = r["added_constraints"][0]["rule"]
assert rule["kind"] == "fixed" and rule["point"] == {"curve_id": pin_curve, "at": "start"}
assert (rule["x"], rule["y"]) == (10, -5) and "x_mm" not in rule  # response wire unchanged
pin_id = r["added_constraints"][0]["constraint_id"]
assert pin_id not in sized_ids
assert r["solve"]["degrees_of_freedom"] == 0, "the pin removed both translations"
run("validate", pinned)
after = run("inspect", pinned)
# Stored coordinates are inputs; the solve did not write itself back.
assert after["sketches"][0]["constraint_edit"]["curves"] == curves
assert [c["constraint_id"]
        for c in after["sketches"][0]["constraint_edit"]["constraints"]] == sized_ids + [pin_id]
lo, hi, volume = measured(pinned)
assert all(math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-5)
           for a, b in zip([hi[j] - lo[j] for j in range(3)], [60, 30, 10]))
assert math.isclose(volume, 18000, rel_tol=1e-6), volume
assert any(math.isclose(x, 10, abs_tol=1e-5) for x in (lo[0], hi[0]))
assert any(math.isclose(y, -5, abs_tol=1e-5) for y in (lo[1], hi[1]))

# 3. One Fixed per profile: a second pin is a structural refusal, not a silent add.
#    The opposite endpoint of the very joint the first pin sits on is also a second pin.
other_curve = after["sketches"][0]["constraint_edit"]["curves"][3]["curve_id"]
for second in [{"rule": "fixed", "curve_id": pin_curve, "at": "end", "x_mm": 70, "y_mm": -5},
               {"rule": "fixed", "curve_id": other_curve, "at": "end", "x_mm": 10, "y_mm": -5}]:
    error = refused_edit(pinned, after, {"request_version": 1, "remove": [], "add": [second]},
                         root / "never.fcad")
    assert error["kind"] == "input", error

# 4. Replacement: remove the exact UUID and add the new pin in one request.
moved = root / "moved.fcad"
r = edit(pinned, after,
         {"request_version": 1, "remove": [pin_id],
          "add": [{"rule": "fixed", "curve_id": pin_curve, "at": "start",
                   "x_mm": -25, "y_mm": 40}]}, moved)
assert r["removed_constraint_ids"] == [pin_id]
assert r["solve"]["degrees_of_freedom"] == 0
new_pin = r["added_constraints"][0]["constraint_id"]
assert new_pin != pin_id
moved_catalog = run("inspect", moved)
assert [c["constraint_id"]
        for c in moved_catalog["sketches"][0]["constraint_edit"]["constraints"]] \
    == sized_ids + [new_pin]
mlo, mhi, mvolume = measured(moved)
# The body translated by exactly the pin delta; no dimension changed.
for j, delta in enumerate((-35, 45)):
    assert math.isclose(mlo[j] - lo[j], delta, abs_tol=1e-5), (mlo, lo)
    assert math.isclose(mhi[j] - hi[j], delta, abs_tol=1e-5), (mhi, hi)
assert math.isclose(mvolume, 18000, rel_tol=1e-6)

# 5. Removing the exact pin gives the two translations back and keeps everything else.
unpinned = root / "unpinned.fcad"
r = edit(moved, moved_catalog, {"request_version": 1, "remove": [new_pin], "add": []}, unpinned)
assert r["added_constraints"] == [] and r["solve"]["degrees_of_freedom"] == 2
run("validate", unpinned)
final = run("inspect", unpinned)
assert [c["constraint_id"]
        for c in final["sketches"][0]["constraint_edit"]["constraints"]] == sized_ids
assert final["sketches"][0]["constraint_edit"]["curves"] == curves
_, _, unpinned_volume = measured(unpinned)
assert math.isclose(unpinned_volume, 18000, rel_tol=1e-6)

assert (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns) == before
print(json.dumps({"artifacts": str(root), "pinned_mm": [lo[:2], hi[:2]],
                  "moved_mm": [mlo[:2], mhi[:2]], "volume_mm3": volume,
                  "dof": {"dimensioned": 2, "pinned": 0, "unpinned": 2}}, ensure_ascii=False))
```

`solve.degrees_of_freedom` — измеренный результат одного cold rebuild
опубликованной копии, а не обещание. Отдельные inspect/export читают отдельные
snapshots. Constraint-copy защищён полным `--expect-version`; экспорты такого
guard не имеют. При отказе/конфликте новой копии нет. Exit 7 после publish
оставляет копию целой, повтор запрещён без проверки назначения. UI собирает тот
же ordered request до публикации; отдельных CLI-команд для UI-жестов нет.
