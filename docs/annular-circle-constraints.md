# §25O — параметрическое кольцо: два радиуса, общий центр, один pin

[Локальные доказательства и ограничения](annular-circle-constraints-verification.md).
Тот же сохраняемый constraint-маршрут, что у
[Line-ограничений (§25E–I)](sketch-constraints-copy.md) и
[одной окружности (§25N)](circle-radius-constraints.md): один снимок, общая
подготовка, атомарный remove/add, baseline cold rebuild, strict-проверка refs,
повторная проверка версии, закрытие SQLite и атомарная no-clobber публикация.
Новое — только словарь: у сохранённой пары окружностей [§25L](annular-sketch-extrude.md)
появляются **радиус каждой окружности**, **концентричность** и **закрепление
общего центра** в точных X/Y mm.

UI: `Edit constraints <имя> — <UUID>…` → строки `Boundary circle · …` и
`Bore circle · …`; CLI — та же команда, что у Line и у одной окружности:

```text
ferritecad edit-sketch-constraints-copy <source.fcad> --sketch UUID \
    --expect-version HASH --request <request.json> -o <copy.fcad> [--json]
```

## Что появляется

Из тех же сохранённых координат — общий центр (12, −7), R = 10, r = 4, h = 15 —
и запроса `concentric` + `radius 6.75` + `radius 2.125` + `fixed center
(−3.5, 4.25)` получается кольцо, чья геометрия взята **из решения PlaneGCS**:
четыре аналитические грани, `Cylinder{6.75}` и `Cylinder{2.125}` каждый под
прежним `ExtrudeSide` ref своей `Circle`, объём π(R−r)(R+r)h.

Запрос называет **один** центр — центр внешней окружности. Центр отверстия
никуда не присваивается: его двигает концентричность, через solver. Это и есть
проверка, которую одинаковые сохранённые центры сами по себе не дают —
закреплённый центр уезжает из (12, −7) в (−3.5, 4.25), и отверстие уезжает с ним.

Сохранённая геометрия Sketch остаётся исходным приближением — (12, −7), 10 и 4 —
ровно как у Line-ограничений и у §25N: solver-ответ не переписывает документ.
Значит, при снятии ограничения соответствующая величина возвращается к
сохранённому приближению, а не к последнему решённому числу; сохранения
последнего решения этот срез не обещает.

## Границы среза

Прежний поддерживаемый frame §25K: один untransformed XY DatumPlane, один
Sketch, один forward literal Blind `Extrude`/`NewBody` и его `Body`. Sketch —
ровно **две** неконструкционные аналитические `Circle` внутри политики
`AnnularExtrusion`. Три возможности: `Radius` (по одной на окружность),
`Concentric` (пара) и `Fixed` центра (одна на профиль). Прежние Line- и
single-Circle маршруты сохраняются без изменений; `edit-annular` (§25M) и
`edit-circle` (§25K) продолжают отвечать за свои классы.

Не входят: Diameter, Tangent, EqualRadius, Arc, смешанные профили, больше одного
отверстия, неконцентрический результат, mouse drag, live solve, boolean Cut,
in-place Save и выражения вместо literal чисел.

## Solver и C ABI не менялись

Реализация `ferritecad-sketch-solver`, C ABI и shim не менялись;
в solver-крейт добавлен тест концентричности.
Концентричность — это `Constraint::Coincident` **двух центров**, а центр
окружности уже является адресуемой точкой эскиза с §25N. Второго вычисления
центра, фиктивной точки обода и ручного счёта DOF нет; степени свободы
по-прежнему сообщает библиотека.

Измерено на настоящей библиотеке, каждое число прочитано у неё, а не записано
заранее (`a_concentric_pair_loses_its_freedom_step_by_step_and_moves_as_one`).
Две окружности намеренно начинаются в **разных** местах — пара, уже стоявшая в
одном центре, двигалась бы вместе при любой реализации Coincident:

| Что | DOF |
| --- | --- |
| две свободные окружности | 6 |
| + `Concentric` | 4 |
| + два `Radius` | 2 |
| + `Fixed` одного центра | 0 |

