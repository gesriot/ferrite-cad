# §25I — относительная ориентация двух Lines

[Точный request/result и UI-контракт](sketch-constraints-copy.md),
[проверки и ограничения](line-pair-orientation-verification.md).
Parallel и Perpendicular — **постоянная связь двух целых Lines** одного
собственного XY профиля: существующие `SketchConstraintRule::Parallel { a, b }`
и `SketchConstraintRule::Perpendicular { a, b }` с двумя `SketchSegmentRef`
Start→End, существующий перевод в solver и прежняя операция
`edit-sketch-constraints-copy`. Новой команды, writer, rebuild, publish, схемы
БД или FFI здесь нет.

Чем это **не** является:

* не Horizontal/Vertical — связь ничего не говорит о собственном направлении
  ни одной из сторон и потому держится на наклонном профиле, который при этом
  остаётся свободным вращаться;
* не записью решённых координат обратно в исходный Sketch — stored координаты
  остаются inputs solver при любой публикации;
* не вторым вычисленным размером — у связи нет числа, и она не подставляет
  угол в градусах.

Поэтому изменение **ведущей** длины меняет размер, но не форму: связи остаются
в силе, и их UUID переживают замену `Distance`.

Структурная политика пары:

* обе стороны — Start/End целой Line выбранного Sketch, и обе проверяются на
  принадлежность выбранному Sketch;
* для занятости слота `(A,B)` и `(B,A)` — одна и та же пара; сохранённый
  `SegmentRef`, записанный End→Start, читается как та же Line и **не**
  переписывается канонизацией;
* **одна относительная ориентация на пару**: Parallel и Perpendicular — два
  ответа на один вопрос об этой паре и занимают один слот. Повторный тот же
  ответ, другой ответ и reversed-дубликат любого из них — структурный отказ
  до solver, без публикации;
* `EqualLength` на той же паре — **другое свойство** тех же двух Lines,
  собственный слот и собственный UUID; оно не занимает и не освобождает слот
  относительной ориентации;
* remove старого UUID + add той же пары (в том числе другого ответа) в одном
  запросе допустим: проверка выполняется по retained состоянию;
* избыточность и противоречие — не структурные категории: их устанавливает
  настоящий solver (`redundant_constraint_ids` либо typed
  `constraint_conflict`). Анализа транзитивности, ранга и косвенных конфликтов
  здесь нет.

Первый сценарий среза — **прямоугольник, собранный из одних отношений**:
замкнутый профиль из четырёх Lines имеет восемь плоских степеней свободы;
`Parallel(L0,L2)`, `Parallel(L1,L3)` и `Perpendicular(L0,L1)` задают форму
прямоугольника (три независимых условия), две длины 60 и 30 mm задают размер, и
остаётся ровно **DOF 3** — две степени положения и одна поворота. Solver
подтверждает и счёт, и независимость: `redundant_constraint_ids` пуст. Профиль
остаётся наклонным, экструзия 10 mm даёт 18000 mm³. Замена ведущей длины 60→45
сохраняет все связи и их UUID и даёт 13500 mm³. Удаление exact UUID
`Perpendicular` возвращает **DOF 4** и сохраняет оба `Parallel` и обе длины;
куда при этом встанет освобождённый профиль — дело solver, и здесь не
обещается.

Граница свободных степеней и координат: `solve.degrees_of_freedom` — измеренный
результат одного cold rebuild опубликованной копии. Stored координаты остаются
входом solver и не перезаписываются решением, поэтому при ненулевом DOF
конкретные XY и axis-aligned extents не являются контрактом: контракт — длины,
нормированные cross/dot выбранных сторон, площадь и объём.

Ограничения среза: нынешний managed собственный XY Line polygon, один positive
literal Blind/NewBody, прежние H/V + Line length + один Fixed + EqualLength +
adjacent Coincident closure. Нет окружностей, дуг, отверстий, углов в градусах,
Tangent, Symmetric, live solver drag, in-place Save, новой схемы и FFI.
Coordinate drag и Snap constrained Sketch по-прежнему недоступны.

