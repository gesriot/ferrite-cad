# §25H — равенство длин двух Lines

[Точный request/result и UI-контракт](sketch-constraints-copy.md),
[проверки и ограничения](equal-line-lengths-verification.md).
Equal length — **постоянная связь двух целых Lines** одного собственного XY
профиля: существующий `SketchConstraintRule::EqualLength { a, b }` с двумя
`SketchSegmentRef` Start→End, существующий перевод в solver и прежняя операция
`edit-sketch-constraints-copy`. Новой команды, writer, rebuild, publish или
схемы БД здесь нет.

Чем это **не** является:

* не копированием числа в второй `Distance` — числа у равенства нет вовсе;
* не записью решённых координат обратно в исходный Sketch — stored координаты
  остаются inputs solver при любой публикации;
* не клиентской таблицей связей — пара живёт в документе как один constraint
  со своим UUID.

Поэтому изменение **ведущей** длины меняет обе стороны: связь остаётся в силе,
и её UUID переживает замену `Distance` (remove exact UUID + add новой длины).

Структурная политика пары:

* обе стороны — Start/End целой Line выбранного Sketch;
* для занятости слота `(A,B)` и `(B,A)` — одна и та же пара; сохранённый
  `SegmentRef`, записанный End→Start, читается как та же Line и **не**
  переписывается канонизацией;
* self-pair, чужая Line, duplicate и reversed duplicate — структурный отказ
  до solver, без публикации;
* remove старого UUID + add той же пары в одном запросе допустим: проверка
  выполняется по retained состоянию;
* избыточность и противоречие — не структурные категории: их устанавливает
  настоящий solver (`redundant_constraint_ids` либо typed `constraint_conflict`).

Первый сценарий среза: H/V прямоугольник с одной длиной 60 mm и одним Fixed
имеет DOF 1; EqualLength соседних сторон даёт квадрат 60×60×10 mm, 36000 mm³ и
DOF 0; замена ведущей длины 60→45 даёт 45×45×10 mm и 20250 mm³, сохраняя UUID
равенства; удаление exact UUID равенства возвращает DOF 1 и сохраняет H/V,
Fixed, ведущую длину и Coincident closure.

Ограничения среза: нынешний managed собственный XY Line polygon, один positive
literal Blind/NewBody, прежние H/V + Line length + один Fixed + adjacent
Coincident closure. Нет окружностей, дуг, отверстий, Parallel, Perpendicular,
live solver drag, in-place Save, новой схемы и FFI. Coordinate drag и Snap
constrained Sketch по-прежнему недоступны.

Ниже запускаемый рецепт через публичный CLI: create → H/V + одна длина + Fixed →
EqualLength → независимый STL/FBX → замена ведущей длины → удаление → отказы.
Нужны OCCT + PlaneGCS, `FERRITECAD` и `FCAD_UFBX_READER` — путь к существующему
pinned reader, собранному из `tools/unity-fbx-smoke/scripts/read_production.c`
с pin из `fetch_ufbx.sh`. На macOS native library paths экспортируются в том же
Bash. Рецепт создаёт свой временный каталог, не меняет fixtures и не требует UI
или заранее известных UUID.

