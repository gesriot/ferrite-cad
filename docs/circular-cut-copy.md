# §26A — первый настоящий Cut в существующем Body

[Локальные доказательства и ограничения](circular-cut-copy-verification.md).
[Решение об истории Body](decisions/0004-feature-predecessor.md).

§26C расширяет этот маршрут на [второй отдельный Cut](sequential-circular-cuts.md);
ниже сохранён контракт первого среза.

Тот же identity-preserving copy-маршрут, что у прежних правок
([§25B](edit-sketch-copy.md), [§25K](edit-circle-copy.md),
[§25M](edit-annular-copy.md), [§25N/O](annular-circle-constraints.md)): один
снимок, проверка версии, read-only источник, baseline cold rebuild,
strict-проверка refs, повторная проверка версии, закрытие SQLite и атомарная
no-clobber публикация. Отличается то, **что** пишется: копия получает второй
Sketch и вторую фичу, а Body остаётся тем же Body.

UI: `Cut circle into <имя> — <UUID>…`; CLI:

```text
ferritecad cut-circular-copy <source.fcad> --body UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
```

## Что появляется

Из плиты 60 × 40 × 10 mm и круглого инструмента r5 в (20, 15):

* глубина 10 mm — сквозное отверстие, **7** аналитических граней, объём
  24000 − π·25·10;
* глубина 4 mm — карман с базовой XY-стороны, **8** граней (появляется дно),
  объём 24000 − π·25·4.

В обоих случаях в копии остаются исходные Sketch и Extrude, добавляются Sketch
инструмента и фича `Cut`, а Body — прежний Body, чей tip теперь эта фича. Это
не импортированный B-Rep и не заменённый профиль: исходная фича по-прежнему
строится, и ссылка на её собственный выход по-прежнему разрешается.

## Семантика истории

Решение записано отдельно — [ADR 4](decisions/0004-feature-predecessor.md) — и
здесь только его следствия:

* фича называет **фичу**, чей результат она меняет: `Extrude.previous`, ребро
  `DependencyRole::Predecessor`;
* Body называет свой `tip_feature`, ребром `BodyTip`, как и раньше;
* владение не хранится дважды: Body владеет тем, до чего дотягивается от tip по
  `previous`. Ребра «фича → Body» нет, поэтому граф ацикличен **по построению**,
  а не потому, что проверка это допустила.

Validator проверяет то, чего построение не даёт: существование и вид
предшественника, наличие ребра, запрет называть одновременно `previous` и
`target_body`, одна фича-потребитель на результат (`feature.forked-history`) и
один Body на tip (`body.shared-tip`) и отсутствие общей истории у Body даже
при разных tip (`body.shared-history`). Прежняя проверка цикла через
`evaluation_order` не ослаблена.

Фича с предшественником хранится как **payload v2** и объявляет
`feature.predecessor.v1`; v2 нет в `readable_schema_versions` прежней сборки —
та сохраняет объект побайтово и открывает документ read-only. Фича, начинающая
тело, остаётся v1. SQL-схема не поднята.

## Поддерживаемый класс

Ровно прежний frame §25K — один untransformed XY DatumPlane, один Sketch, один
forward literal Blind `Extrude`/`NewBody`, его `Body` и ровно три
dependencies — **плюс** одно сужение: Sketch должен быть неограниченным
замкнутым профилем из четырёх Lines, образующих **осепараллельный
прямоугольник**. Это сужение и делает «инструмент внутри детали и не касается её
внешней стенки» измеримым фактом, а не надеждой. Это **не** произвольное тело, и
всё шире отказывается с причиной: импортированный STEP, несколько Body,
изменённая placement, formula-высота, constrained-профиль, не-прямоугольник,
конструкционная геометрия, чужие связи.

Инструмент: одна аналитическая `Circle` **на той же базовой XY-плоскости**,
вырез в **+Z**, конечная положительная Blind-глубина, не больше высоты детали.
UI и CLI называют плоскость и направление прямо. Ни face attachment, ни
произвольных плоскостей, ни Add/Intersect, ни Fillet, ни ThroughAll, ни нового
solver, ни live preview, ни правки уже созданного Cut, ни in-place Save.

## Политика касания

`WALL_CLEARANCE_MM` — это `Tolerance::DEFAULT_LINEAR`, то есть собственный
линейный допуск ядра, а не придуманное здесь число. Инструмент обязан отстоять
от каждой стороны прямоугольника **строго больше** чем на него. Касание внешней
боковой границы этот первый managed-маршрут исключает явно: результат тогда
зависит от округления — то ли паз, то ли sliver-грань, — и число не подправляется
молча, а запрос отказывается.

