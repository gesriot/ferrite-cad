# §25L — аналитический круговой профиль с одним концентрическим отверстием

[Локальные доказательства и ограничения](annular-sketch-extrude-verification.md).
Тот же общий маршрут создания, что у
[одной окружности (§25J)](circle-sketch-extrude.md): один typed request,
`NewDocument`/jobs операция с kernel-checked построением, cold rebuild,
проверка refs, освобождение handles, закрытие SQLite и атомарная no-clobber
публикация. Новой схемы БД, второго writer, второго документного lifecycle,
tessellated payload и boolean Cut здесь нет.

UI: `Profile: Line polygon | Circle | Circle with hole`; CLI:

```text
ferritecad create-annular-extrude <request.json> -o <new.fcad> [--json]
```

## Что появляется

Из общего центра XY, внешнего радиуса `R`, внутреннего `r` и положительной
Blind-высоты `h` создаётся одна **полая** цилиндрическая деталь. В документе —
настоящий Sketch с **двумя** аналитическими `Circle`, у каждой свой постоянный
`StableEntityId`, один `Extrude`/`NewBody` и один `Body`. Отверстие существует
в B-Rep, в STL и в FBX после закрытия и холодного открытия документа.

Отверстие — **второй контур того же профиля**, а не второе тело, не boolean и
не маска экспорта. Обе окружности остаются аналитическими на всём пути:
полигоном ни одна из них не аппроксимируется, и в B-Rep деталь имеет ровно
четыре грани — два `Cylinder` радиусов `R` и `r` и два `Plane`.

## Поддерживаемый класс

Ровно две неконструкционные, неограниченные, концентрические XY `Circle`,
`0 < r < R`, один untransformed XY DatumPlane, forward literal Blind
`Extrude`/`NewBody`, один `Body`.

Отказываются до публикации: произвольные вложенные контуры, больше одного
отверстия, неконцентрическое отверстие, совпадающие или непересекающиеся
окружности, смешанные Line/Arc-профили, несколько тел, circle constraints и
boolean Cut.

## Числовая policy

`AnnularExtrusion` переиспользует политику одной окружности для границы —
конечные центр, радиус и высота, всё в пределах 1 000 000 mm — и добавляет две
собственные величины.

| Величина | Значение | Откуда |
| --- | --- | --- |
| `CONCENTRIC_MM` | `1e-7` | `Tolerance::DEFAULT_LINEAR`, то есть `Precision::Confusion` OCCT на миллиметровом масштабе: ближе этого две точки для ядра — одна точка |
| `MIN_WALL_MM` | `1e-3` | консервативная граница этого среза, см. ниже |

Минимум 0,001 mm — консервативная продуктовая политика, а не универсальная
гарантия точности OCCT. В независимом измерении на OCCT 8.0.1 для `R=10`, `h=1`
расхождение B-Rep с устойчивой формулой `π(R−r)(R+r)h` составило примерно
3,2e-13 при стенке 1e-3 и 5,9e-9 при 1e-7 mm. Формула `π(R²−r²)h` сама теряет
точность при близких радиусах и не годится для такой оценки ошибки ядра.
Успешное построение тонкой стенки само по себе точность не гарантирует.

Граница стенки сравнивается как `R >= r + MIN_WALL_MM`: например, `10` и
`9.999` принимаются, несмотря на округление их разности в `f64`. Погрешность
этого сравнения не превышает одного ULP радиуса и существенно меньше допуска
ядра в разрешённом диапазоне. Концентричность означает евклидово расстояние
между центрами не больше 1e-7 mm; координаты не округляются и не сдвигаются.

## Outer и inner

Граница — окружность **большего** радиуса, отверстие — меньшего. Это читается
из геометрии и только из неё: порядок, в котором две `Circle` записаны в
Sketch, — порядок представления и решает **ничего**. Документ с переставленными
кривыми — тот же чертёж, даёт то же отверстие, и каждая стенка по-прежнему
принадлежит той окружности, которая её подняла.

Оба контура остаются `closed_curve`: фиктивного seam endpoint, `ProfileJoint(id,
id)` и придуманного угла нет ни у одного. Шов, который OCCT кладёт на круговое
ребро, принадлежит параметризации этого ребра и наружу как вершина чертежа не
выдаётся.

