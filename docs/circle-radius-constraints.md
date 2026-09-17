# §25N — параметрическая окружность: сохранённый радиус и закреплённый центр

[Локальные доказательства и ограничения](circle-radius-constraints-verification.md).
Тот же сохраняемый constraint-маршрут, что у
[Line-ограничений (§25E–I)](sketch-constraints-copy.md): один снимок, общая
подготовка, атомарный remove/add, baseline cold rebuild, strict-проверка refs,
повторная проверка версии, закрытие SQLite и атомарная no-clobber публикация.
Новый — словарь: у сохранённой аналитической окружности §25J появляются **радиус
в mm** и **закрепление центра** в точных X/Y mm.

UI: `Edit constraints <имя> — <UUID>…` → строка `Circle · …`; CLI — та же
команда, что и у Line:

```text
ferritecad edit-sketch-constraints-copy <source.fcad> --sketch UUID \
    --expect-version HASH --request <request.json> -o <copy.fcad> [--json]
```

## Что появляется

Из тех же сохранённых координат — центр (12,−7), радиус 10 — и запроса
`radius 6.75` + `fixed center (−3.5, 4.25)` получается цилиндр, чья геометрия
взята **из решения PlaneGCS**: три аналитические грани, `Cylinder{6.75}` именно
под прежним `ExtrudeSide` ref исходной `Circle`, объём π·6.75²·15.

Сохранённая геометрия Sketch остаётся исходным приближением — центр (12,−7),
радиус 10 — ровно как у Line-ограничений: solver-ответ не переписывает
документ. Это же значит, что при снятии размера геометрия возвращается к
сохранённому приближению, а не к последнему решённому радиусу; сохранения
последнего решения этот срез не обещает.

## Границы первого среза

Одна неконструкционная аналитическая `Circle` в XY Sketch, один forward literal
Blind `Extrude`/`NewBody` и его `Body`, прежний frame §25K. Две возможности:
`Radius` и `Fixed` центра. Без Diameter, Tangent, EqualRadius, Arc, смешанных
профилей, окружностей с отверстиями, mouse drag окружности, live solve, boolean
Cut, in-place Save и выражений вместо literal radius. Числа проверяются той же
`CircleExtrusion` policy, что и создание — одна политика, а не копии в UI и
CLI, — и применяется она к **решённым** центру и радиусу перед публикацией.

Пару окружностей §25L/M взял следующий срез —
[§25O](annular-circle-constraints.md) — тем же маршрутом и без изменений здесь:
solver, C ABI, payload v3 и capability остались как есть.

## Архитектура solver

`ferritecad-sketch-solver` получил настоящую аналитическую окружность: три
неизвестных `cx`, `cy`, `r`.

* **Центр — адресуемая точка.** `Circle { circle: CircleId, center: PointId,
  radius: f64 }`: центр это `PointId`, то есть обычная точка эскиза, и его
  закрепляет прежний `Constraint::Fixed` без второго словаря. Второго семейства
  «закрепить центр» нет.
* **Радиус — собственный скаляр.** `Constraint::Radius { circle, radius }`
  адресует `CircleId`, а не точку. Окружность, описанная центром и точкой на
  ободе, несла бы четыре параметра при трёх степенях свободы и сообщала бы
  степень свободы, которой у чертежа нет.
* **Два пространства идентификаторов.** `PointId` и `CircleId` раздельны:
  `Constraint::points()` и `Constraint::circles()` — два списка, и ни один не
  читается по правилу «какие записи что значат».
* **Диагностика и решение настоящие.** `Diagnosis`/`Solution` отвечают в словах
  вызывающего; `Solution::circle(CircleId)` даёт решённый радиус, а центр
  читается и как точка, и через окружность — это один ответ дважды, а не два.
  `worst_residual` считается по **полученному** радиусу, а не по запрошенному
  числу.
* **Отрицательный радиус не публикуется.** planegcs может минимизировать в
  отрицательный параметр; такой ответ становится `DidNotConverge` без координат,
  а не документом, который откажет ядро.

Измеренные степени свободы: свободная окружность 3, с `Radius` 2, с `Fixed`
центра 1, с обоими 0. Каждое число читается из библиотеки в тесте, а не
сравнивается с записанной заранее константой.

## C ABI

Старые поля не переинтерпретированы. `FcGcsConstraint` получил **отдельный**
массив `int32_t circles[2]`; `points[4]` по-прежнему только точки. Каждый вид
использует один массив, и shim проверяет, что неиспользуемый пуст: индекс,
оставленный в чужом массиве, назвал бы настоящую геометрию не того рода, и
система собралась бы и решилась неверно вместо отказа.

