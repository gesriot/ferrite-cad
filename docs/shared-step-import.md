# Общий STEP import (§24H)

`ferritecad_jobs::import_step_document` — один предметный маршрут для сохранения
STEP в новом `.fcad`. Текстовый `import-step` уже использует его. UI import,
`import-step --json` и флаги отмены CLI **не добавлены**. Существующий JSON v1
пяти других команд не меняется.

## Request и завершённый результат

`ImportStepRequest` содержит заимствованные `source: &Path`, `destination: &Path`,
`name: Option<&str>` и `existing: Existing::{Keep { advice }, Replace}`.
`None` имени означает прежний file stem, затем `Imported`; `Some("")` остаётся
явно заданной пустой строкой. Текстовый клиент сохраняет прежнее lossy-преобразование
OS-строки default name и basename. Оно не является обратимым кодированием пути.
`source_name` сохраняет только basename, никогда не полный путь к диску пользователя.

Вызов принимает factory `FnOnce() -> Result<K>`, importer callback
`FnOnce(&mut K, &[u8]) -> Result<Import>` и `&OperationContext`, где
`K: GeometryKernel`. Factory, import, release и уничтожение kernel происходят
синхронно в потоке вызова. Поэтому worker может создать и закончить native session
в своём потоке. Jobs не зависит от OCCT crate, CLI, JSON или exit codes.

Возвращается `Result<StepImportOutcome>`:

| Исход | Owned-факты | Текстовый CLI |
| --- | --- | --- |
| `Published(Box<PublishedStepImport>)`, diagnostics пусты | Файл опубликован; поля ниже | 0 |
| `Published`, diagnostics непусты | Файл опубликован, diagnostics сохранены в исходном порядке | 4 |
| `Rejected(StepReadFacts)` | Только факты чтения и отказа, без destination/document/object identity | 5 |
| `Err(CadError)` | I/O, unsupported, storage/publication error или cancellation; публикации нет | 2; CLI не запрашивает отмену |

`StepReadFacts`: `source_byte_len: u64`, `source_hash: ContentHash` (полный BLAKE3
прочитанных байтов), `imported_by: ImporterIdentity` (id/version/build) и
`diagnostics: Vec<Diagnostic>` (stage/severity/entity/message в порядке reader).
Это собственное наблюдение reader; отсутствие diagnostics не означает корректность
STEP, а исторические diagnostics импорта не являются текущими FBX omissions.

`PublishedStepImport` дополнительно содержит `destination: PathBuf`,
`document_id: DocumentId`, `object_id: ObjectId`, точное `name: String`,
`source: ImportedSourceId`, `source_name: Option<String>` и
`scene: PersistedScene`. Scene — именно проекция V3, возвращённая
`Document::store_step_import`: STEP schema/source unit, ordered definitions
(key/name/solids), ordered placements (occurrence UUID, definition key, parent,
name, local transform, colour source/RGB). Пустые structural definitions и отказ
геометрии не объединяются. Length data сохраняется по прежнему документному
контракту; STEP source unit — записанная декларация, не UI display unit.

Набор фактов достаточен для прежнего текстового отчёта и программного клиента:
счётчики выводятся из ordered vectors, диагностика не вычисляется повторно,
publication identities доступны только у Published. Общий результат не несёт
live `Import` или `ShapeHandle`; это не новый wire/JSON-контракт. Source bytes
остаются целиком внутри `.fcad`, а в результате достаточно их размера и hash.
Повторного открытия `.fcad` ради отчёта и второго `Scene::persist` нет. Независимые
импорты получают новые document/object/source/occurrence UUID; равенство их файлов
или UUID не обещается. Definition keys сравниваются только вместе с source identity.

## Владение и публикация

Порядок для валидного uncancelled request сохранён: source/alias protection →
no-clobber → единственное чтение STEP bytes → factory/import → storage.
Missing source остаётся I/O-отказом до открытия ядра, в том числе в stub-сборке.
После `Import::Imported` job немедленно принимает владение всеми возвращёнными
handles. Локальный RAII guard освобождает каждый уникальный handle ровно один раз
в том же kernel при успехе, storage/publication error и отмене. Один handle,
разделяемый несколькими definitions, не освобождается повторно. Если importer
возвращает `Err` или `Rejected`, callback сам отвечает за свою промежуточную
геометрию, как существующий OCCT adapter; Rejected не передаёт geometry ownership.

Документ создаётся в собственной директории `Temporary` рядом с destination.
`store_step_import` сохраняет байты и проекцию в одной транзакции. SQLite закрывается,
handles освобождаются, затем source/aliases проверяются ещё раз и выполняется
атомарный publish. Keep не заменяет файл, появившийся после preflight; Replace
заменяет только запрошенное назначение целиком после готовности scratch.
Source, hardlink и Unix symlink запрещены как destination также с Replace.
На ошибках удаляется только собственный scratch и его SQLite sidecars; чужое
назначение cleanup не удаляет. Общий `Temporary` не изменён.

## Фазы и отмена

ProgressSink получает следующие milestones; дроби — границы работы, не оценка
времени. Native import может занимать почти всё время операции.

| Fraction | Что уже произошло | Отмена |
| --- | --- | --- |
| 0.0 | До preflight и чтения | Проверяется |
| 0.1 | Прочитаны все STEP bytes, kernel ещё не создан | Проверяется |
| 0.6 | Import вернулся; принятые handles уже принадлежат guard | Проверяется |
| 0.7 | Создан scratch Document, SQLite открыт | Проверяется |
| 0.8 | Записаны source/scene/diagnostics, SQLite ещё открыт | Проверяется |
| 0.9 | Scratch закрыт; handles освобождены; файл ещё не опубликован | Проверяется, затем alias recheck и последняя проверка токена |
| 1.0 | Атомарный publish состоялся | Поздняя отмена не отзывает файл/Published |