## Identities

Ссылок четыре, все — `TopologyRef` с собственными UUID:

* `ExtrudeCap { side: Start }` и `ExtrudeCap { side: End }` — прежние
  типизированные anchors, по одному на торец;
* `ExtrudeSide { profile_segment }` **дважды** — по одной на окружность, каждая
  с `AllDerivedFrom { ancestor }` по UUID своей `Circle`.

Две ссылки, а не одна на обе: наружная стенка трубы и стенка отверстия — две
разные грани готовой детали, и одно правило «все грани этого профиля» разрешало
бы в обе, не давая последующей операции способа сказать, какая имеется в виду.
Ни одна ссылка не привязана к индексу грани OCCT и не зависит от порядка
тесселяции.

## C ABI

`fc_occt_extrude` получил **явные** границы контуров — новый аргумент
`const size_t *loop_segment_counts` и его длину `size_t loop_count`. Прежние
аргументы не переинтерпретированы: `segments`/`segment_count` по-прежнему весь
список сегментов, а границы, которые `segment_count` выразить не может,
приехали отдельно. Один контур — это `loop_count` 1 и одна длина, равная
`segment_count`: прежний путь одноконтурных профилей сохранён побайтово в
поведении.

Контур 0 ограничивает область, остальные — отверстия в ней. `segment_index` в
`fc_occt_extrude_side_faces`/`_cap_edges` индексирует `segments`, то есть идёт
по всем контурам в переданном порядке; `joint_index` в `_sweep_edges`/
`_cap_vertices` считает углы в том же порядке контуров, и контур из одной
замкнутой кривой углов не даёт. Rust проверяет разбиение перед вызовом, bridge
— повторно внутри; ни одна из проверок не стоит одна.

Ориентация внутреннего контура **не предполагается**: каждое отверстие
добавляется в той из двух своих ориентаций, которая даёт грань, принимаемую
`BRepCheck_Analyzer` **и** измеримо уменьшающую её площадь. Готовая грань
проверяется ещё раз, потому что отверстия, каждое из которых лежит внутри
границы, могут пересекаться между собой. Отдельной правки ориентации в STL или
FBX нет и не нужно.

Согласованность Rust/header/layout/counts проверяется тестом
`the_segment_and_surface_numbers_match_the_c_header`: порядок параметров
читается из самого заголовка, а `FcOcctSegment` по-прежнему сверяется
`static_assert` в C++ и `offset_of!` в Rust.

## Кэш

Ключ считает отверстие: `Profile::feed` уже кормит число внутренних контуров и
каждый из них, поэтому изменившийся внутренний радиус даёт другой ключ.
Отдельно: изменённый собственный shim меняет `FERRITECAD_BRIDGE_BUILD`, который
входит в `KernelIdentity` и, значит, в каждый ключ, — записи, сделанные сборкой
до этого среза, не находятся вовсе. Случайного попадания в старый результат
нет; это измерено настоящими Miss/Hit, а не двумя запусками `--cold`.

## Что этот срез НЕ добавляет

Создание геометрии не означает, что её умеют править. Ни координатный редактор
Line (§25B), ни редактор окружности (§25K) двухокружностный Sketch не понимают
и оба честно отказывают со своей причиной; `inspect --json` показывает это в
`editable`/`refusal` и в `circle_edit`. Собственной правки радиусов этот срез не
добавляет. Прежний `edit-extrude` меняет высоту полой детали, сохраняя обе
`Circle`, их UUID, обе side-ссылки и все прочие SQL-ячейки.

## Request v1

Строгий, до 65536 байт, ровно пять полей, `deny_unknown_fields`:

```json
{"schema_version":1,"center_mm":[12.0,-7.0],"outer_radius_mm":10.0,
 "inner_radius_mm":4.0,"height_mm":15.0}
```

* `schema_version` — целое 1. Написание версии то же, что у
  `create-circle-extrude`, которую этот запрос расширяет; `request_version`
  здесь лишнее поле и отвергается.
