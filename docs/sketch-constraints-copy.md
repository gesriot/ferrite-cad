# §25E–H — ограничения Line в новой копии

Контракт реализации: существующие собственные XY Line polygons, 3..256 сегментов,
один positive literal Blind/NewBody и его Body. Stored координаты остаются
исходным состоянием solver; они не являются solved presentation. Coordinate edit,
drag и Snap constrained Sketch по-прежнему недоступны. Отдельная операция ниже
добавляет/удаляет Horizontal/Vertical и евклидову длину Line. Нет in-place Save.

## Request и атомарность

`edit-sketch-constraints-copy SOURCE --sketch UUID --expect-version TOKEN --request REQUEST -o COPY [--json]`

```json
{"request_version":1,"remove":[],"add":[{"curve_id":"UUID_FROM_DISCOVERY","rule":"horizontal"}]}
```

Оба массива обязательны. UUID — canonical UUIDv7. Старые additions остаются
`{curve_id,rule:"horizontal"}` / `{curve_id,rule:"vertical"}` без дополнительных полей.
§25F аддитивно добавляет в request v1
`{curve_id,rule:"distance",distance_mm:N}`: N — JSON number, после декодирования
в f64 конечный и строго положительный, в mm. Это евклидово расстояние между
Start и End одного указанного Line, а не X/Y-проекция. H/V не принимают
`distance_mm`, даже null; Distance требует это поле. Строка, bool, null, NaN,
infinity, overflow, zero/negative и любые неизвестные поля отказываются. Request ограничен 65536 bytes и 512 изменениями, должен быть непустым;
неизвестные поля/версии/rules отказываются. Сначала все удаления в порядке remove,
затем добавления в порядке add; request атомарен. Remove именует существующий H/V,
Line length, Fixed или equal length constraint UUID выбранного Sketch. Повторное удаление, foreign UUID, удаление
Coincident, повторный H/V, H+V или две длины одного сегмента отказываются. Можно удалить H и
добавить V в одном request. Один add всегда ссылается на Start/End указанного Line.
Один Line может иметь H либо V и одну длину одновременно. Даже две одинаковые
длины запрещены. Замена значения — атомарный remove старого exact UUID + add
новой длины: новый constraint UUID создаётся общей операцией один раз; остальные
IDs сохраняются. Это не обновление значения с сохранением UUID заменённой связи.

При добавлении H/V, длины, Fixed или равенства отсутствующие связи замыкания создаются явно: Coincident
между End каждого Line и Start следующего, включая последний/первый. Подходящие
существующие связи (в любой ориентации) сохраняют UUID; дубликаты запрещены.
Новые Coincident добавляются в порядке кривых, затем запрошенные additions. Они видны
в discovery и result. После удаления последних H/V/length/Fixed/равенств Coincident **остаются**;
скрытых Fixed или размеров нет. Этот API не удаляет closure и не возвращает такой
Sketch в unconstrained v1. Снятие H/V/length освобождает условие, не обещает отката формы.

§25G аддитивно добавляет в request v1 четвёртую форму
`{curve_id,rule:"fixed",at,x_mm,y_mm}`: `at` — строка `start` либо `end`,
`x_mm`/`y_mm` — JSON numbers, после декодирования в f64 конечные миллиметры.
Ноль и отрицательные координаты допустимы; NaN, infinity, overflow, строка,
bool, null, отсутствие поля, любой другой `at` и лишний `distance_mm`
отказываются. `at` не принимается H/V и Distance. **Не более одного Fixed на
профиль**: второй add, второй сохранённый Fixed и add на противоположный
endpoint того же adjacent Coincident joint — структурные отказы до публикации.
Сохранённый Fixed не переназначается автоматически и не связывается с другим
endpoint по близости координат. Замена — тот же атомарный remove exact UUID +
add: новый constraint UUID создаёт общая операция один раз. Удаление Fixed
сохраняет Coincident closure, H/V и длины.

§25H аддитивно добавляет в request v1 пятую форму
`{"rule":"equal_length","a_curve_id":UUID_A,"b_curve_id":UUID_B}` — точное
написание. Оба поля обязательны, это canonical UUIDv7 двух **разных** целых
Lines выбранного Sketch. `curve_id`, `distance_mm`, `at` и любые другие поля
этой формой не принимаются; `a_curve_id`/`b_curve_id` не принимаются прочими
rules. Ни одна сторона не ведущая, числа у связи нет. Для занятости слота
`(A,B)` и `(B,A)` — одна пара; сохранённый `SegmentRef`, записанный End→Start,
называет ту же Line и **не** переписывается канонизацией. Self-pair, чужая
Line, duplicate и reversed duplicate в одном запросе или против сохранённого
равенства — структурные отказы до solver. Замена — тот же атомарный remove
exact UUID + add. Удаление равенства сохраняет Coincident closure, H/V, длины
и Fixed. Избыточность и противоречие структурными категориями не являются:
их устанавливает настоящий solver через `redundant_constraint_ids` либо typed
`constraint_conflict`.

