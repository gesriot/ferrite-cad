# §31G — коллинеарность двух сохранённых Lines

Новое ограничение `SketchConstraintRule::Collinear` — **постоянная связь двух разных
Lines** одного собственного XY Line-профиля на **одной бесконечной опорной прямой**.
Это настоящие уравнения PlaneGCS (две `addConstraintPointOnLine` под одним
документным ограничением), а не однократный перенос координат: сохранённые
координаты, UUID, ссылки и остальная SQL-информация остаются теми же; меняются
только список ограничений выбранного Sketch, его layout/версия и строка
`capabilities`. Путь тот же, что у остальных: форма «Sketch constraints» → прежняя
команда `edit-sketch-constraints-copy`, общий job и writer guard. Новой команды,
схемы SQLite, native pin и второго solver нет.

Граница среза: **две разные non-construction Lines поддерживаемого XY-профиля**.
(Профиль с construction-Line редактор не принимает вовсе — общий отказ, закреплён
тестом для этой связи.) Дуги, касание, симметрия, выражения, произвольные плоскости,
`Coincident` на концах и «выровнять» без связи не входят. Hover-опоры §31D и привязка
к центру окружности §31C остаются открытыми; этот срез их не закрывает. §31F
(направленный угол) не менялся и не расширялся.

## Семантика (принято, не заменять другой)

* Две Lines лежат на **одной бесконечной прямой**. Отрезки могут не соприкасаться;
  совпадение концов и перекрытие не требуются. **Parallel недостаточен**: два
  параллельных отрезка на разных прямых — не коллинеарны; такая пара обязана
  сместиться или отказать при конфликте, а не считаться выполненной (на U-профиле
  ниже ободья остаются в 2 мм друг от друга при Parallel и сходятся при Collinear).
* Связь **не несёт направлений**. Запрос называет две Lines (`a_curve_id`,
  `b_curve_id`); в документе каждая хранится целиком, `start → end`. Разворот любого
  отрезка, порядок A/B и то, что отрезки идут в противоположные стороны (уступы
  Z-профиля), — одна и та же связь. Кнопки `A: start>end` / `B: start>end` формы
  относятся только к `DirectedAngle` и на Collinear **не влияют** (подсказка кнопки
  говорит это словами).
* **Ненулевые векторы обязательны до и после решения.** В документе сегмент, названный
  одной точкой дважды, не хранится; в solver вектор не длиннее `1e-9` длины эскиза —
  `SolverError::DegenerateVector` (общий с `DirectedAngle` отказ); в shim — тот же отказ
  уже в `fc_gcs_session_create`; **после** решения схлопнутый отрезок даёт невязку `+∞`
  и `DidNotConverge` (отрезок нулевой длины «лежит на любой прямой», и без этого
  арифметика объявила бы связь выполненной).
* **Слот.** Collinear отвечает на тот же вопрос об относительном положении пары, что
  `Parallel`/`Perpendicular`/`DirectedAngle`, и делит с ними слот
  `Relation(sorted(A, B))`: `(A,B)` и `(B,A)` — один слот, повтор, другой ответ о той
  же паре, пара из одной Line, чужая Line и занятый слот — структурный отказ до
  solver без публикации (сообщение называет «Collinear»). `EqualLength` на той же паре —
  независимое свойство со своим слотом. Замена — прежнее «remove exact UUID + add» в
  одном запросе. Транзитивность (A~B, B~C, A~C) документ **не анализирует**: избыточность
  и конфликт через несколько пар устанавливает только solver.
* **Хранится именно ограничение.** Координаты Lines не подгоняются перед записью,
  связь не заменяется автоматически двумя `Coincident`, `Horizontal`/`Vertical` или
  `DirectedAngle(0)`, фиктивные UUID не создаются. Пара непересекающихся отрезков
  остаётся такой, если этого требуют остальные ограничения. Обычная проверка
  многоугольника остаётся последним общим рубежом и **не ослабляется ради solve**
  (измерено: две соседние Lines, сделанные коллинеарными, дают «collinear or backtracking
  vertex»; дно выемки U, сведённое с низом, — «edges intersect or touch»; в обоих
  случаях ничего не опубликовано).

## Кодирование в PlaneGCS (по фактическому API закреплённой библиотеки)

Закреплён PlaneGCS из FreeCAD 1.0.1. Рассмотрены два готовых уравнения, и выбор
следует из их исходников (`Constraints.cpp`):

* `ConstraintParallel` — **одно** уравнение `cross(d1, d2)`, масштаб
  `1/sqrt(|d1|²·|d2|²)` фиксируется при построении. Оно держит направление и **ничего
  не знает о смещении** прямых — именно его и нельзя считать коллинеарностью.
* `ConstraintPointOnLine(p, lp1, lp2)` — знаковое расстояние точки до бесконечной
  прямой `area/d` в единицах длины (мм).

Collinear кодируется **двумя** `PointOnLine` под **одним** tag (индекс ограничения +1):
оба конца `a` лежат на прямой через `b`. Два уравнения держат ровно то, что есть в
связи, — направление и поперечное смещение; единицы невязки — мм, а не безразмерный
синус и не мм². Направление и порядок концов ни одно из них не читает.

Что измерено на настоящей библиотеке (`tests/collinear.rs`, 9 тестов):