Новый `FcGcsCircle { int32_t center; double radius; }` и новый параметр
`fc_gcs_session_create(..., const FcGcsCircle *circles, size_t circle_count,
...)`. Объявляемые неизвестные — все координаты точек, затем все радиусы.
Радиусы читаются отдельным `fc_gcs_session_radii`, а не как продолжение
`fc_gcs_session_state`: иначе вызывающий, попросивший состояние эскиза с
окружностями, молча получил бы более длинный массив и прочитал радиус как
координату.

Layout сверяется `static_assert` в C++ и `assert_abi_layout()` в Rust перед
первым вызовом каждой сессии — обе стороны объявляют структуры отдельно, и поле,
добавленное в одну, компилируется. Новый вид `FC_GCS_CIRCLE_RADIUS = 8`
дописан в конец нумерации; прежние восемь семейств, drag-жесты и диагностика не
изменились. Radius с нулевым или отрицательным числом отвергается до нативного
вызова и ещё раз в shim.

## Документ и persisted capability

`SketchPointSelector::Center` — своё слово, а не `At`/`Start`, занятое для
случая: центр не позиция точки и не конец линии. `SketchConstraintRule::Radius
{ curve, radius }` адресует **кривую**, потому что радиус не точка чего-либо.

Sketch, у которого хоть одно ограничение говорит про окружность, хранится как
**payload v3** и объявляет новую capability `sketch.constraints.circle.v1`
рядом с `sketch.constraints.v1`.

| Что держит Sketch | Layout | Объявляет |
| --- | --- | --- |
| ничего | v1 | `core.part.v1` |
| Line-ограничения | v2 | + `sketch.constraints.v1` |
| ограничение про окружность | v3 | + `sketch.constraints.circle.v1` |

Почему и новое имя, и новый layout: `sketch.constraints.v1` объявляет
*словарь* — восемь семейств, чьи ссылки суть точки и отрезки линий. Сборка,
реализующая его, читает каждое и перезаписывает Sketch без потерь. `Radius` и
селектор центра этот словарь расширяют, а сборка, написанная под v1, узнать об
этом не может: оставь расширение под тем же именем — и такая сборка увидит
знакомую capability, декодирует узнанное и запишет Sketch назад без
нераспознанного ограничения.

Поэтому двигается и layout. v3 отсутствует в
`readable_schema_versions()` любой прежней сборки: та сохраняет объект
**побайтово** и открывает документ read-only — это и есть отказ, который
защищает данные. Capability делает **причину** читаемой: в таблице
`capabilities`, при negotiation на открытии и человеку, — вместо v3-Sketch,
объявляющего то же, что объявлял v2.

Обе стороны нечестного конверта отказываются прежней проверкой
`require_declared_contract`: v2-заголовок над payload с ограничением про
окружность, v3-заголовок без новой capability и v1-заголовок над любым
ограничением. SQLite-схема не поднималась.

## Evaluator

Одно преобразование, один solve. Окружность регистрируется как геометрия: её
центр — точка (`SketchPointRef` с `Center`), её радиус — `CircleId`. Решение
пишется в **копию** Sketch, которая идёт в арифметику профиля и выбрасывается:
центр через прежний путь точек, радиус — через окружность, о которой solver
отвечал. `closed_curve` профиль строится как раньше; seam endpoint и joint не
появляются, UUID окружности, cap/side refs и порядок кривых сохраняются.

Sketch **без** ограничений по-прежнему не спрашивает solver вообще: прежние
маршруты §25J/L/M работают в сборке, которая никогда не линковала planegcs.
`SketchPresentation` и Scene показывают решённые центр и радиус, потому что
строятся из той же решённой копии.

Незакреплённая окружность в решаемой системе не сдвигается: измерено на
solver boundary двумя независимыми окружностями в обоих порядках.

## Discovery

`inspect --json` не меняет смысла прежних полей. `constraint_edit` получает
**отдельный** список `circles`; `curves` остаётся списком Lines и для
кругового профиля пуст — окружность не изображается ни отрезком, ни двумя
концами.

