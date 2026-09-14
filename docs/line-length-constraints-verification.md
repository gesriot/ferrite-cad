# §25F — реализация и локальная проверка

2026-09-13. База `50671f22c7ed0dcaf0d1e9e4e233a9282c013e7c`, PR #32 MERGED;
после fetch HEAD/main/origin/main совпадали, дерево было чистым. Ветка
`line-length-constraints`. Повторно проверены GitHub metadata: **27/27 checks,
6/6 workflow success** точной базы, runtime `34800229197`. Это CI базы;
CI нового незакоммиченного diff не запускался. Аудит: `/private/tmp/ferrite-25f/base-*.json`.
Чужой a200, исходные fixtures и чужие изменения не затрагивались.

## Предметный результат

[Точный контракт](sketch-constraints-copy.md), [исполняемый рецепт](line-length-constraints.md).
Document расширяет прежний managed класс на положительный Distance между
Start/End одного Line. Одна ориентация H/V и одна длина могут сосуществовать;
дубликаты и arbitrary point-to-point Distance отказываются. `LineLengthMm`
с private validated bits допускает точное Eq без NaN/zero. Ни parsing, ни typing
не помещают непроверенный float в request/history. Runtime dependencies не добавлены.

CLI request v1: `{curve_id,rule:"distance",distance_mm:N}`, H/V формы прежние.
Лишние/отсутствующие поля и неверные типы отказываются. Response сохраняет
`distance`, operation/schema v1 и exit 0/2/7. Никакой новой сериализации domain,
второго import/rebuild/reader ради DTO или нового предметного маршрута нет.
Прежний jobs copy/solve/close/Keep и narrow writer не изменены; в jobs изменён
только существующий fault-test request, теперь с H и длиной.

UI `Edit constraints`: поле mm и Add length, stored value/UUID и exact Remove.
Неприменённый текст и selection не входят в Undo; успешное применение — один
checkpoint. Restore очищает старую локальную ошибку, Save посылает восстановленный
request. Clear, bounded 128 Undo/Redo, running и Save/worker lifecycle сохранены.
Coordinate drag/Snap constrained Sketch по-прежнему запрещены.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`, GPU unset,
loader-failure probes unset. Проверенные env/target:
`/private/tmp/ferrite-25f/native-env.sh`, `/private/tmp/ferrite-24b-native-target`.
Vendor OCCT 8.0.1 и PlaneGCS повторно подтверждены CMake/imports/closure;
OCCT из исходников не пересобирался. Peer CLI собран перед app worker gates.

Команды записаны в `/private/tmp/ferrite-25f/native-tests.sh`:

```bash
source /private/tmp/ferrite-25f/native-env.sh
cargo fmt --all -- --check
cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
cargo test --offline -p ferritecad-document -p ferritecad-jobs -p ferritecad-eval --all-features -- --nocapture --test-threads=1
cargo build --offline -p ferritecad-cli -p ferritecad-app --all-features
cargo test --offline -p ferritecad-cli --all-features --bin ferritecad --test edit_constraints --test edit_sketch --test sketch_extrude --test json_v1 --test edit_extrude --test create -- --nocapture --test-threads=1
for suite in constraints::tests:: sketch:: edits::tests:: exports::stl::tests::native_; do
  cargo test --offline -p ferritecad-app --all-features --bin ferritecad-viewer "$suite" -- --nocapture --test-threads=1
