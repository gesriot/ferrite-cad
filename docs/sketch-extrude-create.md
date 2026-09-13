# §25A — Line polygon → Sketch → Blind Extrude → новый FCAD

Это первый ограниченный редактор черновика, не редактор сохранённого Sketch.
Один внешний простой полигон на XY, координаты в mm, положительная константная
Blind-высота вдоль +Z, NewBody. UI: `Create sketch + Extrude…`; CLI:

```text
ferritecad create-sketch-extrude <request.json> -o <new.fcad> [--json]
```

## Вход и общие правила

Request — UTF-8 JSON, до 65536 bytes, все три поля обязательны. Неизвестные поля,
неизвестная версия, дополнительные компоненты точки, null и неправильные типы
отказываются. Это версия запроса этой операции, независимая от JSON envelope v1
и схемы FCAD. JSON не допускает NaN/Infinity; общий API также проверяет конечность.

```json
{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}
```

- `request_version`: целое число 1.
- `points_mm`: 3..256 упорядоченных пар чисел `[x,y]`. Последнее ребро идёт от
  последней точки к первой **неявно**; первая точка в конце не повторяется.
  Оба направления обхода принимаются и сохраняются. Отверстий нет.
- `height_mm`: конечное положительное число по существующему `ExtrudeExtent::blind`.
  Абсолютные координаты и высота ограничены 1 000 000 mm в этом редакторе.
- Нормализация `-0` → `0` использует существующий `Point2`/kernel DTO.
  Расстояние между любыми двумя вершинами должно быть больше 0.000001 mm.
  Повторные вершины и нулевые рёбра отказываются, а не удаляются автоматически.
- Соседние рёбра с абсолютным cross product не больше `1e-6 × max(lengths)`
  отказываются как коллинеарные/возвратные. Несоседние рёбра не должны пересекаться
  или касаться в пределах 1e-6 mm: orientation проверяется с допуском
  `1e-6 × edge_length`, bounding boxes расширены на 1e-6 mm.
- Удвоенная абсолютная signed area должна превышать `1e-6 × perimeter`.
  Консервативный допуск отказывает близким к вырождению контурам; он не обещает
  пригодность любого прошедшего контура для OCCT. Перед publish обязателен cold rebuild.

Дуги, окружности, holes, constraints, формулы, другие плоскости/единицы, booleans,
ThroughAll и несколько Body здесь не кодируются. Дополнительный ключ не включает
неподдерживаемую возможность молча.

## Владельцы, сохранение и публикация

`PolygonExtrusion::new` в jobs — единственная проверка допустимости входного
полигона для обоих клиентов. UI занимается только текстом/экранными координатами.
`NewDocument::SketchExtrude(PolygonExtrusion)` в `CreateDocumentRequest` использует
`create_document_with_kernel`: тот же внутренний маршрут создания и тот же
`populate_profile`, что и sample plate. Нет нового writer/schema/IR/FFI/dependency.
Обычный `create_document` не позволяет обойти kernel-проверку этого варианта;
empty/sample сохраняют прежнее создание без ядра.

В одном scratch Document/transaction записываются DatumPlane XY → Sketch с Line
curves → Blind Extrude → Body, три dependency и три topology references: start/end
cap и faces from first persisted segment. Object и segment UUID генерируются
один раз при записи. Имена: XY, Profile, Extrude1, Body. Сохранённые reference IDs
и роли опираются на эти identities, не на треугольники или traversal OCCT.
Независимые создания имеют независимые UUID; input JSON не является identity source.

Worker создаёт ядро в своём потоке. Существующий evaluator выполняет cold rebuild,
проверяются один построенный shape и разрешение всех сохранённых references.
Handles освобождаются до выхода из проверки, в том числе при отказе/cancel.
Затем SQLite закрывается и общий Temporary выполняет атомарный Keep publish.
Проверка занятости есть до работы и в самом publish; overwrite/force отсутствуют.
Отказы/cancel до publish удаляют свой scratch и не меняют чужое назначение.
После publish поздняя отмена не отзывает файл. Progress 0.8 — geometry построена,
0.9 — scratch закрыт, 1.0 — файл опубликован. Отмена проверяется на границах фаз
и через существующий context evaluator; мгновенная отмена блокирующего OCCT не обещается.

UI хранит черновик отдельно от принятой сцены. Мышь добавляет координаты с
точностью 0.001 mm на сетке (начальный шаг 10 mm); +X вправо, +Y вверх. Числовые поля редактируют
точные значения независимо от масштаба canvas (сначала 4 экранных points/mm; Fit drawing вписывает числовые координаты, Reset view возвращает исходный масштаб).
`Close contour` — явное действие UI, в request замыкание неявное.
Числовые поля UI ограничены 64 символами (в том числе при paste).
Undo/redo до 128 состояний меняет только черновик; новая правка очищает redo.
Cancel draft удаляет черновик. Save Cancel/Failed и job refusal сохраняют его.
Во время job форма заблокирована; повторный submit отказывает без второго worker.
После публикации черновик закрывается, штатный async Open использует прежние
проверки generation и GPU preparation. Отказ показа не удаляет готовый FCAD.

## JSON result и доставка

Новая opt-in операция `create-sketch-extrude` использует существующий envelope
`schema_version:1`, `ok` и взаимоисключающие `result`/`error`, один объект + LF.
Result содержит **только опубликованные факты**: `destination` (точная переданная
UTF-8 строка пути, без канонизации), `document_id` (canonical UUIDv7).
Все поля обязательны, null нет. Остальные ID и доступность правки/Body клиент
читает через `inspect --json`; повторного открытия ради result нет.

