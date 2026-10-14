# §31F — направленный угол между двумя сохранёнными Lines

Новое ограничение `SketchConstraintRule::DirectedAngle` — **постоянная связь двух
направленных векторов** двух разных Lines одного собственного XY Line-профиля. Это
настоящее уравнение PlaneGCS (`addConstraintL2LAngle`), а не однократный поворот
координат: сохранённые координаты, UUID, ссылки и остальная SQL-информация
остаются теми же; меняются только список ограничений выбранного Sketch, его
layout/версия и строка `capabilities`. Путь тот же, что у остальных: форма
«Sketch constraints» → прежняя команда `edit-sketch-constraints-copy`, общий job и
writer guard. Новой команды, схемы SQLite, native pin и второго solver нет.

Граница среза: **два существующих Line (не construction) поддерживаемого XY-профиля**.
Дуги, касание, коллинеарность, симметрия, выражения, произвольные плоскости и
Diameter-подобные «новые операции» не входят. Hover-опоры §31D и привязка к центру
окружности §31C остаются открытыми; этот срез их не закрывает.

## Семантика (принято, не заменять другой)

* Вектор назван **двумя упорядоченными концами** Line: из `from` в `to`
  (`start→end` либо `end→start`). Угол — **против часовой стрелки** от первого
  вектора ко второму в локальных XY эскиза (Y вверх):
  `θ = atan2(cross(a, b), dot(a, b))`.
* Канонический диапазон `(−180°, 180°]`. `0` — сонаправленность, `180` —
  противоположность, и она хранится **только** как `+180`. `−0` нормализуется в
  `0`. Ввод — **одна полная окружность** `[−180, 180]`: ровно `−180` читается как
  `+180`, всё вне диапазона (`181`, `270`, `360`, `−181`, `180.000001`) —
  **отказ**, а не оборот на `360`: арифметика с плавающей запятой политику не
  выбирает.
* Границы единиц: **градусы** — только UI и CLI (`angle_deg`, поле формы);
  **радианы** — документ и solver (`angle` в `(−π, π]`, `180° ≡ π`). Конверсия одна
  (`DirectedAngle::from_degrees`); обратная (`degrees()`) выбирает округление до
  `1e-9°` только если его повторный разбор даёт те же биты радиан. Иначе выдаётся
  полное число: ненулевой угол не становится нулём, сосед −180° не становится −180°.
  Ненулевой ввод, исчезающий при переводе в радианы, отказывается. Сохранённое
  значение — радианы, без округления. Угол Revolve (`0.01° … 359.99°`, отдельный
  параметр тела) и его политика здесь не используются; `Length` направляющей §31D
  — другая величина другого поля.
* Перестановка векторов меняет знак с нормализацией (`30 → −30`, `180 → 180`);
  разворот одного вектора добавляет `180` с нормализацией (`30 → −150`,
  `180 → 0`); разворот обоих возвращает исходный угол. Направления хранятся как
  сказаны и **не** переписываются в «каноническую» форму: ни сортировка UUID, ни
  `whole_line` (который для ненаправленных связей справедливо игнорирует обратный
  `SegmentRef`) их не теряют — слот читает только то, **какие** Lines названы.
* Нулевая длина вектора — **отказ**, не угол `0`: в запросе (`from == to`
  непроизносимо), в solver до native (`SolverError::DegenerateVector`, также
  вектор короче `1e-9` длины эскиза), в shim (тот же отказ уже в
  `fc_gcs_session_create`) и **после** решения: схлопнутый вектор даёт невязку `+∞`
  и `DidNotConverge`, а не «выполнено» (`atan2(0, 0)` иначе удовлетворило бы `0°`).
  Невязка — `wrap(atan2(cross, dot) − θ)` с переносом через `±180`: решение `+30`
  не удовлетворяет `−30` (промах `60°`), `0` не удовлетворяет `180` (промах `180°`),
  а `170` и `−170` — ближний путь `20°`, а не `340°`.
* `Parallel` остаётся прежним ненаправленным отношением (нулевое векторное
  произведение допускает оба направления). Старые ограничения и их wire-формы не
  меняются.

## Слот и дубликаты (авторское решение, помечено для ревью)

Угол отвечает на **тот же вопрос об относительной ориентации пары**, что `Parallel`
и `Perpendicular`, и занимает **тот же слот** `Relation(sorted(A, B))`. Поэтому
повторный угол, угол на паре с `Parallel`/`Perpendicular` и **любая эквивалентная
запись** (`(B,A,−θ)`, `(A обратный, B, θ+180)` …) — структурный отказ до solver без
публикации; замена — точное «remove UUID + add» в одном запросе (проверка по
retained состоянию). `EqualLength` на той же паре — другое свойство, собственный
слот. Пара — две **разные** Lines этого Sketch. Избыточность и противоречие через
несколько пар (четыре прямых угла замкнутого контура, угол против Parallel другой
пары) устанавливает **только solver** (`redundant_constraint_ids`, typed
`constraint_conflict` с UUID): анализа транзитивности нет. Альтернатива —
отдельный слот ради solver-избыточности `Parallel + Angle(0)` — отвергнута: она
превратила бы «один ответ на пару» в два.