Свобода возвращается при снятии, измеренная снова. Сдвиг закреплённого центра в
другое место двигает **оба** центра: закреплён один, названы оба.

## Роли

Граница — окружность с большим сохранённым радиусом, отверстие — с меньшим.
Это читает **та же** функция, которую §25M уже использует
(`annulus_edit::pair_in_roles` поверх `roles`), из сохранённых радиусов и только
из них: документ, в котором отверстие записано первым, правится с теми же UUID в
тех же ролях.

Роли выбранного источника обязаны сохраниться **после решения**. Прежде чем
что-то опубликовать, общий rebuild берёт решённые центры и радиусы, находит
каждую окружность по её собственному UUID — не по позиции и не по величине — и
подаёт их в `AnnularExtrusion` в **сохранённых** ролях. Пара, поменявшаяся
размерами, отказывается там же, где отказался бы запрос с отверстием больше
границы: переименование ролей по решённым числам легализовало бы обмен.

## Каждое ограничение адресует свою геометрию

* `Radius` называет **свой** Circle UUID. Один радиус на окружность; на паре
  это два независимых слота.
* `Concentric` называет **обе** окружности, и ни одна не ведущая. `(A,B)` и
  `(B,A)` — один слот: это одно отношение, заданное дважды.
* `Fixed` центра — прежнее точечное правило §25N, один pin на профиль. Одного
  допустимого pin вместе с концентричностью хватает, чтобы закрепить оба центра.

## Persisted layout и capability не поднимаются

Проверено первым: **существующий** контракт `payload v3` +
`sketch.constraints.circle.v1` выражает новый словарь без потерь.

`Sketch::schema_version` ставит v3, как только хоть одно ограничение
`needs_circle_vocabulary()`, а это правило уже включает **любой**
`SketchPointSelector::Center` среди точек правила — не только `Fixed`. Значит
`Coincident` двух центров сам по себе поднимает Sketch до v3 и требует
`sketch.constraints.circle.v1`; сборка, знающая эту capability, знает и `Center`,
и `Coincident`, и переписывает такой Sketch без потерь, а сборка, не знающая
её, останавливается прежней проверкой. Новое имя или новый layout добавили бы
слово и ни одного отказа, поэтому не добавлены. Прежние v1/v2 не тронуты, схема
SQLite не поднята, `require_declared_contract` работает как работал.

## Writer

Прежний узкий `write_sketch_constraints`. Его правило — «каждая
Coincident-связь, которая была, осталась» — уточнено до того, что оно
охраняет: **замыкание Line-профиля**. `Coincident` между двумя *центрами* —
не замыкание: это концентричность, она показана пользователю как таковая и
снимается как таковая. Различаются они по тому, **что говорит правило**, а не по
тому, на каком профиле стоят, поэтому связь между двумя концами Line защищена
ровно как была. Проверка при этом стала строже: сохранённая закрывающая связь
должна вернуться под своим UUID **и с тем же правилом**, так что подменённый
payload не может переименовать замыкание в снимаемую концентричность.

## Discovery

`inspect --json` не меняет смысла прежних полей. У каждой записи
`constraint_edit.circles` появляется `role`:

```json
"constraint_edit": {
  "available": true, "refusal": null, "document_refusal": null,
  "curves": [],
  "circles": [
    {"curve_id":"UUID_R","center_mm":[12.0,-7.0],"radius_mm":10.0,"role":"boundary"},
    {"curve_id":"UUID_r","center_mm":[12.0,-7.0],"radius_mm":4.0,"role":"bore"}
  ],
  "constraints": [
    {"constraint_id":"UUID","rule":{"kind":"radius","curve_id":"UUID_R","radius":6.75}},
    {"constraint_id":"UUID","rule":{"kind":"coincident",
      "a":{"curve_id":"UUID_R","at":"center"},"b":{"curve_id":"UUID_r","at":"center"}}},
    {"constraint_id":"UUID","rule":{"kind":"fixed",
      "point":{"curve_id":"UUID_R","at":"center"},"x":-3.5,"y":4.25}}
  ]
}
```