Ядро отказывает отдельно и по своим причинам: результат, который не ровно один
solid; результат, который `BRepCheck_Analyzer` называет неверным; и boolean,
удаливший ноль материала — инструмент мимо детали даёт совершенно валидное тело,
которое просто снова является деталью, и назвать это фичей было бы неправдой.

## Имена после boolean

`GeometryKernel` получил `cut`, и он отвечает **историей**: для каждой
названной sub-shape каждого входа — `Kept`, `Modified` или `Deleted`, плюс
геометрия, в которую она перешла. Ничего не нумеруется по обходу, ничего не
выбирается «ближайшим».

OCCT работает с `SetNonDestructive(true)`: исходные B-Rep детали и инструмента
должны остаться побайтово прежними. Копия стенки инструмента в результате
отмечается настоящей историей как `Modified`; сохранность формы сама по себе
не означает `Kept`.

Имена копии берутся из этой истории и из прежних семантических ролей:

* грани инструмента — под теми ролями, которые им дала его собственная
  экструзия: `ExtrudeSide` на сегмент (стенка отверстия) и `ExtrudeCap` (дно
  кармана). Сквозное отверстие дна не имеет, и ссылка на него не создаётся;
* грани **прежней** фичи — под новыми ролями `CarriedCap`/`CarriedSide`, потому
  что это другая геометрия: после кармана `ExtrudeCap{End}` под Cut — это дно
  отверстия, а `CarriedCap{End}` — верх детали. Одна роль на оба сделала бы
  ссылку на верх детали разрешающейся в дно дырки;
* удалённое записано как удалённое: ссылка на него отказывается как на
  удалённое, что отличается от «такого имени эта фича никогда не давала».

Ссылка на **исторический выход исходной фичи** (`producer_feature` = Extrude) и
ссылка на **текущий tip** (`producer_feature` = Cut) — разные ссылки, и обе
разрешаются: промежуточное тело остаётся адресуемым, а рисуется и экспортируется
только Body.

Strict-правило refs не ослаблено, а усилено: общий copy job теперь требует,
чтобы разрешились и все ссылки, которые операция **добавила**, — иначе можно
было бы опубликовать геометрию, на которую никто не может указать.

## Кэш

Ключ Cut — `cut_cache_key(kernel, target_key, tool_key, tolerance)` с
собственной `ALGORITHM_VERSION`; `target_key` — это ключ фичи, чей результат
режется, поэтому запись двигается при любом изменении выше по цепочке.
`Extrude::cache_key` тоже кормит `previous`. Архив фичи хранит перенесённые
имена и **список удалённых**, поэтому warm rebuild отвечает на потерянную ссылку
то же, что и cold; формат архива поднят до v2, и запись v1 отказывается
целиком (это кэш — он пересчитывается).

Cold и cache Miss/Hit дают одну геометрию и одно разрешение refs. Body — это
Cut, а не Extrude, и после Miss, и после Hit.

## Discovery

`inspect --json` получает у каждой строки `bodies` отдельное поле `cut_edit`,
не меняя смысла прежних полей:

```json
"bodies": [{
  "body_id": "UUID", "name": "Body",
  "cut_edit": {
    "available": true, "refusal": null, "document_refusal": null,
    "target": {
      "body_id": "UUID", "plane_id": "UUID", "tip_feature_id": "UUID",
      "profile_sketch_id": "UUID", "height_mm": 10.0,
      "extents_mm": [[0.0, 0.0], [60.0, 40.0]],
      "direction": "+z along the plane normal",
      "wall_clearance_mm": 1e-7
    }
  }
}]
```

Работает без ядра: доступность по структуре не обещает установленный kernel, а
применение правки без ядра отказывается. После Cut документ перестаёт быть
четырёхобъектным, и прежние редакторы (`editable`, `circle_edit`,
`constraint_edit`, `annulus_edit`) честно отказывают вместо того, чтобы править
половину истории; в §26C `cut_edit` первой копии разрешает ещё один отдельный Cut;
восьмиобъектный результат отказывает третьему добавлению.

Это относится к редакторам Sketch. Прежняя правка literal Blind-высоты
исходного NewBody остаётся доступна и пересчитывает зависимый Cut; сам Cut
не предлагается общим `edit-extrude`. Body с неизвестными полями payload
отказывается до перезаписи tip, чтобы не потерять эти поля.

## Request v1

Строгий, до 65536 байт, ровно четыре поля, `deny_unknown_fields`:

```json
{"request_version":1,"center_mm":[20.0,15.0],"radius_mm":5.0,"depth_mm":10.0}
```