## Строгий запрос CLI (request v1, аддитивно)

```json
{"rule": "directed_angle",
 "a": {"curve_id": "<uuid>", "from": "start", "to": "end"},
 "b": {"curve_id": "<uuid>", "from": "end", "to": "start"},
 "angle_deg": 30}
```

Все поля обязательны; лишние, неизвестные и повторные — отказ на каждом уровне;
`a`, `b` — объекты (массив «по порядку полей» отвергается явной проверкой, потому что
иначе `["<uuid>","start","end"]` разобрался бы как вектор); `from`/`to` — только
`start`/`end` и разные; `angle_deg` — конечное число JSON в `[−180, 180]`
(`NaN`, `Infinity`, `1e999`, строка, `null`, bool — отказ). Любое нарушение —
`kind:"input"`, код 2, без публикации. Прежние формы и ответы прежних видов не
менялись.

Ответ (`added_constraints[]`, `inspect`, `constraint_conflict`) для нового вида —
отдельный `kind`, с теми же `Segment`-формами, что у `parallel`:

```json
{"kind": "directed_angle",
 "a": {"from": {"curve_id": "…", "at": "start"}, "to": {"curve_id": "…", "at": "end"}},
 "b": {"from": {"curve_id": "…", "at": "end"}, "to": {"curve_id": "…", "at": "start"}},
 "angle_deg": 30}
```

## Совместимость и capability

Sketch с таким ограничением хранится как **payload v4** и требует
`sketch.constraints.v1` + `sketch.constraints.angle.v1` (она же в
`SUPPORTED_CAPABILITIES`). v4 кумулятивен (читатель угла знает и словарь
окружностей). Сборка main читает только `[3, 2, 1]`: v4 она **сохраняет дословно** и
открывает документ read-only; ни `Parallel`, ни `Perpendicular` её не подменяют
(измерено, ниже). Sketch без угла остаётся v1/v2/v3; удаление последнего угла
возвращает прежний layout. Версия SQLite-схемы **не** повышалась: механизма
payload-версии и capability достаточно.

## Solver, shim и ABI

Новый вид shim `FC_GCS_DIRECTED_ANGLE = 9`; старые номера `0…8` не
переинтерпретируются. `points[0..4] = a.from, a.to, b.from, b.to`, `circles = [-1,-1]`,
`value` — угол в радианах, `value2` не используется и обязан быть `0`. Раскладка
`FcGcsConstraint` (48 байт, смещения 0/4/20/32/40) и `FcGcsCircle` (16 байт) **не
меняются**; обе стороны ABI по-прежнему утверждают её (`static_assert` и
`assert_abi_layout`), а ручной тест вызывает `fc_gcs_session_create` напрямую и
проверяет: вид 9 строится; `value2 ≠ 0`, непустые `circles`/неиспользованный `points[3]`,
`value ∉ (−π, π]`, `NaN`, вектор из одной точки или нулевой длины, чужой индекс и
неизвестные виды `−1, 10, 11, 255, i32::MAX` — `NULL`. Применено то, что уже есть в
закреплённом PlaneGCS (FreeCAD 1.0.1, `ConstraintL2LAngle`: невязка
`atan2` повёрнутого вектора, перенос через `±π` встроен); pin и вторая библиотека не
менялись. Shim — статическая часть Rust-бинаря, поэтому ранее собранная сборка main
сохраняет старый shim, и это используется для проверки совместимости.

## Форма в окне

В форме «Sketch constraints» Line-профиля **новых строк нет**: форма и так держится
на экране в 768 pt (прежний gate доступности действий не менялся и проходит),
поэтому управление угловым ограничением встроено в две существующие строки пары,
а ширина осталась прежней (правый край 583.5 pt при 598 pt самой широкой прежней
строки):

```
Line pair: [Pair Line A] [Pair Line B] Pair: A ab12cd · B ef34gh  [A: start→end] [B: start→end]
[Add Equal length] [Add Parallel] [Add Perpendicular]  A→B (°): [ CCW + ] [Add directed angle]
```

Кнопки `A: start→end` / `B: start→end` явно говорят, откуда и куда идёт вектор
выбранной Line, и разворачивают его нажатием (`A: end→start`); подсказка
(tooltip) называет сохранённые координаты `A runs (0, 0) → (40, 0) mm (stored, Line
ab12cd)`. Выбор направления — не шаг истории. Поле подписано `A→B (°)`, с
подсказкой `CCW +` в пустом поле и tooltip «против часовой от вектора A к вектору B,
Y вверх; 0 — то же направление, 180 — противоположное; вне −180…180 ничего не
оборачивается». У поля свой `id` (`directed-angle-deg`): оно не смешано ни с длиной
Line, ни с углом Revolve, ни с `Length` направляющей. `Add directed angle`
(недоступна, пока пара не выбрана) разбирает текст и отказывает вслух, не меняя
модель и оставляя текст в форме: пусто, `-`, нечисло, `30,5`, `NaN`, `inf`,
`1e999`, вне `[−180, 180]`, Unicode-минус. Принятое добавление — **один шаг draft**
и один `AddLineConstraint::DirectedAngle` в запросе, который проверяет тот же
`validate_edits`; повторный ответ о паре (`−150` с развёрнутым A, Parallel) отказывает
словом «directed angle». В «Pending additions» запись читается как `Directed angle
30° · A: Line … start→end · B: Line … end→start`; в «Persisted constraints» —
`Directed angle −70° · <UUID>` и `A Line … start→end → B Line … end→start`, со своим
`Remove`; **remove exact UUID + Add** заменяет его тем же запросом. Undo/Redo, Cancel
Save, отказ solver, Apply к несохранённой `DocumentSession`, экспорт до Save и
переключение вкладок проходят через прежний код: форма и её draft — те же. Пока
идёт job, поле ввода — `TextEdit::interactive(false)`, а не только отключённый `Ui`:
сфокусированное поле не принимает текст (тест). Почему не отдельные строки: первая раскладка с отдельными строками вектора, подсказки
и угла переполнила 768 pt, и четыре прежних gate (доступность `Clear pending
changes`, `Save constraints copy…`, `Apply constraints` в Revolve- и chamfer-сессиях)
упали; укороченный список Lines сломал клики по шестому сегменту профиля Revolve.
Остались две существующие строки, ширина прежняя, высота не выросла.