```json
"constraint_edit": {
  "available": true, "refusal": null, "document_refusal": null,
  "curves": [],
  "circles": [{"curve_id":"UUID","center_mm":[12.0,-7.0],"radius_mm":10.0}],
  "constraints": [
    {"constraint_id":"UUID","rule":{"kind":"radius","curve_id":"UUID","radius":6.75}},
    {"constraint_id":"UUID",
     "rule":{"kind":"fixed","point":{"curve_id":"UUID","at":"center"},"x":-3.5,"y":4.25}}
  ]
}
```

`center_mm`/`radius_mm` здесь — **сохранённое** приближение, то есть стартовая
догадка solver'а. Решённые значения не хранятся и в discovery не приходят: их
даёт rebuild, и `edit-sketch-constraints-copy --json` сообщает про него
`solve.degrees_of_freedom` и `solve.redundant_constraint_ids`. Когда правка
убрала последнее ограничение, решать нечего и `solve` равен `null` — это не
ноль степеней свободы, а отсутствие системы.

`circle_edit` (§25K) для окружности **с** ограничениями становится
`available:false` с причиной: правка геометрии мимо ограничений закрыта. После
удаления всех новых ограничений она снова доступна. `annulus_edit` (§25M) и
Line-поля не меняются.

## Request v1

Прежний `edit-sketch-constraints-copy`, прежний `request_version: 1`, прежние
`remove`/`add`. Старые Line-формы принимаются без изменений. Две новые формы
идут тем же дискриминатором `rule`:

```json
{"rule":"radius","curve_id":"UUID","radius_mm":6.75}
{"rule":"fixed","curve_id":"UUID","at":"center","x_mm":-3.5,"y_mm":4.25}
```

* `radius` — своё правило, а не `distance`: расстояние между двумя точками и
  радиус окружности — разные величины, и один запрос, умеющий написать оба, был
  бы в одном поле от того, чтобы дать окружности длину, которой у неё нет.
* `fixed` — прежнее правило с новым значением `at`. `start`/`end` по-прежнему
  закрепляют конец Line; `center` закрепляет центр окружности. Ни одно не
  подставляется за другое: `centre`, `at`, любое иное написание отвергаются.
* Слоты: один радиус на окружность, один pin на профиль. Замена радиуса — один
  атомарный `remove` + `add`, как у замены длины Line; промежуточной публикации
  нет. Дубликат слота — **структурный** отказ подготовки, и он говорит именно
  это, а не выдаёт себя за диагноз PlaneGCS; настоящий конфликт двух разных
  радиусов измеряется на solver boundary, где его можно поставить через
  Document API.
* Line-правило на круговом профиле и круговое правило на Line-профиле
  отвергаются: запрос просит геометрию, которой там нет.

Прежние bounded чтение (65536 байт), UTF-8 preflight, `--expect-version`,
envelope, `operation` и коды 0/2/7 не менялись. Успех сообщает факты
публикации; потеря stdout после состоявшейся публикации — 7, копия цела.

## UI

Тот же constraint draft и тот же worker. Круговой профиль показывает строку
`Circle · <UUID> · centre (x, y) mm · radius r mm` — сохранённые числа, не
решённые — и поля `Radius mm` плюс прежние `Fixed X/Y (mm)`. Кнопки
`Add radius` и `Add Fixed centre`. Один Apply — один шаг Undo, до 128; Undo и
Redo не обращаются ни к одному job. Save Cancel, отказ, устаревший ответ и
отказ последующего Open сохраняют черновик и историю; поля недоступны, пока
worker занят. Валидация — `validate_edits` того же документа; второй копии
правил в UI нет.

Сохранённые `Radius` и `Fixed centre` показывают собственные UUID и адресуемую
Circle. У каждого есть `Remove`: замена радиуса — отметить прежний UUID и
добавить новый радиус в том же запросе; оба шага доступны Undo/Redo. У формы
окружности нет действий H/V, Line length, Line pair и endpoint pin.

В сборке с OCCT без solver можно создать и cold-rebuild обычный цилиндр или
кольцо, а discovery читает ограничения. Добавление ограничения отказывает
`unsupported` без публикации. Даже удаление последнего ограничения из уже
ограниченного источника требует solver: общий copy job сначала cold-rebuild'ит
исходник и проверяет его refs. `solve: null` относится к результату без
оставшихся ограничений, а не отменяет проверку исходника.

## Рецепт

Нужны OCCT, planegcs, `FERRITECAD` и `FCAD_UFBX_READER`.

