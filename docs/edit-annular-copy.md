# §25M — правка сохранённого кольцевого эскиза в новой копии

[Локальные доказательства и ограничения](edit-annular-copy-verification.md).
Тот же identity-preserving copy-маршрут, что у
[правки одной окружности (§25K)](edit-circle-copy.md) и
[координатной правки Line Sketch (§25B)](edit-sketch-copy.md): один снимок,
baseline cold rebuild, проверка refs, повторная проверка версии источника,
закрытие SQLite и атомарная no-clobber публикация. Отличается только
подготовленный payload: общий центр и оба радиуса пары сохранённых
аналитических окружностей, созданной [§25L](annular-sketch-extrude.md).

UI: `Edit annulus <имя> — <UUID>…`; CLI:

```text
ferritecad edit-annular <source.fcad> --sketch UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
```

## Что меняется и что нет

Меняются **только** центр и радиус двух названных окружностей: в SQL это payload
и payload_hash выбранной строки `objects`, плюс `meta.modified_at` новой копии —
ровно две ячейки `objects` и одна `meta`. Сохраняются document UUID, все object
UUID/порядок/имена/parents, **UUID обеих окружностей и порядок, в котором они
записаны**, `plane` и остальные поля Sketch, высота и выражение Extrude, Body,
dependencies, topology refs и все прочие SQL-ячейки, включая посторонние таблицы,
capability rows с их rowid и source claims. Source остаётся побайтово цел; при
любом отказе новой копии нет.

Высота **не** входит в этот запрос. Её по-прежнему меняет `edit-extrude`, и
после правки профиля он работает как раньше, не теряя identity ни одной из
окружностей. Обратное тоже верно: этот редактор принимает копию, у которой
высоту уже изменили, потому что класс — про профиль, а не про число экструзии.

Принятая правка ставит **обе** окружности в один и тот же центр. Документ может
хранить два центра, различающихся в последних разрядах — «концентрично» значит
«в пределах линейного допуска ядра», а не «побитово одинаково», — поэтому
discovery сообщает оба, а правка их выравнивает.

## Поддерживаемый класс

Ровно класс §25L внутри frame §25K: один untransformed XY DatumPlane, один
Sketch, один forward literal Blind `Extrude`/`NewBody` и его `Body`, ровно три
dependencies (plane/profile/body-tip), и Sketch, в котором **ровно две
неконструкционные неограниченные концентрические `Circle`** внутри той же
политики `AnnularExtrusion`, что применяется к новым числам — включая
минимальную стенку 0,001 mm, евклидову концентричность 1e-7 mm и предел
1 000 000 mm.

Роли определяются **сохранёнными радиусами**: граница — большая окружность, дно
отверстия — меньшая. Порядок записи кривых не решает ничего; документ, в котором
отверстие записано первым, правится с теми же UUID в тех же ролях.

Отказываются: Line и Arc, одна кривая, три и более, конструкционная геометрия,
любые constraints, две окружности с одинаковым UUID, неконцентрическая пара,
уже сохранённая геометрия вне политики, параметризованный или иначе
неподдерживаемый Extrude, другие плоскости и связи, и payload, который этот
reader не может переписать без потерь. Произвольных отверстий, boolean Cut,
circle constraints, in-place Save, рисования мышью, новой схемы БД и новых FFI
здесь нет; контракты Line- и Circle-редакторов не ослаблены.

## Работа в UI

Форма показывает сохранённый Sketch UUID, **оба** UUID окружностей с их
сохранёнными центрами и радиусами и неизменяемую высоту. Введите Center X/Y,
Outer radius и Inner radius, затем нажмите **Apply annulus change**: одно
подтверждение всех трёх величин создаёт один шаг Undo/Redo, максимум 128.
Промежуточный ввод (`-`, пустое поле) в историю не попадает; Undo/Redo не
обращается ни к одному job. Save доступен после Apply. Пока worker занят, поля
и действия формы недоступны.

