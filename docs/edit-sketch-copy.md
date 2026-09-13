# §25B — координаты сохранённого Sketch → новая копия

Выбор/drag существующих вершин мышью добавлен в [§25C](sketch-vertex-drag.md);
числовые requests и общий маршрут сохранения прежние.

UI `Edit Sketch <name> — <UUID>…` и CLI `edit-sketch-copy` правят существующую
модель. Они сохраняют document/object/curve/reference UUID; это не повторный
`create-sketch-extrude`. Имена не служат ключами. Нужны native OCCT для правки,
а discovery через `inspect --json` работает без OCCT/PlaneGCS.
[Исполненные проверки и ограничения](edit-sketch-copy-verification.md).

```text
ferritecad edit-sketch-copy <source.fcad> --sketch <UUID> --expect-version <token> --request <coordinates.json> -o <new.fcad> [--json]
```

## Поддерживаемая модель и identities

Ровно четыре корневых объекта: untransformed XY DatumPlane, unconstrained Sketch,
положительный literal Blind Extrude вдоль +Z с NewBody и один Body с этой tip.
Три связи — Plane, Profile, BodyTip, с соответствующими UUID. Порядок объектов и
имена могут быть любыми; они сохраняются. Поддержаны sample plate и §25A, включая
оба исходных winding. Не поддержаны STEP, дополнительные объекты/Body, constraints,
Parameter/formula, construction, другие плоскости, holes, arcs/curves, reversed,
booleans, ThroughAll, добавление/удаление/перестановка сегментов и смена winding.

Line имеет UUID **кривой**, но не UUID вершины. В этом контракте вершина — start
именованного сохранённого Line. Конец каждого Line равен start следующего;
последний замыкается на первый. Исходные Lines должны уже составлять точную
замкнутую последовательность в сохранённом порядке. Мы не сортируем их по
координатам и не сшиваем почти совпавшие концы. Все кривые участвуют в одном
контуре. Число сегментов 3..256, координаты mm. Числовая polygon policy §25A
(допуски, самопересечения/касания, вырождение, предел 1e6 mm) общая для UI/CLI.

## Discovery JSON v1

`inspect --json` добавляет обязательный `result.sketches`, массив в порядке
`Document::objects()` после фильтра Sketch. Пустой/imported-only документ даёт
`[]`, `ok:true`. Все прежние поля, `features` (Extrude), `bodies` (Body), порядок,
null policy и content version сохраняются. Эти три UUID-домена не взаимозаменяемы.

Каждая строка имеет обязательные поля:

| Поле | Тип и смысл |
| --- | --- |
| `sketch_id` | canonical UUIDv7 сохранённого Sketch |
| `name` | точная сохранённая string или null |
| `vertices` | ordered array `{curve_id: UUIDv7, start_mm: [number, number]}` для поддерживаемого контура; null для неподдерживаемого |
| `editable` | boolean; учитывает как local refusal, так и общий copy_access |
| `refusal` | string/null; локальная причина неподдерживаемой модели/Sketch |
| `document_refusal` | string/null; общий запрет copy_access, включая triggers/FK/rowid guards |

`start_mm` всегда содержит два конечных числа в миллиметрах, независимо от display
units. Это сохранённые starts, не GPU pick IDs или индексы граней. `vertices`
может быть доступен при общем запрете copy_access; разрешением служит `editable`.
Наличие строки не обещает solid или успешный rebuild. Metadata, версия, Extrude,
Sketch и Body принадлежат одному закреплённому read-only снимку. Нет второго
открытия, вычисления hash ради Sketch, миграции, записи или kernel/rebuild.
Неизвестные response fields игнорируются; неизвестная версия/operation требует
обновления клиента. Человекочитаемые refusal не являются машинными кодами.

## Request и JSON результата

Request — UTF-8 JSON до 65536 bytes, все поля обязательны, неизвестные поля
запрещены, null не разрешён. `request_version` — integer ровно 1, независимый от
версии response v1 и схемы FCAD. `vertices` — **весь** ordered массив из discovery
с новыми `start_mm`. Каждый `curve_id` присутствует ровно один раз и на прежнем
месте. Missing/duplicate/foreign IDs и перестановка отказываются, даже если
получилась геометрически похожая модель. UUID не генерируются.

