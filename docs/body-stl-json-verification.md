# §24F: Body discovery и STL JSON — безоконная передача

База: `main = origin/main = 88e02f2de55b64c52b90a2d05aea4469677c7364`,
merge [PR #20](https://github.com/gesriot/ferrite-cad/pull/20), head §24E
`6417af6316264c24d7ca217c65a965ac146dcb41`. Перед началом проверены свежий fetch,
чистое дерево, MERGED и success всех **27 checks / 6 workflows** точного merge,
включая [runtime 34238416095](https://github.com/gesriot/ferrite-cad/actions/runs/34238416095).
Это CI базы, не незакоммиченного diff.

Ветка `body-stl-json`. Изменения оставлены незакоммиченными для независимого ревью
Codex; commit/push/PR/merge и trailers не добавлялись. Чужой detached worktree a200
и исходные fixtures не менялись. Исполнитель — Codex. Следующий срез не начат.

## Что изменилось

`stl_bodies(&Document) -> Result<Vec<StlBody>>` в jobs — общий источник сохранённых
Body facts для прежнего STL selector и JSON inspect. Один `objects()` проход
сохраняет документный порядок, имена и UUID, включая Body без tip. Kernel и JSON
helper не нужны. CLI не повторяет фильтрацию или правила адресации.

Inspect добавляет обязательное `result.bodies: [{body_id, name}]` в schema v1:
пустой/imported-only документ даёт `[]`; `name` точное string/null. Старые fields,
Extrude availability и content version сохранены. Один `Document::open_read_only`
владеет metadata, общим ExtrudeEditSource и Body catalog. Content hash вычисляется
один раз; нет второго path reader, per-object dependencies, rebuild или sidecars.
Каталог не обещает геометрию или доступное ядро.

`export_stl_result` готовит один StlExportRequest для текста и JSON и вызывает
существующий `export_document_as_stl`. JSON `operation: "export-stl"` через прежний
`json::emit` сообщает destination, body_id, body_name, числовые triangles/bytes,
`length_unit: "mm"` из реально завершённого StlExport. Output не перечитывается.
UTF-8 source/output проверяются до работы. Успех — 0, execution error — 2,
clap usage/help остаются текстом; ошибка доставки — 7, без повторной операции,
удаления файла или отката force-замены. Закрытый stderr не мешает JSON-ошибке.

Body UUID отличается от Extrude UUID. Ни совпадающее имя, ни ordinal не заменяют
identity. Inspect и export — разные снимки: exporter читает текущий сохранённый
файл, включает новые данные и отказывает исчезнувший UUID. `--expect-version`
экспорту не добавлен. Точная [схема, ошибки и рецепт](cli-json-v1.md) опубликованы
в одном документе, карта CLI и README обновлены. JSON остальных команд, сборки,
imported-only STL, несколько тел одним STL, UI, renderer, C++/FFI и FBX wire
contract не менялись. Cargo manifests/lock и runtime dependency edges не менялись;
входы inventories/notices/SBOM прежние, генераторы состава не требовались.

## Проверки текущего diff

Все запуски без окна, браузера/CUA, osascript, Open/Save и GPU/render tests.
Приложение только компилировалось и исполняло существующие headless worker gates.
Ни включённый экран, ни участие пользователя не требовались.
Локальная платформа — macOS arm64. Логи: `/private/tmp/ferrite-24f/` (временные
артефакты этой сессии, не часть поставки).

Native: OCCT 8.0.1 / planegcs FreeCAD 1.0.1, существующие библиотеки без сборки
OCCT из исходников. `FERRITECAD_REQUIRE_OCCT=1` и `FERRITECAD_REQUIRE_PLANEGCS=1`.
Peer CLI пересобран **до** app worker gates. Target:
`/private/tmp/ferrite-24b-native-target`.

| Проверка | Реальный результат |
| --- | --- |
| Fmt, workspace clippy all targets/all features `-D warnings` | pass |
| Свежая native CLI/app сборка | pass; окно не запускалось |
| Document, все tests | 132 passed, 1 existing ignored manual timing test |
| Jobs, все tests | 41/41 |
| CLI unit/create/edit/STL/FBX/JSON | 6 + 9 + 2 + 9 + 9 + 13 passed |
| `native_json_inspect_edit_contract` | реально выполнен с новым discovery/export/pipe маршрутом |
| Старые headless STL и edit gates | 4/4 + 8/8, без native skips |
| CLI до/после на плитах 60×40×10 и 91×53×17 | точные text stdout/stderr, STL и source hashes совпали; 12 triangles / 684 bytes |
| Рецепт из `cli-json-v1.md` | извлечён и выполнен: JSON create/inspect/edit/Body discovery/export, STL parser, validate/cold rebuild/FBX |
| Actionlint, shellcheck рецепта, export/solver/notice ownership, SPDX headers | pass |

Сборка stub — **новый отдельный** target `/private/tmp/ferrite-24f-stub-target`.
Использованы отсутствующие OpenCASCADE/PlaneGCS пути и CMake с отключённым поиском
Homebrew/package registries. В свежем CMakeCache `OpenCASCADE_DIR-NOTFOUND`;
`otool -L` CLI и viewer не содержит OCCT или planegcs imports.

| Проверка stub | Реальный результат |
| --- | --- |
| CLI/app build | pass, без окна |
| Create/edit/STL/JSON process suites | 9 + 2 + 9 + 13 harness passed; **8 явных native skips**, не геометрия |
| Новые Body discovery/snapshot и execution refusal tests | выполнены, ядро не требуется |
| JSON inspect → выбранный Body → STL без OCCT | structured unsupported, exit 2, источник цел; новый файл отсутствует, force сохраняет существующий sentinel |
| Старый headless app STL stub refusal gate | 1/1 реально выполнен |

Native JSON gate в stub явно skipped; семь старых STL geometry tests также
skipped. Stub-only test в native suite не считается native export доказательством.
GPU tests не запускались и не учитываются как прошедшие или skipped.

Для воспроизведения после настройки native library paths:

```sh
export FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1
cargo fmt --all -- --check
cargo build -p ferritecad-cli -p ferritecad-app --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p ferritecad-document -p ferritecad-jobs --all-features
cargo test -p ferritecad-cli --all-features --bin ferritecad \
  --test create --test edit_extrude --test export_stl --test export_fbx --test json_v1 -- --nocapture
cargo test -p ferritecad-app --all-features --bin ferritecad-viewer \
  exports::stl::tests::native_ -- --nocapture
cargo test -p ferritecad-app --all-features --bin ferritecad-viewer \
  edits::tests:: -- --nocapture
```

Эти app filters не запускают окно/GPU. Не заменяйте их общим app test suite,
когда требуется полностью безоконная проверка без renderer.

## Содержательные gates

Добавлены два process tests внутри существующего JSON suite, без тестовой DTO
библиотеки. Они проверяют required fields, Body/Extrude domains, документный
порядок вместо имени, duplicate/unnamed/Unicode/quotes/LF names, Body без tip,
empty/imported-only, source bytes и отсутствие sidecars. Общий snapshot gate
расширен Body facts: при незавершённом SQL writer catalog/version остаются
закреплёнными; JSON inspect SQLite snapshot совпадает целиком, после commit новое
чтение наблюдает новое имя, порядок и content version.

Прежний обязательный native JSON gate теперь выполняет JSON create → inspect →
явный выбор Body → JSON export и независимый binary STL parser. Два Twin дают
60×40×10 и 60×40×23 mm, 12 triangles / 684 bytes и ожидаемый объём. JSON/text STL
одинаковы на этой ОС. Проверяются UUID priority перед чужим именем-UUID,
переименование/ordinal без смены ID, null, текущая новая высота 31 и отказ после
удаления выбранного Body. Реальный STEP import также даёт `bodies: []` и STL refusal.

Отдельно проверены валидный Extrude UUID в --solid, неоднозначное/несуществующее
имя/ID, отсутствующий файл, WAL/старая схема, NaN/inf/zero/negative deflection,
busy output, source/hardlink/Unix symlink даже с force, non-UTF-8 paths. Остальные
аргументы для каждой проверяемой ошибки валидны; clap и execution ошибки различаются.
`--solid=--json` и имя source после `--` не включают протокол.

`std::io::pipe` helper без Windows ChildStdin relay проверяет закрытый stderr при
отказе, оба закрытых канала при отказе, а также новую публикацию и force-замену с
закрытым stdout, с исправным/закрытым stderr. Полный читаемый STL и exit 7 сохраняются;
остальные файлы и scratch проверяются отдельно.

Две направленные временные поломки, затем полное восстановление исходников и
повтор положительных проверок:

| Поломка | Исполняемое падение |
| --- | --- |
| Body filter возвращает Extrude | discovery process gate обнаружил совпадение Body и feature UUID |
| STL job пропускает write/publish | native JSON gate получил success, но отказал на отсутствующем опубликованном STL |

Логи `mutation-body-domain.log` и `mutation-publication.log`: по одному реально
исполненному упавшему тесту; compile failure и нулевые тесты не засчитывались.
Mutation-инфраструктура не добавлена.

Существующий runtime workflow по-прежнему требует точное имя
`native_json_inspect_edit_contract`, отсутствие skipped и обязательные native
libraries на Linux/macOS/Windows. Четыре STL и восемь edit gates сохранены.
Изменено название шага, новый маршрут включён внутрь прежнего gate; тяжёлый
workflow не добавлен. **CI незакоммиченного diff не запускался**: публикация и
удалённый прогон следуют после независимого ревью Codex. Локальные результаты не
объявляются проверкой Windows/Linux.

## Файлы и diffstat

Включены новые untracked файлы; staging/commit не выполнялись.

| Файл | + | − |
| --- | ---: | ---: |
| `.github/workflows/runtime-layout.yml` | 1 | 1 |
| `README.md` | 12 | 6 |
| `crates/ferritecad-cli/src/export.rs` | 22 | 16 |
| `crates/ferritecad-cli/src/json.rs` | 39 | 1 |
| `crates/ferritecad-cli/src/main.rs` | 8 | 0 |
| `crates/ferritecad-cli/tests/json_v1.rs` | 69 | 2 |
| `crates/ferritecad-cli/tests/json_v1/stl.rs` | 638 | 0 |
| `crates/ferritecad-jobs/src/lib.rs` | 1 | 1 |
| `crates/ferritecad-jobs/src/stl.rs` | 23 | 19 |
| `docs/body-stl-json-verification.md` | 175 | 0 |
| `docs/cli-capabilities.md` | 33 | 8 |
| `docs/cli-json-v1.md` | 100 | 14 |
| `docs/implementation-plan.md` | 25 | 8 |

Всего: 13 файлов, +1146 / −76 строк.

## Независимое ревью Codex, 2026-09-08

Проверена точная база `88e02f2de55b64c52b90a2d05aea4469677c7364`:
`main = origin/main`, все 27 checks успешны. Рассмотрен полный submitted diff,
включая новый process-test module; резервная копия и независимые логи —
`/private/tmp/ferrite-pr21-review/`. Production-код исправлений не потребовал.
Общий Body selector сохраняет порядок и правила UUID/name; inspect использует
один pinned Document. JSON DTO получает только факты завершённого STL job.
Проверено, что валидация положительных deflection не меняет печатаемые значения.

Повторены на текущем diff: fmt, CLI/app build, workspace clippy со всеми
targets/features и `-D warnings`; document 132 passed / 1 старый timing ignored,
jobs 41, CLI 48 (JSON 13), headless app STL 4 и edit 8. Native skips отсутствуют.
Из Markdown заново извлечён и исполнен публичный Python-рецепт до проверки
binary STL, validate, cold rebuild и FBX. Export boundary, notice ownership,
SPDX headers, actionlint и diff whitespace — pass.

Отдельно пересобраны stub CLI/app и повторены create/edit/STL/JSON suites:
9 + 2 + 9 + 13 harness passed, восемь явных native skips. Discovery и
структурированные отказы выполнены; skip не считается проверкой геометрии.
Проверены `OpenCASCADE_DIR-NOTFOUND` в CMakeCache и отсутствие OCCT/PlaneGCS
imports у stub CLI. Окно, GUI, диалоги, Unity и GPU не запускались.

После ревью публикация и CI разрешены пользователем. Результаты GitHub CI
фиксируются отдельно для точного PR head и merge SHA; приведённые выше числа
описывают независимую локальную проверку, а не ещё не выполненный remote CI.