```json
{"schema_version":1,"operation":"create-sketch-extrude","ok":true,"result":{"destination":"L.fcad","document_id":"019ecc22-0841-7c0e-ad61-374b404f7219"}}
```

Успех — exit 0. Execution refusal — `ok:false`, обычные `error.kind/message/causes`,
exit 2. Категории основаны на ErrorKind; сообщения не машинный контракт.
Потеря сериализации/stdout — exit 7, без повторного создания/rollback. Готовый
файл остаётся; клиент проверяет назначение отдельно. Закрытый stderr не мешает
JSON в stdout. В JSON оба пути проверяются на UTF-8 до чтения/создания. Text mode
сохраняет обычную OS path/lossy display policy. Clap help/usage — текст, exit 0/2;
значение после `--` не включает JSON: `... -o out.fcad -- --json` читает файл
с именем `--json` и отвечает текстом. Unknown response fields клиент игнорирует;
неизвестные operation/exit/error kind обрабатывает как требующие решения.

## Исполняемый рецепт агента

Нужны native CLI и существующий независимый pinned ufbx reader `read_fixture`
(собирается средствами `tools/unity-fbx-smoke/scripts`, не Unity Editor).
`FERRITECAD` и `UFBX_READER` — пути исполняемых файлов; `RECIPE_DIR` — **новый**
приватный каталог. JSON/FCAD/STL/FBX остаются там для проверки. Рецепт явно требует
успешную полную публикацию; 1/2/4/5/6/7 никогда не превращаются в продолжение.

```python
import hashlib, json, os, pathlib, struct, subprocess, time
cli = os.environ["FERRITECAD"]
reader = os.environ["UFBX_READER"]
root = pathlib.Path(os.environ["RECIPE_DIR"])
root.mkdir(parents=True, exist_ok=False)
def call(operation, *args):
    p = subprocess.run([cli, operation, *map(str,args), "--json"], capture_output=True)
    if p.returncode == 7:
        raise RuntimeError("Report lost: inspect destination separately; never retry publication")
    data = json.loads(p.stdout)
    assert data["schema_version"] == 1 and data["operation"] == operation
    if p.returncode != 0 or data["ok"] is not True:
        raise RuntimeError((p.returncode, data))  # invalid, notices, rejection, partial, failure
    return data["result"]
request = root / "L.json"
request.write_text(json.dumps({"request_version":1, "points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]], "height_mm":10}), encoding="utf-8")
model = root / "L.fcad"
t0 = time.perf_counter()
created = call("create-sketch-extrude", request, "-o", model)
create_seconds = time.perf_counter()-t0
assert created["destination"] == str(model)
request.unlink()  # only our private request; reopen must not need it
saved = hashlib.sha256(model.read_bytes()).hexdigest()
checked = call("validate", model)
assert checked["valid"] and checked["errors"] == 0
if checked["diagnostics"]:
    raise RuntimeError(checked["diagnostics"])  # explicit decision on warnings
catalog = call("inspect", model)
assert catalog["document_id"] == created["document_id"]
assert len(catalog["bodies"]) == 1 and len(catalog["features"]) == 1
assert catalog["features"][0]["distance_mm"] == 10
body = catalog["bodies"][0]["body_id"]
stl = root / "L.stl"
t0 = time.perf_counter()
mesh = call("export-stl", model, "-o", stl, "--solid", body)
cold_export_seconds = time.perf_counter()-t0
assert mesh["body_id"] == body and mesh["length_unit"] == "mm"
b = stl.read_bytes()
n, = struct.unpack_from("<I", b, 80)
assert len(b) == 84 + 50*n == mesh["bytes"] and n == mesh["triangles"]
vertices = []
volume6 = 0.0
for i in range(n):
    v = struct.unpack_from("<9f", b, 84+50*i+12)
    a,bp,c = v[:3], v[3:6], v[6:]
    vertices.extend((a,bp,c))
    volume6 += a[0]*(bp[1]*c[2]-bp[2]*c[1]) + a[1]*(bp[2]*c[0]-bp[0]*c[2]) + a[2]*(bp[0]*c[1]-bp[1]*c[0])
assert [min(p[j] for p in vertices) for j in range(3)] == [0,0,0]
assert [max(p[j] for p in vertices) for j in range(3)] == [60,40,10]
# Planar integer-coordinate faces are represented exactly by float32 here;
# 1 ppm leaves room for tessellation arithmetic without accepting a convex hull.
assert abs(abs(volume6)/6 - 16000) < 0.016
fbx = root / "L.fbx"
scene = call("export-fbx", model, "-o", fbx)
assert scene["complete"] and scene["omissions"] == []
assert scene["bytes"] == fbx.stat().st_size
external = subprocess.run([reader,str(fbx)], check=True, capture_output=True)
read = json.loads(external.stdout)
assert read["strict"] is True
facts = read["files"][0]
assert facts["fbx_version"] == 7400 and facts["warnings"] == 0
assert facts["mesh_count"] == scene["geometries"] == 1
assert facts["node_count_excluding_implicit_root"] == scene["models"]
assert hashlib.sha256(model.read_bytes()).hexdigest() == saved
print(json.dumps({"document_id":created["document_id"],"body_id":body,"volume_mm3":abs(volume6)/6,"triangles":n,"create_seconds":create_seconds,"cold_export_seconds":cold_export_seconds,"independent_fbx":facts},ensure_ascii=False))
```

`inspect`, `validate` и exports читают отдельные сохранённые снимки; между ними
нет version guard. Импорт STEP не участвует в этом рецепте; его исторические
diagnostics и FBX omissions не являются результатом проверки Sketch.
