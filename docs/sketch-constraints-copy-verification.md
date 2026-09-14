# §25E — протокол реализации и проверки

2026-09-13. Реализация оставлена **unstaged/uncommitted** для независимого ревью.
База: `56737bdf41039ab5c54185b58654d7c6c7e1d43d`, merge PR #30;
ветка `sketch-constraints-copy`. Fetch выполнен; исходные HEAD/main/origin/main
совпадали, дерево было чистым. PR #30 — MERGED, implementation head
`9bfaf025161b90649d410d4e14a63816a73d8ae0`. Чужой detached worktree a200 не менялся.

## База отдельно от текущего diff

На точной базе повторно прочитаны GitHub checks/runs: 27/27 success, все 6 workflow
success. Логи runtime `34785942208` подтверждают 72/72 named native tests, по 24
на Linux/macOS/Windows. Это **CI базы**, не CI этой реализации.
Удалённый CI нового diff не запускался: нет commit, staging, push, PR или merge.
Данные проверки — `/private/tmp/ferrite-25e/base-{pr,checks,runs}.json`,
`base-runtime.log`, `base-native-counts.json`.

## API и запись

[Request, wire types, единицы и executable recipe](sketch-constraints-copy.md).
Document владеет bounded managed H/V + adjacent Coincident policy и narrow writer.
Jobs `edit_sketch_constraints_copy` расширяет существующий copy lifecycle, принимает
owned request (source/expected/Sketch/remove/add/destination), kernel и
OperationContext; возвращает опубликованный путь, сохранённые document/Sketch IDs,
реальные added/removed constraints и report выполненного changed rebuild.
UI/CLI не решают Sketch самостоятельно. Доступность constraint editing независима
от прежней coordinate editing; constrained drag/Snap остаются запрещены.

Первое добавление создаёт недостающие closure IDs, затем H/V; существующие подходящие
Coincident переиспользуются даже с обратным порядком point refs. Удаление именует
exact persisted H/V ID и сохраняет closure. Remove-before-add атомарен: замена H на
V разрешена; duplicate/H+V/foreign IDs/удаление closure отказываются. Public v2→v1
перехода нет: после последнего H/V payload остаётся v2 с Coincident.

Разрешены только schema_version/payload/payload_hash выбранного Sketch и повышение/
добавление `sketch.constraints.v1` в required. Envelope/header/capability согласованы.
Не меняются stored coordinates, остальные object fields/IDs, topology refs, dependencies,
metadata, source bytes/claims, extension rows и rowids. Optional capability rows,
включая optional→required той же capability, сохраняют rowid. Нет изменений DDL,
C++/FFI, native pins или Cargo dependency inputs; inventory/SBOM не требуют обновления.

JSON v1 нового edit: 0 — published; 2 — refusal; 7 — потеря отчёта, файл не
отзывается и операция не повторяется. Явные DTO поверх прежнего fallible emitter;
ошибки остальных команд сохраняют прежний wire. Optional conflict DTO использует
Sketch/constraint/curve UUID, не solver ordinal. Boxing внутреннего error
варианта не меняет JSON и предотвращает раздувание generic envelope на стеке.

## Локальные native проверки текущего diff

Все команды исполнялись последовательно, `CARGO_BUILD_JOBS=1`, tests
`--test-threads=1`, GPU flag unset. Native env:
`/private/tmp/ferrite-25e/native-env.sh`; target
`/private/tmp/ferrite-24b-native-target`; pinned vendor OCCT 8.0.1 и
`vendor/planegcs`. CMake и imports записаны в `native-cmake.log`, `native-imports.log`.
OCCT из исходников не пересобирался. Peer CLI пересобран до app worker gates.

| Проверка | Исполнено на native diff | Лог в `/private/tmp/ferrite-25e/` |
| --- | --- | --- |
| fmt и workspace clippy all-targets/all-features `-D warnings` | pass | `fmt-final.log`, `clippy.log` |
| document + jobs + eval | 368 passed, 1 существующий ignored timing benchmark | `domain-tests.log` |
| CLI binary и 14 process suites: create/edit/Sketch/constraints/JSON/import/shared import/validate/STL/FBX/rebuild/topology | 103 passed, без skips | `cli-regressions.log` |
| Complete JSON FBX + отдельный refusal/protocol route | 1 + 1 passed | `fbx-complete.log`, `fbx-refusals.log` |
| Прежние edit workers | 8/8 | `app-edits__tests__.log` |
| Прежние STL workers | 4/4 | `app-exports__stl__tests__native_.log` |
| Sketch widgets/workers | 4/4 | `app-sketch__tests__.log` |
| Drag/Snap/gesture harness, включая native peer | 17/17 | `app-sketch__drag_tests__.log` |
| Новый H/V widget + native worker/peer | 2/2 | `app-constraints__tests__.log` |
| Headless watchdog | 5/5 | `watchdog-tests.log` |
| Solver ownership / export boundary / licence headers | pass | `solver-ownership.log`, `export-boundary.log`, `license-headers.log` |