* `center_mm` — ровно две конечные координаты в mm, общий центр обеих
  окружностей; ноль и отрицательные допустимы.
* `outer_radius_mm`, `inner_radius_mm` — конечные строго положительные числа в
  mm, `inner < outer`, разность не меньше `MIN_WALL_MM`.
* `height_mm` — конечная строго положительная Blind-высота в mm.
* `radius_mm`, `holes`, `inner_center_mm` и любые другие поля лишние. Отдельного
  центра у отверстия нет: класс этого среза — концентрический.

## JSON v1

Прежний envelope: `schema_version` 1, `operation` `create-annular-extrude`,
`ok` true и `result`, один объект + LF, exit 0. Отказ — `ok` false,
`kind`/`message`/`causes`, exit 2, без публикации и без scratch. Потеря stdout
после состоявшейся публикации — exit 7: документ цел и читаем, повторять
операцию нельзя без проверки назначения. UTF-8 paths обязательны в JSON-режиме;
usage/help остаются clap-текстом. Text и JSON идут через один job и один
emitter; нового протокола, batch и второго emitter нет.

`result` называет **всё**, что было создано, без угадывания по имени и порядку:

```json
"result": {
  "destination": "…/tube.fcad", "document_id": "UUID",
  "sketch_id": "UUID", "extrude_id": "UUID", "body_id": "UUID",
  "outer_curve_id": "UUID", "inner_curve_id": "UUID"
}
```

Оба UUID окружностей названы явно и различны: читатель адресует наружную стенку
и стенку отверстия по отдельности, и ни один из двух не выводится из другого.
Идентификаторы берутся из самого создания, а не из повторного чтения файла.

## Рецепт

Нужны OCCT, `FERRITECAD` и `FCAD_UFBX_READER` — путь к pinned reader,
собранному из `tools/unity-fbx-smoke/scripts/read_production.c` с pin из
`fetch_ufbx.sh`. На macOS native library paths экспортируются в том же Bash.