## Выполняемый рецепт

Нужны OCCT + PlaneGCS и `FERRITECAD` — путь к `ferritecad`. На macOS native library
paths экспортируются в том же shell. Рецепт создаёт свой временный каталог, не
меняет fixtures и не требует заранее известных UUID. Без ядра он печатает
`FCAD_31F_RECIPE_NO_KERNEL`, с ядром без solver — `FCAD_31F_RECIPE_NO_SOLVER` (при
`FCAD_EXPECT_SOLVER=1` это провал), с обоими — `FCAD_31F_RECIPE_OK` с измеренными
числами. Угол и знак читаются из **STL** (вершины основания → кольцо → `atan2`), не из
ответа CLI; Line 0 закреплена в начале координат и сделана горизонтальной только
затем, чтобы кольцо читалось как Line 0, 1, 2, 3 без помощи solver.

```python
# FCAD_31F_AGENT_RECIPE
import hashlib, json, math, os, pathlib, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
expect_solver = os.environ.get("FCAD_EXPECT_SOLVER") == "1"
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-directed-angle-"))
OP = "edit-sketch-constraints-copy"

def call(args, code=0):
    """One CLI call; the JSON answer, or the error it refused with."""
    p = subprocess.run([cli, *map(str, args), "--json"], capture_output=True, encoding="utf-8")
    if p.returncode == 7:
        raise RuntimeError("Report lost; inspect the destination. Do not retry automatically.")
    data = json.loads(p.stdout)
    assert data["schema_version"] == 1 and p.returncode == code, (args, p.returncode, p.stdout)
    return data["result"] if code == 0 else data["error"]

def need(args, what, code=0):
    """A step that needs the kernel (or, for constraints, the solver): a build without
    it refuses typed, and the recipe says so and stops, unless the solver is expected."""
    p = subprocess.run([cli, *map(str, args), "--json"], capture_output=True, encoding="utf-8")
    if p.returncode == 2:
        error = json.loads(p.stdout)["error"]
        if error["kind"] == "unsupported" and "Open CASCADE" in error["message"]:
            print("FCAD_31F_RECIPE_NO_KERNEL", json.dumps(error)); sys.exit(0)
        if error["kind"] == "unsupported" and "sketch solver" in error["message"]:
            if expect_solver:
                sys.exit(f"FCAD_EXPECT_SOLVER=1 but this build has no sketch solver: {error}")
            print("FCAD_31F_RECIPE_NO_SOLVER", json.dumps(error)); sys.exit(0)
    assert p.returncode == 0, (what, p.returncode, p.stdout)
    return json.loads(p.stdout)["result"]

def deg_turn(u, v):
    """Counter-clockwise turn from u to v, in degrees: atan2(cross, dot)."""
    return math.degrees(math.atan2(u[0]*v[1] - u[1]*v[0], u[0]*v[0] + u[1]*v[1]))

def base_ring(path):
    """The solved footprint, from the exported STL alone: the base face's boundary
    as an ordered ring, counter-clockwise, plus the solid's volume."""
    stl = path.with_suffix(".stl")
    body = call(["inspect", path])["bodies"][0]["body_id"]
    call(["export-stl", path, "--solid", body, "-o", stl])
    data = stl.read_bytes()
    n = struct.unpack_from("<I", data, 80)[0]
    assert len(data) == 84 + 50 * n
    edges, six = {}, 0.0
    for i in range(n):
        f = struct.unpack_from("<12fH", data, 84 + 50 * i)
        a, b, c = f[3:6], f[6:9], f[9:12]
        six += (a[0]*(b[1]*c[2]-b[2]*c[1]) + a[1]*(b[2]*c[0]-b[0]*c[2]) + a[2]*(b[0]*c[1]-b[1]*c[0]))
        if all(abs(p[2]) < 1e-6 for p in (a, b, c)):
            k = [(round(p[0], 4), round(p[1], 4)) for p in (a, b, c)]
            for u, v in ((0, 1), (1, 2), (2, 0)):
                key = tuple(sorted((k[u], k[v])))
                edges[key] = edges.get(key, 0) + 1
    adj = {}
    for u, v in (e for e, c in edges.items() if c == 1):
        adj.setdefault(u, []).append(v); adj.setdefault(v, []).append(u)
    start = next(iter(adj)); ring = [start]; prev, cur = None, start
    while True:
        nxt = [w for w in adj[cur] if w != prev]
        if not nxt or nxt[0] == start: break
        prev, cur = cur, nxt[0]; ring.append(cur)
    assert len(ring) == len(adj) == 4, ring
    area = sum(ring[i][0]*ring[(i+1) % 4][1] - ring[i][1]*ring[(i+1) % 4][0] for i in range(4)) / 2
    if area < 0:
        ring.reverse(); area = -area
    assert math.isclose(abs(six) / 6, area * 10, rel_tol=1e-5), (six / 6, area * 10)  # independent volume
    return ring, area, abs(six) / 6

def corners(path):
    """Line k runs from vertex k to vertex k+1 of the ring started at the pinned
    corner; corner k is the counter-clockwise turn from Line k to Line k+1."""
    ring, area, volume = base_ring(path)
    first = min(range(4), key=lambda i: math.hypot(*ring[i]))
    ring = ring[first:] + ring[:first]
    assert math.hypot(*ring[0]) < 1e-3 and abs(ring[1][1]) < 1e-3 and ring[1][0] > 1, ring
    edge = [(ring[(i+1) % 4][0] - ring[i][0], ring[(i+1) % 4][1] - ring[i][1]) for i in range(4)]
    return [deg_turn(edge[i], edge[(i+1) % 4]) for i in range(4)], area, volume

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_mtime_ns

def make(points):
    (root / "profile.json").write_text(json.dumps({"request_version": 1, "points_mm": points, "height_mm": 10}))
    source = root / f"source-{len(list(root.glob('source-*.fcad')))}.fcad"
    need(["create-sketch-extrude", root / "profile.json", "-o", source], "create")
    return source, call(["inspect", source])

def edit(source, catalog, remove, add, out, code=0):
    (root / "request.json").write_text(json.dumps({"request_version": 1, "remove": remove, "add": add}))
    args = [OP, source, "--sketch", catalog["sketches"][0]["sketch_id"], "--expect-version",
            catalog["content_version"], "--request", root / "request.json", "-o", out]
    if code != 0:
        error = call(args, code)
        assert not out.exists(), "a refusal published"
        return error
    return need(args, "edit")

def ids(catalog):
    return [c["constraint_id"] for c in catalog["sketches"][0]["constraint_edit"]["constraints"]]

def vec(curves, i, f="start", t="end"):
    return {"curve_id": curves[i], "from": f, "to": t}

def turn_rule(a, b, degrees):
    return {"rule": "directed_angle", "a": a, "b": b, "angle_deg": degrees}

def said(v):
    return {"from": {"curve_id": v["curve_id"], "at": v["from"]}, "to": {"curve_id": v["curve_id"], "at": v["to"]}}

# 1. A dart: Line 1 meets Line 2 at a reflex corner that turns about -53 degrees.
#    Line 0 is pinned at the origin and made horizontal, only so that the exported
#    footprint can be read back as Line 0, 1, 2, 3 without any help from the solver.
source, catalog = make([[0, 0], [40, 0], [10, 10], [0, 40]])
edit_view = catalog["sketches"][0]["constraint_edit"]
curves = [c["curve_id"] for c in edit_view["curves"]]
assert edit_view["available"] and not edit_view["constraints"] and len(curves) == 4
untouched = sha(source)
V = lambda i, f="start", t="end": vec(curves, i, f, t)
pins = [{"rule": "fixed", "curve_id": curves[0], "at": "start", "x_mm": 0, "y_mm": 0},
        {"rule": "horizontal", "curve_id": curves[0]}]

# 2. -70 degrees counter-clockwise from Line 1 to Line 2, said five ways. Every
#    spelling is one relation: swapping the vectors negates the angle, turning one
#    of them adds 180 degrees (110 = -70 + 180), turning both changes nothing.
spellings = [("A to B", V(1), V(2), -70.0), ("B to A", V(2), V(1), 70.0),
             ("A turned", V(1, "end", "start"), V(2), 110.0),
             ("B turned", V(1), V(2, "end", "start"), 110.0),
             ("both turned", V(1, "end", "start"), V(2, "end", "start"), -70.0)]
published = {}
for i, (name, a, b, degrees) in enumerate(spellings):
    out = root / f"dart-{i}.fcad"
    done = edit(source, catalog, [], pins + [turn_rule(a, b, degrees)], out)
    rule = done["added_constraints"][-1]["rule"]
    assert rule == {"kind": "directed_angle", "a": said(a), "b": said(b), "angle_deg": degrees}, (name, rule)
    assert done["solve"]["degrees_of_freedom"] == 4 and done["solve"]["redundant_constraint_ids"] == []
    turns, area, volume = corners(out)
    assert abs(turns[1] + 70) < 1e-3, (name, turns)          # the reflex corner, measured
    reflex_turn = turns[1]
    assert turns[0] > 0 and turns[2] > 0 and turns[3] > 0 and area > 0, (name, turns)
    assert abs(sum(turns) - 360) < 1e-2
    published[name] = (out, done, round(volume, 3))
assert len({v[2] for v in published.values()}) == 1, "five spellings, one solid"

# 3. +50 is another shape, not the mirror image of the reflex corner: it is convex.
mirror = root / "dart-mirror.fcad"
edit(source, catalog, [], pins + [turn_rule(V(1), V(2), 50.0)], mirror)
turns, area, mirror_volume = corners(mirror)
mirror_turn = turns[1]
assert abs(turns[1] - 50) < 1e-3 and all(t > 0 for t in turns), turns
assert abs(mirror_volume - published["A to B"][2]) > 100

# 4. Replace the angle by its exact UUID in one request; every other UUID, rule and
#    order survives. Then remove it: the freedom comes back.
first, first_done, _ = published["A to B"]
first_catalog = call(["inspect", first])
angle_id = first_done["added_constraints"][-1]["constraint_id"]
replaced = root / "dart-replaced.fcad"
r = edit(first, first_catalog, [angle_id], [turn_rule(V(1), V(2), -60.0)], replaced)
assert r["removed_constraint_ids"] == [angle_id] and r["solve"]["degrees_of_freedom"] == 4
after = call(["inspect", replaced])
assert ids(after) == [i for i in ids(first_catalog) if i != angle_id] + [r["added_constraints"][0]["constraint_id"]]
replaced_turn = corners(replaced)[0][1]
assert abs(replaced_turn + 60) < 1e-3
removed = root / "dart-free.fcad"
r = edit(replaced, after, [r["added_constraints"][0]["constraint_id"]], [], removed)
assert r["added_constraints"] == [] and r["solve"]["degrees_of_freedom"] == 5
assert all(c["rule"]["kind"] != "directed_angle" for c in
           call(["inspect", removed])["sketches"][0]["constraint_edit"]["constraints"])

# 5. Structural refusals before any solving: the pair holds one relation, however it
#    is said; two different Lines, each from one end to the other; one full circle.
def refused(request, source=first, catalog=first_catalog):
    error = edit(source, catalog, [], [request], root / "never.fcad", 2)
    assert error["kind"] == "input" and "constraint_conflict" not in error, error
for request in [turn_rule(V(1), V(2), -70.0), turn_rule(V(2), V(1), 70.0),
                turn_rule(V(1, "end", "start"), V(2), 110.0), turn_rule(V(2), V(1), 45.0),
                {"rule": "parallel", "a_curve_id": curves[1], "b_curve_id": curves[2]},
                {"rule": "perpendicular", "a_curve_id": curves[2], "b_curve_id": curves[1]},
                turn_rule(V(3), V(3, "end", "start"), 90.0),
                turn_rule(V(3), V(3, "start", "start"), 90.0),
                turn_rule(V(2), {"curve_id": "00000000-0000-7000-8000-000000000000", "from": "start", "to": "end"}, 90.0),
                turn_rule(V(2), V(3), 181.0), turn_rule(V(2), V(3), -181.0), turn_rule(V(2), V(3), 270.0),
                {**turn_rule(V(2), V(3), 90.0), "extra": 1},
                {"rule": "directed_angle", "a": V(2), "angle_deg": 90.0},
                {"rule": "directed_angle", "a": V(2), "b": {"curve_id": curves[3], "from": "start"}, "angle_deg": 90.0}]:
    refused(request)

# 6. The solver's own answers, with the UUIDs of their reasons: a closed contour
#    of right turns. The fourth turn is implied by the three before it (redundant, and
#    still published); turned the other way it cannot hold (a conflict, nothing published).
slanted, scatalog = make([[-20, -10], [40, -8], [42, 30], [-20, 30]])
scurves = [c["curve_id"] for c in scatalog["sketches"][0]["constraint_edit"]["curves"]]
S = lambda i, j, degrees: turn_rule(vec(scurves, i), vec(scurves, j), degrees)
rect = root / "right-turns.fcad"
r = edit(slanted, scatalog, [], [S(0, 1, 90.0), S(1, 2, 90.0), S(2, 3, 90.0)], rect)
assert r["solve"]["degrees_of_freedom"] == 5 and r["solve"]["redundant_constraint_ids"] == []
four = root / "right-turns-4.fcad"
r = edit(slanted, scatalog, [], [S(0, 1, 90.0), S(1, 2, 90.0), S(2, 3, 90.0), S(3, 0, 90.0)], four)
minted = [c["constraint_id"] for c in r["added_constraints"][4:]]
assert len(r["solve"]["redundant_constraint_ids"]) == 1 and r["solve"]["redundant_constraint_ids"][0] in minted
assert r["solve"]["degrees_of_freedom"] == 5
redundant = len(r["solve"]["redundant_constraint_ids"])
error = edit(slanted, scatalog, [], [S(0, 1, 90.0), S(1, 2, 90.0), S(2, 3, 90.0), S(3, 0, -90.0)],
             root / "contradiction.fcad", 2)
assert error["kind"] == "constraint", error
reasons = error["constraint_conflict"]["constraints"]
assert reasons and all(c["rule"]["kind"] == "directed_angle" and isinstance(c["constraint_id"], str)
                       and c["rule"]["angle_deg"] in (90.0, -90.0) for c in reasons), reasons

# 7. The opposite is +180 and nothing else: -180 is read as +180, and the response
#    says 180.
box, bcatalog = make([[0, 0], [60, 0], [60, 30], [0, 30]])
bcurves = [c["curve_id"] for c in bcatalog["sketches"][0]["constraint_edit"]["curves"]]
opposite = root / "opposite.fcad"
r_opposite = edit(box, bcatalog, [], [turn_rule(vec(bcurves, 0), vec(bcurves, 2), -180.0)], opposite)
assert r_opposite["added_constraints"][-1]["rule"]["angle_deg"] == 180.0
assert r_opposite["solve"]["degrees_of_freedom"] == 7

assert sha(source) == untouched, "the source changed"
assert not (root / "never.fcad").exists()
print("FCAD_31F_RECIPE_OK", f"reflex={reflex_turn:.3f} mirror={mirror_turn:.3f} replaced={replaced_turn:.3f}",
      f"volumes={published['A to B'][2]}/{mirror_volume:.3f} redundant={redundant} conflict_reasons={len(reasons)}",
      f"opposite={r_opposite['added_constraints'][-1]['rule']['angle_deg']}")
```

