# §26B — правка параметров сохранённого circular Cut

§26D расширяет прежнюю границу: [правка любого из двух Cut](edit-sequential-cuts.md).
Описанные ниже отказы двухзвенной правке относятся к исходному срезу.

[Локальные доказательства и ограничения](edit-circular-cut-copy-verification.md).
[Срез, который этот Cut создаёт](circular-cut-copy.md).
[Решение об истории Body](decisions/0004-feature-predecessor.md).

[§26C](sequential-circular-cuts.md) добавляет второй Cut отдельным Add-маршрутом.
Редактор, описанный здесь, сохраняет six-object frame и отказывает обоим Cut
в восьмиобъектной истории.

Тот же identity-preserving copy-маршрут, что у прежних правок
([§25B](edit-sketch-copy.md), [§25K](edit-circle-copy.md),
[§25M](edit-annular-copy.md), [§25N/O](annular-circle-constraints.md),
[§26A](circular-cut-copy.md)): один снимок, проверка версии, read-only
источник, baseline cold rebuild, strict-проверка refs, повторная проверка
версии, закрытие SQLite и атомарная no-clobber публикация. Отличается только
то, **что** пишется: два уже существующих payload и — ровно в одну сторону —
одна ссылка, которую Cut получает, перестав резать насквозь.

UI: `Edit cut <имя> — <UUID>…`; CLI:

```text
ferritecad edit-circular-cut <source.fcad> --feature UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
```

## Что меняется и что нет

Меняются **только** центр и радиус круга инструмента и конечная Blind-глубина
Cut-фичи. В SQL это `payload` и `payload_hash` ровно двух строк `objects` —
Sketch инструмента и самой Cut-фичи — плюс `meta.modified_at` новой копии и,
**только** при переходе «насквозь → карман», одна новая строка `topology_refs`.

Сохраняются: document UUID, все object UUID, их `parent`, `ordinal`, `name`,
`kind` и `schema_version`; UUID **самой окружности инструмента**; Body и его
`tip_feature`; `Extrude.previous`, `operation`, `profile` и отсутствие
`target_body`; исходные Sketch и Extrude детали целиком; все шесть
dependencies; **каждая** сохранённая topology ref под своей identity; и все
прочие SQL-ячейки, включая посторонние таблицы, triggers/FK, capability rows с
их rowid, copy_access и source claims. Источник остаётся побайтово цел; при
любом отказе новой копии нет.

Высота детали **не** входит в этот запрос. Её по-прежнему меняет
`edit-extrude` у исходной `NewBody`-фичи, и после такой правки зависимый Cut
пересчитывается. Сам Cut общий `edit-extrude` по-прежнему отказывает —
`blind_literal_distance` называет причину прямо, и это отдельный тест.
С [§26G](edit-cut-base-height.md) height edit проверяет всю допустимую историю:
при утолщении добавляет необходимые floor/Origin refs, при потере сохранённого
дна или превышении толщины абсолютной глубиной отказывает до публикации.

## Поддерживаемый класс

Ровно то, что производит [§26A](circular-cut-copy.md) из класса, который он
принимает, и ничего шире:

* **шесть** объектов без parent: untransformed XY `DatumPlane`, Sketch детали,
  forward literal Blind `Extrude`/`NewBody`, его `Body`, Sketch инструмента и
  `Cut`-фича;
* **шесть** dependencies ровно с этими ролями: `Plane` у обоих Sketch,
  `Profile` у обеих фич, `Predecessor` у Cut на исходную фичу и `BodyTip` у
  Body на Cut. Лишнее ребро — например `Parameter` за глубиной — это другой
  класс, и он считается, а не игнорируется;
* профиль детали — неограниченный замкнутый **осепараллельный прямоугольник**
  из четырёх Lines, внутри той же `PolygonExtrusion` policy;
* инструмент — одна неконструкционная неограниченная `Circle` на **той же**
  базовой XY-плоскости;