Поддерживаются H/V, положительный Distance между Start/End одного Line
(сохранённая обратная ориентация endpoints также допустима), один Fixed
endpoint Line с конечными X/Y mm, EqualLength между двумя целыми Lines и
Coincident его соседних стыков. Arbitrary
point-to-point Distance между разными Lines, второй Fixed,
formula/Parameter dimensions не становятся редактируемыми. Существующие H/V/length требуют полного набора closure; другие families/связи, imports,
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
kind — `horizontal`/`vertical`/`coincident`/`distance`, at — `start`/`end`.
kind дополнительно принимает `fixed` и `equal_length`. Distance имеет required `distance: number`
в mm. **Response поле остаётся `distance`**, request использует `distance_mm`;
версии operation/schema не меняются. Fixed сериализуется прежним DTO
`{kind:"fixed",point:{curve_id,at},x,y}` — `point`/`x`/`y`, не `at`/`x_mm`/`y_mm`
входного запроса и не `a`/`b`. EqualLength сериализуется прежним DTO
`{kind:"equal_length",a:{from,to},b:{from,to}}` с сегментами Start→End, а не
`a_curve_id`/`b_curve_id` входного запроса; `distance` у него нет. Endpoints/value/UUID берутся из принятого snapshot или опубликованного
результата, не из текста и не из повторного открытия output.
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

### §25E-1: история несохранённого H/V draft

Кнопки Undo/Redo сохраняют точные ordered `remove/add`, включая UUID удаляемых
persisted constraints. Каждый успешный Add H/V и фактическое переключение Remove
создают один checkpoint. Clear непустого запроса — один обратимый шаг; пустого —
no-op. Выбор сегмента, отказ Add и no-op не входят в историю и сохраняют Redo.
Новая фактическая правка после Undo очищает Redo. Каждый стек ограничен 128
checkpoint: при переполнении удаляется самый старый. Это память текущего draft,
не undo опубликованного документа; новые constraint UUID по-прежнему создаёт
общая операция, а не UI.

Undo/Redo доступны только с соответствующей историей и без выполняющегося job.
Восстановление очищает локальную ошибку предыдущего действия; текущая validation
вычисляется прежним валидатором для восстановленного запроса. Пустой запрос
остаётся открытым draft, но Save требует прежний валидный непустой request.
Кнопки истории и Cancel находятся над прокручиваемыми каталогами; длинный pending
list не скрывает Save/Clear. Глобальные keyboard shortcuts не добавлены.

Save Cancel, ошибка/отмена job и stale reply сохраняют оба стека и edits.
Явный Cancel draft и успешная публикация закрывают draft вместе с историей;
новый draft начинает с пустыми стеками. Save отправляет восстановленные edits
с исходными source/version/Sketch IDs через прежний worker. История не запускает
solver, не меняет CLI/JSON или запрет coordinate drag constrained Sketch.
[Проверки §25E-1](constraint-draft-history-verification.md).


### §25F: поле длины и pending request

`Edit constraints …` открывает `Sketch constraints — new copy`. Stored координаты
показаны отдельно от pending/сохранённых связей и solved результата. Поле
`Line length (mm)` принимает временно незавершённую строку; только успешное
`Add length` проверяет её через `LineLengthMm::new` и добавляет один checkpoint.
Внутренний checked тип хранит биты конечного положительного f64, поэтому точное Eq
не допускает NaN/zero и не использует приблизительное сравнение истории.
Ввод, выбор Line и отказ не очищают Redo. Undo/Redo восстанавливают только ordered
request: выбранная Line и ещё не применённая строка поля остаются как есть.
Save использует принятый request, а не неприменённый текст поля. Add не запускает
solver. Running выключает поле и действия; новый draft сбрасывает поле и историю.

Сохранённая длина отображается с mm и exact UUID; Remove работает для неё так же,
как для H/V. Для замены выберите Remove существующей длины и Add length с новым
числом до Save, либо одно действие `Replace length` (§25F-1). Если закрытый H/V-прямоугольник получает несовместимые длины
противоположных сторон, структурный request допустим, но настоящий solver
отказывает до publish. Typed conflict именует существующие/proposed UUID;
proposed ID при отказе не подтверждает наличие ограничения на диске.

### §25F-1: замена сохранённой длины

`Replace length` относится к выбранному Line и его сохранённому Distance,
включая сохранённую обратную ориентацию endpoints. Показывается stored mm и
exact UUID. Без выбора или без сохранённой длины действие недоступно; `Add length`
для новой связи остаётся. Поле и selection не входят в историю и не перезаписывают
друг друга. `LineLengthMm::new` и `ConstraintSketchChoice::validate_edits` те же;
NaN/zero/overflow/незавершённый текст и отказ validator не меняют request/Undo/Redo.
Solver на ввод, Apply, Undo и redraw не запускается.

