# §24I — JSON STEP import: протокол проверки и передача

Начальные разделы сохраняют протокол передачи до ревью. Независимо повторённые
локальные проверки записаны в отдельном разделе ниже.

2026-09-09, Codex. Работа выполнена без окон, viewer/GUI/Unity/GPU/render tests
не запускались. Изменения **незакоммичены и не staged**; следующий шаг — независимое
ревью. Commit/push/PR/merge не выполнялись, следующий срез не начат.

## База

- Ветка `cli-json-step-import` от чистого актуального main после свежего fetch.
- `HEAD = main = origin/main = e98a8e1d810fd39a35ae06b6ee85208009c6b364`.
- [PR #23](https://github.com/gesriot/ferrite-cad/pull/23) MERGED; head
  `5dc94b03e182a285e1230a020ca1283eecc0be61` и merge имеют дерево
  `27319c222083bd0bf56631785457dd6caa194859`.
- CI точной **базы** повторно прочитан: 23/23 checks, пять workflow success.
  [Runtime merge](https://github.com/gesriot/ferrite-cad/actions/runs/34271924603):
  заново проверены 51/51 named gates (8 edit + 1 JSON + 1 shared import + 3 FBX +
  4 STL на каждой ОС) и ufbx 256 + 6 + 6 + 256 checks, 0 failures на каждой ОС.
- Чужой worktree a200 остаётся detached на
  `5b20719145222befc0bd25647f47ba3d67fee66d`; не изменялся. Исходные fixtures
  не изменялись, для импорта и удаления использованы private copies.

**Это CI базы. CI нового незакоммиченного diff не выполнен.** Изменённый runtime
workflow будет проверен удалённо после независимого ревью и публикации.

## API и wire решение

`import_step_result(&ImportStepArgs)` — один text/JSON адаптер request. Проверка
UTF-8 source/output включается только для JSON до файловой работы; дальше один
`import_step_document`. Jobs, source read/import, handles, storage, SQLite close,
preflight priorities, late alias recheck и publish не менялись. Повторного чтения
STEP/документа ради DTO нет.

Явный DTO CLI, без сериализации domain structs: published destination,
document/object/source UUID, name/basename, source byte length/BLAKE3,
importer id/version/build, step_schema/source_unit, definitions/placements counts
и ordered diagnostics. Diagnostic stage/severity имеют стабильные wire names,
entity/message сохраняются точно. Optional source_name — explicit null;
пустые diagnostics — `[]`. Unknown fields игнорируются, unknown diagnostic values
не считаются отсутствием проблемы. [Полная схема и примеры](cli-json-v1.md#результат-import-step-24i).

| Исход | Wire | Exit |
| --- | --- | --- |
| Published без diagnostics | ok:true, result | 0 |
| Published с diagnostics | ok:true, result, непустые diagnostics | 4 |
| Reader rejection | ok:false; error.kind=input, code=reader_rejected, typed step_read | 5 |
| Operational failure | Прежний error.kind/message/causes, code/step_read отсутствуют | 2 |
| Потеря отчёта любого исхода | Единственный fallible emitter; без повтора/rollback | 7 |

`step_read` содержит только размер/hash bytes, importer и diagnostics. Rejected
не выдумывает publication destination или UUID. Message и causes остаются
человеческой диагностикой; для rejection causes `[]`, STEP findings лежат отдельно.
Common ErrorKind::as_str() сохраняется; JSON не добавлен в jobs или ошибки kernel.
Reader silence не доказывает корректность STEP; historical import diagnostics
не определяют нынешние FBX omissions/complete. Source unit — декларация STEP.

Emitter минимально разделён на выбор envelope/exit и единственную сериализацию/
доставку. Пять старых JSON команд сохраняют wire и коды. Text import сохраняет
stdout/stderr/порядок строк/0/4/5/2. Clap usage/help остаются текстовыми;
самостоятельный --json до -- включает протокол, filename/value --json — нет.

## Native проверки текущего diff

Окружение `/private/tmp/ferrite-24i/native-env.sh`, target
`/private/tmp/ferrite-24b-native-target`. Проверены vendor OCCT 8.0.1 CMake path,
`libTK*.8.0.dylib` и `libplanegcs.dylib` в imports свежего CLI. Native env требует
OCCT/PlaneGCS, GPU unset. CLI/app пересобраны до headless worker gates.
OCCT из исходников не пересобирался; native inputs и FFI не менялись.

| Проверка | Результат |
| --- | --- |
| fmt; workspace clippy all targets/features `-D warnings` | pass |
| document / jobs / export | 132 / 46 / 60 passed; один старый timing benchmark ignored |
| CLI unit/create/edit/text FBX/text STL/import/JSON STEP/JSON v1/shared import | 68 passed (8+9+2+9+9+13+3+13+2), без native skips |
| 8 edit + 4 STL headless app workers | все прежние named gates passed |
| FBX campaign | 3 прежних exact process tests passed, complete/partial/delivery |
| Pinned ufbx 0.23.0 strict | 256 + 6 + 6 + 256 checks, 0 failures |
| export boundary; workflow YAML; doc links/JSON examples; diff whitespace | pass |

Локально подтверждены **18/18 named native gates** без skips: прежние 17 и
`native_json_step_import_publication_rejection_and_delivery`. Workflow сохраняет
51 прежний required execution и добавляет этот gate на каждой ОС (будущие 54);
exact-name проверка и запрет skipped/нулевого исполнения сохранены. Linux/Windows
нового diff локально не исполнялись.

Новый process suite проверяет четыре publications: clean plate, nested assembly,
Unicode parts, diagnostic STEP. После reopen сравниваются JSON и реальные
stored UUID/hash/bytes/name/basename/kernel/schema/unit/counts/diagnostics.
Независимые text/JSON imports имеют разные UUID; persisted scene сравнивается
после явного сопоставления occurrence IDs, с сохранением parent/local transforms/
colours/definition relations. Source bytes и mtime неизменны. Typed DTO test
дополняет процессы null basename, пустыми строками и ordered stage/severity с
Unicode/quotes/LF — не заменяет native import.

Реальные rejected truncated/unnameable STEP дают exit 5 и typed facts без UUID;
read facts сравниваются с native reader. Missing source, no-clobber, force,
source/hardlink/Unix symlink, publication в занятый каталог, UTF-8 refusals,
help/usage и --/flag-shaped source/name проверены процессами. Для usage cases
оставлен один неверный аргумент, остальные допустимы.

Прямые закрытые OS pipes через прежний `tests/support/pipe.rs`:

- 8 publication cases: clean/noticed × new/force × stdout closed/both closed;
  exit 7, читаемый целый `.fcad`, source/прочие файлы неизменны, scratch отсутствует.
- 6 reader rejection cases: fresh/force × stderr closed/stdout closed/both closed;
  исправный stdout доставляет typed error с exit 5, потерянный — 7. Файлы неизменны.
- 6 operational missing-source cases с теми же комбинациями: exit 2/7,
  исправный stdout работает с закрытым stderr; нового файла нет.
- Stub дополнительно исполняет unsupported kernel с теми же refusal pipes.

Отдельный Unix non-UTF-8 path test исполнен на Mac без попытки создавать
недопустимое имя. Отдельный Windows test передаёт unpaired UTF-16 surrogate в
source и output и также вызывается из обязательного native gate; он требует CI.
Новые Windows filenames не содержат кавычек/LF; stored strings проверяются отдельно.

Две временные поломки компилировались и исполнили exact process gate:
noticed publication возвращала 0 вместо 4; reader rejection — 2 вместо 5.
Обе дали исполняемое assertion failure/exit 101, не compile failure и не zero tests.
Файл восстановлен побайтово, весь новый process suite повторён положительно.
После уточнения usage fixtures повторены native/stub suite и clippy/fmt.

## Сохранение прежнего поведения и независимый FBX

С базовым CLI, собранным до правок, сравнены text clean/noticed/rejected:
stdout/stderr/exit совпали после нормализации только новых UUID. Operational JSON
errors всех пяти старых команд совпали **побайтово**, включая отсутствие новых
null/error fields. Остальные старые JSON success/delivery покрыты прежним suite.

В существующей FBX кампании единственный large complex import и compact clean
import теперь используют --json и проверяют publication facts. Затем удаляется
только private STEP; те же text/JSON FBX process routes и strict ufbx читают
сохранённую сцену. Нового тяжёлого workflow или отдельного большого импорта для
каждого pipe case нет. В partial сохранены 46 definitions, 140 nodes,
34 geometries, 986873 triangles и одна source-qualified omission, exit 6.
Прежние complete (8) и partial (4) FBX delivery cases также прошли.

[Публичный Python-рецепт](cli-json-v1.md#рецепт-json-step-import--сохранённый-документ--fbx)
извлечён из Markdown и выполнен настоящим CLI: JSON import 0 → inspect → удаление
private STEP → JSON FBX 0 → независимый ASCII subset parser. Подтверждены counts,
bytes, 12 треугольников и размеры 60×40×10 mm с прежним FBX `(x,z,-y)` в метрах;
hashes оригинальной fixture и сохранённого `.fcad` неизменны. Рецепт явно
останавливается на 4 (нужно решение по diagnostics), 5, 2, 6 и 7; prose не парсит.
Subset parser не назван универсальным FBX reader; pinned ufbx проверен отдельно выше.

## Stub и ограничения

Новый target `/private/tmp/ferrite-24i-stub-target`, env
`/private/tmp/ferrite-24i/stub-env.sh`. CMake wrapper исключает Homebrew:
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`, CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew.
Imports текущего CLI содержат только libiconv/libSystem, без OCCT/PlaneGCS.

CLI + FBX harness: **71 harness passes, 31 явный geometry skip**; skips не считаются
native export/import. Discovery, preflight, structured unsupported и delivery
реально исполнены. Отдельно прошли 5 deterministic jobs tests без skips.
Обновлённый JSON import suite повторён на stub: 3 harness passes, один native skip.

Логи, audit базы, snapshots мутаций, native/stub env, CMake/imports и recipe:
`/private/tmp/ferrite-24i/`. Они не входят в diff. Dependencies/Cargo.lock/inventories/
notices/SBOM inputs не менялись и не требуют регенерации. Jobs, document schema,
persisted identities, FBX writer, C++/FFI, UI/renderer не менялись. Полного assembly
discovery, UI import, batch/stdin/RPC, CLI cancellation и version guard для FBX нет.

## Независимое локальное ревью, 2026-09-09

Проверены общий import job, явная проекция DTO, reader rejection без publication
identity, прежний operational error envelope и единая fallible доставка. Дефектов
production-кода не найдено. В карте возможностей исправлены две устаревшие строки:
JSON теперь у шести команд; высота существующей плиты редактируется через §24C.
План дополнен следующим срезом §24J read-only validate с JSON-диагностикой.

CLI/app заново собраны с native env; fmt и workspace clippy all targets/features
с -D warnings прошли. Повторены document 132, jobs 46, export 60, CLI 68 tests,
8 edit + 4 STL headless workers и 3 FBX process tests. Все 18 named native gates
прошли без skips; один прежний timing test ignored. Strict ufbx дал
256 + 6 + 6 + 256 checks, ноль failures.

На отдельном stub target исполнены 5 deterministic jobs tests и 68 CLI harness
passes; 29 явных geometry skips в этих suites не засчитаны native-проверками.
Stub CMake NOTFOUND и отсутствие OCCT/PlaneGCS imports подтверждены. Большая stub
FBX кампания повторно не запускалась. Окна/GUI/Unity/GPU и OCCT source rebuild
не запускались. Новые mutations при ревью не выполнялись; положительные tests
их семантических свидетелей исполнены заново.

Text clean/noticed/rejected совпали с сохранённым базовым CLI после нормализации
новых object UUID. Operational errors пяти прежних JSON-команд совпали побайтово,
включая отсутствие code/step_read. Первая проба create с missing parent различалась
только случайным UUID scratch в сообщении обоих бинарников; сравнение повторено
на детерминированном no-clobber отказе. Это различие fixture, не регрессия DTO.
Публичный рецепт заново извлечён из Markdown и выполнен до независимого FBX parser
после удаления только private STEP. Export/solver boundaries, notice ownership,
licence headers, actionlint и diff --check прошли. Логи: `/tmp/ferrite-pr24-review/`.

Checks опубликованного head и merge main фиксируются отдельно в PR и итоговом
отчёте после завершения; локальные результаты выше не являются удалённым CI.

## Файлы и diffstat передачи (до ревью), включая untracked

| Файл | + | − | Состояние |
| --- | ---: | ---: | --- |
| `.github/workflows/runtime-layout.yml` | 23 | 0 | modified |
| `README.md` | 5 | 2 | modified |
| `crates/ferritecad-cli/src/import.rs` | 11 | 4 | modified |
| `crates/ferritecad-cli/src/json.rs` | 16 | 1 | modified |
| `crates/ferritecad-cli/src/main.rs` | 8 | 0 | modified |
| `crates/ferritecad-cli/tests/export_fbx_complex.rs` | 30 | 0 | modified |
| `crates/ferritecad-cli/tests/export_fbx_complex/json.rs` | 13 | 0 | modified |
| `crates/ferritecad-cli/tests/shared_step_import.rs` | 1 | 1 | modified |
| `docs/cli-capabilities.md` | 20 | 7 | modified |
| `docs/cli-json-v1.md` | 202 | 8 | modified |
| `docs/implementation-plan.md` | 25 | 10 | modified |
| `docs/shared-step-import.md` | 4 | 3 | modified |
| `crates/ferritecad-cli/src/json/import.rs` | 228 | 0 | untracked |
| `crates/ferritecad-cli/tests/json_import.rs` | 560 | 0 | untracked |
| `docs/cli-json-step-import-verification.md` | 182 | 0 | untracked |

Итого **15 файлов, +1328 / −36**, включая три untracked файла.