```json
{"request_version":1,"vertices":[{"curve_id":"019ecc22-0841-7c0e-ad61-374b404f7219","start_mm":[0,0]},{"curve_id":"019ecc22-0841-7c0e-ad61-374b404f7220","start_mm":[80,0]},{"curve_id":"019ecc22-0841-7c0e-ad61-374b404f7221","start_mm":[0,40]}]}
```

Эти UUID — иллюстрация; реальный клиент извлекает их из своего inspect. Изменение
start одновременно меняет end предшественника. Не повторяйте первую вершину в конце.

Response использует прежний envelope: `schema_version:1`,
`operation:"edit-sketch-copy"`, `ok`, взаимоисключающие `result`/`error`, один
UTF-8 JSON object + LF. Успешный result имеет обязательные `destination:string`
(точный переданный UTF-8 путь без канонизации), `document_id:UUIDv7`,
`sketch_id:UUIDv7`. Это факты состоявшейся публикации, без повторного открытия output.
Exit 0 — published; operational refusal — `ok:false`, прежние
`error.kind/message/causes`, exit 2; потеря сериализации/stdout — exit 7.
Закрытый stderr не мешает JSON stdout; оба закрытых канала не вызывают panic.
Exit 7 не отзывает готовый файл, не повторяет правку; проверяйте output отдельно.

Все три пути JSON-команды проверяются на UTF-8 до файловых операций. Text mode
сохраняет OS paths с обычным lossy display. Clap help/usage остаются текстом;
`--` завершает разбор флагов, имя файла `--json` само не включает протокол.
Request parser отказывает неверной версии/типам/размеру до запуска job.
Нет --force, in-place Save или выбора по имени. Старые восемь JSON-команд не меняются.

## Общий API, владение, снимок и публикация

`PolygonExtrusion` перенесён без изменения правил из jobs в document и по-прежнему
реэкспортируется jobs. `SketchChoice`, `SketchVertex`, `sketch_choices` и
`replace_sketch_coordinates` живут в document. Каталог хранится в
`ExtrudeEditSource.sketches` рядом с общими `version`/`refusal`; прежнее имя типа
сохранено для существующих клиентов. `SketchChoice::validate_coordinates` проверяет
черновик теми же правилами без SQLite. Каталог читает objects один раз в общем
чтении; дополнительная проверка связей ограничена кандидатом из четырёх объектов.
Никаких SQL scans на каждый элемент большого неподдерживаемого каталога нет.
Выбранный Sketch обязан сохранять свои storage bytes при неизменённом decode/encode;
неизвестные поля или неподдерживаемое представление отказываются до копирования,
а не теряются при записи. Остальные payload bytes не сериализуются заново.

`EditSketchRequest {source, expected:DocumentVersion, sketch, vertices, destination}`
передаётся в `edit_sketch_copy(&request, &mut GeometryKernel, &OperationContext)`.
Результат `EditedSketch` содержит опубликованные destination/document_id/sketch.
Caller создаёт и уничтожает kernel на одном worker thread. Никаких JSON/exit codes
или UI в jobs нет. `edit_object_copy` — общий внутренний маршрут с edit-extrude:
preflight aliases/no-clobber → read-only pinned source/version/copy_access →
prepare → SQLite snapshot backup → baseline cold rebuild → transaction → edited
cold rebuild/refs → SQLite close → fresh source version/alias check → atomic Keep.

Для Sketch все сохранённые refs должны разрешаться уже в baseline и не теряться
после правки. Curve/reference IDs и semantic roles не переписываются. Handles
освобождаются после каждого rebuild, включая отказ и отмену, до возврата job.
Snapshot backup сохраняет неизвестные SQL-таблицы; guards не разрешают побочные
trigger/FK изменения. Меняется только payload выбранного Sketch и его payload hash;
имена, порядок, связи, height/extent/plane и metadata **включая modified_at** сохранены.
Узкий `Document::write_sketch_coordinates` проверяет тот же класс правки и использует
прежнюю transaction без пересборки capabilities/reclamation: объекты, capabilities
и source reachability не могут меняться от координат. Optional capability rows и их
rowid сохраняются. Обычный `Document::write`, включая edit-extrude, сохраняет прежний путь.
Служебные страницы/change counters SQLite и физическая компоновка файла не обещаны.

