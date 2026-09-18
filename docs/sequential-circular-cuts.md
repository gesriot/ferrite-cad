# §26C — второй circular Cut в том же Body

§26E расширяет текущую границу до 16 Cut: [контракт и рецепт](circular-cut-history.md).
Ниже сохранён контракт исходного среза; ограничения длины заменены §26E.


§26D расширяет прежнюю границу: [правка любого из двух Cut](edit-sequential-cuts.md).
Описанные ниже отказы двухзвенной правке относятся к исходному срезу.

[ADR 4](decisions/0004-feature-predecessor.md),
[протокол проверки](sequential-circular-cuts-verification.md).

Команда `cut-circular-copy` и форма Add cut принимают как исходную плиту §26A,
так и сохранённый six-object документ §26A/B с одним Cut. Для второго выреза
используется прежний shared job: snapshot, expected version, baseline/edited
cold rebuild, strict refs, повторный version guard, закрытие SQLite и atomic
no-clobber publication. Новый Sketch и Cut добавляются в копию; прежний Body
сохраняет UUID, новый `Cut.previous` равен первому Cut, `Body.tip_feature` — второму.

Поддерживаются through/through, through/pocket, pocket/through, pocket/pocket.
Основание — один неограниченный осепараллельный XY-прямоугольник, без placement;
оба инструмента — неограниченные Circle на той же плоскости, Blind вдоль +Z,
`0 < depth <= height`. Помимо прежнего зазора до внешних стенок требуется
`hypot(c2-c1) - r1 - r2 > Tolerance::DEFAULT_LINEAR = 1e-7 mm`.
Касание, пересечение, вложенность, выход за стенку/высоту отказываются без
коррекции чисел. Третий Cut и редактирование любого Cut восьмиобъектной копии
отказываются. Six-object frame редактора §26B не расширен.

## Имена и совместимость

Переносимая грань хранится под парой `(origin_feature UUID, Cap(side)/Side(segment UUID))`.
Каждый boolean переносит как собственные, так и уже перенесённые имена
предшественника через настоящую OCCT history. Удалённые имена сохраняют
происхождение. Измерения, индексы, ordinal и display names не выбирают identity.

Новые ссылки второго результата используют `OriginCap`/`OriginSide` на исходную
экструзию или первый Cut. Собственные стенка и дно второго Cut остаются
`ExtrudeSide`/`ExtrudeCap`. Старые ссылки побайтово сохраняют прежний producer
и адресуют его исторический выход. Legacy `CarriedCap`/`CarriedSide` сохраняют
смысл собственных граней непосредственного предшественника, адресуя ту же
таблицу происхождения без второго экземпляра грани.

Новая capability `topology.origin-face.v1` обязательна только у новых ролей.
Старый reader сохраняет неизвестные references и открывает документ read-only.
Payload фичи остаётся v2; SQLite schema не меняется. Архив имён v3 хранит
непосредственного предшественника и квалифицированные имена предков.
Archive key включает UUID непосредственного предшественника и ключ его
результата: одинаковая геометрия другого producer не даёт старые имена.
Кэш v1/v2 явно отказывается и перестраивается; теги 1–11 не переопределены,
12–14 обозначают origin start/end/side. Одна физическая грань имеет один слот.

## SQL allowlist

В одной транзакции разрешены ровно:

- две новые строки `objects` для Sketch/Cut с новыми UUID и одна новая Circle
  внутри нового Sketch payload;
- `payload`/`payload_hash` прежней строки Body (новый tip);
- удаление прежнего `BodyTip` ребра, добавление нового и трёх новых ребер
  `Plane`, `Profile`, `Predecessor` — итого 9 dependencies;
- новые `topology_refs` второго Cut: собственная стенка, собственное дно при
  pocket, шесть origin-граней плиты, стенка первого Cut и его дно при pocket;
- `meta.modified_at`;
- добавление только необходимых capability names; существующая optional строка
  меняет `required` только если её capability стала обязательной. Прочие строки
  и их rowid не пересоздаются. У обычного источника §26A/B добавляется только
  `topology.origin-face.v1`.

Все прежние payload, references, UUID, constraints, object metadata, capability
rows/rowid и посторонние SQL-данные сохраняются. Writer заново выводит подготовку
из текущего документа и чисел payload. Занятый новый reference UUID и повторный
UUID внутри добавляемого набора отказываются внутри транзакции до upsert.

## Discovery и интерфейсы

Request v1 и JSON v1 envelope прежние. У `bodies[].cut_edit.target` аддитивно
появились `base_feature_id`, `disk_clearance_mm` и `existing_cut` (null до первого
Cut, иначе `feature_id`, `tool_sketch_id`, `tool_curve_id`, `center_mm`,
`radius_mm`, `depth_mm`). `tip_feature_id` всегда текущий tip;
`profile_sketch_id` и extents/height описывают исходную плиту. Неизвестные поля
ответа клиенты игнорируют по прежнему контракту.