* `role` — `"boundary"`, `"bore"` или `null` для профиля из одной окружности, у
  которой нет второй, чтобы держать роль. Он сообщается, а не выводится
  читателем: роли — то, о чём говорят все запросы и отказы.
* `center_mm`/`radius_mm` — **сохранённое** приближение, стартовая догадка
  solver'а. Решённые значения не хранятся и в discovery не приходят.
* `curves` остаётся списком Lines и для кольца пуст.
* `annulus_edit` (§25M) и `circle_edit` (§25K) становятся `available:false` у
  профиля **с** ограничениями и снова доступны, когда все сняты.
* Обе окружности и обе роли берутся из **того же** снимка, что и всё остальное;
  файл ради DTO второй раз не открывается.

## Request v1

Прежний `edit-sketch-constraints-copy`, прежний `request_version: 1`, прежние
`remove`/`add`. Старые Line- и circle-формы принимаются без изменений. Новая —
одна:

```json
{"rule":"concentric","a_curve_id":"UUID","b_curve_id":"UUID"}
```

* Обе окружности названы полностью и ни одна не ведёт — как у `equal_length`,
  `parallel` и `perpendicular`. Числа у правила нет: «в одном месте» — не
  величина; `at` нет: у окружности здесь может иметься в виду только центр.
* Слоты: один радиус на окружность, один pin на профиль, одна концентричность на
  неупорядоченную пару. Замена радиуса — один атомарный `remove` + `add`;
  промежуточной публикации нет. Дубликат слота — **структурный** отказ
  подготовки, и он говорит именно это, а не выдаёт себя за диагноз PlaneGCS.
* Круговое правило на Line-профиле, Line-правило на кольце и `concentric` на
  профиле из одной окружности отвергаются: запрос просит геометрию, которой там
  нет.
* Снятие концентричности допустимо, **если результат остаётся поддерживаемым
  кольцом**. Снять её, оставив pin, который развёл центры, — атомарный отказ без
  публикации; снять её вместе с pin'ом, когда оба центра возвращаются к общему
  сохранённому приближению, — обычная публикация.

Прежние bounded чтение (65536 байт), UTF-8 preflight, `--expect-version`,
envelope, `operation` и коды 0/2/7 не менялись.

## UI

Тот же constraint draft и тот же worker. Кольцевой профиль показывает две строки
— `Boundary circle · <UUID> · centre (x, y) mm · radius R mm` и `Bore circle ·
…` — сохранённые числа, не решённые. Выбор строки задаёт окружность для `Radius`
и `Fixed centre`; `Pair Circle A`/`Pair Circle B` задают пару, `Add Concentric`
её добавляет. Неприменимые Line-действия (`Add Horizontal`, `Add length`,
`Pin Start`, `Pair Line A`) скрыты.

Сохранённые строки показывают собственные UUID и адресуемую геометрию:
`Radius R mm · <UUID>`, `Concentric · <UUID>` с обеими окружностями,
`Fixed centre (x, y) mm · <UUID>`. У каждой есть `Remove`; концентричность
**не** попадает в нередактируемую строку `Coincident closure`. Один Apply — один
шаг Undo, до 128; Undo и Redo не обращаются ни к одному job. Save Cancel, отказ
worker, устаревший ответ и отказ последующего Open сохраняют черновик и историю
через общий `draft_published`/`draft_load_finished`. Валидация — `validate_edits`
того же документа; второй копии правил в UI нет.

В сборке с OCCT без solver кольцо создаётся и cold-rebuild'ится, discovery
читает ограничения, а добавление ограничения отказывает `unsupported` без
публикации. Даже удаление последнего ограничения из уже ограниченного источника
требует solver: общий copy job сначала cold-rebuild'ит исходник и проверяет его
refs.

## Рецепт

Нужны OCCT, planegcs, `FERRITECAD` и `FCAD_UFBX_READER`.

