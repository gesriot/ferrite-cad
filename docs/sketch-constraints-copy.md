# §25E — H/V constraints в новой копии

Контракт реализации: существующие собственные XY Line polygons, 3..256 сегментов,
один positive literal Blind/NewBody и его Body. Stored координаты остаются
исходным состоянием solver; они не являются solved presentation. Coordinate edit,
drag и Snap constrained Sketch по-прежнему недоступны. Отдельная операция ниже
добавляет/удаляет только Horizontal/Vertical. Нет in-place Save.

## Request и атомарность

`edit-sketch-constraints-copy SOURCE --sketch UUID --expect-version TOKEN --request REQUEST -o COPY [--json]`

```json
{"request_version":1,"remove":[],"add":[{"curve_id":"UUID_FROM_DISCOVERY","rule":"horizontal"}]}
```

Оба массива обязательны. UUID — canonical UUIDv7; rule — `horizontal` или
`vertical`. Request ограничен 65536 bytes и 512 изменениями, должен быть непустым;
неизвестные поля/версии/rules отказываются. Сначала все удаления в порядке remove,
затем добавления в порядке add; request атомарен. Remove именует существующий H/V
constraint UUID выбранного Sketch. Повторное удаление, foreign UUID, удаление
Coincident, повторный H/V или H+V одного сегмента отказываются. Можно удалить H и
добавить V в одном request. Один add всегда ссылается на Start/End указанного Line.

При добавлении H/V отсутствующие связи замыкания создаются явно: Coincident
между End каждого Line и Start следующего, включая последний/первый. Подходящие
существующие связи (в любой ориентации) сохраняют UUID; дубликаты запрещены.
Новые Coincident добавляются в порядке кривых, затем запрошенные H/V. Они видны
в discovery и result. После удаления последнего H/V Coincident **остаются**;
скрытых Fixed/Distance нет. Этот API не удаляет closure и не возвращает такой
Sketch в unconstrained v1. Снятие H/V освобождает условие, не обещает отката формы.

Поддерживаются только H/V одного Line и Coincident его соседних стыков.
Существующие H/V требуют полного набора closure; другие families/связи, imports,
construction, arcs, неизвестные payload fields и более сложные документы получают
refusal и сохраняются. Stored ordered polygon обязан оставаться допустимым.
Сохранение проверяет baseline и изменённую модель cold rebuild, solver и topology refs.
Under-constrained с ненулевым DOF допускается; это не fully constrained.

## JSON v1

`inspect --json` сохраняет все прежние поля. В каждой строке `result.sketches`
добавляется обязательный `constraint_edit`:
`{available, refusal, document_refusal, curves, constraints}`. Boolean available
учитывает общий copy refusal; локальная причина и документная различаются.
curves/constraints — массивы для поддержанного Sketch, null при неподдержанном;
constraints без правил — `[]`. Curves в stored порядке:
`{curve_id,start_mm:[x,y],end_mm:[x,y]}`. Constraint в stored порядке:
`{constraint_id,rule:{kind,a:{curve_id,at},b:{curve_id,at}}}`;
kind — `horizontal`/`vertical`/`coincident`, at — `start`/`end`.
Это persisted IDs и исходные координаты, не экранные индексы или solver ordinals.

Публикация: обычный envelope, schema_version 1,
operation `edit-sketch-constraints-copy`, ok true, exit 0. Required result:
`destination`, `document_id`, `sketch_id`, `added_constraints` (те же DTO),
`removed_constraint_ids` (ordered UUID array), `solve` с числовым
`degrees_of_freedom` и ordered UUID array `redundant_constraint_ids` из changed rebuild.
Пустые массивы — []; ничего не извлекается повторным открытием output.

Отказ: ok false, kind/message/causes и exit 2. Только для typed solver conflict
этой операции error дополнительно содержит `constraint_conflict`:
`{sketch_id,constraints:[constraint DTO]}`, с persistent document identities.
Поле отсутствует при других ошибках. Unknown response fields нужно игнорировать;
unknown rules нельзя трактовать как разрешённую правку. Ошибки старых команд
не получают новых полей. Usage/help остаются clap-текстом.

Один UTF-8 JSON object + LF через общий emitter; source/request/output должны
быть UTF-8 до файловой работы JSON-адаптера. Exit 7 означает потерю отчёта, а не
откат; публикация сохраняется. Не повторять автоматически после 7.

## Запись и владение