Результат этого рецепта на native-сборке (OCCT + PlaneGCS 1.0.1), измерено по STL:

```
FCAD_31F_RECIPE_OK reflex=-70.000 mirror=50.000 replaced=-60.000 volumes=2533.364/11097.030 redundant=1 conflict_reasons=4 opposite=180.0
```

Пять записей одного и того же `−70°` дают одно тело (2533.364 mm³), `+50°` — другое
(11097.030 mm³, все четыре угла выпуклые). Без ядра рецепт печатает
`FCAD_31F_RECIPE_NO_KERNEL`, с ядром без solver — `FCAD_31F_RECIPE_NO_SOLVER`.

## Что исполнено (2026-10-14, Mac arm64, ветка `directed-sketch-angle` от `be7b36a`)

Базовый CI точного merge `be7b36a`: `CI`, `planegcs pin`, `rust notices`, `rust sbom`,
`product sbom` — success; `combined runtime layout` на момент проверки ещё
выполнялся (**не** объявлен успешным и не перезапускался). **CI этого diff не
запускался**: всё ниже — локальные прогоны.

| Слой | Что доказано | Прогон |
| --- | --- | --- |
| Solver, реальный PlaneGCS | `+θ`≠`−θ` (измерено `atan2` по решённым точкам), `0`≠`180`, ±170 через границу — ближний путь, пять записей одного отношения, `Parallel` в обе стороны, DOF и по одному уравнению на угол, четыре прямых угла замкнутого контура: четвёртый избыточен (id вызывающего), противоположный — конфликт, схлопнутый вектор → `DidNotConverge{Some(inf)}`, drag | `tests/angle.rs` (9) |
| Контракт solver без native | диапазон `(−π, π]` отказывается, не оборачивается; вектор из одной точки/нулевой длины; недостающая точка на каждой из 4 позиций | `tests/boundary.rs` (+3, 13), residual (unit) |
| C ABI | вид 9 строится; `value2 ≠ 0`, непустые `circles`/`points[3]`, `value ∉ (−π, π]`, NaN, нулевой вектор, чужой индекс, виды `−1, 10, 11, 255, i32::MAX` → `NULL`; виды 5/6/7 не сдвинулись; раскладка 48/16 байт утверждена обеими сторонами | `planegcs::tests::the_shim_names_the_new_kind…` |
| Документ | конверсия (каждая тысячная доля градуса, ±180, −0, соседи границы), слот и все эквивалентные записи, layout v4 и capability в строке `capabilities`, v2-заголовок над payload с углом и v4-заголовок без угла — отказ в обе стороны, старые семейства остаются v2 | `tests/directed_angle.rs` (4) |
| Writer guard | подложенный план не кладёт второй ответ на пару с углом (согласованный список + слот), не разворачивает/не переоценивает/не переносит сохранённый угол, значение вне диапазона не доходит до строки | `constraint_write_tests::…rewrite_a_stored_angle` |
| Evaluator | направления и радианы пересекают границу как есть; новые отказы solver названы по UUID | `solve::tests::a_directed_angle_keeps_both_directions…` |
| CLI-процесс | строгий запрос (29 нарушений формы и диапазона и ещё NaN, Infinity, 1e999, дубликат ключа, escaped-дубликат); discovery и прежние формы без изменений; слот и «чужие» Lines до solver; профиль-стрела (dart) с вогнутым углом: пять записей `−70°` → `reflex` измерен по решённым точкам файла после reopen + cold rebuild, `+50°` — выпуклый; четырёхугольник `+170°/−170°/±180/0°`: наклон дальней стороны в разные стороны, `−180` отвечает как `+180`, `0°` отказывает политикой профиля; replace/remove по UUID, остальные UUID, правила и порядок сохранены; три прямых угла (DOF 5), четвёртый — избыточен, `−90` — typed conflict с UUID; stale snapshot, занятое назначение, exit 7 после публикации (обе схемы потери вывода), источник побайтово цел; STL независимо разобран, FBX прочитан | `tests/directed_angle.rs` (8) |
| Cache | один sidecar для двух копий с одним сохранённым приближением и разными углами: `Miss` (не устаревший `Hit`), затем `Hit` с тем же объёмом | `native_directed_angle_cache_keys_follow_the_solved_profile` |
| Окно (widgets) | пара, направление каждой стороны, угол: один принятый Add — один шаг; 15 видов нечисла остаются в поле; слот; Undo/Redo не просят job; сфокусированное поле заморожено при job; Save/Apply — один и тот же запрос; cancel/refuse/stale сохраняют draft; замена = remove exact UUID + Add; ширина не больше прежней | `constraints::tests::angle` (2) |
| Окно → worker ↔ CLI | та же запись из окна (worker) и из peer CLI: те же правила, UUID остальных объектов, каждая SQL-ячейка (кроме Sketch payload/hash и метки времени), STL и FBX побайтово; затем замена и удаление по UUID — те же сравнения | `native_directed_angle_form_worker…` |
| Окно → DocumentSession | Apply в несохранённый документ после шага высоты, экспорт до Save, Undo/Redo с теми же UUID, Save, cold reopen, refused edit не режет Redo — против peer CLI | `sessions::tests::native_directed_angle_of_a_dart…` |
| Вкладки | пара, направления и текст принадлежат своей вкладке; Add в одной не меняет другую | `…drafts::a_typed_angle_its_pair_and_directions_follow_their_tab` |
| Старая сборка | см. ниже | `baseline-refusal.py` |