* `request_version` — целое 1, единственное написание версии.
* `center_mm` — ровно две конечные координаты в mm на базовой XY-плоскости
  детали. Плоскости в запросе нет: этот срез режет только её, и поле, способное
  назвать другую, обещало бы attachment, которого нет.
* `radius_mm` — конечный строго положительный радиус инструмента.
* `depth_mm` — конечная строго положительная глубина в mm вдоль +Z, не больше
  высоты детали. Флага «насквозь» нет: сквозное отверстие получается, когда
  числа так говорят.

JSON v1: `operation:"cut-circular-copy"`, result с `destination`,
`document_id`, `body_id`, `feature_id`, `sketch_id`, `tool_curve_id` и
`previous_feature_id`; один объект + LF, exit 0. Отказ — `ok:false`,
`kind`/`message`/`causes`, exit 2, без публикации. Потеря stdout — exit 7:
публикация могла состояться, копия цела, операцию повторять нельзя без проверки
назначения.

## UI

Действие предлагается только у Body, про который документ сказал, что его можно
резать; у остальных — причина в подсказке. Форма называет плоскость и
направление, показывает размеры детали, её высоту и фичу, которую правка
изменит, и говорит, что глубина, равная высоте, режет насквозь. Четыре поля
(Centre X/Y, Radius, Depth), один подтверждённый **Apply cut** — один шаг
bounded Undo/Redo на все четыре; Undo/Redo не обращается ни к одному job.
Изменение числа после Apply снова закрывает Save, пока его не подтвердят.
Save Cancel, отказ операции, занятое назначение и отказ последующего Open
сохраняют черновик через общий механизм `draft_published`/`draft_load_finished`.
Валидация — `validate_cut` того же документа; второй копии правил в UI нет.

## Рецепт

Нужны OCCT, `FERRITECAD` и `FCAD_UFBX_READER`.