Один jobs snapshot/copy/baseline/transaction/changed rebuild/SQLite close/Keep путь
с прежними version, alias, reference, cancellation и generation guards.
Разрешены только payload/hash/schema_version выбранного Sketch и необходимый
переход capability `sketch.constraints.v1` в required. v1 без constraints становится
v2 с корректным envelope/header. Optional capability row при повышении сохраняет
rowid; посторонние rows/metadata, source bytes/claims, все прежние object/curve/
Body/Extrude/reference/constraint IDs остаются. Closure retention сохраняет v2;
публичного v2→v1 перехода здесь нет. Solved координаты не записываются.

UI задаёт те же remove/add в отдельном draft, без solver на redraw. Save Cancel,
отказ, отмена и stale ответ сохраняют draft и принятую сцену. После publish обычный
async Open показывает solved модель и существующие solver facts.

### Точные типы и конфликт

UUID и версии следуют общему [JSON v1](cli-json-v1.md). destination — UTF-8 строка
того пути, который был опубликован (относительный путь не канонизируется).
Все counts/DOF — неотрицательные JSON integers, координаты — конечные numbers/mm.
Имена/refusals — точные строки или null; diagnostic prose может меняться.
Новые UUID выдаются при подготовке request, один раз; failure не обещает, что
эти proposed IDs существуют в опубликованном документе.

`constraint_conflict.sketch_id` идентифицирует выбранный Sketch; constraints —
упорядоченные typed facts существующего `SketchConflict`, не solver ordinals.
Текущий разрешённый класс выдаёт только три rule kinds выше. Defensive projection
также понимает `fixed:{point,x,y}`, `distance:{a,b,distance}` (mm),
`equal_length`, `perpendicular`, `parallel` с a/b сегментами
`{from:point,to:point}`; point имеет тот же `{curve_id,at}`. Неизвестное будущее
правило отдаётся как `{kind:"unknown"}`: клиент останавливает автоматическую
правку. Это не расширяет допустимый request. Невозможность solve/сходимости,
недопустимый solved polygon и потеря topology ref всегда отказывают до publish.

## Запускаемый рецепт агента

Нужны CLI с OCCT + PlaneGCS и независимый pinned ufbx reader из
`tools/unity-fbx-smoke/scripts/read_production.c` (pin читает
`tools/unity-fbx-smoke/scripts/fetch_ufbx.sh`). Укажите готовые executable в
`FERRITECAD` и `FCAD_UFBX_READER`. На macOS задайте native library paths в том же
Bash. Рецепт создаёт только свой временный каталог и печатает его путь; исходные
fixtures не используются. Он не требует UI или заранее известных UUID.

