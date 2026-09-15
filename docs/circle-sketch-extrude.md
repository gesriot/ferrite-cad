# §25J — аналитическая окружность → Sketch → Blind Extrude → новый FCAD

[Локальные доказательства и ограничения](circle-sketch-extrude-verification.md).
Тот же общий маршрут создания, что и у [Line polygon (§25A)](sketch-extrude-create.md):
один unconstrained XY Sketch, один Blind Extrude, один NewBody, атомарная
публикация после cold rebuild. Отличается только то, что Sketch держит: одну
настоящую `SketchGeometry::Circle` вместо цепочки Line.

UI: `Create sketch + Extrude…` → `Profile: Circle`; CLI:

```text
ferritecad create-circle-extrude <request.json> -o <new.fcad> [--json]
```

## Что здесь аналитическое, а что нет

Окружность **не** заменяется многоугольником ни в документе, ни в B-Rep:

* документ хранит `Circle { center, radius }` с собственным `StableEntityId`;
* kernel получает `SegmentGeometry::Circle` — **замкнутую кривую**, которая
  является целым loop сама по себе. У неё нет endpoints, поэтому у профиля нет
  ни одного угла: `ProfileLoop::closed_curve` даёт `joints()` длины 0, и никакой
  `ProfileJoint(id, id)`, фиктивный curve UUID или паника не возникают;
* OCCT строит одно ребро `gp_Circ`, одну **цилиндрическую** поверхность и два
  **плоских** cap. Шов (seam), который OCCT ставит на замкнутом ребре, —
  его собственная параметризация, а не вершина чертежа, и наружу не сообщается;
* экспортированный STL/FBX — **тесселяция**, вписанный призматический
  приближённый вид этой поверхности. Объём меша всегда меньше аналитического;
  точное π по мешу не обещается и не проверяется.

## Вход

Request — UTF-8 JSON, до 65536 bytes, все четыре поля обязательны. Неизвестные
поля, неизвестная версия, лишние компоненты центра, null и неправильные типы
отказываются. JSON не допускает NaN/Infinity; общий API также проверяет
конечность после декодирования.

```json
{"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}
```

- `schema_version`: целое число 1. **Именно это написание** — оно принадлежит
  контракту этой команды; `request_version` соседнего polygon-запроса здесь
  лишнее поле и отказывается, как и наоборот.
- `center_mm`: ровно две конечные координаты `[x, y]` в mm. Ноль и
  отрицательные допустимы.
- `radius_mm`: конечное **строго положительное** число в mm.
- `height_mm`: конечная положительная Blind-высота по существующему
  `ExtrudeExtent::blind`.
- Абсолютный центр, радиус и высота ограничены 1 000 000 mm в этом редакторе —
  тот же bound, что у polygon, и не ограничение ядра.

Отверстий, нескольких контуров, смешанных Circle/Line профилей, circle
constraints, правки радиуса сохранённой окружности, рисования мышью, booleans и
in-place Save в этом срезе нет. Сохранённая окружность **не** становится
доступной прежним Line-редакторам: `Edit Sketch`, coordinate drag, Snap и
`Edit constraints` по-прежнему требуют 3..256 Line-сегментов и отказывают с
прежними сообщениями.

## Владельцы, сохранение и публикация

`CircleExtrusion::new` в document — единственная проверка допустимости для обоих
клиентов. `NewDocument::CircleExtrude(CircleExtrusion)` проходит через тот же
`create_document_with_kernel`. Классификация одна:
`NewDocument::needs_kernel()` истинно для любого нарисованного профиля, поэтому
kernel-free `create_document` отказывает и circle, и polygon, а checked route
строит и проверяет тело **до** публикации. Empty и sample plate по-прежнему
создаются без ядра.

В одном scratch Document/transaction записываются DatumPlane XY → Sketch с одной
Circle → Blind Extrude → Body, три dependency и три topology references: start
cap, end cap и `ExtrudeSide { profile_segment }` с
`AllDerivedFrom { ancestor }` — обе ссылки называют **UUID самой окружности**, а
не индекс грани и не порядок тесселяции. После cold reopen все три разрешаются.