* набор имён, которые Cut дал, **совпадает** с тем, что этот модуль выдаёт для
  сохранённых чисел: стенка отверстия под собственной `Circle`, дно только если
  оно есть, обе `CarriedCap` и `CarriedSide` на каждый сегмент детали.

Идентичности берутся из связей и типов: Body — тот, чей `tip_feature` эта
фича; предшественник — тот, кого называет `previous`; Sketch инструмента — тот,
кого называет `profile`; окружность — единственная кривая в нём. Ни имя, ни
порядок строк, ни «первый попавшийся Circle» не участвуют, и переименование
обоих Sketch в одно и то же слово с перестановкой ordinal ничего не меняет.

Всё шире отказывается с причиной: второй Cut, седьмой объект, чужое ребро,
constrained-профиль или инструмент, не-прямоугольник, formula-глубина,
изменённая placement, импортированный STEP, несколько Body, payload, который
этот reader не может переписать без потерь. Frame прежних **четырёхобъектных**
редакторов не ослаблен: `editable`, `circle_edit`, `constraint_edit` и
`annulus_edit` по-прежнему требуют своих четырёх объектов и по-прежнему честно
отказывают документу с Cut — в том числе одноокружностному Sketch инструмента,
который иначе выглядел бы как цель `edit-circle`.

## Политика pocket ↔ through

Это единственное место, где правка чисел может тронуть сохранённое **имя**, и
поэтому политика записана целиком, до реализации:

* **карман → карман** и **насквозь → насквозь**: ни одно имя не меняется;
* **насквозь → карман**: у Cut появляется дно, и вместе с ним **одна новая**
  ссылка `ExtrudeCap{End}`. Ничего сохранённого не теряется, а общий copy job
  уже требует, чтобы каждое добавленное операцией имя разрешилось. Поэтому это
  поддержано, а не отказано ради симметрии;
* **карман → насквозь**: сохранённая ссылка на дно **потерялась бы**. Грань на
  этой глубине после правки — это дальняя сторона детали, у неё другое значение
  и уже есть своё имя (`CarriedCap{End}`). Удалить сохранённую ссылку или дать
  ей разрешиться в крышку детали — оба варианта хуже отказа, поэтому запрос
  **атомарно отказывается**, называя UUID той самой ссылки, которую он защищает.
  Публикации нет.

Отказ даётся при подготовке, до rebuild и boolean, а строгая проверка refs
остаётся вторым рубежом, а не единственным. Discovery сообщает
`through_allowed` прямо, чтобы ни форме, ни агенту не приходилось выводить
правило заново.

CLI открывает сессию ядра перед вызовом job. Поэтому без установленного ядра
он может вернуть `unsupported` раньше этого предметного отказа; такой ответ
не является доказательством исполнения политики перехода.

## Кэш

Ключ Cut — прежний `cut_cache_key(kernel, target_key, tool_key, tolerance)`.
Правка меняет `tool_key`, поэтому запись Cut обязана промахнуться, а архив
исходной фичи, чьи входы не менялись, законно попадает. Cold и warm дают одну
геометрию и одно разрешение refs; старая полость из кэша не возвращается — это
проверяется правкой **на месте** в том документе, которому принадлежит sidecar.

## Discovery

`inspect --json` получает у каждой строки `features` отдельное поле
`circular_cut_edit`, не меняя смысла прежних `editable`, `distance_mm` и
`refusal` — они по-прежнему отвечают про `edit-extrude`, и `editable` у Cut
по-прежнему `false`.