Save Cancel, отказ операции, занятое назначение и устаревший ответ сохраняют
черновик и его историю. После публикации открывается новая копия прежним
асинхронным Open; до принятия сцены общий механизм `draft_published` /
`draft_load_finished` хранит один резервный черновик, и отказ подготовки сцены
восстанавливает форму вместе с историей. Опубликованный файл при этом остаётся
на диске. Это поведение проверено headless через настоящие widgets и worker;
интерактивный GUI smoke отложен.

## Discovery

`inspect --json` получает у каждой строки `sketches` отдельное поле
`annulus_edit`, не меняя смысла прежних `editable`, `vertices`,
`constraint_edit` и `circle_edit` — они по-прежнему отвечают про свои редакторы,
и приоритет документного отказа сохраняется.

```json
"annulus_edit": {
  "available": true,
  "refusal": null,
  "document_refusal": null,
  "annulus": {
    "outer_curve_id": "UUID", "inner_curve_id": "UUID",
    "center_mm": [12.0, -7.0], "inner_center_mm": [12.0, -7.0],
    "outer_radius_mm": 10.0, "inner_radius_mm": 4.0, "height_mm": 15.0
  }
}
```

* `available`, `refusal`, `document_refusal` и `annulus` — **required** поля;
  `refusal`/`document_refusal`/`annulus` могут быть `null`.
* `available` — boolean: true только когда нет ни документного, ни локального
  отказа. Документный отказ (read-only копия, триггеры, неизвестные
  capabilities) сохраняет приоритет и повторяется в `document_refusal`.
* `annulus` присутствует для поддержанного Sketch и `null` иначе — никогда не
  придуманная пара.
* `outer_curve_id` и `inner_curve_id` названы **в своих ролях** и различны; ни
  один не выводится из другого.
* `center_mm` — центр границы, `inner_center_mm` — центр отверстия. Они
  совпадают в документе, созданном §25L, но политика требует лишь евклидовой
  близости, поэтому сообщаются оба.
* `height_mm` сообщается потому, что решает смысл чисел, а не потому, что этот
  запрос может её изменить.
* Неизвестные поля ответа следует игнорировать; это discovery одной операции.
  Discovery и структурные отказы работают и в сборке без ядра — доступность по
  структуре не обещает установленный kernel, а применение правки без ядра
  отказывается.

## Request v1

Строгий, до 65536 байт, ровно шесть полей, `deny_unknown_fields`:

```json
{"request_version":1,"outer_curve_id":"UUID","inner_curve_id":"UUID",
 "center_mm":[-3.5,4.25],"outer_radius_mm":6.75,"inner_radius_mm":2.125}
```

* `request_version` — целое 1. **Единственное** написание версии;
  `schema_version` запроса создания здесь лишнее поле и отвергается.
* `outer_curve_id`, `inner_curve_id` — canonical UUIDv7 **сохранённых**
  окружностей, каждый в той роли, которую он уже занимает. Перестановка ролей,
  один и тот же UUID дважды и чужой UUID отвергаются; форма UI подставляет
  сохранённые identity, а не текст из поля.
* `center_mm` — ровно две конечные координаты в mm, **общий** центр обеих
  окружностей; ноль и отрицательные допустимы. Отдельного центра у отверстия в
  запросе нет: класс концентрический, и два центра могли бы противоречить друг
  другу.
* `outer_radius_mm`, `inner_radius_mm` — конечные строго положительные mm,
  `inner < outer`, разность не меньше `MIN_WALL_MM`.
* `height_mm`, `radius_mm`, `inner_center_mm`, `vertices` и любые другие поля
  лишние. Числа проверяются той же `AnnularExtrusion` policy, что и создание,
  против **сохранённой** высоты.

## Writer

