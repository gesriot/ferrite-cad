# §24G: JSON FBX — публикация, полнота и типизированные пропуски

Состояние передачи: реализация в рабочем дереве, без staging/commit/push/PR/merge.
Следующий шаг — независимое ревью Codex. Следующий срез не начат.

## Точная база и границы

- База `main = origin/main = 377484a469b2b593c6ad1492ae5c081f0237cef8`.
- Ветка `fbx-json-publication`, создана после fetch и проверки чистого дерева.
- [PR #21](https://github.com/gesriot/ferrite-cad/pull/21) подтверждён MERGED;
  head `28facb37bd148576f19b402e434900607db80f6c`. Оба дерева имеют
  `29f48619564a32a9a7bf707858233c12482c763e`.
- На точной базе независимо проверены 23/23 checks, 5/5 workflow success;
  в [runtime merge](https://github.com/gesriot/ferrite-cad/actions/runs/34249709742)
  — 39/39 именованных native gates, по 13 на Linux/macOS/Windows, без skips.
- Это **CI базы**, не будущего diff. CI незакоммиченных изменений не запускался.
- AGENTS.md в репозитории и родительских каталогах не найден. Чужой detached
  worktree `a200` только перечислен, его файлы/ветка не менялись. Исходные fixtures
  не менялись; импорты использовали приватные копии.

Всё выполнялось без окон: viewer как приложение, GUI smoke, browser/CUA/osascript,
Open/Save, Unity и GPU/render tests не запускались. Компиляция app и 4 STL + 8 edit
тестов использует headless harness. Экран и участие пользователя не нужны.
Jobs/export writer, document schema, UI/renderer, C++/FFI, зависимости и manifest/
Cargo.lock не менялись. Runtime edges прежние; входы inventories/notices/SBOM не
изменились, механическая регенерация состава не требовалась. OCCT из исходников
не пересобирался: использованы существующие pinned native libraries.

## Предметный и wire маршрут

`export_fbx_result(&ExportFbxArgs)` готовит один запрос для text/JSON, сохраняя
прежний порядок: UTF-8 для JSON → защита source/aliases → no-clobber → kernel →
общий export_document_as_fbx → подтверждённый publish. Повторные проверки jobs
при атомарной публикации не изменены. Нет дополнительного read/rebuild/STEP/output
ради DTO. Текстовый summary/omission report остаётся прежним.

JSON v1: operation `export-fbx`, один UTF-8 объект + LF. Result содержит
`destination`, uint64 `bytes`, uint32 `models/geometries/materials`, boolean
`complete`, массив `omissions`. Required поля не исчезают; пустой массив `[]`.
Omission содержит tagged `source` (body/body_id или imported/source_id/definition_key),
`finding` (stage/severity/entity/message), stable typed `refusal` и массив
file-local `placements` (`node/<n>`). Порядок и qualification каждого report entry
сохранены; источник не теряется при одинаковом STEP key. Recorded occurrence
identity для legacy не изобретается. DTO сериализуется serde, не Debug или парсером
текстового отчёта. Схема и правила эволюции — [JSON v1](cli-json-v1.md).

| Состояние | JSON | Exit | Файл |
| --- | --- | ---: | --- |
| Полная публикация | ok:true, complete:true, omissions:[] | 0 | опубликован |
| Частичная публикация | ok:true, complete:false, непустые omissions | 6 | опубликован с сохранённой иерархией/доступной геометрией |
| Execution refusal | ok:false/error | 2 | нового файла/scratch нет; занятое назначение и source сохранены |
| Delivery failure | отчёт недоступен или неполон | 7 | состоявшийся publish/force-замена не отменяются |

`emit_with_exit` минимально расширяет прежний emitter; четыре существующие команды
по-прежнему вызывают `emit` с успешным кодом 0. Общие envelope/error/pipe handling
не дублируются. Partial JSON не пишет прозу на stderr; закрытая диагностика при
отказе не мешает JSON на исправном stdout, оба закрытых канала не вызывают panic.

## Исполненные проверки текущего diff, macOS arm64

Native: OCCT 8.0.1 + прежний pinned PlaneGCS, оба обязательны через
FERRITECAD_REQUIRE_OCCT/FERRITECAD_REQUIRE_PLANEGCS=1. Логи локально в
`/private/tmp/ferrite-24g/`; runtime target `/private/tmp/ferrite-24b-native-target`.

| Проверка | Результат |
| --- | --- |
| Свежая сборка CLI + app, все features | pass; peer CLI пересобран до app worker gates |
| cargo fmt --all -- --check | pass |
| workspace clippy --all-targets --all-features -- -D warnings | pass |
| document tests | 132 passed; 1 прежний ручной timing test ignored |
| jobs tests | 41 passed |
| export library/writer tests | 60 passed (40 + 15 + 5) |
| CLI unit/create/edit/FBX/STL/JSON | 49 passed (7 + 9 + 2 + 9 + 9 + 13) |
| STL headless app gates | 4/4 passed, без skips |
| edit headless app gates | 8/8 passed, без skips |
| Complete/partial FBX process campaign + pinned ufbx | 3/3 process tests; strict ufbx 256 + 6 + 6 + 256 checks, 0 failures |
| Два публичных shell/Python рецепта | pass, включая реальный partial |
| Старый text baseline vs текущий CLI | stdout/stderr/exit/FBX bytes совпали |
| Export/solver boundaries, notice ownership | pass, notice ownership 11 checks |
| License headers, actionlint, shellcheck, diff --check | pass; 306 tracked source files + 3 новых Rust headers проверены |

CLI native JSON suite содержит прежний обязательный
`native_json_inspect_edit_contract`, включая create/inspect/edit/STL и независимый
binary STL parser. Старые девять `export_fbx` process tests не менялись. Ни один
native gate не зачтён через skip. Результаты не объявляются проверкой Linux/Windows.

FBX кампания сохраняет старый
`the_complex_assembly_becomes_one_fbx_that_keeps_every_definition_and_says_what_is_missing`
и добавляет `json::native_json_fbx_complete_publication` и
`json::json_fbx_refusals_preserve_files_and_protocol_in_native_and_stub_builds`.
Управляемый прямой OS pipe helper вынесен из прежнего json_v1 test без изменения
поведения; Windows ChildStdin relay не используется. Новая общая DTO test library
или mutation-инфраструктура не создавались.

Полный native Body и canonical STEP дали по одной Geometry и 12 треугольников;
каждый счётчик JSON сравнен с независимым чтением фактических FBX object records.
JSON/text FBX равны побайтово на этой ОС. Перед изменениями записан дополнительный
text baseline: полный пустой документ и настоящий partial STEP. После изменений
их stdout/stderr, exit 0/6 и FBX/source SHA-256 совпали точно.

Реальная complex fixture: 46 definitions, 140 Models, 34 Geometry, 986873 triangles;
один omission `step.product_definition#2583`, сохранённый finding
validation/fail, текущий IncompleteFace, placements node/71, node/91, node/111,
node/131. JSON exit 6, ok:true, complete:false. Указанные Models — Null без
Geometry, свойства finding/refusal/complete согласованы с JSON; геометрия #2428
сохранилась. Один подготовленный импорт переиспользован на всю кампанию.

Delivery: новый publish и --force × закрыт stdout / закрыты оба канала —
четыре случая для native complete, четыре для imported complete, четыре для
partial. Каждый реальный процесс вернул 7, оставил целый читаемый FBX, равный
ожидаемой публикации; source/другие файлы и отсутствие scratch проверены.
Отдельно закрытый stderr при execution refusal дал исправный error JSON и exit 2,
а оба закрытых канала при отказе — 7 без panic или изменения файлов.

Refusals: missing/broken/WAL/old-schema/minimum-reader, no-clobber, source/hardlink/
Unix symlink даже с force, не-UTF-8 source/output, clap usage/help и flag-shaped
filename/output value. Каждый конкретный отказ проверен с остальными допустимыми
аргументами и сохранностью файлов. Windows unpaired UTF-16 путь покрыт cfg(windows)
process case, но локально не исполнялся. Compact writer-report test дополнительно
проверяет два imported sources с одинаковым key, native Body domain, повторные
legacy-unrecorded placements, Unicode/quotes/LF, пустую entity и enum wire names.

Stub: **новый** `/private/tmp/ferrite-24g-stub-target`, явные отсутствующие OCCT/
PlaneGCS prefixes и CMake wrapper, отключающий Homebrew/package registry discovery.
CMakeCache: `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`. `otool -L` свежих
CLI/app не показывает libTK/planegcs; viewer для этого не запускался. CLI suites:
52 harness-passed, включая реальный JSON unsupported геометрии для нового output
и force-сохранение sentinel. **17 явных geometry skips** (7 старых FBX, 2 complex,
7 STL, 1 прежний native JSON); они не считаются native экспортом. JSON create,
inspect discovery, preflight/delivery refusals действительно исполнены.

## Направленные поломки и воспроизводимость

Две временные правки `src/json/fbx.rs` проверялись настоящим CLI на уже сохранённом
partial `.fcad`, без повторного import и synthetic success DTO:

1. `complete: true` вместо report.is_complete — процесс публикует partial, но
   исполняемая проверка падает `partial publication must not claim complete`.
2. Замена source_id новым UUID — исполняемое сравнение с фактической
   FerriteCADDefinitionId пустого placement падает
   `JSON source qualification must match published placement identity`.

Обе сборки завершились успешно; в каждом случае исполнен один process gate с
конкретным AssertionError, не compile failure и не ноль тестов. Исходник восстановлен
побайтово, затем повторены положительные suites. Временные скрипты/логи остались
в `/private/tmp/ferrite-24g/`, в репозиторий mutation-система не добавлялась.

При разработке исправлены ошибки test fixtures: create --size без --sample создаёт
пустой документ; native sample требует --sample; macOS не позволяет создать
невалидный UTF-8 filename, поэтому отказ проверяется до файловых операций;
реальная partial severity — fail. Stub missing source отказал io ещё при alias
preflight, раньше ядра — ожидаемая прежняя priority. Пробные запуски shell reader
из неверной оболочки/без DYLD завершились ошибкой; в итоговом Bash-прогоне пути
библиотек сохранены через source. Эти попытки не засчитаны проходом.

Команды положительного прогона (native env предварительно задаёт существующие
library paths; на macOS выполняются в Bash, без запуска нового system shell после
экспорта DYLD):

```sh
cargo build --offline -p ferritecad-cli -p ferritecad-app --all-features
cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
cargo test --offline -p ferritecad-document -p ferritecad-jobs -p ferritecad-export --all-features -- --nocapture
cargo test --offline -p ferritecad-cli --all-features --bin ferritecad --test create --test edit_extrude --test json_v1 --test export_stl --test export_fbx -- --nocapture
cargo test --offline -p ferritecad-app --all-features --bin ferritecad-viewer exports::stl::tests::native_ -- --nocapture
cargo test --offline -p ferritecad-app --all-features --bin ferritecad-viewer edits::tests:: -- --nocapture
source tools/check-fbx-complex.sh --offline --all-features
```

[Публичные рецепты](cli-json-v1.md) выполнены из документа без чтения CLI prose.
Новый рецепт создаёт plate, импортирует приватные clean/partial STEP copies,
удаляет только эти copies, читает JSON и независимо проверяет FBX. Partial явно
останавливает downstream complete-model use; exit 2/7 разобраны отдельно, повтор
при потере отчёта запрещён. Между inspect и FBX нет общего snapshot/version guard;
экспорт читает текущий сохранённый файл. Selection, новый geometry API и JSON
остальных команд остаются недоступны.

Существующий combined runtime workflow вызывает эту же FBX кампанию на трёх ОС,
требует exact test names, complete/partial markers, no skips и pinned ufbx. Прежние
39 gates сохранены. Новый тяжёлый workflow не создан. Удалённый прогон изменённого
workflow последует после независимого ревью и публикации; здесь он не выполнен.

## Файлы и diffstat

Включены untracked файлы; staging/commit не выполнялись.

| Файл | + | − |
| --- | ---: | ---: |
| `.github/workflows/runtime-layout.yml` | 20 | 0 |
| `README.md` | 12 | 3 |
| `crates/ferritecad-cli/src/export_fbx.rs` | 18 | 10 |
| `crates/ferritecad-cli/src/json.rs` | 11 | 1 |
| `crates/ferritecad-cli/src/json/fbx.rs` | 222 | 0 |
| `crates/ferritecad-cli/src/main.rs` | 11 | 0 |
| `crates/ferritecad-cli/tests/export_fbx_complex.rs` | 9 | 0 |
| `crates/ferritecad-cli/tests/export_fbx_complex/json.rs` | 435 | 0 |
| `crates/ferritecad-cli/tests/json_v1.rs` | 5 | 11 |
| `crates/ferritecad-cli/tests/support/pipe.rs` | 12 | 0 |
| `docs/cli-capabilities.md` | 30 | 8 |
| `docs/cli-json-v1.md` | 245 | 8 |
| `docs/fbx-json-publication-verification.md` | 205 | 0 |
| `docs/implementation-plan.md` | 31 | 11 |
| `tools/check-fbx-complex.sh` | 35 | 4 |

Всего: 15 файлов, +1301 / −56 строк.

## Независимое ревью Codex, 2026-09-08

Проверена точная база `377484a469b2b593c6ad1492ae5c081f0237cef8`:
`main = origin/main`, все 23 checks успешны. Рассмотрен полный submitted diff,
включая untracked DTO/process modules и helper прямого OS pipe. Резервная копия
и независимые логи — `/private/tmp/ferrite-pr22-review/`.

Production-дефектов не выявлено. Text/JSON используют один экспортный результат;
partial code 6 определяется тем же FbxWriteReport, из которого строится DTO.
Delivery failure обрабатывается прежним emitter и не откатывает публикацию.
Уточнена только формулировка перечня команд в README, в план добавлен §24H.

Повторены fmt, build обоих клиентов, workspace clippy all targets/features
`-D warnings`, document 132 / jobs 41 / export 60 / CLI 49 tests; native
4 STL + 8 edit headless gates и полный `check-fbx-complex.sh`: три process
tests, complete/partial/delivery markers, strict ufbx 256 + 6 + 6 + 256,
ноль отказов. Native skips нет; один прежний timing benchmark ignored.
Оба Python-рецепта заново извлечены из Markdown и исполнены, включая независимое
сопоставление JSON omissions с опубликованными FBX identities/properties.

Отдельно пересобраны stub CLI/app и повторены 52 CLI harness tests с 17 явно
учтёнными geometry skips. Preflight, unsupported, JSON discovery и закрытые
diagnostic pipes действительно проверены. CMakeCache: OCCT-NOTFOUND; otool обоих
бинарей не показывает OCCT/PlaneGCS. Это не доказательство native геометрии.
Export/solver boundary, notice ownership, SPDX headers, actionlint, shellcheck
и diff whitespace — pass. Окно, GUI, Unity, GPU и native C++ rebuild не запускались.

Публикация и CI разрешены пользователем после ревью. Результаты удалённых
workflow фиксируются отдельно для точного PR head и merge SHA; локальные
результаты выше не объявляются проверкой нового diff на Linux/Windows.