```python
# FCAD_25H_AGENT_RECIPE
import hashlib, json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-equal-length-"))
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
def ids(catalog):
    return [c["constraint_id"]
            for c in catalog["sketches"][0]["constraint_edit"]["constraints"]]
def measured(path):
    """Independent STL integration plus the pinned ufbx reader."""
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
    extents = [max(p[j] for p in points) - min(p[j] for p in points) for j in range(3)]
    fbx = path.with_suffix(".fbx")
    run("export-fbx", path, "-o", fbx)
    subprocess.run([reader, "--identity", str(fbx)], check=True)  # independent FBX reader
    return extents, abs(volume6) / 6

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

# 1. H/V, one length and one pin: exactly one dimension is still free.
add = [{"curve_id": c["curve_id"], "rule": "horizontal" if i % 2 == 0 else "vertical"}
       for i, c in enumerate(curves)]
add += [{"curve_id": curves[0]["curve_id"], "rule": "distance", "distance_mm": 60},
        {"curve_id": curves[0]["curve_id"], "rule": "fixed", "at": "start",
         "x_mm": 10, "y_mm": -5}]
base = root / "one-size.fcad"
published = edit(source, catalog, {"request_version": 1, "remove": [], "add": add}, base)
assert published["solve"]["degrees_of_freedom"] == 1, "one dimension is still free"
run("validate", base)
base_catalog = run("inspect", base)
base_ids = ids(base_catalog)
assert len(base_ids) == 10
base_curves = base_catalog["sketches"][0]["constraint_edit"]["curves"]
assert base_curves == curves  # stored inputs, not the solve
length_id = next(c["constraint_id"]
                 for c in base_catalog["sketches"][0]["constraint_edit"]["constraints"]
                 if c["rule"]["kind"] == "distance")
a_id, b_id = base_curves[0]["curve_id"], base_curves[1]["curve_id"]

# 2. Structural refusals: a pair is two different Lines of this Sketch, once.
for bad in [{"rule": "equal_length", "a_curve_id": a_id, "b_curve_id": a_id},
            {"rule": "equal_length", "a_curve_id": a_id,
             "b_curve_id": "00000000-0000-7000-8000-000000000000"}]:
    error = refused_edit(base, base_catalog, {"request_version": 1, "remove": [],
                                              "add": [bad]}, root / "never.fcad")
    assert error["kind"] == "input" and "constraint_conflict" not in error, error
for pair in [[(a_id, b_id), (a_id, b_id)], [(a_id, b_id), (b_id, a_id)]]:
    error = refused_edit(base, base_catalog, {"request_version": 1, "remove": [], "add": [
        {"rule": "equal_length", "a_curve_id": x, "b_curve_id": y} for x, y in pair]},
        root / "never.fcad")
    assert error["kind"] == "input", error

# 3. One equality ties two named Lines and removes the last dimension.
square = root / "square.fcad"
r = edit(base, base_catalog, {"request_version": 1, "remove": [], "add": [
    {"rule": "equal_length", "a_curve_id": a_id, "b_curve_id": b_id}]}, square)
assert len(r["added_constraints"]) == 1, "closure already persisted"
rule = r["added_constraints"][0]["rule"]
assert rule["kind"] == "equal_length"
for side, curve in (("a", a_id), ("b", b_id)):
    assert rule[side] == {"from": {"curve_id": curve, "at": "start"},
                          "to": {"curve_id": curve, "at": "end"}}
assert "a_curve_id" not in rule and "distance" not in rule  # response wire unchanged
equal_id = r["added_constraints"][0]["constraint_id"]
assert equal_id not in base_ids
assert r["solve"]["degrees_of_freedom"] == 0
assert r["solve"]["redundant_constraint_ids"] == []
run("validate", square)
square_catalog = run("inspect", square)
assert ids(square_catalog) == base_ids + [equal_id]
assert square_catalog["sketches"][0]["constraint_edit"]["curves"] == curves
extents, volume = measured(square)
assert all(math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-5)
           for a, b in zip(extents, [60, 60, 10])), extents
assert math.isclose(volume, 36000, rel_tol=1e-6), volume

# 4. Changing the leading length moves both sides; the equality keeps its UUID.
smaller = root / "smaller.fcad"
r = edit(square, square_catalog, {"request_version": 1, "remove": [length_id], "add": [
    {"curve_id": a_id, "rule": "distance", "distance_mm": 45}]}, smaller)
assert r["removed_constraint_ids"] == [length_id]
assert r["solve"]["degrees_of_freedom"] == 0
smaller_catalog = run("inspect", smaller)
smaller_ids = ids(smaller_catalog)
assert equal_id in smaller_ids and length_id not in smaller_ids and len(smaller_ids) == 11
extents, volume = measured(smaller)
assert all(math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-5)
           for a, b in zip(extents, [45, 45, 10])), extents
assert math.isclose(volume, 20250, rel_tol=1e-6), volume

# 5. The stored pair occupies its slot whichever way round it is named.
for x, y in [(a_id, b_id), (b_id, a_id)]:
    error = refused_edit(smaller, smaller_catalog, {"request_version": 1, "remove": [], "add": [
        {"rule": "equal_length", "a_curve_id": x, "b_curve_id": y}]}, root / "never.fcad")
    assert error["kind"] == "input", error

# 6. Removing the exact equality gives the dimension back and keeps the rest.
freed = root / "freed.fcad"
r = edit(smaller, smaller_catalog, {"request_version": 1, "remove": [equal_id], "add": []}, freed)
assert r["added_constraints"] == [] and r["solve"]["degrees_of_freedom"] == 1
run("validate", freed)
freed_catalog = run("inspect", freed)
assert ids(freed_catalog) == [i for i in smaller_ids if i != equal_id]
kinds = [c["rule"]["kind"]
         for c in freed_catalog["sketches"][0]["constraint_edit"]["constraints"]]
assert sorted(kinds) == sorted(["coincident"] * 4 + ["horizontal"] * 2
                               + ["vertical"] * 2 + ["distance", "fixed"])
assert freed_catalog["sketches"][0]["constraint_edit"]["curves"] == curves

# 7. Redundancy and conflict are the real solver's answers, not structural ones.
twice = root / "twice.fcad"
r = edit(source, catalog, {"request_version": 1, "remove": [], "add": add + [
    {"curve_id": curves[1]["curve_id"], "rule": "distance", "distance_mm": 60},
    {"rule": "equal_length", "a_curve_id": curves[0]["curve_id"],
     "b_curve_id": curves[1]["curve_id"]}]}, twice)
said_twice = next(c["constraint_id"] for c in r["added_constraints"]
                  if c["rule"]["kind"] == "equal_length")
assert r["solve"]["redundant_constraint_ids"] == [said_twice]
error = refused_edit(source, catalog, {"request_version": 1, "remove": [], "add": add + [
    {"curve_id": curves[1]["curve_id"], "rule": "distance", "distance_mm": 30},
    {"rule": "equal_length", "a_curve_id": curves[0]["curve_id"],
     "b_curve_id": curves[1]["curve_id"]}]}, root / "conflict.fcad")
assert error["kind"] == "constraint", error
conflict = error["constraint_conflict"]["constraints"]
assert any(c["rule"]["kind"] == "equal_length" for c in conflict)
assert all(isinstance(c["constraint_id"], str) for c in conflict)

assert (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns) == before
print(json.dumps({"artifacts": str(root), "equal_length_id": equal_id,
                  "square_mm3": 36000, "smaller_mm3": volume,
                  "dof": {"one_size": 1, "square": 0, "smaller": 0, "freed": 1},
                  "redundant": said_twice}, ensure_ascii=False))
```

`solve.degrees_of_freedom` — измеренный результат одного cold rebuild
опубликованной копии, а не обещание. Отдельные inspect/export читают отдельные
snapshots. Constraint-copy защищён полным `--expect-version`; экспорты такого
guard не имеют. При отказе/конфликте новой копии нет. Exit 7 после publish
оставляет копию целой, повтор запрещён без проверки назначения. UI собирает тот
же ordered request до публикации; отдельных CLI-команд для UI-жестов нет.