done
```

| Проверка | Результат | Лог в `/private/tmp/ferrite-25f/` |
| --- | --- | --- |
| fmt, workspace clippy all-targets/all-features `-D warnings` | pass | `fmt-final.log`, `clippy-final.log` |
| document/jobs/eval | 370 harness passes = **367 исполненных + 3 stub-only skips**; 1 прежний ignored timing benchmark | `domain-tests.log` |
| CLI binary/create/edit/Sketch/JSON/constraints | **47 passed**, без skips | `cli-tests.log` |
| Финальный CLI constraints suite | **6 passed**, без skips | `cli-constraints-final.log` |
| App constraints/history/length | **8 passed**, без skips | `ui-final.log` |
| App Sketch/gesture/Snap | **21 passed** | `app-sketch__.log` |
| Прежние edit workers/guards | **8 passed** | `app-edits__tests__.log` |
| Прежние native STL workers | **4 passed** | `app-exports__stl__tests__native_.log` |
| Headless watchdog | **5 passed** | `watchdog-tests.log` |
| actionlint, shellcheck, licence headers, solver/export ownership, diff check | pass | одноимённые логи |

Критические named gates: `native_line_length_process_geometry_replacement_conflict_and_delivery`,
`constraints::tests::native_line_length_worker_and_cli_preserve_model_and_solved_body`,
`constraints::tests::line_length_widgets_validate_input_without_changing_history_until_apply`,
`line_length_is_checked_and_replacement_preserves_other_constraints`,
`arbitrary_distance_and_duplicate_lengths_refuse_whole_document`.
Полные исполненные имена всех старых и новых tests находятся в логах.

## Geometry, identity, refusal и delivery

Реальные CLI create (явный stored 80×40×10) → H/V четырёх сторон + длины 60/30 →
cold reopen дают размеры 60×30×10 mm, **DOF 2**, без redundant constraints.
Независимый разбор STL проверяет bounds extents, triangle count/bytes и интеграл
объёма ≈18000 mm³; translation не фиксируется. Отдельный slanted Line получает
50 mm: проверен `hypot(dx,dy)`, обе проекции отличаются от 50; DOF 7.
Scalar сохранённого constraint не используется как доказательство геометрии.

Exact removal и atomic replacement 60→55 дают новый UUID заменяемой длины,
остальные rules/IDs/порядок и closure сохраняются. На source/копиях проверены
stored curves, metadata, dependency/topology facts, source claims, extension rows,
rowids и все запрещённые SQL-изменения. Документный preservation gate исполнен
для H и length, включая optional capability row. Text length process проверяет
прежние строки/порядок stdout по фактам опубликованного документа; JSON остаётся прежним.

Настоящий headless egui → worker отправляет восстановленный Undo/Redo request,
сравнивает его с peer CLI после cold reopen. `verify-worker-artifacts.py` дополнительно
сопоставляет только 10 новых constraint UUID в CBOR payload; все остальные SQL
cells и rowids равны, кроме производного payload hash. STL **и FBX побайтово равны**.
Независимый STL: 12 triangles, 684 bytes, размеры
59.99999809×29.99999905×10 mm, объём 17999.99885559 mm³ (float32 mesh tolerance).
Это **headless worker**, не проверка настоящего GUI. Лог/JSON:
`worker-artifact-verification.{log,json}`.

Противоречивый H/V-прямоугольник с 60 и 70 mm на противоположных сторонах проходит
структурную policy и получает реальный `error.kind:"constraint"` с непустым typed
conflict, включая Distance/UUID. Output/scratch отсутствуют. Проверены input type,
unknown fields/rules/version, nonfinite/overflow/zero/negative, duplicate/foreign
IDs, stale version, source/hardlink/Unix symlink, no-clobber, Unicode/OS paths и
flag-shaped args. Старые H/V отказы сохранены; preflight дополнен length request.

`edit::tests::constraint_copy_refusals_cancellation_races_and_cleanup` теперь
исполняет H+length: отмена до начала/после snapshot/baseline/закрытого scratch,
ошибки geometry/reference/publication, поздний source/alias/occupied race,
0 live handles; отмена после publish возвращает успех. Прямые закрытые OS pipes
сохраняют новые документы и возвращают 7, обе закрытые трубы не вызывают panic.
Отказы при закрытом stderr доставляются через исправный stdout; source bytes/mtime
и состав каталога проверены. Прежние общие guards не дублировались в UI/CLI.

Две временные поломки **скомпилировались и исполнили по одному провалившемуся тесту**:

1. Distance перед solver умножен на 0.5: assertion `length NOT solved`, 30 вместо 60.
2. В preparation изменён stored start.x: preservation assertion увидел −19 вместо −20.
Обе дали 0 passed/1 failed, exit 101; исходники восстановлены побайтово.
Положительные document и native process gates повторены. Логи `mutation-length.log`,
`mutation-stored-coordinates.log`; итогового diff в evaluator нет.

## Stub, reader и рецепт

Отдельный свежий CLI/app target `/private/tmp/ferrite-24j-stub-target`, env
`/private/tmp/ferrite-25f/stub-env.sh`. CMake до/после:
`OpenCASCADE_DIR-NOTFOUND`, `CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew`; binary imports
без OCCT/PlaneGCS (`stub-cmake.log`, `stub-imports.log`).
**17 исполненных**: document 5, CLI protocol/discovery 3, UI draft 6, eval refusal 3.
**5 явных native skips**: три native CLI constraints gates и два native app workers;
не считаются геометрией. Логи `stub-{document,cli-final,ui,eval}.log`.
Все три stub-only теста, пропущенные в native, здесь исполнены: `a_build_with_no_solver_publishes_no_facts`,
`a_build_with_no_solver_refuses_before_the_kernel`,
`a_build_with_no_solver_refuses_without_inventing_a_conflict` (его native log пишет
`not applicable`, также считается skip). Никакого kernel-unavailable geometric success нет.

Pinned ufbx 0.23.0, commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`, SHA256
обоих исходников проверен штатным helper; reader пересобран (`ufbx-pin.log`).
H, V, rectangle/replaced/slanted length, worker UI/CLI и рецепт: **по 6 checks,
0 failures на каждый FBX**. Новые native process/worker exact-name/no-skip gates
добавлены в существующий runtime workflow: ожидается **90** executions вместо 84,
все прежние сохранены. Reader существующей FBX-кампании получает ещё три малых
length FBX; отдельного workflow/large STEP import нет. Обычный CI явно проверяет
новый input/history widget test. Это проверка конфигурации, не исполненный удалённый CI.