```python
# FCAD_25L_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-annulus-"))
DEFLECTION = 0.05
ANGULAR = 0.1

def run(args, code=0):
    p = subprocess.run([cli, *args], capture_output=True, text=True)
    assert p.returncode == code, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, center, outer, inner, height):
    """Everything about the mesh that says the part is hollow."""
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

    # Both walls, at their own radii, facing the right way.
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

    # Nothing covers the axis, so the bore goes through.
    def covers(t, px, py):
        (x1, y1), (x2, y2), (x3, y3) = [(v[0], v[1]) for v in t]
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12:
            return False
        u = ((y2 - y3) * (px - x3) + (x3 - x2) * (py - y3)) / d
        w = ((y3 - y1) * (px - x3) + (x1 - x3) * (py - y3)) / d
        return u >= -1e-9 and w >= -1e-9 and 1 - u - w >= -1e-9
    assert not any(covers(t, *center) for t in tris), "the bore is not open"

    exact = math.pi * (outer ** 2 - inner ** 2) * height
    full = math.pi * outer ** 2 * height
    assert 0 < volume <= exact * (1 + 1e-6), volume
    assert volume < full * 0.999, "the STL is a solid rod"
    assert volume >= math.pi * (max(outer - DEFLECTION, 0) ** 2
                                - (inner + DEFLECTION) ** 2) * height
    return volume

# 1. One hollow part, the one way this makes one.
request = root / "request.json"
tube = root / "tube.fcad"
request.write_text(json.dumps({"schema_version": 1, "center_mm": [12.0, -7.0],
                               "outer_radius_mm": 10.0, "inner_radius_mm": 4.0,
                               "height_mm": 15.0}))
made = run(["create-annular-extrude", str(request), "-o", str(tube), "--json"])
assert made["ok"] and made["operation"] == "create-annular-extrude"
result = made["result"]
outer_id, inner_id = result["outer_curve_id"], result["inner_curve_id"]
assert outer_id != inner_id
for field in ("document_id", "sketch_id", "extrude_id", "body_id"):
    assert result[field], field

# 2. Read back: two circles, four references, and both editors refusing.
seen = run(["inspect", str(tube), "--json"])["result"]
assert seen["document_id"] == result["document_id"]
sketch, = seen["sketches"]
assert sketch["sketch_id"] == result["sketch_id"]
assert sketch["editable"] is False and sketch["vertices"] is None
assert "Line" in sketch["refusal"]
assert sketch["circle_edit"]["available"] is False
assert sketch["circle_edit"]["circle"] is None
assert "exactly one unconstrained Circle" in sketch["circle_edit"]["refusal"]
assert seen["edit_extrude"]["available"] is True
feature, = seen["features"]
assert feature["feature_id"] == result["extrude_id"] and feature["distance_mm"] == 15.0
body, = seen["bodies"]
assert body["body_id"] == result["body_id"]

# 3. Stored consistency, then a cold rebuild that resolves every name.
checked = run(["validate", str(tube), "--json"])
assert checked["ok"] and checked["result"]["valid"], checked
rebuilt = run(["rebuild", str(tube), "--cold"])
assert "2 segments, 1 hole(s)" in rebuilt, rebuilt
assert "solid, 4 named faces" in rebuilt, rebuilt
assert "4 of 4 stored references resolved" in rebuilt, rebuilt
named = run(["print-topology", str(tube)])
for curve in (outer_id, inner_id):
    assert f"extrude side from segment {curve}" in named, curve
assert named.count("resolved") >= 4

# 4. The mesh really is hollow, and the FBX carries the same hole.
volume = measure(tube, (12.0, -7.0), 10.0, 4.0, 15.0)
fbx = root / "tube.fbx"
written = run(["export-fbx", str(tube), "-o", str(fbx), "--json"])
assert written["result"]["complete"] and written["result"]["geometries"] == 1
assert fbx.stat().st_size == written["result"]["bytes"]
read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
assert read.returncode == 0, read.stderr
executed = [l for l in read.stdout.splitlines()
            if l.startswith("FCAD_PRODUCTION_FBX_UFBX_EXECUTED")]
assert executed and executed[0].endswith("failures=0"), read.stdout
summary = [l for l in read.stdout.splitlines() if l.startswith("FCAD_IDENTITY_SUMMARY")]
assert summary and "meshes=1" in summary[0], read.stdout

# 5. The existing height edit still works, and both circles survive it whole.
taller = root / "taller.fcad"
before = tube.read_bytes()
run(["edit-extrude", str(tube), "--feature", result["extrude_id"],
     "--distance-mm", "25", "-o", str(taller), "--json"])
assert tube.read_bytes() == before, "the source was touched"
again = run(["inspect", str(taller), "--json"])["result"]
assert again["features"][0]["distance_mm"] == 25.0
copied = run(["print-topology", str(taller)])
for curve in (outer_id, inner_id):
    assert f"extrude side from segment {curve}" in copied, curve
assert "4 of 4 references resolved" in copied, copied

# 6. Measure the taller part the same way: the hole is still a hole.
taller_volume = measure(taller, (12.0, -7.0), 10.0, 4.0, 25.0)
assert taller_volume > volume * 1.6, (volume, taller_volume)

# 7. Refusals publish nothing and never replace anything.
names = sorted(p.name for p in root.iterdir())
for bad in ({"schema_version": 1, "center_mm": [0, 0], "outer_radius_mm": 10,
             "inner_radius_mm": 10, "height_mm": 5},
            {"schema_version": 1, "center_mm": [0, 0], "outer_radius_mm": 10,
             "inner_radius_mm": 12, "height_mm": 5},
            {"schema_version": 1, "center_mm": [0, 0], "outer_radius_mm": 10,
             "inner_radius_mm": 9.9999, "height_mm": 5},
            {"schema_version": 1, "center_mm": [0, 0], "radius_mm": 10,
             "height_mm": 5}):
    request.write_text(json.dumps(bad))
    refused = run(["create-annular-extrude", str(request), "-o",
                   str(root / "never.fcad"), "--json"], code=2)
    assert refused["ok"] is False and refused["error"]["kind"] in ("input", "unsupported")
assert sorted(p.name for p in root.iterdir()) == names, "a refusal left something"
print("FCAD_25L_RECIPE_OK", round(volume, 6), round(taller_volume, 6))
```