Формы сборки (каждая — отдельно, не вместо другой):

* **native** (OCCT + PlaneGCS): весь перечень выше; без регрессий — solver (6 наборов),
  document, eval (90), jobs, scene, export, topology, kernel, types, solver-lab, все
  32 CLI-наборов кроме двух GPU-pixel, весь viewer (**714 passed, 2 ignored**) под
  `tools/watch-viewer-memory.py` (лимит 1536 MiB, пик **287.5 MiB**, давление 1).
* **true stub** (нет OCCT, нет solver; `Open CASCADE was not usable`, `otool -L`:
  0 `libTK`, без `DYLD`): 16 выделенных ворот с маркерами (реальный блок `ci.yml`),
  рецепт → `FCAD_31F_RECIPE_NO_KERNEL`, те же пакеты и весь viewer (714 passed, пик
  144.0 MiB).
* **mixed** (OCCT без solver, `--no-default-features`): документ с углом не
  пересчитывается и не публикуется как решённый (`export-stl` и новая правка отказывают
  `unsupported`), прежний unconstrained маршрут (cold rebuild, validate, export-stl)
  работает; рецепт → `FCAD_31F_RECIPE_NO_SOLVER` (реальные строки шага workflow,
  release).
* **не исполнено**: Windows и Linux, удалённый CI diff, окно приложения и CUA,
  Unity, GPU-наборы (pixel-тесты CLI, `ferritecad-viewport-gpu`).

