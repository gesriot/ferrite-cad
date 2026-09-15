# §25K — правка сохранённой окружности в новой копии

[Локальные доказательства и ограничения](edit-circle-copy-verification.md).
Тот же identity-preserving copy-маршрут, что у
[координатной правки Line Sketch (§25B)](edit-sketch-copy.md): один снимок,
baseline cold rebuild, проверка refs, повторная проверка версии источника,
закрытие SQLite и атомарная no-clobber публикация. Отличается только
подготовленный payload: центр и радиус одной сохранённой аналитической
окружности, созданной [§25J](circle-sketch-extrude.md).

UI: `Edit circle <имя> — <UUID>…`; CLI:

```text
ferritecad edit-circle <source.fcad> --sketch UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
```

## Что меняется и что нет

Меняются **только** центр и радиус названной окружности: в SQL это payload и
payload_hash выбранной строки `objects`, плюс `meta.modified_at` новой копии.
Сохраняются document UUID, все object UUID/порядок/имена/parents, **UUID самой
окружности**, `plane` и остальные поля Sketch, высота и выражение Extrude, Body,
dependencies, topology refs и все прочие SQL-ячейки, включая посторонние
таблицы, capability rows с их rowid и source claims. Source остаётся побайтово
цел; при любом отказе новой копии нет.

Высота **не** входит в этот запрос. Её по-прежнему меняет `edit-extrude`, и
после правки окружности он работает как раньше, не теряя identity окружности.

## Поддерживаемый класс

Ровно класс §25J: один untransformed XY DatumPlane, один Sketch, один forward
literal Blind `Extrude`/`NewBody` и его `Body`, ровно три dependencies
(plane/profile/body-tip), и Sketch, в котором **ровно одна неконструкционная
неограниченная `Circle`** внутри той же политики `CircleExtrusion`, что
применяется к новым числам.

Отказываются: Line и Arc, несколько или смешанные кривые, конструкционная
геометрия, любые constraints, параметризованный или иначе неподдерживаемый
Extrude, другие плоскости и связи, и payload, который этот reader не может
переписать без потерь (неизвестные поля и неканонические envelope сохраняются
отказом, а не тихим отбрасыванием). Окружностных constraints, отверстий,
смешанных профилей, рисования и drag мышью, booleans, in-place Save и новой
схемы БД здесь нет; контракты Line-редакторов не ослаблены.

## Работа в UI

Форма показывает сохранённые Sketch/Circle UUID, центр, радиус и неизменяемую
высоту. Введите Center X/Y и Radius, затем нажмите **Apply circle change**:
одно подтверждение всех трёх полей создаёт один шаг Undo/Redo, максимум 128.
Промежуточный ввод (`-`, пустое поле) не попадает в историю; эквивалентное
написание того же числа не создаёт нового шага. Save доступен после Apply.
Пока worker занят, поля и действия формы недоступны.

Save Cancel, отказ операции и устаревший ответ сохраняют черновик. После
публикации открывается новая копия; до принятия сцены сохраняется один
резервный черновик. Если загрузка или подготовка сцены откажет, форма и её
история восстанавливаются, опубликованный файл остаётся на диске. Успешное
принятие сцены завершает редактирование. Это поведение проверено headless;
интерактивный GUI smoke отложен.

## Discovery

`inspect --json` получает у каждой строки `sketches` отдельное поле
`circle_edit`, не меняя смысла прежних `editable`, `vertices` и
`constraint_edit` — они по-прежнему отвечают про Line-редакторы.

```json
"circle_edit": {
  "available": true,
  "refusal": null,
  "document_refusal": null,
  "circle": {"curve_id": "UUID", "center_mm": [12.0, -7.0],
             "radius_mm": 10.0, "height_mm": 15.0}
}
```

* `available`, `refusal`, `document_refusal` и `circle` — **required** поля;
  `refusal`/`document_refusal`/`circle` могут быть `null`.
* `available` — boolean: true только когда нет ни документного, ни локального
  отказа. Документный отказ (read-only копия, триггеры, неизвестные
  capabilities) сохраняет приоритет и повторяется в `document_refusal`.
* `circle` присутствует для поддержанного Sketch и `null` иначе — никогда не
  придуманная окружность. Для неподдержанного Sketch структура может
  сообщаться при документном отказе: редактирование отказано, факты — нет.
* `height_mm` сообщается потому, что решает смысл чисел, а не потому, что этот
  запрос может её изменить.
* Неизвестные поля ответа следует игнорировать; это discovery одной операции,
  а не модель документа. Discovery работает и в сборке без ядра — доступность
  по структуре не обещает установленный kernel, и применение правки без ядра
  отказывается.

## Request v1

Строгий, до 65536 байт, ровно четыре поля, `deny_unknown_fields`:

```json
{"request_version":1,"curve_id":"UUID","center_mm":[12.0,-7.0],"radius_mm":10.0}
```

