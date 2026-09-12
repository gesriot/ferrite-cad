# §24J — read-only validate: проверка и передача

2026-09-09, Codex. Изменения **незакоммичены и не staged**. Commit/push/PR/merge
не выполнялись; следующий шаг — независимое ревью. Работа полностью без окон:
viewer/GUI/Unity/GPU/render tests, browser/CUA/osascript и диалоги не запускались.

## Точная база

После `git fetch origin main` чистые main/origin/main остались на
`14ef1168b813ecc42506d10e9371b53f53617c49`; создана ветка
`read-only-json-validation`. [PR #24](https://github.com/gesriot/ferrite-cad/pull/24)
MERGED. Его head `83e83aa599ed4553563e5787bad2ce21805a0349` и merge имеют одинаковое
дерево `ddbfc5c752e5de795238183d59fac4da01137470`.

GitHub API повторно подтвердил **23/23 checks, 5/5 workflow success** точной базы.
[Runtime merge](https://github.com/gesriot/ferrite-cad/actions/runs/34369651073):
в скачанном логе пересчитаны 18 named gates на каждой ОС, всего **54/54** без skips;
strict ufbx на каждой ОС — 256 + 6 + 6 + 256 checks, 0 failures.
Логи: `/private/tmp/ferrite-24j/base-pr.json`, `base-checks.json`, `base-runs.json`,
`base-runtime.log`.

**Это CI базы. CI незакоммиченного diff не выполнен.** Новый ordinary CI step будет
исполнен на Linux/macOS/Windows после независимого ревью и публикации. Существующий
combined runtime workflow и его 54 native gates/strict ufbx не изменены.
Чужой a200 остаётся detached на `5b20719145222befc0bd25647f47ba3d67fee66d`;
его дерево не открывалось и не изменялось. Исходные fixtures не изменены.

## API и wire

[Общий протокол](read-only-validation.md): `validate_document(&Path)` возвращает
`ValidatedDocument { document_id, report: ValidationReport }`. Один read-only
закреплённый снимок, существующий validator, явный close на success/error до
возврата. Путь — весь запрос; живых connection/handles в результате нет.
CLI готовит один адаптер text/JSON, explicit DTO содержит document_id, valid,
errors/warnings (u64), ordered diagnostics с code/severity/message/object_id.
UUID канонические, отсутствующий object_id — null, пустой diagnostics — [].
Нет декоративного content hash, kernel/rebuild, повторного reader или нового
набора validation rules. Исправлен найденный ниже дефект общего read-only guard. Domain structs целиком не сериализуются.

| Исход JSON validate | Envelope | Exit |
| --- | --- | --- |
| Проверка без errors, включая warnings | ok:true, result.valid:true | 0 |
| Проверка с errors | ok:true, result.valid:false | 1 |
| Открытие/чтение/декодирование невозможно | ok:false, прежний error.kind/message/causes | 2 |
| Потеря любого отчёта | Fallible emitter; без повтора и изменения файлов | 7 |

STEP rejection fields к validate не добавлены. Прежние шесть JSON-команд
сохраняют wire и 0/4/5/6/2/7. Text validate сохраняет текущие тексты, порядок и
0/1; намеренно изменён старый writable open: legacy/WAL/minimum reader теперь
дают refusal/2 без миграции или нормализации. Dump-graph/clear-cache не менялись.

## Failing-first и чувствительность gates

До production-изменений собран CLI точной базы и сохранён как
`/private/tmp/ferrite-24j/base-ferritecad`. Текущая fixture plate уже имеет schema 3
и не воспроизвела запись. Поэтому из локальной истории извлечена **существовавшая
fixture schema 2**: `git show 3f938bf:crates/ferritecad-fixtures/plate/plate.fcad`.
На её приватной копии прежний text validate дал exit 0, изменил bytes и mtime:
SHA-256 `358bbd937d09da19d08e9ebdbbcc09137bc4944a9616e263529d59c076c9aa68`
стал `193f582ab26cbb5a95371570b4c33f957ba61c73490df06224fd5be551a30b64`.
Лог `failing-first.log`, исходная историческая копия `legacy-v2.fcad`.
После исправления text/JSON дают 2/needs migration, bytes/mtime/entries неизменны
(`text-parity-and-legacy-after.log`). Ни одна исходная fixture не переписана.

Две временные production-поломки проверены настоящим CLI на stub target:

- CLI снова открывает `Document::open`: точный process test
  `validation_read_only_refusals_preserve_storage` исполняется и падает — получил
  exit 0 вместо 2 на legacy. `mutation-read-only.log`.
- JSON объявляет invalid report valid:true/exit 0: точный process test
  `validation_reports_match_shared_operation_without_writes` исполняется и падает
  — получил 0 вместо 1. `mutation-valid-exit.log`.

Оба случая — runtime assertion, по одному исполненному failed test, не compile
failure/zero tests. Исходники восстановлены в finally; все четыре process tests
повторно passed (`mutations.log`, `mutation-restored.log`). Новой mutation-системы
в репозитории нет. Дополнительно сравнен **реальный базовый и новый text CLI** на
одних bytes/path для valid, invalid и warnings+errors: stdout/stderr/exit совпали,
bytes/mtime не изменились (`text-parity-and-legacy-after.log`).

## Дополнительный дефект сохранности sidecars

Финальный настоящий stub process обнаружил дефект уже существовавшего
`Document::open_read_only`: при DELETE-заголовке и чужих WAL/SHM он дал exit 0
и расширил SHM с **12 до 32768 bytes**, изменив mtime. Это воспроизведено до
исправления на приватных sentinels, `sidecar-failing-first.log`; дополнительный
process suite упал исполняемо (`sidecar-test-before.log`); настоящий CLI до/после
сверен отдельно (`sidecar-failing-first.log`, `sidecar-after.log`).

Общий document guard теперь до SQLite проверяет наличие `-wal`/`-shm` по
запрошенному пути и canonical target (важно для symlink). Он отказывает
unsupported без изменения/удаления sidecars даже при DELETE-заголовке; ошибки
проверки файлов остаются I/O. Добавлены direct document gate для отдельного WAL,
отдельного SHM и Unix symlink, а также process gate для stale WAL+SHM и отдельного
hot rollback journal (I/O refusal, без recovery). Правила validator не менялись.
Это конкретное исправление необходимо для контракта §24J; все read-only клиенты
используют общий guard. Native/stub regression и FBX/ufbx повторены после правки,
а не засчитаны по предыдущей версии reader.

## No-native проверки текущего diff

Свежий target `/private/tmp/ferrite-24j-stub-target`, env
`/private/tmp/ferrite-24j/stub-env.sh`. CMake wrapper исключает Homebrew prefix;
`stub-cmake.txt` содержит `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`.
В `stub-imports.txt` свежего CLI только libiconv/libSystem, без OCCT и PlaneGCS.
PlaneGCS не включён в отдельной default-features stub-сборке. GPU unset.

| Проверка | Исполнено |
| --- | --- |
| Новая CLI suite `--test validate` | **4/4 passed, zero skips/ignored** |
| Новый jobs snapshot/close test | 1/1 passed |
| Новый document metadata/report snapshot test и WAL/SHM guard | 2/2 passed |
| Полные document / jobs tests | 134 / 47 passed; 1 старый timing benchmark ignored |
| CLI unit + create/edit/STL/FBX/import/JSON/shared import/validate и FBX harness | Harness: 75 passed; **31 явный geometry early-return skip**, не доказательство геометрии |
| Точный shell step ordinary CI, извлечённый из workflow | 4 process names + 2 snapshot names + sidecar guard name и counts подтверждены локально |

Все семь новых именованных проверок реально исполнены без ядра. Старые stub
geometry skips: text FBX 7, FBX campaign 2, STL 7, text import 12, JSON import 1,
JSON edit/STL 1, shared native import 1. Другие ветки старых suites проверяют
реальные preflight/unsupported/delivery refusals. Их зелёные early returns не
считаются native gates. Логи `stub-document-jobs.log`, `stub-cli.log`,
`stub-cli.log` (включая финальный validate), `stub-job-validation.log`, `stub-snapshot-final.log`,
`ci-step-execution.log` и `ci-validation*.log`.

Четыре process tests покрывают empty/sample, warnings-only `object.unknown-type`,
повторения codes, invalid readable report с конкретным Extrude UUID и
`reference.missing-edge`/`object.payload-hash-mismatch`, повреждённый embedded
source и unreachable source с null object_id. Unknown dependency role, bad CBOR,
column/envelope mismatch, broken/missing file, legacy schema, future schema/format,
minimum reader, wrong application ID, WAL/stale sidecars/hot journal проверены как operational refusals.
Сверяются общая операция, текст и JSON на одних bytes, messages/order/counts,
исходные bytes/mtime, mtime и полный состав каталога. Чужие cache и WAL/SHM/journal/
other sentinels остаются прежними, missing path не появляется.

Доставка: девять реальных OS pipe комбинаций — valid/invalid/operational refusal ×
stdout closed/stderr closed/both closed. Исправный stdout доставляет отчёт даже
при закрытом stderr (0/1/2); потеря stdout даёт 7, без panic и файловых изменений.
Использован существующий `tests/support/pipe.rs`, не ChildStdin relay.

Permission test требует фактического отказа write-open на readonly file; Unix
дополнительно требует отказа создания в readonly directory и отказа чтения файла
без read permission. Все три отказа ОС исполнены на macOS, затем permissions
приватных файлов восстановлены. Проверка не засчитывает chmod под privileged
пользователем: если write разрешён, test fails с объяснением.

Unicode/кавычки/LF в stored strings, UTF-8 filename с пробелами, help/usage,
`validate -- --json` и `validate --json -- --json` проверены. На macOS не-UTF-8
OS argument отказывает до I/O; APFS не допускает создание такого filename.
Дополнительный Linux case проверяет настоящий не-UTF-8 filename и text policy;
Windows case передаёт непарный UTF-16 surrogate. Эти платформенные ветки здесь
не исполнены; они входят в ordinary CI. Windows readonly directory через Unix
mode не моделируется; там проверяется реальный readonly file write refusal.

## Native регрессия текущего diff

Env `/private/tmp/ferrite-24j/native-env.sh`, target
`/private/tmp/ferrite-24b-native-target`. Vendor OCCT 8.0.1/PlaneGCS подтверждены
CMake и imports (`native-cmake.txt`, `native-imports.txt`); оба require-флага = 1,
GPU unset. CLI/app пересобраны; peer CLI готов до отфильтрованных worker gates.
OCCT source заново не собирался; native inputs/FFI не менялись.

| Проверка | Результат |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| workspace clippy `--all-targets --all-features -- -D warnings` | pass |
| Document / jobs / export | 134 / 47 / 60 passed; 1 прежний timing benchmark ignored |
| CLI unit/create/edit/text FBX/text STL/import/JSON import/JSON v1/shared import/validate | 8 + 9 + 2 + 9 + 9 + 13 + 3 + 13 + 2 + 4 = **72 passed**, no skips |
| Headless app edit / STL workers | 8 / 4 passed, no skips; полный app/GPU suite не запускался |
| Прежняя complete/partial FBX campaign | 3/3 passed, no skips |
| Pinned ufbx 0.23.0 strict | **256 + 6 + 6 + 256 checks**, 0 failures |
| Licence headers / STEP corpus integrity | pass |

Все **18 прежних named native gates этой macOS** реально исполнены: 8 edit,
1 JSON inspect/edit/STL, 1 shared import, 1 JSON import, 3 FBX, 4 STL.
Полный и partial FBX проверены прежним production-маршрутом, включая OS pipes,
с сохранённой геометрией и independent reader. Complex импорт не размножался
ради validate. Linux/Windows native diff локально не запускался.
Логи `native-build.log`, `clippy.log`, `native-libraries.log`, `native-cli.log`,
`native-cli.log` (включая финальный validate), `native-edit-workers.log`, `native-stl-workers.log`,
`native-fbx-campaign.log`. Числа сведены в `counts.json`.

## Публичный рецепт и ограничения

Из [Markdown JSON v1](cli-json-v1.md#рецепт-json-step-import--read-only-validate--fbx)
извлечён блок `sh` в `/private/tmp/ferrite-24j/public-recipe.sh` и исполнен в Bash
с native env и `FCAD_CLI`. Он импортирует **приватную** canonical STEP-копию через
JSON, проверяет IDs/source facts/diagnostics, удаляет только эту копию, вызывает
validate JSON, явно решает по valid/diagnostics, экспортирует JSON FBX и независимо
читает ASCII subset: counts/bytes/vertices/polygon indices, 12 triangles и размеры
60×40×10 mm. Bytes/mtime документа и hash оригинальной STEP fixture сохранены.
Рецепт явно останавливается при validation findings, STEP 4/5, operational 2,
partial FBX 6 и lost report 7. Результат записан в `public-recipe.log`:
`/var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-json-validate-0iiajlik`.

Это проверка сохранённой согласованности без ядра, не доказательство геометрии.
Validate/export читают отдельные snapshots, без общего version guard. UI validate,
repair/migration, новые validation rules, batch/RPC и следующий срез не добавлены.
Runtime dependencies, Cargo.lock, inventories/notices/SBOM inputs не менялись;
генераторы состава не требовались. GUI/Unity/экран не являются условием завершения.

## Файлы и diffstat

Ниже полный diff относительно базы, включая untracked; staging не использован.

| Файл | + | − | Статус |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 25 | 0 | tracked |
| `README.md` | 9 | 2 | tracked |
| `crates/ferritecad-cli/src/json.rs` | 3 | 0 | tracked |
| `crates/ferritecad-cli/src/json/validate.rs` | 45 | 0 | new |
| `crates/ferritecad-cli/src/main.rs` | 33 | 10 | tracked |
| `crates/ferritecad-cli/tests/json_v1.rs` | 1 | 1 | tracked |
| `crates/ferritecad-cli/tests/validate.rs` | 534 | 0 | new |
| `crates/ferritecad-document/src/document.rs` | 42 | 1 | tracked |
| `crates/ferritecad-document/tests/snapshot.rs` | 105 | 0 | tracked |
| `crates/ferritecad-jobs/src/lib.rs` | 3 | 0 | tracked |
| `crates/ferritecad-jobs/src/validate.rs` | 78 | 0 | new |
| `docs/cli-capabilities.md` | 20 | 12 | tracked |
| `docs/cli-json-v1.md` | 101 | 12 | tracked |
| `docs/implementation-plan.md` | 28 | 13 | tracked |
| `docs/read-only-validation-verification.md` | 224 | 0 | new |
| `docs/read-only-validation.md` | 72 | 0 | new |

Всего: **16 файлов, +1323 / −51**. Следующий шаг — независимое ревью.