```json
"features": [{
  "feature_id": "UUID", "name": "Cut", "distance_mm": 4.0,
  "editable": false, "refusal": "…only NewBody extrusion distances…",
  "circular_cut_edit": {
    "available": true, "refusal": null, "document_refusal": null,
    "saved": {
      "feature_id": "UUID", "body_id": "UUID", "plane_id": "UUID",
      "previous_feature_id": "UUID", "profile_sketch_id": "UUID",
      "tool_sketch_id": "UUID", "tool_curve_id": "UUID",
      "center_mm": [20.0, 15.0], "radius_mm": 5.0, "depth_mm": 4.0,
      "height_mm": 10.0, "extents_mm": [[0.0, 0.0], [60.0, 40.0]],
      "direction": "+z along the plane normal", "wall_clearance_mm": 1e-7,
      "leaves_a_floor": true, "floor_reference_id": "UUID",
      "through_allowed": false
    }
  }
}]
```

* `available`, `refusal`, `document_refusal` и `saved` — **required**;
  `refusal`/`document_refusal`/`saved` могут быть `null`.
* `available` — true только когда нет ни документного, ни локального отказа.
  Документный отказ сохраняет приоритет и повторяется в `document_refusal`.
* `saved` присутствует только для поддержанного Cut и `null` иначе — никогда
  не придуманная фича.
* `through_allowed` — false ровно тогда, когда `floor_reference_id` не `null`.
* Неизвестные поля ответа следует игнорировать; это discovery одной операции.
  Работает **без ядра** и без окна: доступность по структуре не обещает
  установленный kernel, и применение правки без ядра отказывается.

Каталог берётся из **того же** снимка и того же `content_version`, что и всё
остальное в ответе; UI читает его через `ExtrudeEditSource`, а не открывает
файл второй раз.

## Request v1

Строгий, до 65536 байт, ровно пять полей, `deny_unknown_fields`:

```json
{"request_version":1,"tool_curve_id":"UUID","center_mm":[30.0,20.0],
 "radius_mm":7.5,"depth_mm":6.0}
```

* `request_version` — целое 1, единственное написание версии; `schema_version`
  здесь лишнее поле и отвергается.
* `tool_curve_id` — canonical UUID **сохранённой** окружности инструмента.
  Чужой UUID отвергается; форма UI подставляет сохранённую identity, а не текст
  из поля.
* `center_mm` — ровно две конечные координаты в mm на базовой XY-плоскости
  детали. Плоскости в запросе нет: этот срез правит только её.
* `radius_mm` — конечный строго положительный радиус.
* `depth_mm` — конечная строго положительная глубина в mm вдоль +Z, не больше
  высоты детали, и не равная ей, если у Cut есть сохранённое дно. Флага
  «насквозь» нет.

Центр, радиус и глубина проходят **ту же** проверку, что и §26A: политика
`CircleExtrusion`, предел глубины по высоте детали и зазор строго больше
`WALL_CLEARANCE_MM` = `Tolerance::DEFAULT_LINEAR` до каждой стороны
прямоугольника. Эта проверка одна на UI, CLI и job; копии boolean нет, и
непосредственный механизм остаётся прежним — non-destructive OCCT и настоящая
история.

## JSON v1

Успех — прежний envelope: `schema_version` 1, `operation` `edit-circular-cut`,
`ok` true, `result` с `destination`, `document_id`, `body_id`, `feature_id`,
`sketch_id`, `tool_curve_id`, `previous_feature_id` и `leaves_a_floor`; один
объект + LF, exit 0. Отказ — `ok` false, `kind`/`message`/`causes`, exit 2, без
публикации и без scratch. Потеря stdout — exit 7: публикация **могла**
состояться, копия цела, повторять операцию вслепую нельзя — проверьте
назначение. UTF-8 paths обязательны в JSON-режиме; usage/help остаются
clap-текстом. Text и JSON идут через один job.

## UI

Действие предлагается только у фичи, про которую документ сказал, что её числа
можно изменить; у остальных — причина в подсказке, и у исходной `NewBody` эта
причина прямо называет `edit-extrude`. Форма открывается на **сохранённых**
числах, называет выбранный Cut, его Sketch и окружность, плоскость и
направление, размеры детали и фичу, которую правка изменит, говорит, карман это
сегодня или отверстие, и — если дно сохранено — что глубина обязана остаться
меньше высоты.