Реальные строки workflow выполнены локально, а не только разобраны: блок `ci.yml`
(mixed и stub), оба шага `runtime-layout.yml` (release), именованные ворота
`planegcs-pin.yml` на release-прогоне solver-пакета — этот прогон нашёл и поправил
ошибочное имя ворот (`planegcs::tests::the_shim…`), — и блок
`tools/check-fbx-complex.sh`, прочитавший пять FBX закреплённым ufbx 0.23.0
(исходники по закреплённым SHA-256; `checks=6 failures=0`; усечённая копия
`failures=2`). `tools/check-actions-run-size.py`: 208 блоков, максимум 20432 B
(`runtime-layout.yml:432`, не изменён), максимум packed argv 20463 B при пределе
21000; `actionlint` и `shellcheck` чисты. Для `runtime-layout.yml` понадобились два
**новых** шага: существующие большие шаги уже на 19–20 KB.

Также: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features
-- -D warnings`, `check-licence-headers.sh`, `check-solver-ownership.sh`,
`check-planegcs-pins.sh`, `check-export-boundary.sh` — чисто.

### Направленные поломки (всё компилируется, ворота исполняются)

Каждая внесена, прогнана против ворот, которые обязаны её поймать, и восстановлена
**побайтово** (SHA-256 совпал); затем те же положительные ворота зелёные.

| Поломка | Ворота и результат |
| --- | --- |
| знак потерян (`value: angle.abs()` при кодировании для shim) | `tests/angle.rs` — 4 из 9 падают на выполненных утверждениях; CLI dart-тест падает |
| направление потеряно (`DirectedLine::segment` всегда `start→end`) | `tests/directed_angle.rs` — «хранится как сказано» падает; CLI dart-тест падает |
| capability потеряна (`sketch.constraints.angle.v1` вне `SUPPORTED_CAPABILITIES`) | документ открывается read-only — тест падает; повторная правка файла с углом (CLI replace/remove) падает |
| проверка writer потеряна (`rederive` не зовёт `managed`) | подложенный план со вторым ответом на пару проходит бы — тест падает |

### Старая сборка отказывает честно

Замороженный CLI чистого main `be7b36a` (`ferritecad-main-be7b36a`,
SHA-256 `33279efe…`; прежние native-библиотеки, `libplanegcs` не менялась) и CLI из
bundle `31e-review` (его хэш **не** совпадает со свежей сборкой `be7b36a`: метка bundle 02:10 раньше
author-date 02:14 коммита `be847b3`, поэтому он лишь второй свидетель) получают
файл, записанный новой сборкой: `inspect` перечисляет ноль Sketch (объект
сохранён дословно), любая правка — `unsupported: document cannot be edited: it
requires sketch.constraints.angle.v1, which this build does not implement`,
`export-stl`/`export-fbx` — `object … has type "sketch", which this build preserves
but cannot rebuild`; ни `Parallel`, ни `Perpendicular` не появляется, файл
побайтово цел (хэш и mtime), выходных файлов нет, новая сборка читает его по-прежнему.

## Авторские решения для ревью

1. **Один слот с `Parallel`/`Perpendicular`** (отвергнут отдельный слот ради
   solver-избыточности `Parallel + Angle(0)`).
2. `−180` на входе принимается и хранится как `+180`; `180.000001`, `270`, `360` отказ.
3. Округление до `1e-9°` допустимо только при точном обратном совпадении радиан;
   иначе выдаётся полное число. Независимое ревью заменило безусловное округление,
   которое превращало `1e-10°` в `0°`; failing-first тест и процессный `inspect`
   проверяют малые значения и соседей границы. Underflow ненулевого ввода — отказ.
4. Layout v4 кумулятивен; смесь углов и окружностей в одном Sketch не запрещается.
5. Нулевой вектор: порог `1e-9` длины эскиза для solver и невязки, точное `> 0` в shim.
6. Запрос — `from`/`to` в каждой стороне, оба обязательны, разные; вложенные массивы
   отвергаются явной проверкой сверх `serde`.
7. Форма: кнопка направления на каждую сторону (подсказка — координаты) и угол на
   строках пары вместо новых строк; сигнал знака — `CCW +` в пустом поле и tooltip.

## Ограничения (чего здесь нет)

Дуги, касание, коллинеарность, симметрия, выражения, произвольные плоскости,
construction-Lines, второй solver, смена native pin, новая схема SQLite, ABI-версия
как отдельное число (раскладка не менялась); перетаскивание вершин и привязки на
Sketch с ограничениями по-прежнему недоступны, как и для прежних ограничений. Угол пары, которая не образует простой
многоугольник (например `0°` у противоположных сторон прямоугольника), решается, но
профиль отказывает политикой многоугольника, и публикации нет. Hover-опоры §31D,
привязка к центру §31C, §30, Milestone 5C, продукт целиком и расследование OOM
**не закрыты**.

## Оконная проверка (оставлена независимому ревьюеру)

Приватные входные модели создаёт короткий скрипт (каталог должен не существовать);
результатов в нём нет:

```python
import json, os, pathlib, subprocess, sys
cli, root = os.environ["FERRITECAD"], pathlib.Path(sys.argv[1]); root.mkdir(parents=True)
def run(*a):
    p = subprocess.run([cli, *map(str, a), "--json"], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (a, p.stdout); return json.loads(p.stdout)["result"]
for name, pts in {"dart": [[0, 0], [40, 0], [10, 10], [0, 40]],
                  "slanted": [[-20, -10], [40, -8], [42, 30], [-20, 30]]}.items():
    (root / "p.json").write_text(json.dumps({"request_version": 1, "points_mm": pts, "height_mm": 10}))
    run("create-sketch-extrude", root / "p.json", "-o", root / f"{name}.fcad")
```

Сценарий (≈10 минут; окно открывает ревьюер, сравнение — через CLI):

1. Открыть `dart.fcad` → `Edit constraints …`. Выбрать `Segment 2` → `Pair Line A`,
   `Segment 3` → `Pair Line B`, нажать `B: start→end` (станет `B: end→start`),
   ввести `110`, `Add directed angle`. В «Pending additions» — одна запись с обоими
   направлениями; `Undo`/`Redo` меняют её, не запуская job.
2. Тот же `Add directed angle` ещё раз и `Add Parallel` на той же паре — отказ словом
   «directed angle», текст остаётся. Ввести `270` — отказ, `270` остаётся в поле.
3. `Save constraints copy…` → отменить диалог (draft цел) → снова, сохранить рядом.
4. На несохранённом документе `Apply constraints`; экспорт STL **до** Save; `Undo` и
   `Redo` сессии; `Save As`.
5. Сравнить с peer CLI: `edit-sketch-constraints-copy dart.fcad --sketch <sketch>
   --expect-version <hash> --request r.json -o peer.fcad` с
   `{"rule":"directed_angle","a":{"curve_id":L1,"from":"start","to":"end"},"b":
   {"curve_id":L2,"from":"end","to":"start"},"angle_deg":110}` и `export-stl` обоих.
6. Открыть вторую копию того же файла в другой вкладке: пара, направления и текст
   не перетекают между вкладками.
7. Замена: файл, уже несущий угол, → `Remove` у строки `Directed angle …`, пара,
   другое значение, `Add directed angle`, `Apply` — один запрос `remove` + `add`.