Блокирующий OCCT import не прерывается внутри вызова: токен проверяется на границах,
а полученная после отмены геометрия освобождается. Отказ reader заканчивается на
0.6 и не сообщает 100%. Ошибка или отмена до publish сохраняет source и занятое
destination, не оставляет нового `.fcad`, scratch или sidecars. После publish
нет повторной проверки, способной превратить состоявшуюся публикацию в `Cancelled`.

## Публичный безоконный рецепт

Из корня checkout; native `ferritecad` в PATH или абсолютный `FCAD_CLI`.
Python использует публичный CLI; import остаётся текстовым, решение принимается
по exit code. Рецепт копирует чистую fixture, удаляет **только свою STEP-копию** и
независимо читает объявленный ASCII FBX subset: реальные объекты, vertices,
polygon indices и размеры. Он не является универсальным FBX reader.

```sh
python3 - <<'PY'
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

cli = os.environ.get("FCAD_CLI", "ferritecad")
root = Path(tempfile.mkdtemp(prefix="ferrite-shared-step-"))
original = Path("fixtures/step/canonical/01-single-part.step")
original_hash = hashlib.sha256(original.read_bytes()).digest()
source = root / "private STEP.step"
shutil.copyfile(original, source)
document, fbx = root / "part.fcad", root / "part.fbx"
p = subprocess.run([cli, "import-step", str(source), "-o", str(document)], capture_output=True)
if p.returncode == 5:
    assert not document.exists()
    raise SystemExit("Reader rejected STEP; no document published")
if p.returncode == 2:
    assert not document.exists()
    raise SystemExit(p.stderr.decode(errors="replace"))
if p.returncode not in (0, 4):
    raise SystemExit(f"Unexpected import exit {p.returncode}; inspect destination before retry")
# This fixture is clean. For a different STEP, exit 4 explicitly requires
# reviewing the preserved diagnostics before treating the scene as complete.
assert p.returncode == 0, p.stdout.decode(errors="replace")
assert source.read_bytes() == original.read_bytes()
source.unlink()  # Only the copy created above.
reading = subprocess.run([cli, "inspect", str(document), "--json"], capture_output=True)
assert reading.returncode == 0
value = json.loads(reading.stdout)
assert value["ok"] and value["result"]["bodies"] == []
# Imported STEP is not a native Body; use the whole-scene FBX route.
before = hashlib.sha256(document.read_bytes()).digest()
p = subprocess.run([cli, "export-fbx", str(document), "-o", str(fbx), "--json"], capture_output=True)
if p.returncode == 7:
    raise SystemExit("Report delivery failed; keep and inspect destination, do not retry automatically")
report = json.loads(p.stdout)
if p.returncode == 2:
    assert not report["ok"]
    raise SystemExit(report["error"])
assert p.returncode in (0, 6) and report["ok"]
r = report["result"]
assert r["destination"] == str(fbx) and r["bytes"] == fbx.stat().st_size
text = fbx.read_text(encoding="utf-8")
for field, kind in [("models", "Model"), ("geometries", "Geometry"), ("materials", "Material")]:
    assert r[field] == len(re.findall(r"^\s*" + kind + r": [0-9]+,", text, re.M))
if p.returncode == 6:
    assert not r["complete"] and r["omissions"]
    for omission in r["omissions"]:
        print(json.dumps(omission, ensure_ascii=False))
    raise SystemExit("Partial FBX kept; complete-model processing stopped")
assert r["complete"] and r["omissions"] == [] and r["geometries"] == 1
vertices = re.search(r"Vertices: \*(\d+) \{\s*a: ([^}]+)\}", text)
coordinates = [float(x) for x in vertices[2].strip().split(",")]
assert len(coordinates) == int(vertices[1])
# Existing FBX contract: metres, (x, z, -y), Y up.
assert [max(coordinates[i::3]) - min(coordinates[i::3]) for i in range(3)] == [0.06, 0.01, 0.04]
indices = re.search(r"PolygonVertexIndex: \*(\d+) \{\s*a: ([^}]+)\}", text)
indices = [int(x) for x in indices[2].strip().split(",")]
assert len(indices) == 36 and sum(i < 0 for i in indices) == 12
assert hashlib.sha256(document.read_bytes()).digest() == before
assert hashlib.sha256(original.read_bytes()).digest() == original_hash
assert {p.name for p in root.iterdir()} == {"part.fcad", "part.fbx"}
print("Shared STEP import and independently checked FBX:", root)
PY
```

Import и последующий FBX export читают разные сохранённые состояния. Экспорт
использует embedded bytes `.fcad`; `--expect-version` у FBX нет. Более полный
[JSON FBX рецепт](cli-json-v1.md#рецепт-fbx-полнота-и-отдельная-проверка-файла)
явно обрабатывает настоящую partial assembly. Обязательная кампания
`source tools/check-fbx-complex.sh --all-features` дополнительно использует pinned
ufbx 0.23.0 в strict mode без окна/Unity; на macOS source запускайте в Bash с
уже заданными DYLD-путями native библиотек.

Локальные результаты и ограничения: [протокол проверки §24H](shared-step-import-verification.md).