| Что | Результат |
| --- | --- |
| две свободные пары отрезков | DOF 8 → 6: ровно два уравнения |
| `a` закреплена, `b` свободна | DOF 2: две позиции `b` вдоль прямой |
| замкнутая пара (`Coincident` на стыке) | DOF 6 → **5**, не 4 и не 6: одно из двух уравнений истинно уже, второе (направление) нужно; решённый файл измерен — наклон (синус 0.37, около 22°) снят |
| то же во всех написаниях (кто ведёт, чей конец первый) | то же |
| связь не redundant при частичной избыточности | `redundant` пуст: см. ниже |
| Collinear дважды (обратное написание) | `redundant` = ровно один из двух id, `conflicting` пуст, DOF учтён один раз |
| три отрезка уже на одной прямой, три попарные связи | `redundant` = ровно один id |
| те же три, но старт не на прямой | **не видно**: DOF 6 вместо 8, `redundant` пуст (ограничение, ниже) |
| закреплённое смещение (`Fixed` вне прямой) | `Conflicting`, в причинах id Collinear и закреплений |
| схлопнутый `a` при свободном `b` | planegcs «успешен», измеренная невязка `+∞` → `DidNotConverge` |
| drag одного конца | связь держится, конец следует за указателем |

**Частичная избыточность — по API, а не по догадке.** В `GCS.cpp` (`identifyConflicting…`)
tag считается redundant, только если **каждое** его уравнение redundant;
уравнения, избыточные частично, попадают в отдельный `partiallyRedundantTags`.
Shim отдаёт только `getRedundant`, поэтому UUID связи нельзя объявить полностью
избыточным лишь потому, что одна её составляющая повторяет уже сказанное
(замкнутая пара — реальный solver-тест, а не DTO). Уточнение для чтения диагностики:
`getPartiallyRedundant` shim не отдаёт, документ такого понятия не имеет.

**Ограничение, не скрываемое.** PlaneGCS ранжирует Якобиан в **стартовых**
положениях. Зависимость, которая выполняется только на решении (третья парная
Collinear из A~B, B~C, A~C, когда сохранённые координаты ещё не на одной прямой;
Parallel + замыкание + Collinear), на старте выглядит независимой: `redundant` пуст,
а DOF занижен (в примере 6 вместо 8). Решение при этом точное (измерено), а
ограничения не противоречат; тот же класс ограничения уже есть у прежних семейств.
Если сохранённые координаты уже удовлетворяют зависимости, redundant называется
(строка таблицы выше).

**Невязка** (Rust, `residual.rs`, судит ответ solver): наихудшее из — синус угла между
отрезками (безразмерный) и четырёх знаковых расстояний (мм): оба конца `a` от прямой `b`
и оба конца `b` от прямой `a`. Нулевой cross сам по себе не доказательство: параллельные
отрезки на разных прямых дают расстояние, а не нуль. Результат симметричен в `a`/`b`;
отрезок не длиннее `1e-9` — `+∞`. Предел прежний, `RESIDUAL_LIMIT = 1e-6`.

**Shim и ABI.** Новый вид `FC_GCS_COLLINEAR = 10`; номера `0…9` не
переинтерпретируются. `points[0..4] = a.from, a.to, b.from, b.to`, `circles = [-1,-1]`,
`value` и `value2` обязаны быть `0` (не 0, NaN, ±∞ — `NULL`). Раскладка `FcGcsConstraint`
(48 байт) и `FcGcsCircle` (16) **не меняется**; обе стороны утверждают её
(`static_assert` и `assert_abi_layout`), ручной тест
`planegcs::tests::the_shim_names_the_collinear_kind_and_refuses_what_the_header_forbids`
вызывает `fc_gcs_session_create` напрямую: вид 10 строится (в обоих написаниях концов);
непустые `circles`, `points[3] = -1`, чужой/огромный индекс, `value ≠ 0` (включая `1e-300`,
NaN), `value2 ≠ 0`, отрезок из одной точки или нулевой длины, неизвестные виды
`-1, 11, 12, 255, i32::MAX` — `NULL`; старые виды 7 и 9 не сдвинулись. Старый тест
прежнего вида исправлен только в списке «неизвестных»: `10` перестал быть неизвестным.
Pin (FreeCAD 1.0.1) и вторая библиотека не менялись; shim — статическая часть
Rust-бинаря, поэтому ранее собранный main сохраняет старый shim.

## Строгий запрос CLI (request v1, аддитивно)

```json
{"rule": "collinear", "a_curve_id": "<uuid>", "b_curve_id": "<uuid>"}
```

Оба UUID явные; никаких скрытых вторых идентификаторов, направлений, чисел. Лишние,
неизвестные и повторные поля (в том числе `from`/`to`, `a`/`b`-объекты формы угла,
`angle_deg`), отсутствие любого из двух, не-UUID, число/`null`/bool/массив вместо UUID,
иное написание правила (`Collinear`, `colinear`) — `kind:"input"`, код 2, без публикации.
Self-pair, чужая Line и занятый слот — тоже `input`, до solver. Прежние формы и ответы
прежних видов, `request_version` и operation не менялись.

Ответ (`added_constraints[]`, `inspect`, `constraint_conflict`) для нового вида — свой
`kind` в прежней `Segment`-форме (обе Lines целиком, `start → end`, как хранятся):