* `request_version` — целое 1. **Единственное** написание версии: `schema_version`
  запроса создания здесь лишнее поле и отвергается.
* `curve_id` — canonical UUIDv7 **сохранённой** окружности. Чужой UUID
  отвергается; форма UI подставляет сохранённую identity, а не текст из поля.
* `center_mm` — ровно две конечные координаты в mm; ноль и отрицательные
  допустимы.
* `radius_mm` — конечное строго положительное число в mm.
* `height_mm`, `vertices` и любые другие поля здесь лишние. Центр, радиус и
  сохранённая высота проверяются той же `CircleExtrusion` policy, что и
  создание, включая предел 1 000 000 mm.

## JSON v1

Успех — прежний envelope: `schema_version` 1, `operation` `edit-circle`,
`ok` true, `result` с `destination`, `document_id`, `sketch_id` и `curve_id`,
один объект + LF, exit 0. Отказ — `ok` false, `kind`/`message`/`causes`, exit 2,
без публикации и без scratch. Потеря stdout после публикации — exit 7:
копия цела, повторять операцию нельзя без проверки назначения. UTF-8 paths
обязательны в JSON-режиме; usage/help остаются clap-текстом. Text и JSON идут
через один job.

## Рецепт

Нужны OCCT, `FERRITECAD` и `FCAD_UFBX_READER` — путь к pinned reader,
собранному из `tools/unity-fbx-smoke/scripts/read_production.c` с pin из
`fetch_ufbx.sh`. На macOS native library paths экспортируются в том же Bash.