Ignored только `edit::tests::measure_extrude_catalog_read` — прежний ручной timing
benchmark, не geometry skip. Native H/V process gate проверяет слегка наклонные H
и V с отрицательными/ненулевыми координатами: после close/reopen реальный cold
rebuild изменяет solved coordinates, даёт ненулевой DOF, удерживает стыки <1e-6 mm
и H/V residual <1e-7 mm. Stored curves остаются прежними. Независимая интеграция
binary STL треугольников сравнивает bounds/volume с measured solved polygon,
не закрепляя произвольные under-constrained положения на разных платформах.

Add → inspect → remove exact UUID → cold reopen исполнены настоящими процессами.
Invalid all-horizontal polygon, дубликаты/H+V, foreign constraint UUID, stale
version, no-clobber, source/hardlink/Unix symlink дают refusal с сохранностью.
Unknown request fields/version/rules, UTF-8 preflight, Unicode/quotes/LF names,
help/usage, `--`/flag-shaped source, global copy refusal против per-feature family
refusal покрыты отдельно. Windows surrogate argument case находится в suite,
локально на macOS не исполнялся; Windows исполнения следуют после публикации.

Прямые OS pipes (`tests/support/pipe.rs`) доказали error JSON с закрытым stderr;
закрытый stdout/оба канала возвращают 7. После успешного H/V publish читаемая
копия с пятью persisted constraints сохраняется. В отказных cases нет новой
копии/scratch; source bytes/mtime сохраняются.

## Фазы, cleanup и направленные поломки

`edit::tests::constraint_copy_refusals_cancellation_races_and_cleanup` использует
существующий fault harness с MockKernel для точной инъекции и реальным PlaneGCS:
отмена до начала, после snapshot, baseline rebuild, закрытого scratch; ошибка
changed geometry/потеря resolved topology ref; destination появился, поздний
source change/alias, исчезнувший payload перед publish. Проверяются kind, source
bytes/mtime, чужое назначение, полный состав directory, 0 live handles. Поздняя
отмена на progress=1 возвращает Published; файл не отзывается. Native geometry
проверена отдельно реальными процессами и UI worker. Лог `job-constraint-faults.log`.

Две временные поломки **собрались и исполнились**:

1. H/V исключены из solver input, сохранены в документе. Named native process
   упал на `H/V was NOT delivered to solver` с исходными наклонными координатами;
   0 passed / 1 failed, exit 101 — `mutation-solver.log`.
2. Narrow writer удаляет attached source claim. Исполненный document gate упал
   на сравнение `imported_source_refs`; 0 passed / 1 failed, exit 101 —
   `mutation-claim.log`.

Свои изменённые строки восстановлены побайтово из локальных backups; evaluator
`solve.rs` не имеет итогового diff. Положительные проверки повторены:
`document-constraints-restored.log`, `cli-constraints-restored.log`, затем полный
набор выше. Новая mutation infrastructure в репозитории не добавлена.

## Настоящий stub

Отдельный target `/private/tmp/ferrite-24j-stub-target`, настройки
`/private/tmp/ferrite-24j/stub-env.sh`. Пересобраны CLI/app с текущим diff.
CMake до/после: `OpenCASCADE_DIR-NOTFOUND`, `CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew`.
`otool -L` обоих свежих бинарников не содержит OCCT/PlaneGCS:
`stub-cmake.log`, `stub-build.log`, `stub-imports.log`.

CLI suites: 31 reported passed = **25 исполненных + 6 явных geometry skips**.
Document constraint writer: 3/3 без skips. UI constraints: **1 исполненный + 1 native
skip**. Всего 29 исполненных, 7 skips. Имена skips полностью записаны в
`stub-cli.log`, `stub-ui.log`; они не засчитаны как native geometry. Discovery,
protocol/preflight/delivery и сохранность работают без ядра; supported H/V edit
в stub даёт structured unsupported refusal, не geometric success.

## CI wiring и независимый reader

Сохранены старые 72 named native executions, validation/watchdog/gesture/Snap
и complete/partial FBX campaign. В существующий combined runtime добавлены 4
exact-name/no-skip gates на каждой ОС (ожидается 84 native executions), CLI
пересобирается до worker. В обычном CI явно исполняются три kernel-free process
имени и H/V draft test. YAML разобран Ruby/Psych, shell syntax проверен; это
проверка конфигурации, не удалённый CI.

