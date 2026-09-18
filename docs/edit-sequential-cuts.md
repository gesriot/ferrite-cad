# §26D — правка любого из двух последовательных circular Cut

§26E расширяет текущую границу до 16 Cut: [контракт и рецепт](circular-cut-history.md).
Ниже сохранён контракт исходного среза; ограничения длины заменены §26E.


## Политика topology refs (зафиксирована до кода)

Выбор по UUID идёт через одну проверку шести- или восьмиобъектной истории:
Body.tip → second.previous → first.previous → NewBody. Основание, выбранная
фича, непосредственный predecessor и tip имеют отдельные значения. Имена,
ordinal и порядок SQL rows не выбирают соответствие.

Каждая сохранённая ссылка сохраняет UUID, owner, producer, role и selection.
Исторический producer продолжает адресовать свой исторический результат;
OriginCap/OriginSide на final Body сохраняют origin_feature. Измерения
проверяют уже разрешённые handles, никогда не выбирают identity.

| Выбранное звено | Переход | Изменение refs |
|---|---|---|
| Любое | pocket → pocket, through → through | Никаких новых или удалённых refs |
| Второе (или единственное) | through → pocket | Один собственный ExtrudeCap(End) |
| Первое из двух | through → pocket | Собственный ExtrudeCap(End) на первом и OriginCap(first, End) на втором |
| Любое | pocket → through | Отказ подготовки с UUID всех защищаемых floor refs |

Два имени появившегося первого дна относятся к разным producer namespaces:
историческому первому результату и конечному второму. В пределах одного
producer поверхность не получает два имени. Дно нельзя переназначить дальней
крышке, удалить или оставить неназванным в final Body. Повторная допустимая
правка не создаёт дополнительных refs. Отказ потере дна исходит из подготовки;
strict refs после rebuild остаются независимой проверкой.

## Контракт

Прежние `edit-circular-cut`, request v1 с `tool_curve_id`, JSON v1 и exits
0/2/7 сохраняются. `previous_feature_id` означает непосредственного
предшественника выбранного Cut. UI использует тот же каталог и copy worker.

Поддержаны ровно прежние шесть объектов или восемь объектов с двумя Cut:
одна unconstrained axis-aligned XY plate, unconstrained Circle на том же datum,
Blind +Z. Зазор дисков и зазор до стенок строго больше 1e-7 mm. Третье звено,
constrained profiles/tools, произвольная история, attachment, Add/Intersect,
ThroughAll, live preview и in-place Save не входят.

SQL allowlist: только payload/payload_hash выбранных Sketch и Cut,
meta.modified_at и перечисленные новые floor refs с необходимыми capabilities.
Все UUID, deps, Body tip, previous chain, исходные bytes, прочие payload,
headers, metadata, optional capability rows/rowid и чужие таблицы сохраняются.
Writer повторно выводит подготовку из актуального документа и чисел payload;
занятые UUID и дубликаты внутри нового набора проверяет в транзакции.

Общий snapshot/version guard, alias/no-clobber, cancellation, baseline/edited
cold rebuild, strict refs, SQLite close, cleanup и atomic publication
сохраняются. Archive v3 и predecessor-qualified cache keys не меняются.

## Discovery и интерфейс

В `features[].circular_cut_edit.saved` аддитивно появляются:

- `base_feature_id`: исходный NewBody;
- `tip_feature_id`: конечный Cut, который экспонирует Body;
- `neighboring_tool`: null у одного Cut, иначе `feature_id`, `tool_sketch_id`,
  `tool_curve_id`, `center_mm`, `radius_mm`, `depth_mm` соседнего инструмента;
- `disk_clearance_mm`: 1e-7;
- `protected_floor_reference_ids`: все защищаемые UUID, пустой список для through.

Прежние `previous_feature_id` и `floor_reference_id` означают непосредственного
предшественника и собственную историческую ссылку на дно выбранной фичи.
`through_allowed` true только при пустом списке защищаемых refs. Каталог читает
тот же закреплённый snapshot, ядро ему не требуется. Stub CLI открывает kernel
до job: его ранний отказ не доказывает поздний version guard.

Оба `Edit cut <имя> — <UUID>…` доступны независимо. Формы используют общую
валидацию соседнего диска; первоначальные числа берутся через round-trip
`f64::to_string`. Первый Undo восстанавливает их точно, один Apply — один шаг.
Save Cancel, failure/stale completion и отказ async Open сохраняют draft.
Правка первого пересчитывает второй, Body tip и history не переставляются.

## Исполняемый рецепт агента

Установить `FERRITECAD` на свежий CLI, `FCAD_UFBX_READER` на pinned
`read_production`. Извлечь блок по маркеру и выполнить Python 3. Рецепт создаёт
временные модели, проверяет source bytes/SQL, обе полости независимым binary
STL parser и читает восемь небольших FBX штатным pinned reader. Результаты
headless и GUI учитываются раздельно в [verification](edit-sequential-cuts-verification.md).