```json
{"kind": "collinear",
 "a": {"from": {"curve_id": "…", "at": "start"}, "to": {"curve_id": "…", "at": "end"}},
 "b": {"from": {"curve_id": "…", "at": "start"}, "to": {"curve_id": "…", "at": "end"}}}
```

## Storage

See the [current document format](file-format.md).

## Форма в окне

Pair Line A/B и вся прежняя форма — те же; на строке отношений пары добавлена кнопка
**`Add Collinear`**:

```
Line pair: [Pair Line A] [Pair Line B] Pair: A ab12cd · B ef34gh  [A: start>end] [B: start>end]
[Add Equal length] [Add Parallel] [Add Perpendicular] [Add Collinear]  A>B (°): [ CCW + ] [Add directed angle]
```

Подсказка `Add Collinear`: «Both Lines on one infinite line: the same direction and no
offset between them. They may be apart, and either may run either way; the A/B
start>end buttons are not used.» Один Add — один шаг draft и один
`AddLineConstraint::Relation { relation: Collinear }` в запросе, который проверяет тот же
`validate_edits`; в «Pending additions» запись читается как `Collinear · Lines A и B`, в
«Persisted constraints» — `Collinear · <UUID>` и `Lines A and B` со своим `Remove`;
**remove exact UUID + Add** заменяет её одним запросом. Undo/Redo, Save Cancel, отказ
solver, Apply к несохранённой `DocumentSession`, экспорт до Save и вкладки проходят через
прежний код (одна и та же форма и draft).

**Высота не выросла** (строки те же, кнопка на уже существующей строке отношений;
прежние gate доступности действий на 988×768 — `constraint_editor_keeps_actions_reachable_with_many_pending_additions`,
Revolve- и chamfer-сессии — проходят без правки). **Ширина выросла**:
строка не помещается в прежние 598 pt самого широкого абзаца, её правый край теперь
671.1 pt, и окно (не меняющее размер вручную, по содержимому) стало настолько шире.
Прежний gate §31F «строки угла не шире остальной формы» не мог сохраниться в этом виде и
заменён на: форма не шире 700 pt, и `Add Equal length`, `Add Parallel`, `Add Collinear`, поле
угла лежат на одной строке с `Add Perpendicular` (форма не стала выше). Новых
символов вне штатного шрифта нет (только `>` и `·`, как в §31F).

## Выполняемый рецепт

Нужны OCCT + PlaneGCS и `FERRITECAD` — путь к `ferritecad`. На macOS native library paths
экспортируются в том же shell. Рецепт создаёт свой временный каталог, не меняет fixtures
и не требует заранее известных UUID. Без ядра печатает `FCAD_31G_RECIPE_NO_KERNEL`, с ядром
без solver — `FCAD_31G_RECIPE_NO_SOLVER` (при `FCAD_EXPECT_SOLVER=1` это провал), с
обоими — `FCAD_31G_RECIPE_OK` с измеренными числами. Всё измерено по **STL**
(вершины основания → кольцо → знаковые расстояния), не по ответу CLI; начало Line 0
закреплено только затем, чтобы кольцо читалось как Line 0, 1, …, 7 без помощи solver.
STL хранит float32 и кольцо округляется до `1e-4`, поэтому допуск рецепта `1e-3` мм.