Ниже запускаемый рецепт через публичный CLI: create → относительный
прямоугольник → независимый STL/FBX → замена ведущей длины → удаление →
отказы → настоящие redundant и conflict. Нужны OCCT + PlaneGCS, `FERRITECAD` и
`FCAD_UFBX_READER` — путь к существующему pinned reader, собранному из
`tools/unity-fbx-smoke/scripts/read_production.c` с pin из `fetch_ufbx.sh`. На
macOS native library paths экспортируются в том же Bash. Рецепт создаёт свой
временный каталог, не меняет fixtures и не требует UI или заранее известных
UUID.

```python
# FCAD_25I_AGENT_RECIPE
import hashlib, json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-line-relations-"))
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
def footprint(path):
    """Independent STL integration plus the pinned ufbx reader.

    The solved profile is free to sit and to turn, so the footprint is measured
    as a polygon — side lengths and normalised cross/dot between its edges —
    and never as axis-aligned extents."""
    stl = path.with_suffix(".stl")
    body = run("inspect", path)["bodies"][0]["body_id"]
    report = run("export-stl", path, "--solid", body, "-o", stl)
    data = stl.read_bytes()
    n = struct.unpack_from("<I", data, 80)[0]
    assert n == report["triangles"] and len(data) == report["bytes"] == 84 + 50 * n
    base, volume6 = set(), 0.0
    for i in range(n):
        f = struct.unpack_from("<12fH", data, 84 + 50 * i)
        a, b, c = f[3:6], f[6:9], f[9:12]
        for p in (a, b, c):
            if abs(p[2]) < 1e-6:
                base.add((round(p[0], 6), round(p[1], 6)))
        volume6 += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2])
                    + a[2]*(b[0]*c[1]-b[1]*c[0]))
    assert len(base) == 4, base
    cx = sum(p[0] for p in base) / 4
    cy = sum(p[1] for p in base) / 4
    ring = sorted(base, key=lambda p: math.atan2(p[1] - cy, p[0] - cx))
    edges = []
    for i, p in enumerate(ring):
        q = ring[(i + 1) % 4]
        d = (q[0] - p[0], q[1] - p[1])
        length = math.hypot(*d)
        assert length > 1, ("degenerate side", d)
        edges.append(((d[0] / length, d[1] / length), length))
    fbx = path.with_suffix(".fbx")
    run("export-fbx", path, "-o", fbx)
    subprocess.run([reader, "--identity", str(fbx)], check=True)  # independent FBX reader
    return edges, abs(volume6) / 6
def rectangle(edges, long, short):
    """Opposite sides parallel, corners square, both sizes, and still turned."""
    for i, (u, length) in enumerate(edges):
        v = edges[(i + 2) % 4][0]
        w = edges[(i + 1) % 4][0]
        assert abs(u[0]*v[1] - u[1]*v[0]) < 1e-6, ("not parallel", u, v)
        assert abs(u[0]*w[0] + u[1]*w[1]) < 1e-6, ("not perpendicular", u, w)
        assert abs(u[0]) > 0.01 and abs(u[1]) > 0.01, ("solved onto an axis", u)
    sides = sorted(round(length, 6) for _, length in edges)
    assert all(math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-5)
               for a, b in zip(sides, [short, short, long, long])), sides

source = root / "slanted.fcad"
profile = root / "profile.json"
profile.write_text(json.dumps({"request_version": 1,
    "points_mm": [[-20,-10],[40,-8],[42,30],[-20,30]], "height_mm": 10}))
run("create-sketch-extrude", profile, "-o", source)
before = (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns)
catalog = run("inspect", source)
sketch = catalog["sketches"][0]
curves = [c["curve_id"] for c in sketch["constraint_edit"]["curves"]]
assert sketch["constraint_edit"]["available"] and len(curves) == 4
assert not sketch["constraint_edit"]["constraints"]
def pair(rule, i, j):
    return {"rule": rule, "a_curve_id": curves[i], "b_curve_id": curves[j]}
def size(i, mm):
    return {"curve_id": curves[i], "rule": "distance", "distance_mm": mm}

# 1. A rectangle asked for as relationships. Eight planar degrees of freedom of
#    a closed quadrilateral, minus three for the shape and two for the size,
#    leave two of position and one of rotation; the solver finds nothing
#    redundant, which is what makes those five independent.
add = [pair("parallel", 0, 2), pair("parallel", 1, 3), pair("perpendicular", 0, 1),
       size(0, 60), size(1, 30)]
rect = root / "rectangle.fcad"
published = edit(source, catalog, {"request_version": 1, "remove": [], "add": add}, rect)
assert published["solve"]["degrees_of_freedom"] == 3, "position and rotation are free"
assert published["solve"]["redundant_constraint_ids"] == []
run("validate", rect)
added = published["added_constraints"]
assert [c["rule"]["kind"] for c in added] == ["coincident"] * 4 + [
    "parallel", "parallel", "perpendicular", "distance", "distance"]
relations = {}
for c, (i, j) in zip(added[4:7], [(0, 2), (1, 3), (0, 1)]):
    rule = c["rule"]
    for side, k in (("a", i), ("b", j)):
        assert rule[side] == {"from": {"curve_id": curves[k], "at": "start"},
                              "to": {"curve_id": curves[k], "at": "end"}}
    assert "a_curve_id" not in rule and "distance" not in rule  # response wire unchanged
    relations[(rule["kind"], i, j)] = c["constraint_id"]
perpendicular_id = relations[("perpendicular", 0, 1)]
length_id = added[7]["constraint_id"]
rect_catalog = run("inspect", rect)
rect_ids = ids(rect_catalog)
assert len(rect_ids) == 9
assert rect_catalog["sketches"][0]["constraint_edit"]["curves"] == \
    sketch["constraint_edit"]["curves"]  # stored inputs, not the solve
edges, volume = footprint(rect)
rectangle(edges, 60, 30)
assert math.isclose(volume, 18000, rel_tol=1e-6), volume

# 2. Structural refusals: a pair is two different Lines of this Sketch, and it
#    answers the orientation question once.
for bad in [pair("parallel", 0, 0), pair("perpendicular", 2, 2),
            {"rule": "parallel", "a_curve_id": curves[0],
             "b_curve_id": "00000000-0000-7000-8000-000000000000"},
            pair("parallel", 0, 2), pair("parallel", 2, 0),
            pair("perpendicular", 0, 2), pair("perpendicular", 2, 0)]:
    error = refused_edit(rect, rect_catalog, {"request_version": 1, "remove": [],
                                              "add": [bad]}, root / "never.fcad")
    assert error["kind"] == "input" and "constraint_conflict" not in error, error
for both in [[pair("parallel", 1, 2), pair("perpendicular", 2, 1)],
             [pair("perpendicular", 1, 2), pair("perpendicular", 2, 1)]]:
    error = refused_edit(rect, rect_catalog,
                         {"request_version": 1, "remove": [], "add": both},
                         root / "never.fcad")
    assert error["kind"] == "input", error

# 3. An equal length on a pair that already holds an orientation is a different
#    property of the same two Lines, so it gets its own slot and its own UUID.
also = root / "also-equal.fcad"
r = edit(rect, rect_catalog, {"request_version": 1, "remove": [], "add": [
    {"rule": "equal_length", "a_curve_id": curves[0], "b_curve_id": curves[2]}]}, also)
assert [c["rule"]["kind"] for c in r["added_constraints"]] == ["equal_length"]
assert ids(run("inspect", also)) == rect_ids + [r["added_constraints"][0]["constraint_id"]]

# 4. Changing the leading length changes the size, not the shape, and every
#    relation keeps its own UUID.
smaller = root / "smaller.fcad"
r = edit(rect, rect_catalog, {"request_version": 1, "remove": [length_id],
                              "add": [size(0, 45)]}, smaller)
assert r["removed_constraint_ids"] == [length_id]
assert r["solve"]["degrees_of_freedom"] == 3
smaller_catalog = run("inspect", smaller)
smaller_ids = ids(smaller_catalog)
assert all(i in smaller_ids for i in relations.values())
assert length_id not in smaller_ids and len(smaller_ids) == 9
kept = [c for c in rect_catalog["sketches"][0]["constraint_edit"]["constraints"]
        if c["constraint_id"] != length_id]
assert smaller_catalog["sketches"][0]["constraint_edit"]["constraints"] == \
    kept + r["added_constraints"], "every other UUID, rule and order survives"
edges, volume = footprint(smaller)
rectangle(edges, 45, 30)
assert math.isclose(volume, 13500, rel_tol=1e-6), volume

# 5. Removing the exact Perpendicular gives one degree of freedom back and
#    leaves the rest standing. Where the freed profile lands is not promised.
freed = root / "freed.fcad"
r = edit(smaller, smaller_catalog, {"request_version": 1, "remove": [perpendicular_id],
                                    "add": []}, freed)
assert r["added_constraints"] == [] and r["solve"]["degrees_of_freedom"] == 4
run("validate", freed)
freed_catalog = run("inspect", freed)
assert ids(freed_catalog) == [i for i in smaller_ids if i != perpendicular_id]
kinds = [c["rule"]["kind"]
         for c in freed_catalog["sketches"][0]["constraint_edit"]["constraints"]]
assert sorted(kinds) == sorted(["coincident"] * 4 + ["parallel"] * 2 + ["distance"] * 2)
assert freed_catalog["sketches"][0]["constraint_edit"]["curves"] == \
    sketch["constraint_edit"]["curves"]

# 6. Redundancy and conflict are the real solver's answers, not structural ones.
twice = root / "twice.fcad"
r = edit(source, catalog, {"request_version": 1, "remove": [],
                           "add": add + [pair("perpendicular", 1, 2)]}, twice)
fourth = [c["constraint_id"] for c in r["added_constraints"]
          if c["rule"]["kind"] == "perpendicular"][1]
assert r["solve"]["redundant_constraint_ids"] == [fourth]
assert r["solve"]["degrees_of_freedom"] == 3
error = refused_edit(source, catalog, {"request_version": 1, "remove": [],
                                       "add": add + [pair("parallel", 1, 2)]},
                     root / "conflict.fcad")
assert error["kind"] == "constraint", error
conflict = error["constraint_conflict"]["constraints"]
assert any(c["rule"]["kind"] == "parallel" for c in conflict)
assert any(c["rule"]["kind"] == "perpendicular" for c in conflict)
assert all(isinstance(c["constraint_id"], str) for c in conflict)

assert (hashlib.sha256(source.read_bytes()).hexdigest(), source.stat().st_mtime_ns) == before
print(json.dumps({"artifacts": str(root), "relations": {f"{k}": v
                  for k, v in relations.items()},
                  "rect_mm3": 18000, "smaller_mm3": volume,
                  "dof": {"rectangle": 3, "smaller": 3, "freed": 4},
                  "redundant": fourth}, ensure_ascii=False))
```

`solve.degrees_of_freedom` — измеренный результат одного cold rebuild
опубликованной копии, а не обещание. Отдельные inspect/export читают отдельные
snapshots. Constraint-copy защищён полным `--expect-version`; экспорты такого
guard не имеют. При отказе/конфликте новой копии нет. Exit 7 после publish
оставляет копию целой, повтор запрещён без проверки назначения. UI собирает тот
же ordered request до публикации; отдельных CLI-команд для UI-жестов нет.