Рецепт извлечён из текущего `docs/line-length-constraints.md` и исполнен реальным
bundled CLI (`agent-recipe-final.log`, `agent-recipe.py`), включая независимое STL
измерение и ufbx. Первый вариант ошибочно предполагал, что default sample 80×40:
assertion поймал фактическую ширину 60. Fixture/рецепт исправлены на явный 80×40,
native gate и рецепт повторены; это исправление тестового предположения, не модели.

## GUI и память — ограничение

Свежий цельный bundle: 51 private library, 0 unexpected; codesign deep/strict pass,
native/staged UUID `CE5D55C3-EF00-33A6-87D6-807C5F4E02E3`. Один PID 61861 запущен
под watchdog cap1536 MiB **до CUA**, PID/path проверены shell. CUA отказал:
“The Mac is locked and automatic unlock could not unlock it.”
**GUI smoke не исполнен.** Security settings не менялись; после отказа только свой
PID завершён SIGTERM, его отсутствие подтверждено shell. CUA больше не вызывался.

Guard не сработал: aborted=false, viewer return −15, launcher exit143 из-за явной
остановки. 63.43 s, peak footprint **194.16 MiB**, RSS 114.84 MiB, pressure=1,
swap initial/max/final 2991783936 bytes, min free disk39.67 GiB. Это **попытка
запуска при закрытом экране**, не memory/GUI acceptance. Evidence:
`gui-memory.jsonl`, `gui-watch-summary.json`, `gui-stop.log`, `gui-exit-pid.log`.
Headless watchdog tests сначала отказали из-за sandbox sysctl permission, затем
исполнены с разрешённым доступом к метрикам — 5/5; оба лога сохранены.

Причина прежнего OOM не установлена. Большой STEP/complex FBX, Unity, полный GPU
suite, полный workspace test suite и Linux/Windows локально не запускались.
Document schema, native pins/FFI, dependencies/Cargo.lock и inventories не менялись.
Весь diff unstaged/uncommitted; staging/commit/push/PR/merge не выполнялись.
Следующий шаг — независимое ревью. Следующий срез не начат.

## Полный состав diff

Все 16 файлов перечислены ниже; `new` — untracked, учитывается как добавление целого файла.
Индекс пуст. Ветка `line-length-constraints`, HEAD остаётся точной базой выше.