Версия сравнивается со снимком, который копируется, и ещё раз с текущим source
непосредственно перед публикацией. Изменённый/заменённый source отказывает stale.
Есть неизбежная граница после последней проверки: это не блокировка пути против
произвольного внешнего процесса. Отмена проверяется на фазах 0.1 (snapshot),
0.4 (baseline проверен, до записи), 0.95 (scratch закрыт) и в evaluator. Нативный
блокирующий вызов не получает новой FFI отмены. 1.0 означает publish; поздняя
отмена не превращает состоявшуюся публикацию в отказ. Keep защищает появившийся
output, aliases перепроверяются. Ошибка до publish удаляет только свой scratch.

UI выбирает UUID из принятой сцены, заполняет прежний canvas и блокирует
добавление/удаление сегментов и высоту. Числовые поля mm меняют контур; undo/redo
и cancel относятся только к черновику. Save Cancel/Failed/worker refusal оставляют
черновик и принятую сцену. Общий Edits владеет одним worker и generation; stale
ответ не закрывает новый черновик. После publish обычный async Open применяет
прежние generation/GPU preparation guards. Ошибка показа не удаляет файл.

## Исполняемый рецепт

Нужны native CLI и pinned независимый ufbx `read_fixture`, как в §25A.
`FERRITECAD`, `UFBX_READER` — абсолютные пути, `RECIPE_DIR` — новый приватный каталог.
Следующий Python-блок запускается целиком; UUID и версия нигде не заданы заранее.

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
source = model
source_bytes = source.read_bytes()
source_mtime = source.stat().st_mtime_ns
catalog = call("inspect", source)
choices = [s for s in catalog["sketches"] if s["editable"]]
assert len(choices) == 1  # explicit policy for this recipe's single Sketch
sketch = choices[0]
vertices = sketch["vertices"]
assert len(vertices) == 6
for vertex in vertices:
    if vertex["start_mm"][0] == 60:
        vertex["start_mm"][0] = 80
edit_request = root / "edit.json"
edit_request.write_text(json.dumps({"request_version":1,"vertices":vertices}),encoding="utf-8")
model = root / "edited L.fcad"
t0 = time.perf_counter()
published = call("edit-sketch-copy",source,"--sketch",sketch["sketch_id"],"--expect-version",catalog["content_version"],"--request",edit_request,"-o",model)
edit_seconds = time.perf_counter()-t0
assert published["document_id"] == created["document_id"]
assert published["sketch_id"] == sketch["sketch_id"]
assert source.read_bytes() == source_bytes and source.stat().st_mtime_ns == source_mtime
saved = hashlib.sha256(model.read_bytes()).hexdigest()
again = call("inspect",model)
assert again["sketches"][0]["vertices"] == vertices
assert again["content_version"] != catalog["content_version"]
assert again["features"] == catalog["features"] and again["bodies"] == catalog["bodies"]
# Explicit cold rebuild has a text report; its prose is not parsed.
subprocess.run([cli,"rebuild",str(model),"--cold"],check=True,capture_output=True)
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
assert [max(p[j] for p in vertices) for j in range(3)] == [80,40,10]
# Planar integer-coordinate faces are represented exactly by float32 here;
# 1 ppm leaves room for tessellation arithmetic without accepting a convex hull.
assert abs(abs(volume6)/6 - 20000) < 0.02
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
assert source.read_bytes() == source_bytes and source.stat().st_mtime_ns == source_mtime
print(json.dumps({"document_id":created["document_id"],"body_id":body,"volume_mm3":abs(volume6)/6,"triangles":n,"create_seconds":create_seconds,"edit_seconds":edit_seconds,"cold_export_seconds":cold_export_seconds,"independent_fbx":facts},ensure_ascii=False))
```

Рецепт останавливается на invalid/warnings, execution refusal, STEP notices/rejection,
partial FBX и lost report: никакого продолжения по одному ok. Между отдельными
inspect/validate/export нет общего снимка. Только edit закреплён --expect-version;
STL/FBX читают текущее сохранённое содержимое и не получают новый version guard.