```python
# FCAD_25K_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-edit-circle-"))
DEFLECTION = 0.01
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
def measured(path, center, radius, height):
    """Independent STL parsing at an explicit deflection, then the pinned reader.

    The base ring is measured as the polygon it is: its own distinct vertices,
    its own perimeter and its own bound box. Nothing divides a triangle count
    to guess how many sides the exporter chose, and no exact pi is expected."""
    stl = path.with_suffix(".stl")
    body = run("inspect", path)["bodies"][0]["body_id"]
    report = run("export-stl", path, "--solid", body,
                 "--linear-deflection", DEFLECTION, "-o", stl)
    data = stl.read_bytes()
    n = struct.unpack_from("<I", data, 80)[0]
    assert n == report["triangles"] and len(data) == report["bytes"] == 84 + 50 * n
    lo = [float("inf")] * 3
    hi = [float("-inf")] * 3
    base = []
    for i in range(n):
        f = struct.unpack_from("<12fH", data, 84 + 50 * i)
        for p in (f[3:6], f[6:9], f[9:12]):
            assert all(math.isfinite(x) for x in p)
            for j in range(3):
                lo[j] = min(lo[j], p[j])
                hi[j] = max(hi[j], p[j])
            if abs(p[2]) < 1e-6 and not any(
                    math.hypot(q[0] - p[0], q[1] - p[1]) < 1e-6 for q in base):
                base.append((p[0], p[1]))
    assert len(base) >= 3
    cx = sum(p[0] for p in base) / len(base)
    cy = sum(p[1] for p in base) / len(base)
    ring = sorted(base, key=lambda p: math.atan2(p[1] - cy, p[0] - cx))
    perimeter = sum(math.hypot(ring[(i + 1) % len(ring)][0] - p[0],
                               ring[(i + 1) % len(ring)][1] - p[1])
                    for i, p in enumerate(ring))
    # Bound each actual chord; many short chords cannot excuse one large gap.
    angles = []
    for x, y in ring:
        dx, dy = x - center[0], y - center[1]
        assert abs(math.hypot(dx, dy) - radius) < 1e-4
        angles.append(math.atan2(dy, dx))
    angles.sort()
    for i, angle in enumerate(angles):
        next_angle = angles[i+1] if i+1 < len(angles) else angles[0] + 2*math.pi
        gap = next_angle - angle
        assert 0 < gap < math.pi
        assert radius * (1 - math.cos(gap/2)) <= DEFLECTION + 1e-4
    exact = 2 * math.pi * radius
    assert exact * (1 - 1e-3) < perimeter <= exact, (perimeter, exact)
    for j, (middle, half) in enumerate(((center[0], radius), (center[1], radius),
                                        (height / 2, height / 2))):
        assert middle - half - 1e-4 <= lo[j] and hi[j] <= middle + half + 1e-4, (j, lo[j], hi[j])
        assert hi[j] - lo[j] > 2 * half - 0.05, (j, hi[j] - lo[j])
    fbx = path.with_suffix(".fbx")
    run("export-fbx", path, "-o", fbx)
    subprocess.run([reader, "--identity", str(fbx)], check=True)  # independent FBX reader
    return {"sides": len(ring), "perimeter_mm": perimeter,
            "analytic_mm3": math.pi * radius ** 2 * height}

# 1. A document with one saved analytic circle, made the one way this makes one.
create = root / "create.json"
create.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                              "radius_mm": 10.0, "height_mm": 15.0}))
source = root / "source.fcad"
run("create-circle-extrude", create, "-o", source)
kept = source.read_bytes()

# 2. The identities and the version come from one JSON reading; no SQLite, no
#    prose, and no guessing which circle is meant.
catalog = run("inspect", source)
sketch = catalog["sketches"][0]
discovery = sketch["circle_edit"]
assert discovery["available"] is True and discovery["refusal"] is None
saved = discovery["circle"]
assert saved["center_mm"] == [12.0, -7.0] and saved["radius_mm"] == 10.0
assert saved["height_mm"] == 15.0
# The Line editors still answer for themselves, and still refuse this Sketch.
assert sketch["editable"] is False and sketch["vertices"] is None
assert sketch["constraint_edit"]["available"] is False

# 3. Move and resize that circle into a new copy.
request = root / "edit.json"
request.write_text(json.dumps({"request_version": 1, "curve_id": saved["curve_id"],
                               "center_mm": [-3.5, 4.25], "radius_mm": 6.75}))
copy = root / "edited.fcad"
published = run("edit-circle", source, "--sketch", sketch["sketch_id"],
                "--expect-version", catalog["content_version"],
                "--request", request, "-o", copy)
assert published["destination"] == str(copy)
assert published["curve_id"] == saved["curve_id"], "the circle keeps its UUID"
assert published["sketch_id"] == sketch["sketch_id"]
assert published["document_id"] == catalog["document_id"]

# 4. Reopen it: same identities, new numbers, the height untouched.
run("validate", copy)
after = run("inspect", copy)["sketches"][0]["circle_edit"]["circle"]
assert after["curve_id"] == saved["curve_id"]
assert after["center_mm"] == [-3.5, 4.25] and after["radius_mm"] == 6.75
assert after["height_mm"] == 15.0, "this edit carries no height"
assert subprocess.run([cli, "rebuild", str(copy), "--cold"]).returncode == 0
edited = measured(copy, (-3.5, 4.25), 6.75, 15.0)

# 5. The existing height edit still works, and the circle survives it whole.
feature = run("inspect", copy)["features"][0]
assert feature["editable"] and feature["distance_mm"] == 15.0
taller = root / "taller.fcad"
run("edit-extrude", copy, "--feature", feature["feature_id"],
    "--distance-mm", 25, "-o", taller)
grown = run("inspect", taller)["sketches"][0]["circle_edit"]["circle"]
assert grown["curve_id"] == saved["curve_id"]
assert grown["center_mm"] == [-3.5, 4.25] and grown["radius_mm"] == 6.75
assert grown["height_mm"] == 25.0
measured(taller, (-3.5, 4.25), 6.75, 25.0)

# 6. Refusals publish nothing and never replace anything.
for bad in [{"request_version": 2, "curve_id": saved["curve_id"],
             "center_mm": [0, 0], "radius_mm": 1},
            {"schema_version": 1, "curve_id": saved["curve_id"],
             "center_mm": [0, 0], "radius_mm": 1},
            {"request_version": 1, "curve_id": saved["curve_id"],
             "center_mm": [0, 0], "radius_mm": 0},
            {"request_version": 1, "curve_id": saved["curve_id"],
             "center_mm": [0, 0], "radius_mm": 1, "height_mm": 9},
            {"request_version": 1, "curve_id": "00000000-0000-7000-8000-000000000000",
             "center_mm": [0, 0], "radius_mm": 1}]:
    request.write_text(json.dumps(bad))
    error = refused("edit-circle", source, "--sketch", sketch["sketch_id"],
                    "--expect-version", catalog["content_version"],
                    "--request", request, "-o", root / "never.fcad")
    assert error["kind"] in ("input", "unsupported"), error
    assert not (root / "never.fcad").exists()
# A version that is no longer the source's is refused with everything else valid.
request.write_text(json.dumps({"request_version": 1, "curve_id": saved["curve_id"],
                               "center_mm": [1.0, 1.0], "radius_mm": 2.0}))
error = refused("edit-circle", source, "--sketch", sketch["sketch_id"],
                "--expect-version", run("inspect", copy)["content_version"],
                "--request", request, "-o", root / "never.fcad")
assert error["kind"] == "input" and "changed since it was read" in error["message"]
assert source.read_bytes() == kept, "the source is untouched throughout"

print(json.dumps({"artifacts": str(root), "circle": saved["curve_id"],
                  "edited": edited}, ensure_ascii=False))
```

`analytic_mm3` — это π r² h, вычисленное рецептом; настоящий аналитический
объём B-Rep измеряют native gates, а не он. Отдельные inspect/export читают
отдельные snapshots. При отказе новой копии нет; exit 7 после публикации
оставляет копию целой и запрещает автоматический повтор.