Значение, отличное от stored, даёт один proposed request: UUID старой длины в
`remove` ровно один раз (позиция сохраняется, если removal уже был) и одна новая
Distance выбранного Line в `add` (обновление на прежнем месте или append). Прочие
pending H/V/длины/удаления и их порядок сохраняются. Повторный Apply того же
значения к тому же запросу — no-op. Возврат к stored длине снимает только этот
UUID из `remove` и pending Distance этого Line из `add`: это явно «оставить
существующую длину с прежним UUID». Пустой draft после такой отмены допустим
как состояние редактора и не отправляется в jobs; Save по-прежнему требует
валидный непустой request. Один успешный Apply — один checkpoint на весь
remove/add. Running выключает поле и действия.

### Точные типы и конфликт

UUID и версии следуют общему [JSON v1](cli-json-v1.md). destination — UTF-8 строка
того пути, который был опубликован (относительный путь не канонизируется).
Все counts/DOF — неотрицательные JSON integers, координаты — конечные numbers/mm.
Имена/refusals — точные строки или null; diagnostic prose может меняться.
Новые UUID выдаются при подготовке request, один раз; failure не обещает, что
эти proposed IDs существуют в опубликованном документе.

`constraint_conflict.sketch_id` идентифицирует выбранный Sketch; constraints —
упорядоченные typed facts существующего `SketchConflict`, не solver ordinals.
Текущий разрешённый класс выдаёт пять rule kinds выше. Defensive projection
также понимает `perpendicular` и `parallel` с a/b сегментами
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

### §25G: закрепление одной вершины профиля

Тонкий UI-адаптер в том же окне `Edit constraints`. Над каталогом сохранённых
связей: выбранный Line из прежнего segment list, явный `Pin Start`/`Pin End`,
поля `Fixed X (mm)` и `Fixed Y (mm)` и действие `Add Fixed point`. Под ними
строка «Stored start/end of the selected Line: (x, y) mm» показывает **stored**
координату выбранного endpoint, поэтому выбор конца не требует чтения текущего
solved drawing. Персистентный Fixed отображается в каталоге как
`Fixed point <start|end> (x, y) mm · UUID` — своим именем, а не прежним
fallback-ярлыком `Coincident closure` — и поддерживает тот же exact `Remove`.

Один `Add Fixed point` — один прежний bounded `History::change`; неприменённые
поля X/Y, выбор сегмента и выбор endpoint в историю не входят. Отказ валидатора
(второй Fixed, нефинитное или непарсящееся число) и no-op сохраняют Redo и
не меняют draft. Running блокирует действие и сохраняет набранное. Пустой draft
в job не отправляется; Save Cancel, ошибка/отмена и stale reply сохраняют оба
стека и edits. Замена закрепления собирается прежними средствами: `Remove` на
сохранённом Fixed и `Add Fixed point` дают один атомарный remove+add request.
UI и CLI используют одну предметную политику; UUID нового ограничения по-прежнему
создаёт общая операция при реальной публикации, а не UI.

### §25H: равенство длин двух Lines

Тонкий UI-адаптер в том же окне `Edit constraints`. Над каталогом сохранённых
связей строка `Equal length:` даёт `Equal Line A`, `Equal Line B` и
`Add Equal length`. Обе стороны берутся из того же прежнего segment list: выбор
сегмента и нажатие `Equal Line A`/`Equal Line B` запоминает эту Line как сторону
пары, а строка `Pair: A <id> · B <id>` показывает выбранную пару (`none` до
выбора). Второго каталога и второго списка нет.

Выбор сегмента и выбор любой стороны пары — selections: они не входят в bounded
историю и не очищают Redo. Один успешный `Add Equal length` — один прежний
`History::change`. Отказ общего валидатора (self-pair, duplicate, reversed
duplicate, чужая Line) и no-op сохраняют draft, оба стека и Redo. Running
блокирует действие и сохраняет обе стороны пары. `Add Equal length` недоступен,
пока не выбраны обе стороны; solver на выбор, Add, Undo и redraw не запускается.

Персистентное равенство показано в каталоге как `Equal length · UUID` и
`Lines <a> = <b>` — своим именем и обеими Lines, а не прежним fallback-ярлыком
`Coincident closure` — и поддерживает тот же exact `Remove`. Замена пары
собирается прежними средствами: `Remove` на сохранённом равенстве и
`Add Equal length` дают один атомарный remove+add request. UI и CLI используют
одну предметную политику; UUID нового ограничения по-прежнему создаёт общая
операция при реальной публикации, а не UI. Новые поля не вытесняют
Save/Undo/Redo/Cancel за пределы доступной области окна.