```python
# FCAD_26A_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-circular-cut-"))
DEFLECTION = 0.05
ANGULAR = 0.1
WIDTH, DEPTH, HEIGHT = 60.0, 40.0, 10.0
CENTER, RADIUS = (20.0, 15.0), 5.0

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, depth):
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

    # Closed and wound one way.
    q = lambda v: tuple(round(x, 4) for x in v)
    directed = {}
    for t in tris:
        p = [q(v) for v in t]
        for k in range(3):
            directed[(p[k], p[(k + 1) % 3])] = directed.get((p[k], p[(k + 1) % 3]), 0) + 1
    assert all(v == 1 for v in directed.values()), "not one oriented surface"
    assert all((b, a) in directed for (a, b) in directed), "the mesh is open"

    # The bore wall: on the tool's circle and spanning the depth. A facet of a
    # pocket floor also has its corners on that circle, which is why "wall"
    # means the part of it that is not flat.
    def on_bore(t):
        return all(abs(math.hypot(v[0] - CENTER[0], v[1] - CENTER[1]) - RADIUS)
                   < DEFLECTION + 1e-4 for v in t)
    def spans(t):
        zs = [v[2] for v in t]
        return max(zs) - min(zs) > 1e-4
    wall = [t for t in tris if on_bore(t) and spans(t)]
    assert len(wall) >= 12, "no bore wall"
    def radial(t):
        a, b, c = t
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        nx = u[1] * v[2] - u[2] * v[1]
        ny = u[2] * v[0] - u[0] * v[2]
        mx = (a[0] + b[0] + c[0]) / 3 - CENTER[0]
        my = (a[1] + b[1] + c[1]) / 3 - CENTER[1]
        return nx * mx + ny * my
    assert all(radial(t) < 0 for t in wall), "a boss, not a hole"

    zs = sorted(v[2] for t in wall for v in t)
    assert abs(zs[0]) < 1e-4 and abs(zs[-1] - depth) < 1e-4, (zs[0], zs[-1])

    # Through or not, said by the mesh rather than by the request.
    def covers(t, z):
        if any(abs(v[2] - z) > 1e-4 for v in t):
            return False
        (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12:
            return False
        a = ((y2 - y3) * (CENTER[0] - x3) + (x3 - x2) * (CENTER[1] - y3)) / d
        b = ((y3 - y1) * (CENTER[0] - x3) + (x1 - x3) * (CENTER[1] - y3)) / d
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

    # A mesh bore is inscribed, so it removes a little less than the exact
    # cylinder. Bounded on both sides rather than compared to an analytic pi.
    exact = WIDTH * DEPTH * HEIGHT - math.pi * RADIUS ** 2 * depth
    inscribed = WIDTH * DEPTH * HEIGHT - math.pi * (RADIUS - DEFLECTION) ** 2 * depth
    assert exact - 1e-6 <= volume <= inscribed + 1e-6, volume
    return volume

# 1. A source the shipped creation command makes.
create = root / "create.json"
source = root / "plate.fcad"
create.write_text(json.dumps({"request_version": 1,
                              "points_mm": [[0.0, 0.0], [WIDTH, 0.0],
                                            [WIDTH, DEPTH], [0.0, DEPTH]],
                              "height_mm": HEIGHT}))
assert run(["create-sketch-extrude", str(create), "-o", str(source), "--json"])["ok"]

# 2. The body, its version and what a cut would be cutting, from one reading.
seen = run(["inspect", str(source), "--json"])["result"]
version = seen["content_version"]
body, = seen["bodies"]
cut_edit = body["cut_edit"]
assert cut_edit["available"] is True and cut_edit["refusal"] is None
target = cut_edit["target"]
assert target["height_mm"] == HEIGHT
assert target["extents_mm"] == [[0.0, 0.0], [WIDTH, DEPTH]]
assert target["direction"] == "+z along the plane normal"
tip = target["tip_feature_id"]
before = source.read_bytes()

request = root / "request.json"
results = {}
for name, depth, faces, refs in (("holed", HEIGHT, 7, 10), ("pocket", 4.0, 8, 11)):
    copy = root / f"{name}.fcad"
    request.write_text(json.dumps({"request_version": 1, "center_mm": list(CENTER),
                                   "radius_mm": RADIUS, "depth_mm": depth}))
    published = run(["cut-circular-copy", str(source), "--body", body["body_id"],
                     "--expect-version", version, "--request", str(request),
                     "-o", str(copy), "--json"])
    assert published["ok"]
    result = published["result"]
    # The body is the body it was, and the cut consumes what was the tip.
    assert result["body_id"] == body["body_id"]
    assert result["previous_feature_id"] == tip
    assert source.read_bytes() == before, "the source was touched"

    # 3. Reopen: the history is two features long and the body tips at the cut.
    after = run(["inspect", str(copy), "--json"])["result"]
    assert len(after["features"]) == 2
    assert after["bodies"][0]["body_id"] == body["body_id"]
    # The four-object editors honestly step back from a longer history.
    assert after["bodies"][0]["cut_edit"]["available"] is False
    for sketch in after["sketches"]:
        assert sketch["editable"] is False
        assert sketch["circle_edit"]["available"] is False
        assert sketch["constraint_edit"]["available"] is False
    checked = run(["validate", str(copy), "--json"])
    assert checked["ok"] and checked["result"]["valid"], checked
    rebuilt = run(["rebuild", str(copy), "--cold"])
    assert f"{refs} of {refs} stored references resolved" in rebuilt, rebuilt
    assert "tip Cut" in rebuilt, rebuilt

    named = run(["print-topology", str(copy)])
    assert "carried cap start" in named and "carried cap end" in named, named
    assert named.count("carried side from segment") == 4, named

    # 4. The exported solid is the part with that cut in it.
    results[name] = measure(copy, depth)
    fbx = root / f"{name}.fbx"
    written = run(["export-fbx", str(copy), "-o", str(fbx), "--json"])
    assert written["result"]["complete"]
    # One finished body, not the tool and two intermediates.
    assert written["result"]["geometries"] == 1
    read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
    assert read.returncode == 0, read.stderr
    executed = [l for l in read.stdout.splitlines()
                if l.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED")]
    assert executed and executed[0].endswith("failures=0"), read.stdout

assert results["pocket"] > results["holed"], "a pocket removes less than a hole"

# 5. Refusals publish nothing.
names = sorted(p.name for p in root.iterdir())
for center, radius, depth in ((CENTER, 0.0, 4.0), (CENTER, -5.0, 4.0),
                              (CENTER, RADIUS, 0.0), (CENTER, RADIUS, HEIGHT + 1),
                              ((0.0, 15.0), RADIUS, 4.0), ((5.0, 15.0), RADIUS, 4.0),
                              ((200.0, 200.0), RADIUS, 4.0)):
    request.write_text(json.dumps({"request_version": 1, "center_mm": list(center),
                                   "radius_mm": radius, "depth_mm": depth}))
    refused = run(["cut-circular-copy", str(source), "--body", body["body_id"],
                   "--expect-version", version, "--request", str(request),
                   "-o", str(root / "never.fcad"), "--json"], code=2)
    assert refused["ok"] is False
    assert refused["error"]["kind"] in ("input", "unsupported"), refused["error"]
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"
assert source.read_bytes() == before
print("FCAD_26A_RECIPE_OK", round(results["holed"], 6), round(results["pocket"], 6))
```
