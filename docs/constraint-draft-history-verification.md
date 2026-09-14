# §25E-1 — проверка истории H/V draft

2026-09-13. База `b439af7a6a056ef53ae140f977ca3c54a1563689`, PR #31 MERGED.
После fetch HEAD/main/origin/main совпадали, дерево было чистым. Создана ветка
`constraint-draft-history`. Implementation head PR31
`29cf5978001e349b04000e6d5fb0751190281630` имеет то же дерево
`b1c31aa3317028b0c863e313cac6627537358b2c`.
Повторная проверка GitHub: **27/27 checks, 6/6 workflow success** точной базы,
runtime `34792495549`. Это CI базы; удалённый CI незакоммиченного diff не запускался.

## Изменение

`constraints.rs`: локальная история только ordered `SketchConstraintEdits`,
два стека по 128, кнопки Undo/Redo, checkpoint на фактической правке. Выбор,
ошибка и no-op не меняют историю. Restore очищает устаревшую локальную ошибку;
прежний validator определяет доступность Save. История живёт в Draft, поэтому
существующие generation/lifecycle guards сохраняют или удаляют её вместе с ним.
Новых constraint UUID, solver на redraw, глобальных shortcuts и persistent undo нет.
[Контракт](sketch-constraints-copy.md#25e-1-история-несохранённого-hv-draft).

Production diff ограничен UI. Domain/jobs/writer, CLI/JSON, schema, зависимости,
Cargo.lock, native/FFI, inventories и workflow не менялись. Исходные fixtures,
чужой a200 и чужие процессы не изменялись. Серьёзных независимых дефектов при
работе не обнаружено; исправление длинного pending list из PR31 сохранено.

## Локальные проверки текущего diff

Логи и настройки: `/private/tmp/ferrite-25e1/`. Сборки последовательные,
`CARGO_BUILD_JOBS=1`; tests `--test-threads=1`; GPU flag и loader-failure probes
выключены. Native target `/private/tmp/ferrite-24b-native-target`, существующие
vendor OCCT 8.0.1 и PlaneGCS. OCCT из исходников не пересобирался.

```bash
source /private/tmp/ferrite-25e1/native-env.sh
cargo fmt --all -- --check
cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
cargo build --offline -p ferritecad-cli -p ferritecad-app --all-features
for suite in constraints::tests:: sketch:: edits::tests:: exports::stl::tests::native_; do
  cargo test --offline -p ferritecad-app --all-features --bin ferritecad-viewer \
    "$suite" -- --nocapture --test-threads=1
done
```

Fmt/clippy/build прошли; свежий peer CLI собран до worker tests.
**39/39 исполненных native app tests, без skips/ignored:**

| Фильтр | Passed | Лог |
| --- | ---: | --- |
| `constraints::tests::` | 6 | `native-constraints__tests__.log` |
| `sketch::` (widgets, gestures, Snap и native peer) | 21 | `native-sketch__.log` |
| `edits::tests::` (сохранность, отмена, stale, shutdown) | 8 | `native-edits__tests__.log` |
| `exports::stl::tests::native_` | 4 | `native-exports__stl__tests__native_.log` |

Новые реальные egui-переходы проверяют Add H → Undo → Redo, точный persisted
Remove UUID on/off → Undo/Redo, Clear → Undo, ordered additions, успешное
ветвление, отказ duplicate/H+V, no-op и выбор без потери Redo. При running оба
непустых стека и request неизменны после нажатий disabled controls. Проверка
141 последовательного состояния подтверждает удержание последних 128 и точные
ordered UUID/правила после полного Undo/Redo, включая no-op на границе.

Lifecycle test сохраняет одновременно непустые Undo и Redo при Save Cancel,
ошибке/отмене worker и stale reply; Cancel/new draft очищает историю. Dense test
по-прежнему использует 64 Line и 32 pending additions в unconstrained/constrained
каталогах: clip rect/viewport всех пяти действий, Clear/Undo/Redo и Save точного
восстановленного запроса проверяются настоящими нажатиями.

`native_constraint_worker_and_cli_preserve_model_and_solved_body` отправляет
восстановленный через Undo/Redo запрос через настоящий worker. Проверяются
исходные source/version/Sketch, сохранность при занятом output, закрытие draft
после publish и пустая история нового draft. Copy/peer CLI сравниваются по
сохраняемым SQL facts, IDs/rowids/claims, ordered rules с сопоставлением новых
constraint UUID; cold reopen и STL обоих клиентов совпадают. Исходник неизменен.

## Отдельный stub

```bash
source /private/tmp/ferrite-25e1/stub-env.sh
cargo build --offline -p ferritecad-cli -p ferritecad-app --all-features
cargo test --offline -p ferritecad-app --all-features --bin ferritecad-viewer \
  constraints::tests:: -- --nocapture --test-threads=1
```

Target `/private/tmp/ferrite-24j-stub-target`. До/после свежей сборки CMake:
`OpenCASCADE_DIR-NOTFOUND`, `CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew`.
`otool -L` свежих CLI/viewer не содержит OCCT/PlaneGCS (`stub-cmake.log`,
`stub-imports.log`). Устаревшая PATH-подсказка из старого env не использовалась;
реальный CMake cache исключает Homebrew. **5 исполненных draft tests + 1 явный
native skip** (`native_constraint_worker_and_cli_preserve_model_and_solved_body`),
хотя harness сообщает 6 passed. Skip не считается геометрической проверкой.
Полный лог `stub-constraints.log`.

## GUI и ограничения

Свежий цельный bundle собран штатными runtime-closure/stage-runtime-layout
helpers: private libraries 50 OCCT + 1 PlaneGCS, unexpected dependencies 0.
`codesign --verify --deep --strict` прошёл. Native/staged viewer UUID совпал:
`A8563149-EE47-3BD8-A324-E73DCBD662D4` (`uuids.log`, `signature.log`,
`closure-{cli,viewer}.txt`, `stage.log`). Loader failure probes не запускались.

Один собственный PID 85966 запущен под `tools/watch-viewer-memory.py` **до CUA**,
cap 1536 MiB; PID/path проверены shell. В настоящем окне на приватном четырёхугольнике:
Add Horizontal → Undo (пустой draft, та же Line) → Redo → Save Cancel → повторный
Undo/Redo → Save `gui-history.fcad`. Async Open показал опубликованную копию,
7 DOF и отсутствие redundant constraints. После Close CUA больше не вызывался;
shell подтвердил отсутствие PID, watchdog — exit 0, aborted=false.

Watchdog: sampled peak footprint **207.84 MiB**, RSS 130.27 MiB, pressure=1,
swap initial/max 3025338368 bytes, final 3016949760 bytes; minimum free disk
40.83 GiB; 175.81 s. Логи: `gui-memory.jsonl`, `gui-watch-summary.json`,
`gui-pid-verification.log`, `gui-exit-pid.log`. Это измерение одного малого smoke.

`verify-gui.py` исполнен свежим bundled CLI: GUI и CLI копии сохраняют исходные
document/Sketch/Body/feature identities и stored curves; ordered constraints rules
совпадают (четыре Coincident + выбранный H), новые constraint UUID независимы.
Validate и `rebuild --cold` прошли. STL одинаковы побайтово; независимый разбор
подтвердил 12 triangles, 684 bytes, bounds [-20,-10,0]…[42,30,10] mm и 24400 mm³.
Source SHA256 и mtime не изменились. Лог `gui-artifact-verification.log`.
Первый вызов проверки артефакта пропустил обязательный `--cold`, получил честный
exit 2 и не засчитан; исправленная команда исполнена, уже созданная CLI-копия
переиспользована без повторной публикации (`gui-artifact-first-attempt.log`).

Полный workspace test suite, большой STEP/complex FBX, Unity и полный GPU suite
не запускались: предметные маршруты не менялись. Причина прежнего OOM этим
срезом не устанавливается. Linux/Windows текущего diff локально не исполнялись.
Изменения unstaged/uncommitted; staging/commit/push/PR/merge не выполнялись.
Следующий шаг — независимое ревью; следующий срез не начат.

## Итоговый состав

Четыре файла: **500 insertions(+), 13 deletions(-)**, включая untracked протокол.

| Файл | Назначение |
| --- | --- |
| `crates/ferritecad-app/src/constraints.rs` | bounded draft history, controls, 3 новых и 3 расширенных теста |
| `docs/sketch-constraints-copy.md` | контракт §25E-1 |
| `docs/implementation-plan.md` | итог среза |
| `docs/constraint-draft-history-verification.md` (untracked) | этот протокол |

Точные line counts и статус всех файлов сохранены в
`/private/tmp/ferrite-25e1/final-diffstat.log` и `final-status.log`;
`git diff --check` прошёл, index пуст. Локальные временные inputs, публикации,
bundle и логи лежат вне репозитория в каталоге задачи.


## Независимое ревью §25E-1, 2026-09-13

Проверены bounded stacks, exact ordered request, no-op/branching, disabled controls,
Save adapter и lifecycle. Дефектов реализации не обнаружено; production code и
тесты при ревью не менялись. Повторены fmt, workspace clippy all targets/features
с -D warnings, свежая сборка peer CLI/app и все 39 native app tests: 6 constraints,
21 sketch/drag/Snap, 8 edit, 4 STL; skips/ignored отсутствуют. Native campaign под
watchdog: sampled peak 55.95 MiB, exit 0; использован уже тёплый target.

Отдельно пересобран настоящий stub CLI/app: imports без OCCT/PlaneGCS,
CMake OpenCASCADE_DIR-NOTFOUND. Пять draft tests исполнены, один native worker
явно skipped. После закрытия GUI исполнены пять watchdog tests. License headers
и diff --check прошли. Никакой native skip не засчитан как геометрия.

Свежий review bundle `/private/tmp/ferrite-pr32-review/gui/FerriteCAD.app`:
подпись проверена, 51 private library и 0 unexpected; native/staged UUID
`A8563149-EE47-3BD8-A324-E73DCBD662D4`. Собственный PID 20391 запущен под
watchdog до CUA. В настоящем окне выполнены Add H → Undo → Redo → Save Cancel →
Undo → Redo → publish/async Open (7 DOF); новый draft начал с пустой историей.
Затем Remove exact H `01a09db1-1c96-78a1-a9bd-780e164028f7` → Undo → Redo →
Clear → Undo → publish/async Open (8 DOF). Четыре Coincident сохранены.
После штатного Close CUA не вызывался, отсутствие PID проверено shell.

GUI watchdog: sampled peak **327.14 MiB**, cap 1536 MiB, pressure=1;
swap initial/max/final 3016949760 bytes, min free disk 40.56 GiB;
234.58 s, exit 0, aborted=false. Это два последовательных publish/Open малого
профиля, а не сравнимый с исходным одним publish performance benchmark.
Причина прежнего OOM остаётся неизвестной.

Реальные GUI-файлы проверены свежим bundled CLI: добавленные constraint UUID
сопоставлены явно, остальные SQL cells/rowids/identities/source claims сохранены.
Удаление из одного и того же сохранённого input по exact UUID дало одинаковые
GUI/CLI SQL cells. Source SHA256/mtime неизменны. Validate и cold rebuild прошли;
STL и FBX в обеих парах равны побайтово. Независимый STL: 24400 mm³ с H,
23780 mm³ после удаления, bounds [-20,-10,0]…[42,30,10] mm. Существующий pinned
strict ufbx reader прочитал четыре GUI/CLI FBX по 6 checks, 0 failures.

Логи, snapshot перед ревью, GUI/SQL/mesh comparisons и memory samples:
`/private/tmp/ferrite-pr32-review/`. Точная база повторно подтверждена: 27/27 checks,
6/6 workflows. Head/merge CI нового diff проверяются отдельно после публикации.
Большой STEP/partial FBX, Unity, полный GPU и rebuild native libraries локально
не запускались; исходные fixtures и чужой a200 не изменены. Следующий §25F
(сохраняемая длина Line через общий UI/CLI job) только запланирован.