Writer не доверяет публично изменяемому prepared payload. Он заново читает
строку, отказывает Sketch, изменившемуся между подготовкой и записью,
**заново выводит** разрешённую правку из чисел самого payload против текущего
документа и сравнивает payload целиком. Прямой вызов с подменённым UUID любой
из окружностей, с переставленными радиусами при неизменных UUID, с другим
`plane`, с поднятым `construction`, с третьей кривой, с одной кривой, с
добавленными constraints, с радиусом вне политики, с разъехавшимися центрами
или с устаревшей подготовкой отказывает **до** изменения и не двигает
`content_version`.

Эта проверка — одна для обоих аналитических редакторов
(`write_prepared_sketch_geometry`): два её экземпляра могли бы разойтись, и
разошедшийся принял бы подделку. Strict-правило topology refs не ослаблено:
правка профиля обязана сохранить каждую ссылку, которая разрешалась до неё.

## JSON v1

Успех — прежний envelope: `schema_version` 1, `operation` `edit-annular`,
`ok` true, `result` с `destination`, `document_id`, `sketch_id`,
`outer_curve_id` и `inner_curve_id`, один объект + LF, exit 0. Отказ — `ok`
false, `kind`/`message`/`causes`, exit 2, без публикации и без scratch. Потеря
stdout — exit 7: если публикация уже состоялась, копия цела и повторять
операцию нельзя без проверки назначения. UTF-8 paths обязательны в
JSON-режиме; usage/help остаются clap-текстом. Text и JSON идут через один job
и один прежний emitter.

## Рецепт

Нужны OCCT, `FERRITECAD` и `FCAD_UFBX_READER` — путь к pinned reader,
собранному из `tools/unity-fbx-smoke/scripts/read_production.c` с pin из
`fetch_ufbx.sh`. На macOS native library paths экспортируются в том же Bash.