```python
# FCAD_25E_AGENT_RECIPE
import collections, json, math, os, pathlib, re, struct, subprocess, tempfile
cli = os.environ["FERRITECAD"]
reader = os.environ["FCAD_UFBX_READER"]
root = pathlib.Path(tempfile.mkdtemp(prefix="ferrite-constraints-agent-"))
def run(operation, *args):
    p = subprocess.run([cli, operation, *map(str, args), "--json"], capture_output=True)
    if p.returncode == 7:
        raise RuntimeError("Report lost; inspect destination manually. Do not retry publication.")
    data = json.loads(p.stdout)
    assert data["schema_version"] == 1 and data["operation"] == operation
    if p.returncode != 0 or not data["ok"]:
        raise RuntimeError((p.returncode, data))  # refusal/STEP 4/5 need explicit decisions
    r = data["result"]
    if operation == "validate" and (not r["valid"] or r["diagnostics"]):
        raise RuntimeError(("Validation requires a decision", r))
    if operation == "export-fbx" and (not r["complete"] or r["omissions"]):
        raise RuntimeError(("Partial FBX cannot stand in for a complete model", r))
    return r
request = root / "request.json"
source = root / "slanted.fcad"
request.write_text(json.dumps({"request_version": 1,
    "points_mm": [[-20,-10],[40,-8],[42,30],[-20,30]], "height_mm": 10}))
run("create-sketch-extrude", request, "-o", source)
source_bytes = source.read_bytes()
catalog = run("inspect", source)
choices = [s for s in catalog["sketches"] if s["constraint_edit"]["available"]]
assert len(choices) == 1, "Choose a Sketch explicitly if the catalog is ambiguous"
s = choices[0]
segments = [c for c in s["constraint_edit"]["curves"]
            if c["start_mm"] == [-20,-10] and c["end_mm"] == [40,-8]]
assert len(segments) == 1
curve = segments[0]["curve_id"]
request.write_text(json.dumps({"request_version":1,"remove":[],
    "add":[{"curve_id":curve,"rule":"horizontal"}]}))
saved = root / "horizontal.fcad"
result = run("edit-sketch-constraints-copy", source, "--sketch", s["sketch_id"],
    "--expect-version", catalog["content_version"], "--request", request, "-o", saved)
assert result["document_id"] == catalog["document_id"]
assert result["destination"] == str(saved)
assert result["solve"]["degrees_of_freedom"] > 0  # allowed, not fully constrained
run("validate", saved)
after = run("inspect", saved)
row = next(x for x in after["sketches"] if x["sketch_id"] == s["sketch_id"])
assert row["constraint_edit"]["curves"] == s["constraint_edit"]["curves"]  # stored inputs
assert row["constraint_edit"]["constraints"] == result["added_constraints"]
h = [c for c in result["added_constraints"] if c["rule"]["kind"] == "horizontal"]
assert len(h) == 1 and h[0]["rule"]["a"]["curve_id"] == curve
assert len(after["bodies"]) == 1
stl = root / "horizontal.stl"
mesh = run("export-stl", saved, "--solid", after["bodies"][0]["body_id"], "-o", stl)
b = stl.read_bytes()
n, = struct.unpack_from("<I", b, 80)
assert len(b) == 84 + 50*n == mesh["bytes"] and n == mesh["triangles"]
vertices, volume6 = [], 0.0
bottom_edges = collections.Counter()
for i in range(n):
    t = [struct.unpack_from("<3f", b, 84+50*i+12+12*j) for j in range(3)]
    vertices += t
    if all(abs(p[2]) < 1e-6 for p in t):
        for j in range(3):
            bottom_edges[tuple(sorted((t[j], t[(j+1)%3])))] += 1
    a, c, d = t
    volume6 += (a[0]*(c[1]*d[2]-c[2]*d[1]) + a[1]*(c[2]*d[0]-c[0]*d[2])
                + a[2]*(c[0]*d[1]-c[1]*d[0]))
lo = [min(p[i] for p in vertices) for i in range(3)]
hi = [max(p[i] for p in vertices) for i in range(3)]
assert all(math.isfinite(v) for p in vertices for v in p)
assert lo[2] == 0 and hi[2] == 10 and abs(volume6)/6 > 1
boundary = [edge for edge, count in bottom_edges.items() if count == 1]
# Select the long negative-Y boundary of this known input, not a tessellation diagonal.
horizontal = [edge for edge in boundary
              if max(p[1] for p in edge) < 0 and abs(edge[0][0]-edge[1][0]) > 30]
assert len(horizontal) == 1 and abs(horizontal[0][0][1]-horizontal[0][1][1]) < 1e-5
assert horizontal[0] != ((-20.,-10.,0.),(40.,-8.,0.)), "Stored slant must change"
fbx = root / "horizontal.fbx"
run("export-fbx", saved, "-o", fbx)
read = subprocess.run([reader, "--identity", str(fbx)], capture_output=True, text=True)
assert read.returncode == 0 and re.search(r"checks=[6-9]\d* failures=0", read.stdout)
# Remove the exact discovered H, retaining all closure identities. Use the new version.
request.write_text(json.dumps({"request_version":1,"remove":[h[0]["constraint_id"]],"add":[]}))
removed = root / "removed.fcad"
r = run("edit-sketch-constraints-copy", saved, "--sketch", s["sketch_id"],
    "--expect-version", after["content_version"], "--request", request, "-o", removed)
assert r["added_constraints"] == [] and r["removed_constraint_ids"] == [h[0]["constraint_id"]]
run("validate", removed)
remaining = run("inspect", removed)["sketches"][0]["constraint_edit"]["constraints"]
assert remaining == [c for c in result["added_constraints"] if c["rule"]["kind"] == "coincident"]
assert source.read_bytes() == source_bytes
print(json.dumps({"directory":str(root),"degrees_of_freedom":result["solve"]["degrees_of_freedom"],
    "stl_bounds_mm":[lo,hi],"volume_mm3":abs(volume6)/6,"ufbx":read.stdout.strip()}))
```

Последующие inspect/validate/export читают отдельные снимки. Expected version
защищает copy edit; экспорт не имеет version guard. Наличие stored H/V и успешная
validation сами по себе не доказывают geometry: рецепт экспортирует после холодного
rebuild и читает реальные STL/FBX. Удаление H освобождает условие; retained closure
и будущие solves не обещают восстановить координаты какой-либо предыдущей сцены.