| Файл | + | − | Статус |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 4 | 3 | modified |
| `.github/workflows/runtime-layout.yml` | 2 | 2 | modified |
| `crates/ferritecad-app/src/constraints.rs` | 301 | 69 | modified |
| `crates/ferritecad-cli/src/edit_constraints.rs` | 39 | 25 | modified |
| `crates/ferritecad-cli/tests/edit_constraints.rs` | 441 | 95 | modified |
| `crates/ferritecad-document/src/lib.rs` | 3 | 2 | modified |
| `crates/ferritecad-document/src/sketch_constraints.rs` | 47 | 12 | modified |
| `crates/ferritecad-document/tests/constraint_edit.rs` | 151 | 7 | modified |
| `crates/ferritecad-jobs/src/edit.rs` | 12 | 4 | modified |
| `docs/cli-capabilities.md` | 2 | 2 | modified |
| `docs/cli-json-v1.md` | 13 | 1 | modified |
| `docs/implementation-plan.md` | 14 | 2 | modified |
| `docs/sketch-constraints-copy.md` | 53 | 16 | modified |
| `tools/check-fbx-complex.sh` | 1 | 1 | modified |
| `docs/line-length-constraints-verification.md` | 199 | 0 | new |
| `docs/line-length-constraints.md` | 114 | 0 | new |

Итого: **16 файлов, 1396 добавлений, 241 удалений**, включая untracked.
Снимки `final-status.log` и `final-diffstat.log` находятся в `/private/tmp/ferrite-25f/`.

## Независимое ревью, 2026-09-13

Дефектов реализации не обнаружено; production code и тесты при ревью не менялись.
Проверены строгий tagged request, checked length/history equality, отдельные slots
orientation/length, whole-document refusal, remove-before-add и сохранность closure.
Точная база повторно проверена: PR #32 MERGED, main/origin/main
`50671f22c7ed0dcaf0d1e9e4e233a9282c013e7c`, 27/27 checks success.

Повторены fmt, workspace clippy all targets/features -D warnings, document/jobs/eval:
367 исполненных tests, три неприменимых stub-only skips и один прежний ignored
benchmark. CLI 47/47 и headless app 41/41 (8 constraints, 21 Sketch/gesture/Snap,
8 edit, 4 STL) прошли без native skips. Настоящий stub повторно собран: CMake
OpenCASCADE_DIR-NOTFOUND и imports без OCCT/PlaneGCS; 17 исполненных tests,
пять явно пропущенных native gates. Actionlint, shellcheck, export boundary,
336 licence headers, diff check и пять watchdog tests прошли.

**GUI-ограничение исходной передачи закрыто независимым smoke.** Свежий цельный
review bundle имеет 51 private library, 0 unexpected и проверенную deep/strict
подпись; native/staged UUID `CE5D55C3-EF00-33A6-87D6-807C5F4E02E3` совпал.
Собственный PID 89025 запущен под watchdog до CUA и подтверждён shell.
В живом окне: H/V четырёх сторон + длины 60/30 → Undo → ошибочный ввод `-`
(отказ сохраняет Redo) → Redo → Save Cancel → Undo/Redo → publish/async Open.
Неприменённый `-` не подменяет принятый request. Новый draft имеет пустую историю
и поле; persisted lengths доступны через прокрутку. Затем exact Remove длины
60 + Add 55 → Undo/Redo → publish/async Open. Обе модели показывают DOF 2,
redundant None. После штатного Close CUA не вызывался, отсутствие PID проверено.

GUI/CLI пары сравнены по всем SQL cells/rowids: явно сопоставлены только 10 новых
constraint UUID первой публикации и один UUID замены, плюс производный hash.
Остальные IDs/rules/closure/curves/claims равны. Исходные SHA256/mtime не изменились.
Validate и cold rebuild прошли; STL и FBX в обеих парах побайтово одинаковы.
Независимый STL: 59.99999809×29.99999905×10 mm / 17999.99885559 mm³, затем
55×30×10 mm / 16500 mm³. Свежесобранный pinned ufbx прочитал четыре FBX:
по 6 checks, 0 failures. Публичный рецепт повторён из текущего Markdown и дал
60×30×10 mm / ≈18000 mm³, ufbx 6/6.

Watchdog: peak footprint **277.03 MiB**, cap 1536 MiB, pressure=1;
swap initial/max/final 2991783936 bytes, min free disk 39.31 GiB;
209.19 s, exit 0, aborted=false. Это два последовательных publish/Open малого
профиля, а не доказательство устранения прежнего OOM; его причина не установлена.
Большой STEP/partial FBX, Unity, полный GPU и native rebuild локально не повторялись.

Логи, snapshots, GUI-файлы, SQL/mesh comparisons и memory samples:
`/private/tmp/ferrite-pr33-review/`. CI head и merge нового diff проверяется
отдельно после публикации. Чужой a200 и исходные fixtures не изменены.