Прежний `edit-extrude` меняет только высоту такого цилиндра через существующий
copy job: Circle, её UUID, центр, радиус, все прочие UUID, dependencies и
SQL-данные сохраняются, меняется одно выражение высоты.

## JSON v1

Успех — обычный envelope: `schema_version` 1, `operation`
`create-circle-extrude`, `ok` true, `result` с `destination` и `document_id`,
один объект + LF, exit 0. Отказ — `ok` false, `kind`/`message`/`causes`, exit 2,
без публикации и без scratch. Потеря stdout после публикации — exit 7:
документ остаётся целым, повторять нельзя без проверки назначения. Usage/help
остаются clap-текстом. Новое назначение не перезаписывает занятый путь,
request-файл или его hard/symlink alias.

## Рецепт

Ниже запускаемый рецепт через публичный CLI: JSON create → inspect → validate →
cold rebuild → STL/FBX → правка высоты → повторные экспорты. Нужны OCCT,
`FERRITECAD` и `FCAD_UFBX_READER` — путь к существующему pinned reader,
собранному из `tools/unity-fbx-smoke/scripts/read_production.c` с pin из
`fetch_ufbx.sh`. На macOS native library paths экспортируются в том же Bash.
Рецепт создаёт свой временный каталог, не меняет fixtures и не требует UI или
заранее известных UUID.