Каталог и проверка принадлежат document; UI/CLI не дублируют policy.
Add cut показывает существующий инструмент и требуемый зазор, использует
прежние Apply/Undo/Redo/Save Cancel, worker и async Open. Edit cut остаётся
отдельным действием, недоступным для обоих Cut восьмиобъектного документа.

## Исполняемый рецепт агента

Установить `FERRITECAD` на свежий CLI и `FCAD_UFBX_READER` на pinned
`read_production`. Блок ниже извлекается по маркеру и исполняется Python 3;
он вызывает настоящий CLI и оставляет временные модели для GUI smoke.

```python
# FCAD_26C_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile, sqlite3
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-sequential-"))
WIDTH, DEPTH, HEIGHT = 80.0, 50.0, 12.0
DEFLECTION, ANGULAR = 0.05, 0.1

def run(args, code=0):
    p = subprocess.run([cli, *map(str,args)], capture_output=True, text=True)
    assert p.returncode == code, (args,p.returncode,p.stdout,p.stderr)
    return json.loads(p.stdout) if "--json" in args else p.stdout

def measure(path, tools):
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

    for CENTER, RADIUS, depth in tools:
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
    exact = WIDTH * DEPTH * HEIGHT - sum(math.pi*r*r*d for _,r,d in tools)
    inscribed = WIDTH * DEPTH * HEIGHT - sum(math.pi*(r-DEFLECTION)**2*d for _,r,d in tools)
    assert exact - 1e-6 <= volume <= inscribed + 1e-6, volume
    return volume


def cut(source, destination, tool, code=0):
    catalog=run(["inspect",source,"--json"])["result"]
    body=catalog["bodies"][0]
    request=root/"cut.json"
    center,radius,depth=tool
    request.write_text(json.dumps({"request_version":1,"center_mm":center,"radius_mm":radius,"depth_mm":depth}))
    return run(["cut-circular-copy",source,"--body",body["body_id"],"--expect-version",catalog["content_version"],"--request",request,"-o",destination,"--json"],code)

create=root/"plate.json"
create.write_text(json.dumps({"request_version":1,"points_mm":[[0,0],[WIDTH,0],[WIDTH,DEPTH],[0,DEPTH]],"height_mm":HEIGHT}))
plate=root/"plate.fcad"
run(["create-sketch-extrude",create,"-o",plate,"--json"])
volumes=[]
for d1 in [12.0,4.0]:
    one=root/f"first-{d1:g}.fcad"
    t1=([20.0,20.0],5.0,d1)
    cut(plate,one,t1)
    original=one.read_bytes()
    for d2 in [12.0,7.0]:
        t2=([55.0,30.0],7.0,d2)
        two=root/f"two-{d1:g}-{d2:g}.fcad"
        result=cut(one,two,t2)
        cat=run(["inspect",one,"--json"])["result"]
        assert result["result"]["previous_feature_id"]==cat["bodies"][0]["cut_edit"]["target"]["tip_feature_id"]
        after=run(["inspect",two,"--json"])["result"]
        assert after["bodies"][0]["cut_edit"]["available"]
        assert sum(f["circular_cut_edit"]["available"] for f in after["features"]) == 2
        assert run(["validate",two,"--json"])["result"]["valid"]
        run(["rebuild",two,"--cold"])
        volumes.append(measure(two,[t1,t2]))
        fbx=two.with_suffix(".fbx")
        assert run(["export-fbx",two,"-o",fbx,"--json"])["result"]["geometries"]==1
        p=subprocess.run([reader,"--identity",str(fbx)],capture_output=True,text=True)
        assert p.returncode==0 and "failures=0" in p.stdout, (p.stdout,p.stderr)
        with sqlite3.connect(one) as a, sqlite3.connect(two) as b:
            old=a.execute("SELECT * FROM topology_refs").fetchall()
            new=b.execute("SELECT * FROM topology_refs").fetchall()
            assert all(row in new for row in old)
            assert b.execute("SELECT count(*) FROM objects").fetchone()[0]==8
            assert b.execute("SELECT count(*) FROM deps").fetchone()[0]==9
        assert one.read_bytes()==original
    for bad in [([32.0,20.0],7.0,7.0),([20.0,20.0],2.0,7.0),([55.0,30.0],7.0,13.0)]:
        never=root/"never.fcad"
        assert not cut(one,never,bad,2)["ok"] and not never.exists()
        assert one.read_bytes()==original
print("FCAD_26C_RECIPE_OK",*[round(v,6) for v in volumes],root)
```