```python
# FCAD_31G_AGENT_RECIPE
import hashlib, json, math, os, pathlib, struct, subprocess, sys, tempfile
cli = os.environ["FERRITECAD"]
expect_solver = os.environ.get("FCAD_EXPECT_SOLVER") == "1"
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-collinear-"))
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
            print("FCAD_31G_RECIPE_NO_KERNEL", json.dumps(error)); sys.exit(0)
        if error["kind"] == "unsupported" and "sketch solver" in error["message"]:
            if expect_solver:
                sys.exit(f"FCAD_EXPECT_SOLVER=1 but this build has no sketch solver: {error}")
            print("FCAD_31G_RECIPE_NO_SOLVER", json.dumps(error)); sys.exit(0)
    assert p.returncode == 0, (what, p.returncode, p.stdout)
    return json.loads(p.stdout)["result"]

def base_ring(path):
    """The solved footprint, from the exported STL alone: the base face's boundary as an
    ordered ring (counter-clockwise), plus the solid's volume from the whole mesh."""
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
    assert len(ring) == len(adj) == 8, ring
    area = sum(ring[i][0]*ring[(i+1) % 8][1] - ring[i][1]*ring[(i+1) % 8][0] for i in range(8)) / 2
    if area < 0:
        ring.reverse(); area = -area
    assert math.isclose(abs(six) / 6, area * 10, rel_tol=1e-5), (six / 6, area * 10)  # independent volume
    return ring, abs(six) / 6

def lines(path, pinned):
    """Line k as (start, end): the ring read from the vertex at the pinned start of Line 0."""
    ring, volume = base_ring(path)
    first = min(range(8), key=lambda i: math.hypot(ring[i][0] - pinned[0], ring[i][1] - pinned[1]))
    ring = ring[first:] + ring[:first]
    assert math.hypot(ring[0][0] - pinned[0], ring[0][1] - pinned[1]) < 1e-3, ring
    return [(ring[k], ring[(k + 1) % 8]) for k in range(8)], volume

def gap(ls, i, j):
    """The signed distance of A's two ends from B's line and of B's from A's (positive
    to the left of the line as it runs), and the sine of the angle between them."""
    (a0, a1), (b0, b1) = ls[i], ls[j]
    da, db = (a1[0]-a0[0], a1[1]-a0[1]), (b1[0]-b0[0], b1[1]-b0[1])
    la, lb = math.hypot(*da), math.hypot(*db)
    side = lambda p, f, d, l: (d[0]*(p[1]-f[1]) - d[1]*(p[0]-f[0])) / l
    off = [side(a0, b0, db, lb), side(a1, b0, db, lb), side(b0, a0, da, la), side(b1, a0, da, la)]
    return off, (da[0]*db[1] - da[1]*db[0]) / (la * lb)

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

def curves_of(catalog):
    return [c["curve_id"] for c in catalog["sketches"][0]["constraint_edit"]["curves"]]

def col(curves, i, j):
    return {"rule": "collinear", "a_curve_id": curves[i], "b_curve_id": curves[j]}

def rule(name, curves, i, j):
    return {"rule": name, "a_curve_id": curves[i], "b_curve_id": curves[j]}

def said(curves, i, j):
    whole = lambda k: {"from": {"curve_id": curves[k], "at": "start"}, "to": {"curve_id": curves[k], "at": "end"}}
    return {"kind": "collinear", "a": whole(i), "b": whole(j)}

# 1. A U: its two rims (Lines 0 and 4) are parallel, share no point and are 2 mm apart.
#    The start of Line 0 is pinned where it already is, only so that the exported
#    footprint can be read back as Line 0, 1, ... 7 without any help from the solver.
U = [[50, 22], [35, 22], [35, 8], [15, 8], [15, 20], [0, 20], [0, 0], [50, 0]]
source, catalog = make(U)
c = curves_of(catalog)
assert catalog["sketches"][0]["constraint_edit"]["available"] and len(c) == 8
untouched = sha(source)
pin = {"rule": "fixed", "curve_id": c[0], "at": "start", "x_mm": 50, "y_mm": 22}
horizontal = {"rule": "horizontal", "curve_id": c[0]}

# 2. What the measurement says about the unsolved source: the signs and the scale.
#    Both rims run west; Line 4 is 2 below Line 0, so Line 0's ends are 2 to the right
#    of Line 4's line and Line 4's ends are 2 to the left of Line 0's.
before, _ = lines(source, (50, 22))
off, sine = gap(before, 0, 4)
assert all(abs(a - b) < 1e-3 for a, b in zip(off, [-2, -2, 2, 2])) and abs(sine) < 1e-6, (off, sine)
offset_before = max(abs(d) for d in off)

# 3. Collinear: the rims come onto one line though they never touch. Either Line may lead.
published = {}
for name, i, j in (("A first", 0, 4), ("B first", 4, 0)):
    out = root / f"u-{i}-{j}.fcad"
    done = edit(source, catalog, [], [pin, col(c, i, j)], out)
    assert done["added_constraints"][-1]["rule"] == said(c, i, j), done["added_constraints"][-1]
    assert done["solve"]["degrees_of_freedom"] == 12 and done["solve"]["redundant_constraint_ids"] == []
    ls, volume = lines(out, (50, 22))
    for p, q in ((0, 4), (4, 0)):
        off, sine = gap(ls, p, q)
        assert max(abs(d) for d in off) < 1e-3 and abs(sine) < 1e-4, (name, off, sine)
        offset_after = max(abs(d) for d in off)
    assert all(math.hypot(a[1][0]-a[0][0], a[1][1]-a[0][1]) > 5 for a in ls), "a Line collapsed"
    published[name] = (out, done, round(volume, 2))
first = published["A first"][0]

# 4. The negative control one Parallel passes: the same pair, parallel and still 2 apart.
parallel = root / "u-parallel.fcad"
done = edit(source, catalog, [], [pin, rule("parallel", c, 0, 4)], parallel)
ls, parallel_volume = lines(parallel, (50, 22))
off, sine = gap(ls, 0, 4)
assert abs(sine) < 1e-4 and all(abs(abs(d) - 2) < 1e-2 for d in off), (off, sine)
offset_parallel = min(abs(d) for d in off)
assert abs(published["A first"][2] - round(parallel_volume, 2)) > 1, "one solid, two relations"

# 5. Lines that run opposite ways: the ledges of a Z (Lines 2 and 6) are 1 mm apart.
Z = [[0, 0], [40, 0], [40, 9], [60, 9], [60, 21], [20, 21], [20, 10], [0, 10]]
zsource, zcatalog = make(Z)
zc = curves_of(zcatalog)
zout = root / "z.fcad"
edit(zsource, zcatalog, [], [{"rule": "fixed", "curve_id": zc[0], "at": "start", "x_mm": 0, "y_mm": 0}, col(zc, 2, 6)], zout)
zl, _ = lines(zout, (0, 0))
off, sine = gap(zl, 2, 6)
assert max(abs(d) for d in off) < 1e-3 and abs(sine) < 1e-4, (off, sine)
assert zl[2][1][0] > zl[2][0][0] and zl[6][1][0] < zl[6][0][0], "Lines 2 and 6 still run opposite ways"

# 6. Replace it by its exact UUID with a Parallel, then remove that: freedom comes back.
first_catalog = call(["inspect", first])
collinear_id = published["A first"][1]["added_constraints"][-1]["constraint_id"]
replaced = root / "u-replaced.fcad"
r = edit(first, first_catalog, [collinear_id], [rule("parallel", c, 0, 4)], replaced)
assert r["removed_constraint_ids"] == [collinear_id] and r["solve"]["degrees_of_freedom"] == 13
after = call(["inspect", replaced])
assert ids(after) == [i for i in ids(first_catalog) if i != collinear_id] + [r["added_constraints"][0]["constraint_id"]]
off, sine = gap(lines(replaced, (50, 22))[0], 0, 4)
assert all(abs(abs(d) - 2) < 1e-2 for d in off), "the rims are 2 apart again"
removed = root / "u-free.fcad"
r = edit(replaced, after, [r["added_constraints"][0]["constraint_id"]], [], removed)
assert r["added_constraints"] == [] and r["solve"]["degrees_of_freedom"] == 14

# 7. Structural refusals before any solving: the pair holds one relation, whichever way
#    it is said, and the request has no field but the two Lines.
def refused(request, source=first, catalog=first_catalog):
    error = edit(source, catalog, [], [request], root / "never.fcad", 2)
    assert error["kind"] == "input" and "constraint_conflict" not in error, error
for request in [col(c, 0, 4), col(c, 4, 0), rule("parallel", c, 0, 4), rule("parallel", c, 4, 0),
                rule("perpendicular", c, 0, 4),
                {"rule": "directed_angle", "a": {"curve_id": c[0], "from": "start", "to": "end"},
                 "b": {"curve_id": c[4], "from": "start", "to": "end"}, "angle_deg": 0},
                col(c, 3, 3),
                {"rule": "collinear", "a_curve_id": c[0], "b_curve_id": "00000000-0000-7000-8000-000000000000"},
                {**col(c, 2, 6), "from": "start"}, {**col(c, 2, 6), "angle_deg": 0},
                {"rule": "collinear", "a_curve_id": c[2]}, {"rule": "Collinear", "a_curve_id": c[2], "b_curve_id": c[6]}]:
    refused(request)

# 8. The solver's own answers, with the UUIDs of their reasons. Both rims are told to be
#    horizontal at the heights they have, by a pin, two walls and their lengths: nothing
#    with them is a line, so a collinearity conflicts, with the collinearity among the reasons.
held = [{"rule": "fixed", "curve_id": c[6], "at": "start", "x_mm": 0, "y_mm": 0},
        {"rule": "horizontal", "curve_id": c[6]},
        {"rule": "vertical", "curve_id": c[5]}, {"rule": "distance", "curve_id": c[5], "distance_mm": 20},
        {"rule": "horizontal", "curve_id": c[4]},
        {"rule": "vertical", "curve_id": c[7]}, {"rule": "distance", "curve_id": c[7], "distance_mm": 22},
        {"rule": "horizontal", "curve_id": c[0]}]
pinned_file = root / "u-pinned.fcad"
edit(source, catalog, [], held, pinned_file)
error = edit(pinned_file, call(["inspect", pinned_file]), [], [col(c, 0, 4)], root / "conflict.fcad", 2)
assert error["kind"] == "constraint", error
reasons = error["constraint_conflict"]["constraints"]
assert any(r["rule"]["kind"] == "collinear" for r in reasons) and all(isinstance(r["constraint_id"], str) for r in reasons), reasons
conflict_reasons = len(reasons)
#    Two ledges already on one line, each also horizontal: three facts in four equations.
Zf = [[0, 0], [40, 0], [40, 10], [60, 10], [60, 21], [20, 21], [20, 10], [0, 10]]
fsource, fcatalog = make(Zf)
fc = curves_of(fcatalog)
r = edit(fsource, fcatalog, [], [{"rule": "horizontal", "curve_id": fc[2]}, {"rule": "horizontal", "curve_id": fc[6]}, col(fc, 2, 6)], root / "z-flat.fcad")
minted = [x["constraint_id"] for x in r["added_constraints"][8:]]
assert len(r["solve"]["redundant_constraint_ids"]) == 1 and r["solve"]["redundant_constraint_ids"][0] in minted
assert r["solve"]["degrees_of_freedom"] == 13
redundant = len(r["solve"]["redundant_constraint_ids"])

# 9. The profile rules stay the last word: two neighbours made collinear leave a straight
#    corner, and the floor of the notch brought down to the bottom leaves no material.
for request in (col(c, 0, 1), col(c, 2, 6)):
    error = edit(source, catalog, [], [request], root / "never.fcad", 2)
    assert error["kind"] == "input" and "polygon" in error["message"], error

assert sha(source) == untouched, "the source changed"
assert not (root / "never.fcad").exists() and not (root / "conflict.fcad").exists()
print("FCAD_31G_RECIPE_OK", f"offset_before={offset_before:.3f} collinear={offset_after:.5f} parallel={offset_parallel:.3f}",
      f"volumes={published['A first'][2]}/{round(parallel_volume, 2)} conflict_reasons={conflict_reasons} redundant={redundant}")
```