```python
# FCAD_25N_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-circle-constraints-"))
DEFLECTION = 0.05
ANGULAR = 0.1

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, center, radius, height):
    """The solid those numbers describe, read out of the exported bytes."""
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

    # Closed and wound one way.
    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            e = (p[k], p[(k + 1) % 3])
            directed[e] = directed.get(e, 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "the mesh is open"

    # Every rim vertex on the solved radius about the solved centre.
    rim = [t for t in tris
           if all(abs(math.hypot(v[0] - center[0], v[1] - center[1]) - radius) < 1e-4
                  for v in t)]
    assert len(rim) >= 12, "no wall at the solved radius"
    lo = [min(v[j] for t in tris for v in t) for j in range(3)]
    hi = [max(v[j] for t in tris for v in t) for j in range(3)]
    assert abs((hi[2] - lo[2]) - height) < 1e-4, (lo, hi)
    for j in (0, 1):
        assert abs((lo[j] + hi[j]) / 2 - center[j]) < 1e-3, (j, lo, hi)
        assert hi[j] - lo[j] >= 2 * radius - 2 * DEFLECTION - 1e-4
    exact = math.pi * radius * radius * height
    assert 0 < volume <= exact * (1 + 1e-6), volume
    assert volume >= math.pi * (radius - DEFLECTION) ** 2 * height
    return volume

# 1. A source the shipped creation command makes.
create = root / "create.json"
source = root / "cylinder.fcad"
create.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                              "radius_mm": 10.0, "height_mm": 15.0}))
assert run(["create-circle-extrude", str(create), "-o", str(source), "--json"])["ok"]

# 2. Explicit UUIDs and the exact version come from one JSON reading.
seen = run(["inspect", str(source), "--json"])["result"]
version = seen["content_version"]
sketch, = seen["sketches"]
discovery = sketch["constraint_edit"]
assert discovery["available"] is True and discovery["refusal"] is None
assert discovery["curves"] == [], "a circle profile offers no Line"
circle, = discovery["circles"]
curve = circle["curve_id"]
assert circle["center_mm"] == [12.0, -7.0] and circle["radius_mm"] == 10.0
assert discovery["constraints"] == []
# The editors of the earlier slices keep their own answers.
assert sketch["editable"] is False and sketch["vertices"] is None
assert sketch["circle_edit"]["available"] is True
assert seen["edit_extrude"]["available"] is True

# 3. A radius and a pinned centre, in one request.
request = root / "request.json"
sized = root / "sized.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": [
    {"rule": "radius", "curve_id": curve, "radius_mm": 6.75},
    {"rule": "fixed", "curve_id": curve, "at": "center",
     "x_mm": -3.5, "y_mm": 4.25}]}))
before = source.read_bytes()
published = run(["edit-sketch-constraints-copy", str(source),
                 "--sketch", sketch["sketch_id"], "--expect-version", version,
                 "--request", str(request), "-o", str(sized), "--json"])
assert published["ok"]
result = published["result"]
assert result["sketch_id"] == sketch["sketch_id"]
assert result["solve"]["degrees_of_freedom"] == 0, result["solve"]
assert result["solve"]["redundant_constraint_ids"] == []
added = [c["constraint_id"] for c in result["added_constraints"]]
assert len(added) == 2 and added[0] != added[1]
assert source.read_bytes() == before, "the source was touched"

# 4. Reopen: the stored sketch is still the starting guess, the constraints are
#    stored beside it, and the cold rebuild resolves every name.
after = run(["inspect", str(sized), "--json"])["result"]
kept = after["sketches"][0]["constraint_edit"]
assert kept["circles"][0]["curve_id"] == curve
assert kept["circles"][0]["center_mm"] == [12.0, -7.0], "solved values overwrote the guess"
assert kept["circles"][0]["radius_mm"] == 10.0
rules = sorted(json.dumps(c["rule"], sort_keys=True) for c in kept["constraints"])
assert any('"kind": "radius"' in r and '6.75' in r for r in rules), rules
assert any('"at": "center"' in r for r in rules), rules
# A constrained circle no longer slips through the unconstrained circle editor.
assert after["sketches"][0]["circle_edit"]["available"] is False
checked = run(["validate", str(sized), "--json"])
assert checked["ok"] and checked["result"]["valid"], checked
rebuilt = run(["rebuild", str(sized), "--cold"])
assert "solid, 3 named faces" in rebuilt, rebuilt
assert "3 of 3 stored references resolved" in rebuilt, rebuilt
named = run(["print-topology", str(sized)])
assert f"extrude side from segment {curve}" in named, named

# 5. The exported solid is the one the solver decided.
volume = measure(sized, (-3.5, 4.25), 6.75, 15.0)
fbx = root / "sized.fbx"
written = run(["export-fbx", str(sized), "-o", str(fbx), "--json"])
assert written["result"]["complete"] and written["result"]["geometries"] == 1
assert fbx.stat().st_size == written["result"]["bytes"]
read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
assert read.returncode == 0, read.stderr
executed = [l for l in read.stdout.splitlines()
            if l.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED")]
assert executed and executed[0].endswith("failures=0"), read.stdout

# 6. Replacement keeps the pin and every other identity.
catalog = run(["inspect", str(sized), "--json"])["result"]
stored = catalog["sketches"][0]["constraint_edit"]["constraints"]
radius_id = next(c["constraint_id"] for c in stored if c["rule"]["kind"] == "radius")
pin_id = next(c["constraint_id"] for c in stored if c["rule"]["kind"] == "fixed")
replaced = root / "replaced.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": [radius_id], "add": [
    {"rule": "radius", "curve_id": curve, "radius_mm": 8.125}]}))
again = run(["edit-sketch-constraints-copy", str(sized),
             "--sketch", sketch["sketch_id"],
             "--expect-version", catalog["content_version"],
             "--request", str(request), "-o", str(replaced), "--json"])
assert again["result"]["removed_constraint_ids"] == [radius_id]
assert again["result"]["solve"]["degrees_of_freedom"] == 0
kept = run(["inspect", str(replaced), "--json"])["result"]["sketches"][0]
assert any(c["constraint_id"] == pin_id for c in kept["constraint_edit"]["constraints"]), \
    "the pin lost its identity in a radius replacement"
wider = measure(replaced, (-3.5, 4.25), 8.125, 15.0)
assert wider > volume

# 7. Removing both constraints hands the sketch back to edit-circle, and the
#    geometry returns to the stored starting guess.
catalog = run(["inspect", str(replaced), "--json"])["result"]
ids = [c["constraint_id"]
       for c in catalog["sketches"][0]["constraint_edit"]["constraints"]]
assert len(ids) == 2
freed = root / "freed.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": ids, "add": []}))
gone = run(["edit-sketch-constraints-copy", str(replaced),
            "--sketch", sketch["sketch_id"],
            "--expect-version", catalog["content_version"],
            "--request", str(request), "-o", str(freed), "--json"])
assert gone["result"]["solve"] is None, "nothing left to solve is not zero freedom"
plain = run(["inspect", str(freed), "--json"])["result"]["sketches"][0]
assert plain["constraint_edit"]["constraints"] == []
assert plain["circle_edit"]["available"] is True
measure(freed, (12.0, -7.0), 10.0, 15.0)

# 8. The existing height edit keeps the constraints and the solved radius.
taller = root / "taller.fcad"
run(["edit-extrude", str(sized), "--feature",
     run(["inspect", str(sized), "--json"])["result"]["features"][0]["feature_id"],
     "--distance-mm", "25", "-o", str(taller), "--json"])
raised = run(["inspect", str(taller), "--json"])["result"]
assert raised["features"][0]["distance_mm"] == 25.0
assert len(raised["sketches"][0]["constraint_edit"]["constraints"]) == 2
measure(taller, (-3.5, 4.25), 6.75, 25.0)

# 9. Refusals publish nothing.
names = sorted(p.name for p in root.iterdir())
for bad in ([{"rule": "radius", "curve_id": curve, "radius_mm": 0.0}],
            [{"rule": "radius", "curve_id": curve, "radius_mm": -4.0}],
            [{"rule": "radius", "curve_id": curve, "radius_mm": 6.75},
             {"rule": "radius", "curve_id": curve, "radius_mm": 8.125}],
            [{"rule": "horizontal", "curve_id": curve}],
            [{"rule": "fixed", "curve_id": curve, "at": "start",
              "x_mm": 0.0, "y_mm": 0.0}]):
    request.write_text(json.dumps({"request_version": 1, "remove": [], "add": bad}))
    refused = run(["edit-sketch-constraints-copy", str(source),
                   "--sketch", sketch["sketch_id"], "--expect-version", version,
                   "--request", str(request), "-o", str(root / "never.fcad"),
                   "--json"], code=2)
    assert refused["ok"] is False
    assert refused["error"]["kind"] in ("input", "unsupported"), refused["error"]
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"
assert source.read_bytes() == before
print("FCAD_25N_RECIPE_OK", round(volume, 6), round(wider, 6))
```