Четыре поля (Centre X/Y, Radius, Depth), один подтверждённый **Apply cut** —
один шаг bounded Undo/Redo на все четыре, максимум 128; Undo/Redo не обращается
ни к одному job. Изменение числа после Apply снова закрывает Save, пока его не
подтвердят. Save Cancel, отказ операции, занятое назначение и отказ
последующего Open сохраняют черновик через общий механизм
`draft_published`/`draft_load_finished` — тот же, что у всех прежних правок, а
не отдельный для Cut. Worker — общий `Edits`, и job живёт в нём.

Это **не** предварительный просмотр: live preview в срез не входит, и форма
ничего не рисует в сцене. Показанное — это то, что сохранено, и то, что будет
запрошено.

## Что в срез не входит

Второй Cut, face attachment, произвольные плоскости, Add/Intersect, Fillet,
ThroughAll, constrained-инструмент, solver между числами и геометрией, live
preview, правка Cut в середине более длинной истории, in-place Save и новая
схема БД. Контракты прежних редакторов не ослаблены.

## Рецепт

Нужны OCCT, `FERRITECAD` и `FCAD_UFBX_READER`.

```python
# FCAD_26B_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-edit-cut-"))
DEFLECTION, ANGULAR = 0.05, 0.1
WIDTH, DEPTH, HEIGHT = 60.0, 40.0, 10.0

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    if p.returncode == 7:
        raise RuntimeError("Report lost; inspect publication. Do not retry blindly.")
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, center, radius, depth):
    """Everything the mesh must be for the part to really have that cut."""
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

    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            directed[(p[k], p[(k + 1) % 3])] = directed.get((p[k], p[(k + 1) % 3]), 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "the mesh is open"

    on_bore = lambda t: all(abs(math.hypot(v[0] - center[0], v[1] - center[1]) - radius)
                            < DEFLECTION + 1e-4 for v in t)
    spans = lambda t: max(v[2] for v in t) - min(v[2] for v in t) > 1e-4
    wall = [t for t in tris if on_bore(t) and spans(t)]
    assert len(wall) >= 12, "no bore wall"
    def radial(t):
        a, b, c = t
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        nx = u[1] * v[2] - u[2] * v[1]
        ny = u[2] * v[0] - u[0] * v[2]
        mx = (a[0] + b[0] + c[0]) / 3 - center[0]
        my = (a[1] + b[1] + c[1]) / 3 - center[1]
        return nx * mx + ny * my
    assert all(radial(t) < 0 for t in wall), "a boss, not a hole"
    zs = sorted(v[2] for t in wall for v in t)
    assert abs(zs[0]) < 1e-4 and abs(zs[-1] - depth) < 1e-4, (zs[0], zs[-1])

    def covers(t, z):
        if any(abs(v[2] - z) > 1e-4 for v in t):
            return False
        (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12:
            return False
        a = ((y2 - y3) * (center[0] - x3) + (x3 - x2) * (center[1] - y3)) / d
        b = ((y3 - y1) * (center[0] - x3) + (x1 - x3) * (center[1] - y3)) / d
        return a >= -1e-9 and b >= -1e-9 and 1 - a - b >= -1e-9
    if depth < HEIGHT:
        assert any(covers(t, depth) for t in tris), "a pocket has a floor"
        assert any(covers(t, HEIGHT) for t in tris), "and a closed far side"
    else:
        assert not any(covers(t, HEIGHT) for t in tris), "a hole is open"

    lo = [min(v[j] for t in tris for v in t) for j in range(3)]
    hi = [max(v[j] for t in tris for v in t) for j in range(3)]
    assert [round(v, 4) for v in lo] == [0.0, 0.0, 0.0]
    assert [round(v, 4) for v in hi] == [WIDTH, DEPTH, HEIGHT]

    exact = WIDTH * DEPTH * HEIGHT - math.pi * radius ** 2 * depth
    inscribed = WIDTH * DEPTH * HEIGHT - math.pi * (radius - DEFLECTION) ** 2 * depth
    assert exact - 1e-6 <= volume <= inscribed + 1e-6, volume
    return volume

def saved_cut(path):
    """The one editable Cut, from one reading, chosen by what the document says."""
    seen = run(["inspect", str(path), "--json"])["result"]
    cuts = [f for f in seen["features"] if f["circular_cut_edit"]["available"]]
    assert len(cuts) == 1, "this recipe edits a document with exactly one Cut"
    return seen["content_version"], cuts[0]["circular_cut_edit"]["saved"]

def edit(source, saved, version, center, radius, depth, out, code=0):
    request = root / "edit.json"
    request.write_text(json.dumps({"request_version": 1,
                                   "tool_curve_id": saved["tool_curve_id"],
                                   "center_mm": list(center),
                                   "radius_mm": radius, "depth_mm": depth}))
    return run(["edit-circular-cut", str(source), "--feature", saved["feature_id"],
                "--expect-version", version, "--request", str(request),
                "-o", str(out), "--json"], code=code)

# 1. A part with one saved cut, made by the shipped commands.
create = root / "create.json"
plate = root / "plate.fcad"
create.write_text(json.dumps({"request_version": 1,
                              "points_mm": [[0.0, 0.0], [WIDTH, 0.0],
                                            [WIDTH, DEPTH], [0.0, DEPTH]],
                              "height_mm": HEIGHT}))
assert run(["create-sketch-extrude", str(create), "-o", str(plate), "--json"])["ok"]
seen = run(["inspect", str(plate), "--json"])["result"]
body = seen["bodies"][0]
cut_request = root / "cut.json"
cut_request.write_text(json.dumps({"request_version": 1, "center_mm": [20.0, 15.0],
                                   "radius_mm": 5.0, "depth_mm": 4.0}))
pocket = root / "pocket.fcad"
assert run(["cut-circular-copy", str(plate), "--body", body["body_id"],
            "--expect-version", seen["content_version"],
            "--request", str(cut_request), "-o", str(pocket), "--json"])["ok"]
before = pocket.read_bytes()

# 2. What the saved cut is, from one reading. No SQLite, no prose, no guessing.
version, saved = saved_cut(pocket)
assert saved["center_mm"] == [20.0, 15.0] and saved["radius_mm"] == 5.0
assert saved["depth_mm"] == 4.0 and saved["height_mm"] == HEIGHT
assert saved["leaves_a_floor"] is True
assert saved["through_allowed"] is False, "a saved floor may not be cut away"
assert saved["floor_reference_id"] is not None

# 3. Move the tool and change the depth into a new copy.
moved = root / "moved.fcad"
published = edit(pocket, saved, version, (30.0, 20.0), 7.5, 6.0, moved)["result"]
assert published["feature_id"] == saved["feature_id"], "the cut keeps its UUID"
assert published["tool_curve_id"] == saved["tool_curve_id"]
assert published["body_id"] == saved["body_id"]
assert published["previous_feature_id"] == saved["previous_feature_id"]
assert published["leaves_a_floor"] is True
assert pocket.read_bytes() == before, "the source was touched"

# 4. Reopen: same identities, new numbers, the history untouched.
after = run(["inspect", str(moved), "--json"])["result"]
assert len(after["features"]) == 2
_, now = saved_cut(moved)
assert now["feature_id"] == saved["feature_id"]
assert now["tool_sketch_id"] == saved["tool_sketch_id"]
assert now["profile_sketch_id"] == saved["profile_sketch_id"]
assert now["center_mm"] == [30.0, 20.0] and now["radius_mm"] == 7.5
assert now["depth_mm"] == 6.0 and now["height_mm"] == HEIGHT
checked = run(["validate", str(moved), "--json"])
assert checked["ok"] and checked["result"]["valid"], checked
rebuilt = run(["rebuild", str(moved), "--cold"])
assert "11 of 11 stored references resolved" in rebuilt, rebuilt
assert "tip Cut" in rebuilt, rebuilt
measure(moved, (30.0, 20.0), 7.5, 6.0)

fbx = moved.with_suffix(".fbx")
written = run(["export-fbx", str(moved), "-o", str(fbx), "--json"])
assert written["result"]["complete"] and written["result"]["geometries"] == 1
read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
assert read.returncode == 0, read.stderr
executed = [l for l in read.stdout.splitlines()
            if l.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED")]
assert executed and executed[0].endswith("failures=0"), read.stdout

# 5. The transition policy, both ways, each with everything else valid.
refused = edit(pocket, saved, version, (20.0, 15.0), 5.0, HEIGHT,
               root / "never.fcad", code=2)
assert refused["ok"] is False and refused["error"]["kind"] == "unsupported"
assert saved["floor_reference_id"] in refused["error"]["message"]
assert not (root / "never.fcad").exists()

hole_request = root / "hole.json"
hole_request.write_text(json.dumps({"request_version": 1, "center_mm": [20.0, 15.0],
                                    "radius_mm": 5.0, "depth_mm": HEIGHT}))
hole = root / "hole.fcad"
assert run(["cut-circular-copy", str(plate), "--body", body["body_id"],
            "--expect-version", seen["content_version"],
            "--request", str(hole_request), "-o", str(hole), "--json"])["ok"]
hole_version, hole_saved = saved_cut(hole)
assert hole_saved["through_allowed"] is True
assert hole_saved["floor_reference_id"] is None
shortened = root / "shortened.fcad"
got = edit(hole, hole_saved, hole_version, (20.0, 15.0), 5.0, 3.0, shortened)["result"]
assert got["leaves_a_floor"] is True, "a shortened hole gains a floor"
assert "11 of 11 stored references resolved" in run(["rebuild", str(shortened), "--cold"])
measure(shortened, (20.0, 15.0), 5.0, 3.0)

# 6. Refusals publish nothing and never replace anything.
names = sorted(p.name for p in root.iterdir())
for center, radius, depth in ((( 20.0, 15.0), 0.0, 4.0), ((20.0, 15.0), -5.0, 4.0),
                              ((20.0, 15.0), 5.0, 0.0), ((20.0, 15.0), 5.0, HEIGHT + 1),
                              ((5.0, 15.0), 5.0, 4.0), ((2.0, 15.0), 5.0, 4.0),
                              ((200.0, 200.0), 5.0, 4.0)):
    refused = edit(pocket, saved, version, center, radius, depth,
                   root / "never.fcad", code=2)
    assert refused["error"]["kind"] in ("input", "unsupported"), refused["error"]
# A curve this cut does not draw, with everything else correct.
foreign = dict(saved, tool_curve_id="00000000-0000-7000-8000-000000000000")
assert edit(pocket, foreign, version, (20.0, 15.0), 5.0, 4.0,
            root / "never.fcad", code=2)["error"]["kind"] == "input"
# A version that is no longer the source's.
assert "changed since it was read" in edit(
    pocket, saved, run(["inspect", str(moved), "--json"])["result"]["content_version"],
    (20.0, 15.0), 5.0, 4.0, root / "never.fcad", code=2)["error"]["message"]
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"
assert pocket.read_bytes() == before

print("FCAD_26B_RECIPE_OK", saved["feature_id"], saved["tool_curve_id"])
```

Отдельные `inspect`/`export` читают отдельные snapshots. При отказе новой копии
нет; exit 7 после публикации оставляет копию целой и запрещает автоматический
повтор.