```python
# FCAD_25O_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-annular-constraints-"))
DEFLECTION = 0.05
ANGULAR = 0.1

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, center, outer, inner, height):
    """Everything the mesh must be for the solved part to really be hollow."""
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

    # Both walls, at their own radii about the solved centre, facing the right way.
    def on(t, radius):
        return all(abs(math.hypot(v[0] - center[0], v[1] - center[1]) - radius)
                   < DEFLECTION + 1e-4 for v in t)
    def radial(t):
        a, b, c = t
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2]]
        mx = (a[0] + b[0] + c[0]) / 3 - center[0]
        my = (a[1] + b[1] + c[1]) / 3 - center[1]
        return n[0] * mx + n[1] * my
    bore = [t for t in tris if on(t, inner)]
    skin = [t for t in tris if on(t, outer)]
    assert len(bore) >= 12 and len(skin) >= 12, "a wall is missing"
    assert all(radial(t) < 0 for t in bore), "the cavity is inside out"
    assert all(radial(t) > 0 for t in skin), "an outer facet faces the axis"

    # Nothing covers the axis, so the hole goes through.
    def covers(t):
        (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12:
            return False
        a = ((y2 - y3) * (center[0] - x3) + (x3 - x2) * (center[1] - y3)) / d
        b = ((y3 - y1) * (center[0] - x3) + (x1 - x3) * (center[1] - y3)) / d
        return a >= -1e-9 and b >= -1e-9 and 1 - a - b >= -1e-9
    assert not any(covers(t) for t in tris), "a triangle covers the axis"

    lo = [min(v[j] for t in tris for v in t) for j in range(3)]
    hi = [max(v[j] for t in tris for v in t) for j in range(3)]
    assert abs((hi[2] - lo[2]) - height) < 1e-4, (lo, hi)
    for j in (0, 1):
        assert abs((lo[j] + hi[j]) / 2 - center[j]) < 1e-3, (j, lo, hi)
        assert hi[j] - lo[j] >= 2 * outer - 2 * DEFLECTION - 1e-4
    # A mesh is an inscribed prism: bounded above by the exact ring, and well
    # below the solid rod the same outline would give without the hole.
    exact = math.pi * (outer - inner) * (outer + inner) * height
    assert 0 < volume <= exact * (1 + 1e-6), volume
    assert volume >= exact * 0.99
    assert volume < math.pi * outer * outer * height * 0.999, "no hole in the mesh"
    return volume

# 1. A source the shipped creation command makes.
create = root / "create.json"
source = root / "ring.fcad"
create.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                              "outer_radius_mm": 10.0, "inner_radius_mm": 4.0,
                              "height_mm": 15.0}))
assert run(["create-annular-extrude", str(create), "-o", str(source), "--json"])["ok"]

# 2. Explicit UUIDs, roles and the exact version come from one JSON reading.
seen = run(["inspect", str(source), "--json"])["result"]
version = seen["content_version"]
sketch, = seen["sketches"]
discovery = sketch["constraint_edit"]
assert discovery["available"] is True and discovery["refusal"] is None
assert discovery["curves"] == [], "an annular profile offers no Line"
assert discovery["constraints"] == []
roles = {c["role"]: c for c in discovery["circles"]}
assert set(roles) == {"boundary", "bore"}, discovery["circles"]
boundary, bore = roles["boundary"]["curve_id"], roles["bore"]["curve_id"]
assert roles["boundary"]["radius_mm"] == 10.0 and roles["bore"]["radius_mm"] == 4.0
assert roles["boundary"]["center_mm"] == roles["bore"]["center_mm"] == [12.0, -7.0]
assert sketch["editable"] is False and sketch["vertices"] is None
assert sketch["annulus_edit"]["available"] is True
assert seen["edit_extrude"]["available"] is True

# 3. Concentricity, two radii and one pin, in one request. Only the boundary's
#    centre is named; the bore follows through the shared centre.
request = root / "request.json"
sized = root / "sized.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": [], "add": [
    {"rule": "concentric", "a_curve_id": boundary, "b_curve_id": bore},
    {"rule": "radius", "curve_id": boundary, "radius_mm": 6.75},
    {"rule": "radius", "curve_id": bore, "radius_mm": 2.125},
    {"rule": "fixed", "curve_id": boundary, "at": "center",
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
assert len(added) == 4 and len(set(added)) == 4
assert source.read_bytes() == before, "the source was touched"

# 4. Reopen: the stored sketch is still the starting guess, both roles survive,
#    and the cold rebuild resolves every name.
after = run(["inspect", str(sized), "--json"])["result"]
kept = after["sketches"][0]["constraint_edit"]
saved = {c["curve_id"]: c for c in kept["circles"]}
assert saved[boundary]["role"] == "boundary" and saved[bore]["role"] == "bore"
assert saved[boundary]["radius_mm"] == 10.0 and saved[bore]["radius_mm"] == 4.0
assert saved[boundary]["center_mm"] == [12.0, -7.0], "solved values overwrote the guess"
rules = [json.dumps(c["rule"], sort_keys=True) for c in kept["constraints"]]
assert any('"kind": "coincident"' in r and '"at": "center"' in r for r in rules), rules
assert any('"kind": "radius"' in r and '6.75' in r for r in rules), rules
assert any('"kind": "radius"' in r and '2.125' in r for r in rules), rules
assert any('"kind": "fixed"' in r for r in rules), rules
# The geometry editors step back while constraints hold the profile.
assert after["sketches"][0]["annulus_edit"]["available"] is False
assert after["sketches"][0]["circle_edit"]["available"] is False
checked = run(["validate", str(sized), "--json"])
assert checked["ok"] and checked["result"]["valid"], checked
rebuilt = run(["rebuild", str(sized), "--cold"])
assert "solid, 4 named faces" in rebuilt, rebuilt
assert "4 of 4 stored references resolved" in rebuilt, rebuilt
named = run(["print-topology", str(sized)])
for curve in (boundary, bore):
    assert f"extrude side from segment {curve}" in named, named

# 5. The exported solid is the ring the solver decided.
volume = measure(sized, (-3.5, 4.25), 6.75, 2.125, 15.0)
fbx = root / "sized.fbx"
written = run(["export-fbx", str(sized), "-o", str(fbx), "--json"])
assert written["result"]["complete"] and written["result"]["geometries"] == 1
assert fbx.stat().st_size == written["result"]["bytes"]
read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
assert read.returncode == 0, read.stderr
executed = [l for l in read.stdout.splitlines()
            if l.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED")]
assert executed and executed[0].endswith("failures=0"), read.stdout

# 6. Replacing one radius keeps the shared centre, the pin and every other UUID.
catalog = run(["inspect", str(sized), "--json"])["result"]
stored = catalog["sketches"][0]["constraint_edit"]["constraints"]
pick = lambda f: next(c["constraint_id"] for c in stored if f(c["rule"]))
bore_radius = pick(lambda r: r["kind"] == "radius" and r["curve_id"] == bore)
pin = pick(lambda r: r["kind"] == "fixed")
concentric = pick(lambda r: r["kind"] == "coincident")
widened = root / "widened.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": [bore_radius], "add": [
    {"rule": "radius", "curve_id": bore, "radius_mm": 3.5}]}))
again = run(["edit-sketch-constraints-copy", str(sized),
             "--sketch", sketch["sketch_id"],
             "--expect-version", catalog["content_version"],
             "--request", str(request), "-o", str(widened), "--json"])
assert again["result"]["removed_constraint_ids"] == [bore_radius]
assert again["result"]["solve"]["degrees_of_freedom"] == 0
survivors = [c["constraint_id"]
             for c in run(["inspect", str(widened), "--json"])["result"]
             ["sketches"][0]["constraint_edit"]["constraints"]]
assert pin in survivors and concentric in survivors, survivors
thinner = measure(widened, (-3.5, 4.25), 6.75, 3.5, 15.0)
assert thinner < volume

# 7. Concentricity comes off only when what is left is still a ring.
catalog = run(["inspect", str(widened), "--json"])["result"]
names = sorted(p.name for p in root.iterdir())
request.write_text(json.dumps({"request_version": 1, "remove": [concentric],
                               "add": []}))
refused = run(["edit-sketch-constraints-copy", str(widened),
               "--sketch", sketch["sketch_id"],
               "--expect-version", catalog["content_version"],
               "--request", str(request), "-o", str(root / "never.fcad"),
               "--json"], code=2)
assert refused["ok"] is False
assert refused["error"]["kind"] in ("input", "unsupported", "constraint"), refused["error"]
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"

freed = root / "freed.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": [concentric, pin],
                               "add": []}))
loosened = run(["edit-sketch-constraints-copy", str(widened),
                "--sketch", sketch["sketch_id"],
                "--expect-version", catalog["content_version"],
                "--request", str(request), "-o", str(freed), "--json"])
assert loosened["result"]["solve"]["degrees_of_freedom"] == 4, loosened["result"]
measure(freed, (12.0, -7.0), 6.75, 3.5, 15.0)

# 8. Taking everything off hands the profile back to the annulus editor.
catalog = run(["inspect", str(freed), "--json"])["result"]
ids = [c["constraint_id"]
       for c in catalog["sketches"][0]["constraint_edit"]["constraints"]]
assert len(ids) == 2
plain = root / "plain.fcad"
request.write_text(json.dumps({"request_version": 1, "remove": ids, "add": []}))
gone = run(["edit-sketch-constraints-copy", str(freed),
            "--sketch", sketch["sketch_id"],
            "--expect-version", catalog["content_version"],
            "--request", str(request), "-o", str(plain), "--json"])
assert gone["result"]["solve"] is None, "nothing left to solve is not zero freedom"
bare = run(["inspect", str(plain), "--json"])["result"]["sketches"][0]
assert bare["constraint_edit"]["constraints"] == []
assert bare["annulus_edit"]["available"] is True
measure(plain, (12.0, -7.0), 10.0, 4.0, 15.0)

# 9. The existing height edit keeps the constraints and the solved geometry.
taller = root / "taller.fcad"
run(["edit-extrude", str(sized), "--feature",
     run(["inspect", str(sized), "--json"])["result"]["features"][0]["feature_id"],
     "--distance-mm", "25", "-o", str(taller), "--json"])
raised = run(["inspect", str(taller), "--json"])["result"]
assert raised["features"][0]["distance_mm"] == 25.0
assert len(raised["sketches"][0]["constraint_edit"]["constraints"]) == 4
measure(taller, (-3.5, 4.25), 6.75, 2.125, 25.0)

# 10. Refusals publish nothing.
names = sorted(p.name for p in root.iterdir())
for bad in ([{"rule": "concentric", "a_curve_id": boundary, "b_curve_id": bore},
             {"rule": "radius", "curve_id": boundary, "radius_mm": 2.0},
             {"rule": "radius", "curve_id": bore, "radius_mm": 10.0}],
            [{"rule": "concentric", "a_curve_id": boundary, "b_curve_id": bore},
             {"rule": "radius", "curve_id": boundary, "radius_mm": 6.0},
             {"rule": "radius", "curve_id": bore, "radius_mm": 5.9999999}],
            [{"rule": "radius", "curve_id": boundary, "radius_mm": 6.75},
             {"rule": "radius", "curve_id": bore, "radius_mm": 2.125},
             {"rule": "fixed", "curve_id": boundary, "at": "center",
              "x_mm": -3.5, "y_mm": 4.25}],
            [{"rule": "concentric", "a_curve_id": boundary, "b_curve_id": bore},
             {"rule": "concentric", "a_curve_id": bore, "b_curve_id": boundary}],
            [{"rule": "concentric", "a_curve_id": boundary, "b_curve_id": boundary}],
            [{"rule": "radius", "curve_id": boundary, "radius_mm": 0.0}],
            [{"rule": "horizontal", "curve_id": boundary}]):
    request.write_text(json.dumps({"request_version": 1, "remove": [], "add": bad}))
    refused = run(["edit-sketch-constraints-copy", str(source),
                   "--sketch", sketch["sketch_id"], "--expect-version", version,
                   "--request", str(request), "-o", str(root / "never.fcad"),
                   "--json"], code=2)
    assert refused["ok"] is False
    assert refused["error"]["kind"] in ("input", "unsupported", "constraint"), refused["error"]
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"
assert source.read_bytes() == before
print("FCAD_25O_RECIPE_OK", round(volume, 6), round(thinner, 6))
```