```python
# FCAD_26D_AGENT_RECIPE
import json, math, os, pathlib, struct, subprocess, tempfile, sqlite3
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-edit-sequential-"))
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

def edit(source, destination, selected, tool, code=0):
    cat=run(["inspect",source,"--json"])["result"]
    saved=next(f["circular_cut_edit"]["saved"] for f in cat["features"] if f["feature_id"]==selected)
    request=root/"edit.json"
    c,r,d=tool
    request.write_text(json.dumps({"request_version":1,"tool_curve_id":saved["tool_curve_id"],"center_mm":c,"radius_mm":r,"depth_mm":d}))
    result=run(["edit-circular-cut",source,"--feature",selected,"--expect-version",cat["content_version"],"--request",request,"-o",destination,"--json"],code)
    if code==0:
        assert result["result"]["previous_feature_id"]==saved["previous_feature_id"]
        assert result["result"]["feature_id"]==selected
    return result

def sql(path):
    with sqlite3.connect(path) as c:
        data={}
        for name, in c.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name"):
            quoted='"'+name.replace('"','""')+'"'
            try: cur=c.execute(f"SELECT rowid,* FROM {quoted} ORDER BY rowid")
            except sqlite3.OperationalError: cur=c.execute(f"SELECT * FROM {quoted} ORDER BY 1,2")
            data[name]=([x[0] for x in cur.description],cur.fetchall())
        return data

def preserved(source, copy, saved, added):
    a,b=sql(source),sql(copy)
    assert a.keys()==b.keys()
    import uuid
    ids=[uuid.UUID(saved[k]).bytes for k in ["feature_id","tool_sketch_id"]]
    for name,(cols,rows) in a.items():
        assert cols==b[name][0]
        now=b[name][1]
        if name=="topology_refs":
            assert len(now)==len(rows)+added and all(row in now for row in rows)
            continue
        assert len(rows)==len(now)
        for old,new in zip(rows,now):
            for i,(x,y) in enumerate(zip(old,new)):
                assert x==y or (name=="meta" and cols[i]=="modified_at") or (name=="objects" and old[cols.index("id")] in ids and cols[i] in ["payload","payload_hash"]),(name,cols[i])

create=root/"plate.json"
create.write_text(json.dumps({"request_version":1,"points_mm":[[0,0],[WIDTH,0],[WIDTH,DEPTH],[0,DEPTH]],"height_mm":HEIGHT}))
plate=root/"plate.fcad"
run(["create-sketch-extrude",create,"-o",plate,"--json"])
volumes=[]
for d1 in [12.,4.]:
    one=root/f"one-{d1:g}.fcad"
    t1=([20.,20.],5.,d1)
    cut(plate,one,t1)
    for d2 in [12.,7.]:
        t2=([55.,30.],7.,d2)
        two=root/f"two-{d1:g}-{d2:g}.fcad"
        cut(one,two,t2)
        with sqlite3.connect(two) as c:
            c.executescript("UPDATE objects SET rowid=-rowid,ordinal=100-ordinal,name='same'; UPDATE capabilities SET rowid=rowid+100; INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0); CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB); INSERT INTO extra_data VALUES(91,x'010203');")
        original=two.read_bytes()
        cat=run(["inspect",two,"--json"])["result"]
        choices=[f["circular_cut_edit"]["saved"] for f in cat["features"] if f["circular_cut_edit"]["available"]]
        assert len(choices)==2
        first=next(c for c in choices if c["feature_id"]!=c["tip_feature_id"])
        second=next(c for c in choices if c["feature_id"]==c["tip_feature_id"])
        assert second["previous_feature_id"]==first["feature_id"]
        for index,saved in enumerate([first,second]):
            tools=[t1,t2]
            c,r,d=tools[index]
            tools[index]=([c[0]+2.125,c[1]-1.25],r+0.625,3.25 if index==0 else 5.5)
            copy=root/f"edit-{d1:g}-{d2:g}-{index}.fcad"
            edit(two,copy,saved["feature_id"],tools[index])
            preserved(two,copy,saved,(2 if index==0 else 1) if d==HEIGHT else 0)
            assert run(["validate",copy,"--json"])["result"]["valid"]
            run(["rebuild",copy,"--cold"])
            volumes.append(measure(copy,tools))
            fbx=copy.with_suffix(".fbx")
            run(["export-fbx",copy,"-o",fbx,"--json"])
            p=subprocess.run([reader,"--identity",str(fbx)],capture_output=True,text=True)
            assert p.returncode==0 and "checks=6 failures=0" in p.stdout,(p.stdout,p.stderr)
            again=root/f"again-{d1:g}-{d2:g}-{index}.fcad"
            edit(copy,again,saved["feature_id"],tools[index])
            preserved(copy,again,saved,0)
            current=run(["inspect",copy,"--json"])["result"]
            named=next(f["circular_cut_edit"]["saved"] for f in current["features"] if f["feature_id"]==saved["feature_id"])
            never=root/"never.fcad"
            error=edit(copy,never,saved["feature_id"],(tools[index][0],tools[index][1],HEIGHT),2)
            assert all(i in error["error"]["message"] for i in named["protected_floor_reference_ids"])
            assert not never.exists()
            error=edit(two,never,saved["feature_id"],(tools[1-index][0],tools[index][1],3.),2)
            assert not error["ok"] and not never.exists()
            assert two.read_bytes()==original
print("FCAD_26D_RECIPE_OK",*[round(v,6) for v in volumes],root)
```