## Что исполнено (2026-10-14, Mac arm64, ветка `collinear-sketch-lines` от `bdf25e5`)

Базовый CI точного merge `bdf25e5` (PR #109): `CI`, `planegcs pin`, `rust notices`,
`rust sbom`, `product sbom`, `combined runtime layout` — все шесть **success**. **CI этого
diff не запускался**: всё ниже — локальные прогоны. Подробности, команды и маркеры:
[collinear-sketch-lines-verification.md](collinear-sketch-lines-verification.md).

| Слой | Что доказано | Прогон |
| --- | --- | --- |
| Solver, реальный PlaneGCS | отступ держится там, где Parallel его не держит (параллельные отрезки в 5 мм сходятся на одну прямую, контроль Parallel остаётся в 5 мм); все шесть написаний концов и порядка; DOF 8→6 и 2 при закреплённой `a`; замкнутая пара 6→5 и **не** redundant; повтор — ровно один redundant; конфликт называет Collinear; схлопнутый отрезок → `DidNotConverge{inf}`; drag | `tests/collinear.rs` (9) |
| Контракт solver без native | нулевой отрезок на каждой из 4 позиций и чужая точка — typed отказ до native; невязка: отступ, синус, симметрия, `+∞`, нечисло | `tests/boundary.rs` (+2, 15), `residual::tests` (unit) |
| C ABI | вид 10 строится (в обоих написаниях), `value`/`value2` ≠ 0, непустые `circles`/`points[3]`, чужой индекс, нулевой отрезок, виды `-1, 11, 12, 255, i32::MAX` → `NULL`; виды 7 и 9 не сдвинулись; раскладка 48/16 байт утверждена обеими сторонами | `planegcs::tests::the_shim_names_the_collinear_kind…` |
| Документ | layout v5 и capability в строке `capabilities`, свой именованный вид на проводе (не `parallel`), слот в обоих порядках с Parallel/Perpendicular/DirectedAngle, замена в обе стороны, возврат к v2, v2/v4-заголовок над payload с Collinear и v5-заголовок без неё — отказ, сегмент из одной точки не хранится, construction-Line в профиле отказывает все пары | `tests/collinear.rs` (4) |
| Writer guard | подложенный план не кладёт второй ответ на пару, не обращает сохранённый Collinear в Parallel под его UUID, не разворачивает его сегменты и не переносит на другую пару | `constraint_write_tests::…downgrade_it` |
| Evaluator | обе Lines и их порядок пересекают границу как есть, и это не Parallel | `solve::tests::a_collinear_is_stated…` |
| CLI-процесс | строгий запрос (19 нарушений формы и 4 текстовых: дубликат ключа, escaped-дубликат, NaN, 1e999); discovery и прежние формы без изменений; слот и «чужие» Lines до solver; U-профиль (ободья в 2 мм) и тот же, повёрнутый на 23°; Z-профиль с отрезками в противоположные стороны; знак и масштаб измерения проверены на несолвеном источнике; отрицательный контроль Parallel; replace/remove по UUID; конфликт с закреплённым смещением и typed причинами; настоящая redundant (H+H+Collinear); соседние Lines и дно выемки отказывает профильная проверка; stale snapshot; STL разобран независимо, FBX прочитан | `tests/collinear.rs` (9) |
| Cache | один sidecar для двух копий с одним сохранённым приближением и разными решениями: `Miss` (не устаревший `Hit`), затем `Hit` с тем же объёмом | `native_collinear_cache_keys_follow_the_solved_profile` |
| Окно (widgets) | пара, Add — один шаг, направления A/B не читаются и не потребляются; слот называет «Collinear»; Undo/Redo не просят job; кнопка заморожена при job; Save/Apply — один и тот же запрос; cancel/refuse/stale сохраняют draft; замена = remove exact UUID + Add; форма не выше (все шесть прежних gate доступности 768 pt) | `constraints::tests::collinear` (2) |
| Окно → worker ↔ CLI | та же связь из окна (worker) и из peer CLI: те же правила и UUID остальных объектов, каждая SQL-ячейка (кроме Sketch payload/hash и метки времени), STL и FBX побайтово; контроль Parallel (DOF 15) отличается; замена на Parallel и удаление — те же сравнения | `native_collinear_form_worker…` |
| Окно → DocumentSession | Apply в несохранённый документ после шага высоты, экспорт до Save, Undo/Redo с теми же UUID, Save, cold reopen, refused edit не режет Redo — против peer CLI (общий `native_constraint_gate`) | `sessions::tests::native_collinear_of_a_u…` |
| Вкладки | пара и «Pending additions» принадлежат своей вкладке; Add в одной не меняет другую | `…drafts::a_picked_pair_and_a_pending_collinear_follow_their_tab` |
| Старая сборка | см. ниже | `baseline-refusal.py` |

Формы сборки (каждая — отдельно, не вместо другой; skips считаны отдельно):

* **native** (OCCT + PlaneGCS 1.0.1): весь перечень выше; без регрессий — solver (6 наборов),
  eval, scene, solver-lab, document, jobs, export, topology, kernel, types и CLI-наборы
  (кроме трёх `*_complex` с большим STEP/FBX корпусом и двух GPU-pixel, которых этот срез не
  касается); весь viewer под `tools/watch-viewer-memory.py` (лимит 1536 MiB):
  **720 passed, 2 ignored**, пик 291.3 MiB, давление 1, swap 0; 23 строки `skipped:` — тесты
  других форм сборки.
* **true stub** (нет OCCT, нет solver; `Open CASCADE was not usable`, `otool -L`: 0 `libTK`,
  0 planegcs, без `DYLD`): 15 ворот с маркерами (реальный блок `ci.yml`), рецепт →
  `FCAD_31G_RECIPE_NO_KERNEL`, тот же viewer: 720 passed (210 строк `skipped:` — тесты,
  которым нужны ядро или solver), пик 34.4 MiB. Из 9 CLI-тестов выполняются 2 (strict,
  discovery), 7 — skips.
* **mixed** (OCCT без solver, `--no-default-features`; `otool -L`: 50 `libTK`, 0 planegcs):
  документ с Collinear не пересчитывается и не публикуется как решённый (`export-stl` и новая
  правка отказывают `unsupported`), прежний unconstrained маршрут (cold rebuild, validate,
  export-stl) работает; запросные отказы до solver (`the_pair_and_the_lines_of_a_request…`)
  исполняются и здесь; рецепт → `FCAD_31G_RECIPE_NO_SOLVER` (реальный шаг workflow, release).
* **не исполнено**: Windows и Linux, удалённый CI diff, окно приложения и CUA, Unity,
  GPU-наборы (pixel-тесты CLI, `ferritecad-viewport-gpu`), большой STEP/FBX корпус.

Реальные строки workflow выполнены локально, а не только разобраны: блок `ci.yml`
(mixed и stub; ворота окна под watchdog, а не прямым `cargo test`), оба новых шага
`runtime-layout.yml` (release; native — вместе с рецептом, извлечённым из этого Markdown, и
FBX-артефактами), шаг `planegcs-pin.yml` на release-прогоне solver-пакета и solver-lab
(отрицательный контроль: без имени ворот и маркера шаг падает), и FBX-блок
`tools/check-fbx-complex.sh`, прочитавший три файла закреплённым ufbx 0.23.0 (`checks=6
failures=0`; усечённая копия `failures=2`). `tools/check-actions-run-size.py`: 210 блоков,
максимум 20432 B (`runtime-layout.yml:432`, не изменён), максимум packed argv 20463 B при
пределе 21000; `actionlint` и `shellcheck` чисты. В `runtime-layout.yml` понадобились два
**новых** шага: существующие большие шаги уже на 19–20 KB. Измерения рецепта на native-сборке
(из STL):

```
FCAD_31G_RECIPE_OK offset_before=2.000 collinear=0.00010 parallel=2.000 volumes=7867.85/7900.0 conflict_reasons=10 redundant=1
```

(`0.00010` — квантование кольца STL до `1e-4` мм; контроль Parallel остаётся в 2.000 мм.)

Также: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`check-licence-headers.sh`, `check-solver-ownership.sh`, `check-planegcs-pins.sh`,
`check-export-boundary.sh`, `git diff --check` — чисто.

### Направленные поломки (всё компилируется, ворота исполняются)

Каждая внесена, прогнана против ворот, которые обязаны её поймать, и восстановлена
**побайтово** (SHA-256 совпал); затем те же положительные ворота зелёные. Используется тот же
скрипт, что в §31F (ignored `target/`), в репозитории новой инфраструктуры нет.

| Поломка | Ворота и результат |
| --- | --- |
| **смещение потеряно**: `Collinear` в evaluator переводится в один `Parallel` | eval-перевод падает (`left: Parallel…, right: Collinear…`); CLI U-тест падает (`dof 15` вместо `14`, до измерения); worker окна падает (`Some(Some(15))` вместо `14`) |
| **capability потеряна** (`sketch.constraints.collinear.v1` вне `SUPPORTED_CAPABILITIES`) | документ открывается read-only — тест падает; правка файла, уже несущего Collinear (CLI replace/remove), падает `unsupported: document cannot be edited` |
| проверка writer потеряна (`rederive` не зовёт `managed`) | подложенный план со вторым ответом на пару проходит бы — оба forged-plan теста падают (`a Parallel was written onto a collinear pair`) |
| shim кодирует Collinear как Parallel (то же число точек) | 7 из 9 solver-тестов падают; смещение ловит измеренная невязка: `expected a solution, got DidNotConverge { worst_residual: Some(5.0) }` |

### Старая сборка отказывает честно

Замороженный CLI чистого main `bdf25e5` (`ferritecad-main-bdf25e5`, SHA-256
`a781ea80…`; он знает угол, но не коллинеарность) получает файл, записанный новой
сборкой: `inspect` перечисляет ноль Sketch (объект сохранён дословно), любая правка —
`unsupported: document cannot be edited: it requires sketch.constraints.collinear.v1, which
this build does not implement`, `export-stl`/`export-fbx` — `object … has type "sketch", which
this build preserves but cannot rebuild`; ни `Parallel`, ни `Perpendicular` не появляется,
файл побайтово цел (хэш и mtime), выходных файлов нет, новая сборка читает его по-прежнему.

## Авторские решения для ревью

1. **Один слот с `Parallel`/`Perpendicular`/`DirectedAngle`.** Collinear отвечает на тот
   же вопрос о паре; отдельный слот ради solver-избыточности «Parallel + Collinear»
   отвергнут (он превратил бы «один ответ на пару» в два).
2. **Кодирование — две `PointOnLine` под одним tag**, а не `Parallel` + одна `PointOnLine`:
   оба уравнения в единицах длины, ни одно не зависит от направления отрезков, и
   замкнутая пара даёт **частично** избыточную связь (одно уравнение истинно, другое
   нужно) без ложного «redundant». Цена — оба конца `a` против прямой `b` (несимметричное
   кодирование); симметрию восстанавливает невязка, судящая ответ.
3. **Невязка** — наихудшее из синуса угла и четырёх знаковых расстояний, смешанный
   числовой масштаб при прежнем `RESIDUAL_LIMIT = 1e-6` (как у прежних семейств). Порог
   нулевого отрезка — тот же `1e-9`, что у угла.
4. **Layout v5 кумулятивен**: Sketch с углом и Collinear хранится как v5 и называет только
   `sketch.constraints.collinear.v1`; читатель v4 его не читает (сохраняет и отказывает).
5. **Форма стала шире, а не выше.** Кнопка `Add Collinear` — на строке отношений пары;
   правый край строки 671.1 pt (раньше 577.5, при абзаце шириной 598). Прежний gate §31F
   «строки угла не шире остальной формы» заменён на «форма не шире 700 pt и строка осталась
   одной» (иначе кнопка не помещалась бы без новой строки, а высота запаса не имеет).
6. **Redundancy диагностируется как есть.** `getPartiallyRedundant` shim не отдаёт;
   ранжирование на стартовых положениях — ограничение закреплённой библиотеки (см. выше),
   проверено тестом, а не «исправлено» вторым решателем или анализом транзитивности.
7. **Профильные правила — последний рубеж.** Не слабляются ради solve: соседние Lines
   «в линию» и дно выемки «на низ» отказываются обычной проверкой многоугольника.

## Ограничения (чего здесь нет)

Дуги, касание, симметрия, выражения, произвольные плоскости, construction-Lines (профиль с
ними редактор не принимает вовсе), второй solver, смена native pin, новая схема SQLite,
ABI-версия как отдельное число (раскладка не менялась), анализ транзитивности пар,
выравнивание «без связи», `Coincident` на концах как замена; перетаскивание вершин и
привязки на Sketch с ограничениями по-прежнему недоступны, как и для прежних ограничений.
Hover-опоры §31D, привязка к центру §31C, §30, Milestone 5C, продукт целиком и
расследование OOM **не закрыты**. Окно приложения и CUA в этом срезе не запускались.

## Воспроизводимый оконный сценарий

Приватные входные модели создаёт короткий скрипт (каталог должен не существовать);
результатов в нём нет:

```python
import json, os, pathlib, subprocess, sys
cli, root = os.environ["FERRITECAD"], pathlib.Path(sys.argv[1]); root.mkdir(parents=True)
def run(*a):
    p = subprocess.run([cli, *map(str, a), "--json"], capture_output=True, encoding="utf-8")
    assert p.returncode == 0, (a, p.stdout); return json.loads(p.stdout)["result"]
for name, pts in {"u": [[50, 22], [35, 22], [35, 8], [15, 8], [15, 20], [0, 20], [0, 0], [50, 0]],
                  "z": [[0, 0], [40, 0], [40, 9], [60, 9], [60, 21], [20, 21], [20, 10], [0, 10]]}.items():
    (root / "p.json").write_text(json.dumps({"request_version": 1, "points_mm": pts, "height_mm": 10}))
    run("create-sketch-extrude", root / "p.json", "-o", root / f"{name}.fcad")
```

Сценарий (≈10 минут; окно открывает ревьюер, сравнение — через CLI):

1. Открыть `u.fcad` → `Edit constraints …`. `Segment 1` → `Pair Line A`, `Segment 5` →
   `Pair Line B`, `Add Collinear`. В «Pending additions» — одна запись
   `Collinear · Lines … and …`. Нажать `A: start>end` и `B: start>end` — запись и
   запрос не меняются (направления на Collinear не влияют). `Undo`/`Redo` меняют запись,
   не запуская job.
2. Тот же `Add Collinear` ещё раз, `Add Parallel` и `Add Perpendicular` на той же паре —
   отказ словом «Collinear», запись одна. Пара из одной Line — отказ. `Add Equal length` на
   той же паре принимается (свой слот).
3. Проверить глазами: все кнопки строки на экране, нижние действия
   (`Clear pending changes`, `Save constraints copy…`, `Apply constraints`) на 768 pt
   достижимы, строка не обрезана.
4. `Save constraints copy…` → отменить диалог (draft цел) → снова, сохранить рядом.
5. На несохранённом документе `Apply constraints`; экспорт STL **до** Save; `Undo` и
   `Redo` сессии; `Save As`.
6. Сравнить с peer CLI: `edit-sketch-constraints-copy u.fcad --sketch <sketch>
   --expect-version <hash> --request r.json -o peer.fcad` с
   `{"rule":"collinear","a_curve_id":L1,"b_curve_id":L5}` и `export-stl` обоих; затем
   измерить из STL знаковые расстояния концов ободьев от прямых друг друга (формула
   рецепта): оба файла дают ≈0, а контроль Parallel (`{"rule":"parallel",…}`) — ±2 мм.
7. Открыть вторую копию того же файла в другой вкладке: пара и «Pending additions» не
   перетекают между вкладками.
8. Замена: файл, уже несущий Collinear, → `Remove` у строки `Collinear · …`, пара,
   `Add Parallel`, `Apply` — один запрос `remove` + `add`. Z-профиль (`z.fcad`, `Segment 3`
   и `Segment 7` — уступы, идущие в противоположные стороны) должен вести себя так же.