```python
# FCAD_25M_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-edit-annulus-"))
DEFLECTION = 0.05
ANGULAR = 0.1

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, center, outer, inner, height):
    """Everything the mesh must be for the part to really be hollow."""
    stl = path.with_suffix(".stl")
    report = run(["export-stl", str(path), "-o", str(stl),
                  "--linear-deflection", str(DEFLECTION),
                  "--angular-deflection", str(ANGULAR), "--json"])
    raw = stl.read_bytes()
    n = struct.unpack_from("<I", raw, 80)[0]
    assert len(raw) == 84 + 50 * n and report["result"]["triangles"] == n
    tris, six = [], 0.0
    for i in range(n):
        off = 84 + 50 * i + 12
        t = [list(struct.unpack_from("<fff", raw, off + 12 * k)) for k in range(3)]
        tris.append(t)
        a, b, c = t
        six += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
    volume = six / 6.0

    # Closed and wound one way: every directed edge exactly once.
    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            e = (p[k], p[(k + 1) % 3])
            directed[e] = directed.get(e, 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "the mesh is open"

    def radius(v):
        return math.hypot(v[0] - center[0], v[1] - center[1])
    def radial(t):
        a, b, c = t
        u = [b[i] - a[i] for i in range(3)]
        w = [c[i] - a[i] for i in range(3)]
        nx = u[1] * w[2] - u[2] * w[1]
        ny = u[2] * w[0] - u[0] * w[2]
        mx = sum(v[0] for v in t) / 3 - center[0]
        my = sum(v[1] for v in t) / 3 - center[1]
        return nx * mx + ny * my
    tol = DEFLECTION + 1e-4
    bore = [t for t in tris if all(abs(radius(v) - inner) < tol for v in t)]
    skin = [t for t in tris if all(abs(radius(v) - outer) < tol for v in t)]
    assert len(bore) >= 12 and len(skin) >= 12, "a wall is missing"
    assert all(radial(t) < 0 for t in bore), "the cavity is inside out"
    assert all(radial(t) > 0 for t in skin), "the outside faces inward"

    def covers(t, px, py):
        (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12:
            return False
        u = ((y2 - y3) * (px - x3) + (x3 - x2) * (py - y3)) / d
        w = ((y3 - y1) * (px - x3) + (x1 - x3) * (py - y3)) / d
        return u >= -1e-9 and w >= -1e-9 and 1 - u - w >= -1e-9
    assert not any(covers(t, *center) for t in tris), "the bore is not open"

    # The stable reference: subtracting squared radii loses precision.
    exact = math.pi * (outer - inner) * (outer + inner) * height
    full = math.pi * outer * outer * height
    assert 0 < volume <= exact * (1 + 1e-6), volume
    assert volume < full * 0.999, "the STL is a solid rod"
    lo = [min(v[j] for t in tris for v in t) for j in range(3)]
    hi = [max(v[j] for t in tris for v in t) for j in range(3)]
    assert abs((hi[2] - lo[2]) - height) < 1e-4, (lo, hi)
    for j, half in ((0, outer), (1, outer)):
        assert hi[j] - lo[j] >= 2 * half - 2 * DEFLECTION - 1e-4
    return volume

# 1. A source this project's own shipped command makes.
create = root / "create.json"
source = root / "tube.fcad"
create.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                              "outer_radius_mm": 10.0, "inner_radius_mm": 4.0,
                              "height_mm": 15.0}))
made = run(["create-annular-extrude", str(create), "-o", str(source), "--json"])
assert made["ok"]

# 2. Explicit UUIDs and the exact version come from one JSON reading.
seen = run(["inspect", str(source), "--json"])["result"]
version = seen["content_version"]
sketch, = seen["sketches"]
discovery = sketch["annulus_edit"]
assert discovery["available"] is True and discovery["refusal"] is None
saved = discovery["annulus"]
assert saved["outer_curve_id"] == made["result"]["outer_curve_id"]
assert saved["inner_curve_id"] == made["result"]["inner_curve_id"]
assert saved["outer_curve_id"] != saved["inner_curve_id"]
assert saved["center_mm"] == saved["inner_center_mm"] == [12.0, -7.0]
assert (saved["outer_radius_mm"], saved["inner_radius_mm"]) == (10.0, 4.0)
assert saved["height_mm"] == 15.0
# The editors of the earlier slices still answer for themselves.
assert sketch["editable"] is False and sketch["vertices"] is None
assert sketch["circle_edit"]["available"] is False
assert sketch["circle_edit"]["circle"] is None
assert seen["edit_extrude"]["available"] is True

# 3. Move the centre and both radii into a new copy.
request = root / "edit.json"
copy = root / "edited.fcad"
request.write_text(json.dumps({
    "request_version": 1,
    "outer_curve_id": saved["outer_curve_id"],
    "inner_curve_id": saved["inner_curve_id"],
    "center_mm": [-3.5, 4.25],
    "outer_radius_mm": 6.75, "inner_radius_mm": 2.125}))
before = source.read_bytes()
published = run(["edit-annular", str(source), "--sketch", sketch["sketch_id"],
                 "--expect-version", version, "--request", str(request),
                 "-o", str(copy), "--json"])
assert published["ok"]
result = published["result"]
assert result["sketch_id"] == sketch["sketch_id"]
assert result["outer_curve_id"] == saved["outer_curve_id"]
assert result["inner_curve_id"] == saved["inner_curve_id"]
assert result["document_id"] == seen["document_id"]
assert source.read_bytes() == before, "the source was touched"

# 4. Reopen: the same identities, the new numbers, the height untouched.
after = run(["inspect", str(copy), "--json"])["result"]
kept = after["sketches"][0]["annulus_edit"]["annulus"]
assert kept["outer_curve_id"] == saved["outer_curve_id"]
assert kept["inner_curve_id"] == saved["inner_curve_id"]
assert kept["center_mm"] == kept["inner_center_mm"] == [-3.5, 4.25]
assert (kept["outer_radius_mm"], kept["inner_radius_mm"]) == (6.75, 2.125)
assert kept["height_mm"] == 15.0, "the height is not in this request"
checked = run(["validate", str(copy), "--json"])
assert checked["ok"] and checked["result"]["valid"], checked
rebuilt = run(["rebuild", str(copy), "--cold"])
assert "2 segments, 1 hole(s)" in rebuilt, rebuilt
assert "solid, 4 named faces" in rebuilt, rebuilt
assert "4 of 4 stored references resolved" in rebuilt, rebuilt
named = run(["print-topology", str(copy)])
for curve in (kept["outer_curve_id"], kept["inner_curve_id"]):
    assert f"extrude side from segment {curve}" in named, curve

# 5. The mesh really is the hollow part those numbers describe, and the FBX
#    carries the same hole.
volume = measure(copy, (-3.5, 4.25), 6.75, 2.125, 15.0)
fbx = root / "edited.fbx"
written = run(["export-fbx", str(copy), "-o", str(fbx), "--json"])
assert written["result"]["complete"] and written["result"]["geometries"] == 1
assert fbx.stat().st_size == written["result"]["bytes"]
read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
assert read.returncode == 0, read.stderr
executed = [l for l in read.stdout.splitlines()
            if l.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED")]
assert executed and executed[0].endswith("failures=0"), read.stdout
summary = [l for l in read.stdout.splitlines() if l.startswith("FCAD_IDENTITY_SUMMARY")]
assert summary and "meshes=1" in summary[0], read.stdout

# 6. The existing height edit still owns the height, and keeps both circles.
taller = root / "taller.fcad"
run(["edit-extrude", str(copy), "--feature", after["features"][0]["feature_id"],
     "--distance-mm", "25", "-o", str(taller), "--json"])
raised = run(["inspect", str(taller), "--json"])["result"]
assert raised["features"][0]["distance_mm"] == 25.0
still = raised["sketches"][0]["annulus_edit"]["annulus"]
assert still["outer_curve_id"] == saved["outer_curve_id"]
assert still["inner_curve_id"] == saved["inner_curve_id"]
assert (still["outer_radius_mm"], still["inner_radius_mm"]) == (6.75, 2.125)
assert still["height_mm"] == 25.0
taller_volume = measure(taller, (-3.5, 4.25), 6.75, 2.125, 25.0)
assert taller_volume > volume * 1.6, (volume, taller_volume)

# 7. Refusals publish nothing and never replace anything.
names = sorted(p.name for p in root.iterdir())
for bad in ({"outer_radius_mm": 10.0, "inner_radius_mm": 10.0},
            {"outer_radius_mm": 10.0, "inner_radius_mm": 12.0},
            {"outer_radius_mm": 10.0, "inner_radius_mm": 9.9999},
            {"outer_curve_id": saved["inner_curve_id"],
             "inner_curve_id": saved["outer_curve_id"]},
            {"height_mm": 30.0}):
    payload = {"request_version": 1,
               "outer_curve_id": saved["outer_curve_id"],
               "inner_curve_id": saved["inner_curve_id"],
               "center_mm": [-3.5, 4.25],
               "outer_radius_mm": 6.75, "inner_radius_mm": 2.125}
    payload.update(bad)
    request.write_text(json.dumps(payload))
    refused = run(["edit-annular", str(source), "--sketch", sketch["sketch_id"],
                   "--expect-version", version, "--request", str(request),
                   "-o", str(root / "never.fcad"), "--json"], code=2)
    assert refused["ok"] is False
    assert refused["error"]["kind"] in ("input", "unsupported"), refused["error"]
# A version that is no longer the source's is refused with everything else valid.
request.write_text(json.dumps({"request_version": 1,
                               "outer_curve_id": saved["outer_curve_id"],
                               "inner_curve_id": saved["inner_curve_id"],
                               "center_mm": [-3.5, 4.25],
                               "outer_radius_mm": 6.75, "inner_radius_mm": 2.125}))
stale = run(["edit-annular", str(source), "--sketch", sketch["sketch_id"],
             "--expect-version", "0" * 64, "--request", str(request),
             "-o", str(root / "never.fcad"), "--json"], code=2)
assert "has changed" in stale["error"]["message"], stale["error"]
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"
assert source.read_bytes() == before
print("FCAD_25M_RECIPE_OK", round(volume, 6), round(taller_volume, 6))
```