```python
# FCAD_25J_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-circle-"))
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
def refused(operation, *args):
    p = subprocess.run([cli, operation, *map(str, args), "--json"], capture_output=True)
    assert p.returncode == 2, p
    data = json.loads(p.stdout)
    assert data["ok"] is False
    return data["error"]
def measured(path, radius, height, center):
    """Independent STL integration plus the pinned ufbx reader.

    A tessellated cylinder is an inscribed prism: its volume is below the
    analytic one and its bound box touches it only at the facet corners. The
    bound below uses an explicit chord error, while the perimeter is measured
    from actual vertices. Cap triangle counts do not define perimeter facets;
    nothing here claims exact pi from a mesh."""
    stl = path.with_suffix(".stl")
    body = run("inspect", path)["bodies"][0]["body_id"]
    linear, angular = 0.05, 0.1  # mm and radians, explicitly requested
    report = run("export-stl", path, "--solid", body, "-o", stl,
                 "--linear-deflection", linear, "--angular-deflection", angular)
    data = stl.read_bytes()
    n = struct.unpack_from("<I", data, 80)[0]
    assert n == report["triangles"] and len(data) == report["bytes"] == 84 + 50 * n
    points, volume6 = [], 0.0
    for i in range(n):
        f = struct.unpack_from("<12fH", data, 84 + 50 * i)
        a, b, c = f[3:6], f[6:9], f[9:12]
        assert all(math.isfinite(v) for point in (a, b, c) for v in point)
        points.extend((a, b, c))
        volume6 += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2])
                    + a[2]*(b[0]*c[1]-b[1]*c[0]))
    analytic = math.pi * radius ** 2 * height
    # Every chord within e of a circle contains the disk r-e on its inner
    # side. Independently measure the actual perimeter and its chord errors.
    inside = math.pi * max(0, radius - linear) ** 2 * height
    mesh = abs(volume6) / 6
    assert inside * (1 - 1e-6) <= mesh <= analytic * (1 + 1e-6)
    angles = sorted(set(math.atan2(p[1] - center[1], p[0] - center[0]) % (2 * math.pi)
                        for p in points if abs(math.hypot(p[0] - center[0], p[1] - center[1]) - radius) < 1e-4))
    assert len(angles) >= 3
    gaps = [b-a for a, b in zip(angles, angles[1:] + [angles[0] + 2*math.pi])]
    assert all(gap < math.pi and radius * (1 - math.cos(gap/2)) <= linear + 1e-4 for gap in gaps)
    perimeter_volume = radius ** 2 / 2 * sum(map(math.sin, gaps)) * height
    assert abs(mesh - perimeter_volume) <= analytic * 1e-5
    for j, (middle, half) in enumerate(((center[0], radius), (center[1], radius),
                                        (height / 2.0, height / 2.0))):
        lo = min(p[j] for p in points)
        hi = max(p[j] for p in points)
        assert middle - half - 1e-4 <= lo and hi <= middle + half + 1e-4, (j, lo, hi)
        assert hi - lo >= 2 * half - 2 * linear - 1e-4, (j, hi - lo)
    fbx = path.with_suffix(".fbx")
    run("export-fbx", path, "-o", fbx)
    subprocess.run([reader, "--identity", str(fbx)], check=True)  # independent FBX reader
    return {"triangles": n, "mesh_mm3": mesh, "analytic_mm3": analytic}

# 1. One circle, by centre and radius, published only after it was built.
request = root / "request.json"
request.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                               "radius_mm": 10.0, "height_mm": 15.0}))
cylinder = root / "cylinder.fcad"
created = run("create-circle-extrude", request, "-o", cylinder)
assert created["destination"] == str(cylinder)
run("validate", cylinder)
catalog = run("inspect", cylinder)
assert created["document_id"] == catalog["document_id"]
assert len(catalog["bodies"]) == 1
# The Line editors do not quietly accept a circle.
sketch = catalog["sketches"][0]
assert sketch["editable"] is False and sketch["vertices"] is None
assert sketch["constraint_edit"]["available"] is False

# 2. A cold rebuild of the published document, and the mesh read independently.
assert subprocess.run([cli, "rebuild", str(cylinder), "--cold"]).returncode == 0
first = measured(cylinder, 10.0, 15.0, (12.0, -7.0))

# 3. The same document, taller, through the existing height edit.
feature = catalog["features"][0]
assert feature["editable"] and feature["distance_mm"] == 15.0
extrude = feature["feature_id"]
taller = root / "taller.fcad"
run("edit-extrude", cylinder, "--feature", extrude, "--distance-mm", 25, "-o", taller)
run("validate", taller)
second = measured(taller, 10.0, 25.0, (12.0, -7.0))
assert math.isclose(second["analytic_mm3"] / first["analytic_mm3"], 25 / 15, rel_tol=1e-12)

# 4. Refusals publish nothing and never replace a destination.
for bad in [{"schema_version": 2, "center_mm": [0, 0], "radius_mm": 1, "height_mm": 1},
            {"schema_version": 1, "center_mm": [0, 0], "radius_mm": 0, "height_mm": 1},
            {"schema_version": 1, "center_mm": [0, 0], "radius_mm": 1, "height_mm": -1},
            {"schema_version": 1, "center_mm": [0], "radius_mm": 1, "height_mm": 1},
            {"schema_version": 1, "center_mm": [0, 0], "radius_mm": 1, "height_mm": 1,
             "points_mm": []}]:
    request.write_text(json.dumps(bad))
    error = refused("create-circle-extrude", request, "-o", root / "never.fcad")
    assert error["kind"] in ("input", "unsupported"), error
    assert not (root / "never.fcad").exists()
request.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                               "radius_mm": 10.0, "height_mm": 15.0}))
kept = cylinder.read_bytes()
refused("create-circle-extrude", request, "-o", cylinder)
assert cylinder.read_bytes() == kept

print(json.dumps({"artifacts": str(root), "cylinder": first, "taller": second},
                 ensure_ascii=False))
```

`analytic_mm3` — это π r² h, вычисленное рецептом; величина, измеренная по мешу,
названа `mesh_mm3` и всегда меньше. Настоящий аналитический объём B-Rep измеряют
native gates, а не этот рецепт. Отдельные inspect/export читают отдельные
snapshots. При отказе новой копии нет. Exit 7 после publish оставляет документ
целым, повтор запрещён без проверки назначения.
