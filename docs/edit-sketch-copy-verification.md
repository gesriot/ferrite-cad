# §25B — проверка координатной правки сохранённого Sketch

Дата: 2026-09-13. Ветка `edit-sketch-copy`, база/HEAD
`86e2c07996b2255d975ad80dc15874e65b34a4a8`. На момент передачи реализации все изменения были
**незакоммиченными и unstaged**. Результаты последующего независимого ревью ниже.

## База и границы проверки

В начале выполнен fetch: `main = origin/main` на указанном SHA, дерево чистое.
PR #27 — MERGED; head `ad9cffa4a0e47d6c7085374bb8b1d3a8ef33de59` и merge имеют
дерево `f22326090036f3340c4c134afffb3f7715f943a1`. Проверены 27/27 success checks,
6/6 success workflow; [runtime merge](https://github.com/gesriot/ferrite-cad/actions/runs/34750338963)
содержит 60/60 required native executions (20 точных имён на трёх ОС).
Аудит: `/private/tmp/ferrite-25b/base-runtime.log`, `base-gates.json`.
Это CI **базы**. CI нового незакоммиченного diff не запускался.

AGENTS.md в репозитории и родительских каталогах не найден. Чужой detached
worktree a200 не изменялся; исходные fixtures не редактировались. Commit, staging,
push, PR, merge и следующий срез не выполнялись.

## API и сохранённые факты

[Точный протокол и публичный рецепт](edit-sketch-copy.md) описывают:

- `inspect --json` v1: additive ordered `result.sketches`, UUID Sketch, точное
  имя/null, vertices с persisted curve UUID и start_mm/null, local refusal и
  отдельный document refusal. Metadata, каталоги и версия — один read-only snapshot.
- `edit-sketch-copy <source> --sketch UUID --expect-version TOKEN --request JSON -o COPY [--json]`.
  Request v1 содержит **все** curve IDs в сохранённом порядке и новые starts;
  каждый start замыкает end предыдущего Line. Нет новых vertex IDs.
- Общие document eligibility/PolygonExtrusion, `EditSketchRequest` и
  `edit_sketch_copy` в jobs; UI/CLI используют один copy/rebuild/publish маршрут.
  JSON сохраняет envelope v1: published 0, refused 2, lost report 7.
- Поддержаны один root XY Line Sketch, один положительный literal Blind/NewBody,
  одна Body и ровно три ожидаемые зависимости. Constraints, extra Body, construction,
  формулы, кривые, смена winding/числа/порядка сегментов явно отказываются.
- Меняются только payload выбранного Sketch и его hash. Document/object/curve/ref
  UUID, names/ordinals/parents, plane/height/extent, dependencies, topology refs,
  metadata (включая modified_at), другие SQL-таблицы и capability rowids сохранены.
  Неизвестные поля выбранного payload отказываются до потери при decode/encode.
  Физическая компоновка и служебные change counters SQLite не являются контрактом.

`Document::write_sketch_coordinates` — узкая операция, повторно проверяющая тот же
класс правки; она пользуется существующей transaction и сохраняет metadata,
capabilities и reachability. Обычный `Document::write` и edit-extrude сохраняют
свой прежний путь. Схема, C++/FFI, зависимости, Cargo.lock и inventories не менялись.

## Исполненные проверки окончательного кода

Все Cargo-сборки с `CARGO_BUILD_JOBS=1`, тесты с `--test-threads=1`.
Логи ниже находятся в `/private/tmp/ferrite-25b/`.

| Проверка | Результат | Лог |
|---|---|---|
| `cargo fmt --all -- --check` | pass | `fmt-final.log` |
| workspace clippy all targets/all features, `-D warnings` | pass | `clippy-final.log` |
| document + jobs, all features | 186 passed; 1 existing ignored timing test | `document-jobs-final.log` |
| CLI create/edit-extrude/import/shared-import/JSON-import/JSON-v1/validate/STL/FBX/create-sketch/edit-sketch | 73 passed, без geometry skips | `cli-regression-final.log` |
| JSON FBX complete native + imported, 8 delivery cases | exact gate passed | `fbx-complete-final.log` |
| JSON FBX refusals | exact gate passed | `fbx-refusals-final.log` |
| Fresh CLI + app build | pass | `build-final.log` |
| Headless old edit workers | 8 passed | `ui-edit-final.log` |
| Headless old native STL workers | 4 passed; дополнительный stub-only test вернулся рано и не засчитан native | `ui-stl-final.log` |
| Headless Sketch widgets/create/edit worker + peer CLI | 4 passed, включая обе native routes | `ui-sketch-final.log` |
| Headless create state machine / native workers | 17 passed | `ui-create-final.log` |
| Dialog adapter | 3 passed; ожидаемые caught panics | `ui-dialog-final.log` |
| macOS watchdog | 5 passed | `memory-guard-tests.log` |
| actionlint / licence headers / export boundary / diff whitespace | pass | `actionlint.log`, `headers.log`, `export-boundary.log` |

Единственный ignored document test — `edit::tests::measure_extrude_catalog_read`,
прежний ручной timing experiment, не geometry gate. Отфильтрованные doc-tests/
solver-info с нулём tests не считаются отдельными доказательствами.

Native env: `/private/tmp/ferrite-25b/native-env.sh`, target
`/private/tmp/ferrite-24b-native-target`; vendor OCCT 8.0.1 и vendor/planegcs.
Реальные imports/closure: 50 OCCT toolkits + planegcs; CMake использует vendor
OpenCASCADE. GPU requirement unset. OCCT из исходников не пересобирался.

Отдельная свежая stub CLI/app сборка: `/private/tmp/ferrite-24j/stub-env.sh`, target
`/private/tmp/ferrite-24j-stub-target`. CMake `OpenCASCADE_DIR-NOTFOUND`, CLI imports
только `/usr/lib/libiconv.2.dylib` и `/usr/lib/libSystem.B.dylib`; PlaneGCS не linked.
Факты: `stub-build-final.log`, `stub-cmake-final.log`, `stub-imports-final.log`.
`stub-cli-final.log`: 22 test functions reported passed, из них **19 выполненных
негеометрических проверок**, 3 явных geometry skips: две новые native Sketch
проверки и прежний `native_json_inspect_edit_contract`. Jobs edit mock suite:
10 passed (`stub-jobs-final.log`); отдельный stub STL worker: 1 executed/passed
(`stub-stl-worker-final.log`). Stub refusal не засчитан как успешная геометрия.

Тяжёлая partial STEP/FBX кампания локально не повторялась: её импорт, writer,
identity/omission граница не менялись. Прежние complete/partial FBX и strict ufbx
проверки сохранены в runtime workflow. Linux/Windows проверки нового diff
предстоят после ревью/публикации; локально проверена macOS.

## Геометрия, identities, отказы и мутации

`native_edit_sketch_process_identity_geometry_and_delivery` исполняет реальные
CLI-процессы: L 60→80, discovery/version/curve IDs → edit → inspect/validate/cold
rebuild → STL/FBX. Независимый binary STL parser подтвердил bounds 80×40×10,
volume 20000 mm³ и 20 triangles. Допуск объёма 0.02 mm³ (1 ppm); не принимается
convex hull вместо L. Три прежних refs разрешаются; tessellation найденных faces
проверяет cap z=0, cap z=10 и side y=0 от того же persisted первого сегмента,
до x=80 и z=10. Проверены все UUID, SQL metadata/deps/refs, extension blob/rowid,
optional capability/rowid, source bytes/mtime и отсутствие scratch.

`native_sketch_edit_refusals_preserve_source_and_destinations` проверяет stale,
missing/duplicate/foreign/reordered curve IDs, crossing/winding, unsupported,
no-clobber, source/hardlink/Unix symlink. Имена/ordinal могут измениться без смены
ID; со свежей версией выбор остаётся по UUID. Остальные три CLI tests покрывают
read-only snapshot при uncommitted внешнем writer без sleeps, common refusal,
пустой каталог, Unicode/quotes/LF, JSON escaping, help/usage/`--`, OS non-UTF paths.
Windows-specific non-UTF-16 case добавлен, локально на macOS не исполнялся.

Jobs tests проверяют отказ/отмену до работы и на фазах .1/.4/.95, позднюю отмену
на 1.0, изменение source, появившийся output, late alias, publication
refusal, отказ rebuild и потерянный ref. Нет публикации до успешного cold check;
после publish поздняя отмена возвращает успех. Live handles равны нулю **до**
разрушения test kernel на успехе/ошибке/отмене; проверка не полагается на destructor.
В финальном аудите тесты усилены: точные ErrorKind и полный состав каталога
для фазовых отказов; publication I/O failure инъецируется удалением только своего
закрытого scratch payload. `cleanup-strengthened.log`, `cli-cleanup-final.log`,
`stub-cleanup-final.log`; production после GUI smoke не менялся.
Реальные закрытые OS pipes дают 7, опубликованные FCAD остаются читаемыми;
execution error доставляется через stdout при закрытом stderr. Оба закрытых
канала не вызывают panic. Штатный общий emitter не дублировался.

Две направленные временные поломки повторены после финального storage fix:

1. Перегенерация второго curve UUID: native identity/geometry gate выполнил один
   test и упал на проверке UUID (`mutant-curve-id.log`).
2. Выключение `require_version`: native refusal gate выполнил один test и упал
   на stale exit (`mutant-stale-version.log`).

Это исполняемые assertion failures, не compile failure/zero tests. Исходники
восстановлены через finally, затем повторены положительные suites и clippy.
Скрипт локального эксперимента: `mutate.py`, итог `mutation-summary.log`.

Дополнительный failing-first: реальный CLI первоначально пересобирал capabilities,
терял optional row 999 и менял rowid 101→1 (`capabilities-proof/before-fix.json`).
Узкая координатная запись исправила это; в native process gate добавлена проверка
точных rows `[101, core.part.v1, 1]` и `[999, future.optional, 0]`.

## GUI, артефакты и память

Первый GUI-проход проверил точные поля, видимый контур, Undo/Redo, Save Cancel,
публикацию/async Open, видимый отказ вырожденной геометрии и отмену черновика.
Он был до последнего storage fix; UI-код после него не менялся.
`gui-watch.jsonl`: peak 198.345 MiB, normal pressure, неизменный swap, exit 0.

После исправления capabilities пересобраны CLI/app и новый bundle
`/private/tmp/ferrite-25b/gui-current/FerriteCAD.app`. Runtime closure и staging
выполнены штатными scripts. `package-current-facts.txt`: 50 toolkits, ad-hoc
signature, cold rebuild и solver-info из staged libraries; отрицательные проверки
с отсутствующей toolkit/planegcs прошли. Build-каталоги скрывались только sandbox
дочернего probe, глобальные permissions не менялись.

Финальный GUI smoke: приватный исходник с дополнительной capability 999; явный
Sketch → два x 60→80 → системный Save → видимое открытие `gui.fcad` → Close.
Ровно один viewer под watchdog, PID 23576, cap 1536 MiB. Peak footprint
**201.361 MiB**, pressure `[1]`, swap 3797155840 bytes до/после без роста,
92.717 s, exit 0, aborted false. После Close CUA не читала app; выход проверен
через PID и guard log. `gui-current-run.jsonl`, `gui-current-memory.json`.
Предшествующая попытка guard завершилась **до Popen** из-за занятого имени его
stdout-лога; это не pass и viewer тогда не запускался (`gui-current-watch.*`).

`gui-current-verification.json`: GUI и peer CLI на одном source совпали по всем
SQL cells, UUID, metadata, deps/refs и capability rowids; FCAD также побайтово
совпали (SHA256 `412f758948f5979eb7306c655a86c06e76d886357ad01b9f96306e925170ea6a`).
Source bytes/mtime неизменны. Validate/cold rebuild успешны; STL/FBX побайтово
равны артефактам публичного рецепта. Pinned strict ufbx 0.23.0
(commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`) прочитал FBX: ASCII 7400,
1 mesh / 1 model, 0 warnings. Unity не запускалась; Linux/Windows GUI не заявляется.

Отдельный окончательный CLI замер (`edit-time-current.txt`): 0.07 s real,
0.05 s user, 0.01 s sys, max RSS 26656768 bytes (~25.42 MiB), peak footprint
3506632 bytes (~3.34 MiB), 0 swaps. Это один малый прогон, не performance threshold.
Причина пользовательского OOM остаётся неизвестной.

## Воспроизводимый рецепт и следующий шаг

Python-блок из [Markdown-протокола](edit-sketch-copy.md#исполняемый-рецепт)
извлечён и выполнен настоящим CLI: `recipe-final.py`, `recipe-final.log`,
артефакты `recipe-final/`. Для повтора экспортируйте `FERRITECAD` (native CLI),
`UFBX_READER` (pinned read_fixture) и **новый** приватный `RECIPE_DIR`, затем
исполните этот блок. IDs/version извлекаются из JSON; CLI prose не разбирается.
Рецепт явно останавливается на invalid/diagnostics/partial/refusal/lost report.
Отдельные inspect/validate/export не имеют общего snapshot; edit сравнивает
expected version со скопированным снимком и снова перед publish.

Runtime workflow сохраняет 60 прежних native, 21 validation execution и 5 guard
checks. Добавлены три точных anti-skip имени на каждой ОС: два новых CLI gate и
`sketch::tests::native_saved_sketch_draft_and_cli_preserve_same_model` — всего
**69 планируемых required native executions**, ещё не выполненных удалённым CI
на этом diff. Нет новых workflow, GUI/GPU CI, dependencies или mutation framework.

Ограничения: coordinate-only, без in-place Save/force, без смены topology/order/
winding, constraints, imported editing и persistent undo. Существующий blocking
native вызов не получил мгновенной отмены; token проверяется между фазами.
Последняя проверка source не является блокировкой против произвольного внешнего
процесса. §9/§10 не объявлены завершёнными. Дальше — независимое ревью этого diff.

## Независимое ревью и сообщения macOS

Ревью воспроизвело дополнительный отказ на координатной правке Sketch с сохранённой
source-связью: `put_object` удалял `imported_source_refs` во временной копии,
последующий cold rebuild отказывал `imported-source.unreachable`. Исходник не
менялся, неполная копия не публиковалась. Узкая запись теперь обновляет ровно
`objects.payload` и `payload_hash`, сохраняя ownership. Существующий native process
gate усилен прикреплёнными source bytes/связью: сначала один исполненный test упал
(exit 101, execution refusal 2), после исправления прошёл, включая delivery cases.
Логи независимого ревью: `/private/tmp/ferrite-pr28-review/`.

Последние crash reports macOS сопоставлены с `package-current.log`: 2026-09-13
04:06:15 PDT, viewer PID 23247, UUID `BD2D2B8B-550B-37BB-B568-896E359D68B0`,
`DYLD / Library missing / @rpath/libplanegcs.dylib`; CLI PID 23460 — отсутствующая
`libTKBO.8.0.dylib`. Тот же PID, UUID и путь есть в намеренном hidden-library probe.
Предыдущая пара 03:47 соответствует `gui-final`. Это тестовые отказы до Rust main,
не доказательство несовместимости macOS и не OOM. Библиотеки восстановлены;
существующий bundle прошёл `codesign --verify --deep --strict` без запуска.

Полные macOS gates `check-staged-layout.sh` и `check-release-package.sh` теперь
отказываются до запуска/изменения файлов без явного
`FCAD_ALLOW_LOADER_FAILURE_PROBES=1`. CI задаёт его на runner; обязательные
отрицательные проверки не пропускаются. Локальный README использует
`--no-execute` и не выдаёт structural facts за runtime proof. Новый лёгкий
`check-runtime-probe-opt-in.sh` проверяет точное значение, обе входные точки,
сохранность прежних facts и отсутствие запуска; включён в существующий workflow.
Настройки crash reporter, безопасность macOS и отчёты Apple не менялись.


После исправления повторены fmt, workspace clippy all targets/features `-D warnings`,
466 document/eval/jobs/UI tests (1 прежний timing ignored), 68 CLI tests,
4 sketch + 17 create + 8 edit + 4 native STL headless tests, без native skips.
Serial native watchdog: 108.37 s, peak 564.10 MiB, pressure 1, swap без роста, exit 0.
Публичный JSON-рецепт независимо выполнен: 80×40×10, volume 20000 mm³, 20 STL
triangles, strict ufbx 0.23.0 — 1 mesh/model, 0 warnings. Source-claim regression
входит в старое exact gate name и будет исполнен на трёх ОС.

Новый staged bundle ревью: `/private/tmp/ferrite-pr28-review/gui/FerriteCAD.app`,
UUID `16816181-EE74-39DA-A0BC-9D7F610A6841`, совпадает со свежим native viewer.
Сборка/closure/staging и ad-hoc signature проверены без hidden-library запусков.
Реальный macOS GUI: два поля X 60→80, Undo/Redo, Save Cancel с сохранением
черновика, публикация `Review GUI.fcad`, async Open и видимая L-модель в Iso.
GUI/CLI FCAD совпали побайтово и по SQL; STL/FBX тоже совпали; GUI-файл отдельно
прошёл validate/cold rebuild и strict ufbx. Watchdog PID 70331: 119.72 s,
peak 186.00 MiB, pressure 1, swap без роста, exit 0. После Close приложение через
CUA не опрашивалось; shell подтвердил отсутствие viewer. Новые deliberate crash
probes локально не запускались; причина первоначального пользовательского OOM
этим не устанавливается. Linux/Windows GUI не проверялись.

Повторный stub: текущий CLI imports содержит только libiconv/libSystem; suites
edit-sketch/create-sketch/JSON-v1 — 22 harness passes, из них 3 явных geometry
skips, 19 выполненных проверок; jobs edit — 10/10 без skips. Native экспортом эти
skips не считаются (`stub-cli.log`, `stub-jobs.log`, `stub-imports.txt`).

Shellcheck с разрешением sourced scripts (`-x`), actionlint и новый opt-in gate
прошли. Удалённый CI на этом review diff проверяется после публикации, отдельно
от приведённой выше базы и локальных прогонов.

## Diffstat исходной передачи, включая тогдашние untracked

<!-- DIFFSTAT -->

| Файл | + | − | Статус |
|---|---:|---:|---|
| `.github/workflows/runtime-layout.yml` | 29 | 0 | tracked |
| `README.md` | 11 | 2 | tracked |
| `crates/ferritecad-app/src/edits.rs` | 67 | 5 | tracked |
| `crates/ferritecad-app/src/main.rs` | 55 | 1 | tracked |
| `crates/ferritecad-app/src/sketch.rs` | 383 | 40 | tracked |
| `crates/ferritecad-cli/src/edit_sketch.rs` | 119 | 0 | untracked |
| `crates/ferritecad-cli/src/json.rs` | 36 | 0 | tracked |
| `crates/ferritecad-cli/src/main.rs` | 4 | 0 | tracked |
| `crates/ferritecad-cli/tests/edit_sketch.rs` | 796 | 0 | untracked |
| `crates/ferritecad-document/src/document.rs` | 41 | 7 | tracked |
| `crates/ferritecad-document/src/edit.rs` | 8 | 4 | tracked |
| `crates/ferritecad-document/src/lib.rs` | 4 | 0 | tracked |
| `crates/ferritecad-document/src/polygon.rs` | 152 | 0 | untracked |
| `crates/ferritecad-document/src/sketch_edit.rs` | 305 | 0 | untracked |
| `crates/ferritecad-jobs/src/edit.rs` | 358 | 32 | tracked |
| `crates/ferritecad-jobs/src/lib.rs` | 4 | 1 | tracked |
| `crates/ferritecad-jobs/src/polygon.rs` | 2 | 151 | tracked |
| `docs/cli-capabilities.md` | 15 | 2 | tracked |
| `docs/cli-json-v1.md` | 14 | 3 | tracked |
| `docs/edit-sketch-copy-verification.md` | 231 | 0 | untracked |
| `docs/edit-sketch-copy.md` | 246 | 0 | untracked |
| `docs/implementation-plan.md` | 17 | 2 | tracked |
| `docs/sketch-extrude-create.md` | 2 | 1 | tracked |

Всего: **23 files, +2899 / −251**, включая этот отчёт и все untracked файлы.