Pinned ufbx 0.23.0, commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`, SHA256
исходников проверен штатным fetch helper. Текущий `read_production.c` скомпилирован
отдельно; H и V CLI artifacts прочитаны strict reader: **6 + 6 checks, 0 failures**.
Новый hook `check-fbx-complex.sh` также исполнен на этих публикациях и выдал
`FCAD_SKETCH_CONSTRAINT_UFBX_EXECUTED` (`reader-hook.log`). В CI он переиспользует
reader существующей кампании; новый complex import ради H/V не добавлен.

Полная большая partial-FBX campaign локально не повторялась: её boundary не
менялся. `export_fbx_identity` случайно попал в первый общий список; этот
ненужный complex test остановлен (SIGTERM моего PID 55603) и **не засчитан**.
Прерванный лог сохранён как `cli-regressions-interrupted.log`; весь нужный CLI
набор затем прошёл отдельным законченным запуском. Иных процессов это не касалось.

## Реальный GUI smoke и память

Свежий цельный bundle `/private/tmp/ferrite-25e/gui/FerriteCAD.app`: 51 private
library (50 OCCT + PlaneGCS), статически проверено замыкание всех @rpath targets.
`codesign --verify --deep --strict` pass. Native/staged viewer имеют одинаковый UUID
`5D5C1804-E9C3-3868-A028-9D9225DAB2E1` (`signature.log`, `uuids.log`, `stage.log`).
Loader failure probes/`FCAD_ALLOW_LOADER_FAILURE_PROBES` не включались.

Один owned PID 71472 был запущен под watchdog cap1536 MiB **до CUA getApp**.
На реальной сцене выбраны Top и Edit H/V, Segment 1 `(-20,-10) → (40,-8)`;
добавлен Horizontal. Save Cancel оставил selection и pending H. Повторный Save
опубликовал `gui-horizontal.fcad`; обычный async Open показал выровненную нижнюю
сторону, Under-constrained / 7 DOF / no redundant. Из этой копии удалён exact H
`01a09d13-8061-7671-9148-d7f93d82a334`, четыре Coincident остались. После Save
`gui-removed.fcad` открыт с 8 DOF и освобождённым условием H. Viewer закрыт штатной
кнопкой Close; после Close CUA/AX/screenshot больше не вызывались.

Watchdog `gui-memory.jsonl`: **peak 189.72 MiB**, pressure=1, swap
3235119104 → 3235119104 bytes, exit0 / aborted=false, 149.15 s. PID отсутствует,
подтверждён shell. Это наблюдение малого профиля; причина прежнего OOM не установлена.
Headless watchdog tests сначала отказали в sandbox на sysctl pressure, затем
5/5 прошли с доступом к измерениям. Первый GUI launcher остановился до запуска
viewer из-за совпавшего имени stdout-лога; исправлено только имя evidence-файла,
без обхода memory guard. Ни один отказной launcher не считается GUI smoke.

**Проверены сами GUI-файлы**, а не только тестовая модель. Peer CLI на тех же
source bytes добавил H с другими новыми UUID: rules/point refs явно сопоставлены,
все прочие SQL-ячейки/rowid/metadata/IDs совпали; отличались лишь mapped IDs и
payload hash. Удаление того же H из GUI-копии дало побайтово равные SQL cells
GUI/CLI, closure IDs остались. Source SHA256 и mtime неизменны. Validate и cold
rebuild прошли для всех четырёх файлов; STL и FBX побайтово равны в каждой паре.

Независимый STL: bounds `[-20,-10,0]…[42,30,10]` mm; H — **24400 mm³**, removed —
**23780 mm³**, в GUI и CLI. Это измерения текущего запуска, не platform-wide
обещание положения under-constrained solution. Все четыре GUI/peer FBX strict
ufbx прочитал по 6 checks, 0 failures. `gui-evidence/verification.json`,
`gui-verification.log`, `gui-evidence/*-ufbx.log`, `gui-evidence/memory.json`.

## Публичный рецепт и передача

Python-блок `FCAD_25E_AGENT_RECIPE` извлечён непосредственно из опубликованного
Markdown и выполнен настоящим свежим CLI. Он обнаружил UUID/version, добавил H,
проверил validation и сохранённые facts, независимо разобрал STL, прочёл FBX
pinned reader и удалил exact H по новой версии. Результат: 7 DOF, 24400 mm³,
strict ufbx 6/6; `agent-recipe.log`. Во время проверки исправлен сам рецепт:
перемещение minimum Y не обязательно для under-constrained solve, поэтому он
проверяет реально выровненную boundary edge STL, а не произвольное положение.

Файлы реализации: document `sketch_constraints.rs`, narrow writer/exported types
и общий discovery; jobs `edit.rs`; CLI `edit_constraints.rs`, `json/constraints.rs`
и common emitter; app `constraints.rs`, worker/Save/Open adapters и test helpers.
Добавлены document/process/UI/phase gates; обновлены JSON v1, capability map,
README, план и этот протокол; два существующих workflow и reader hook расширены.
Полный финальный список и diffstat **включая untracked**:
`/private/tmp/ferrite-25e/final-diffstat.txt`, `final-status.txt`.

Ограничения: только собственный XY Line polygon 3..256, один literal positive
Blind/NewBody; H/V + adjacent Coincident. Нет arbitrary families, dimensions,
топологического редактирования, live solver на pointer move, in-place Save или
persistent undo. Отдельные inspect/edit/export не являются одним длительным
снимком; version guard есть у copy edit. GUI/Unity/GPU suites сверх описанного
малого guarded smoke не запускались. Linux/Windows CI текущего diff ожидает ревью
и публикации. **Следующий шаг — независимое ревью; ничего не опубликовано.**


## Независимое ревью §25E, 2026-09-13

Ревью обнаружило потерю Save/Clear за нижней границей окна при длинном списке
pending additions. Исполняемый egui-тест на 32 добавлениях сначала упал по
видимости Save (1 тест, assertion failure), затем прошёл после ограничения
высоты списка отдельным ScrollArea. Финальный тест при viewport 988×768 проверяет
пустой и уже constrained 64-Line каталог, реальные clip rect и клики Save/Clear.
Это проверка виджетов, а не заявление о ручных 32 кликах в GUI.

Повторены fmt, workspace clippy all targets/features с -D warnings, свежие CLI/app,
license headers, solver/export boundaries, actionlint, shellcheck и diff --check.
Document/jobs/eval/UI: 470 harness passes, из них 468 исполненных и два прежних
stub-only контроля с явным skip в native; один прежний timing test ignored.
Оба stub-only контроля затем исполнены в настоящем stub (по одному passed).
Native CLI: 103 passed без skips; app: constraints 3, sketch/drag 21, creates 17,
edit 8, STL 4, без skips. Финальный constraints suite повторён после расширения
регрессии на уже constrained каталог. Native campaign под watchdog: sampled peak
581.41 MiB, exit 0, pressure normal. CARGO_BUILD_JOBS=1, тесты последовательно.

Отдельный свежий stub CLI/app подтверждён imports без OCCT/PlaneGCS: 32 реально
исполненных негeометрических контроля, 7 отдельно учтённых native skips. Пять
тестов watchdog прошли после закрытия viewer. Ни skip, ни прерванный тест не
засчитаны как геометрия. Большой partial STEP/FBX и полный GPU локально не
повторялись; native libraries и loader-failure probes не пересобирались/не запускались.

Свежий подписанный review bundle имеет UUID
`1F37A90B-B3AD-3E16-B456-9809F3CA20E0`, равный текущему debug viewer.
Один PID 18640 запущен под watchdog до подключения CUA. В настоящем окне:
Save Cancel сохранил selected Line и pending H; повторный Save опубликовал
`gui-horizontal.fcad` и async Open показал 7 DOF. Удаление exact H
`01a09d30-d923-7bb2-a525-2e9c7b45598f` опубликовало `gui-removed.fcad`, 8 DOF,
четыре прежних Coincident сохранены. После штатного Close CUA не вызывался,
отсутствие PID и exit 0 подтверждены shell/watchdog.

Memory: sampled peak **231.08 MiB** при cap 1536 MiB, pressure=1,
swap initial/max 3218341888 bytes, final 3209953280 bytes, min free disk 41.57 GiB,
146.34 s, aborted=false. Это малый профиль, причина прежнего OOM не установлена.

GUI-публикации независимо сопоставлены с новым CLI: при добавлении явно
сопоставлены только пять новых constraint UUID и производный hash; остальные
SQL cells/rowid/IDs совпали. Удаление из того же GUI-source по exact UUID дало
одинаковые SQL cells. Исходник сохранил SHA256 и mtime. Validate/cold rebuild
прошли, STL/FBX в каждой GUI/CLI паре равны побайтово. Независимый STL подтвердил
bounds [-20,-10,0]…[42,30,10] mm, H 24400 mm³, removed 23780 mm³.
Текущий strict reader пересобран из pinned ufbx; четыре GUI/CLI FBX, два H/V
native artifacts и публичный рецепт прошли по 6 checks без failures.
Рецепт извлечён непосредственно из Markdown и исполнен свежим bundled CLI.

Логи, failing-first/positive evidence и GUI SQL/mesh comparisons:
`/private/tmp/ferrite-pr31-review/`. Linux/Windows и CI опубликованного head/merge
учитываются отдельно после публикации; результаты выше относятся к локальному diff.
