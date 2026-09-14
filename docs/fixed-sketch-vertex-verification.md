# §25G — реализация и локальная проверка

2026-09-14. База `fdcb47e3ff697c5aecb495d6e66a97b07dee8e66`, PR #34 MERGED
(2026-09-14T15:14:35Z, ветка `replace-line-length-ui`). После `git fetch`
HEAD/main/origin/main совпадали, дерево было чистым. Ветка
`fixed-sketch-vertex`. CI точного merge SHA перепроверен: **6/6 workflows
success** (CI, runtime layout, planegcs pin, product sbom, rust sbom,
rust notices). Это CI базы; CI нового незакоммиченного diff не запускался и
базе не приписывается. Чужой detached worktree
`/Users/drt/.codex/worktrees/a200/ferrite-cad`, fixtures и чужие изменения
не затрагивались; ничего не staged, не commit, не push.

## Предметный результат

[Точный контракт](sketch-constraints-copy.md), [исполняемый рецепт](fixed-sketch-vertex.md).

Три вещи разделены и измеряются отдельно: **stored coordinates** (записаны в
`.fcad`, остаются inputs solver и никогда не перезаписываются решением),
**solved coordinates** (presentation одного rebuild; измеряются по телу, а не
читаются из stored curves) и **degrees of freedom** (измеренный результат
solve). Полностью размерный прямоугольник 60×30 имеет DOF 2 — две трансляции;
один Fixed endpoint снимает ровно их и даёт DOF 0; удаление возвращает DOF 2.

Document расширяет прежний managed класс одним `SketchConstraintRule::Fixed`.
Слот занятости стал трёхчастным: одна ориентация и одна длина на Line и
**не более одного pin на профиль**, какой бы Line и endpoint его ни нёс —
поэтому ссылка на противоположный endpoint того же adjacent Coincident joint
тоже является вторым pin и отказывается структурно, без изобретения связи по
близости координат. Checked `SketchCoordinateMm` допускает ноль и отрицательные
mm и не допускает NaN/infinity/overflow; оба нуля нормализованы в один паттерн,
поэтому точное `Eq` истории корректно и NaN в него не попадает.
`LineEndpoint` делает невыразимым `at`, которого у Line нет, вместо проверки
в runtime; положительный `LineLengthMm` как тип координаты не используется.
Существующий перевод Fixed в solver (`ferritecad-eval/src/solve.rs`) и прежний
narrow writer не менялись. Runtime dependencies, схема БД, DDL и FFI не менялись.

CLI request v1 аддитивно принимает `{"rule":"fixed","curve_id":UUID,
"at":"start"|"end","x_mm":N,"y_mm":N}`. Старые H/V/Distance формы, лимит 65536
bytes, 512 изменений, UTF-8 правила, operation/schema v1 и коды 0/2/7 сохранены.
Response Fixed остаётся прежним DTO `point/x/y`; `x_mm`/`y_mm` — только вход.

UI `Edit constraints`: `Pin Start`/`Pin End`, поля `Fixed X (mm)`/`Fixed Y (mm)`
и `Add Fixed point`. Рядом с выбором конца показана **stored** координата
выбранного endpoint, поэтому выбор не требует чтения solved drawing. Persisted
Fixed отображается как `Fixed point <start|end> (x, y) mm · UUID` — не прежним
fallback-ярлыком `Coincident closure` — и удаляется по exact identity. Один
Apply — один прежний bounded `History::change`; поля и selection вне истории.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`, GPU unset,
loader-failure probes unset. Env/target: `/private/tmp/ferrite-25g/native-env.sh`,
`/private/tmp/ferrite-24b-native-target`. Существующие pinned OCCT 8.0.1 и
PlaneGCS переиспользованы; OCCT из исходников не пересобирался. Peer CLI собран
перед app worker gates. Команды — `/private/tmp/ferrite-25g/native-tests.sh`.

| Проверка | Результат | Лог в `/private/tmp/ferrite-25g/` |
| --- | --- | --- |
| fmt, workspace clippy all-targets/all-features `-D warnings`, `git diff --check` | pass, 0 warnings | `clippy-final.log` |
| document/jobs/eval `--all-features` | **372 passed, 0 failed**, 1 прежний ignored benchmark; 3 stub-only skips | `domain-tests.log` |
| CLI binary/create/edit/Sketch/JSON/constraints | **48 passed**, 0 skips | `cli-tests.log` |
| Финальный CLI `edit_constraints` | **7 passed**, 0 skips | `cli-constraints-final.log` |
| App `constraints::tests::` | **11 passed**, 0 skips | `app-constraints__tests__.log` |
| App `sketch::` | **21 passed** | `app-sketch__.log` |
| App `edits::tests::` | **8 passed** | `app-edits__tests__.log` |
| Jobs cancellation/race/cleanup gate | **1 passed** | `jobs-faults.log` |
| licence headers (336 файлов), shellcheck, actionlint | pass | одноимённые логи |

Новые критические named gates:
`native_fixed_endpoint_pins_the_body_and_removal_restores_two_degrees_of_freedom`,
`constraints::tests::native_fixed_point_worker_and_cli_pin_the_same_solved_body`,
`constraints::tests::fixed_point_widgets_pick_an_endpoint_and_replace_the_stored_pin_in_one_step`,
`a_fixed_endpoint_is_checked_replaced_and_removed_without_touching_other_ids`,
`several_pins_or_an_unrepresentable_pin_refuse_the_whole_document`.
Все прежние H/V и length gates исполнены без ослабления.

## Geometry, identity, refusal и delivery

Настоящий CLI-процесс: create 80×40×10 → H/V четырёх сторон + длины 60/30 даёт
**DOF 2**; добавление одного Fixed на Start первой линии в (10, −5) mm даёт
**DOF 0** после cold reopen (`rebuild_cold` + реальный `solve_report`, а не
сообщение DTO). Независимый разбор опубликованного STL (`/private/tmp/ferrite-25g/artifacts`):

| Публикация | triangles | extents mm | XY lo → hi | объём mm³ |
| --- | ---: | --- | --- | ---: |
| `pinned` | 12 | 60 × 30 × 10 | (10, −5) → (70, 25) | 18000.00000 |
| `pin-moved` | 12 | 60 × 30 × 10 | (−25, 40) → (35, 70) | 18000.00000 |
| `pin-origin` | 12 | 60 × 30 × 10 | (−60, −30) → (0, 0) | 18000.00000 |
| `unpinned` | 12 | 59.999998 × 29.999999 × 10 | произвольно | 17999.99886 |

Закреплённая вершина действительно оказалась в запрошенных миллиметрах.
Замена закрепления (10, −5) → (−25, 40) переместила тело ровно на дельту
(−35, +45) и не изменила ни один размер; удаление exact pin вернуло DOF 2 и
произвольное положение под-ограниченной модели (float32 mesh tolerance). Ноль и
отрицательные координаты приняты как обычные (`pin-origin`, pin в (0, −0)).

Stored curves во всех копиях побайтово равны исходным: solve себя не записал.
`stored_same` проверяет metadata, dependency/topology facts, имена, parent,
ordinal и `validate`. Списки constraint UUID сравниваются целиком: замена pin
создаёт ровно один новый UUID и сохраняет порядок и identity остальных десяти,
включая Coincident closure, H/V и обе длины; удаление pin оставляет ровно те же
десять. Closure при удалении Fixed не удаляется.

Headless egui → настоящий worker отправил построенный виджетами request,
опубликовал копию, и peer CLI-процесс воспроизвёл тот же ordered request над тем
же source. UI и CLI дали **побайтово равные STL и FBX**; различаются только
действительно новые constraint UUID (11 rules равны попарно, 11 id различны).
Все SQL-таблицы, rowids, source claims и прочие cells равны, кроме payload/hash/
schema_version выбранного Sketch и required capability `sketch.constraints.v1`.

Структурные отказы (два pin в одном request; pin на противоположный endpoint
того же joint; дубликат; foreign curve UUID) проверены отдельно от настоящего
solver failure: первые дают `error.kind:"input"` без `constraint_conflict`,
второй — противоречивые длины 60/70 вместе с pin — даёт `kind:"constraint"` с
непустым typed conflict. Ни один не создал output и не изменил каталог/source.
Wire-отказы request (отсутствующие/лишние поля, `distance_mm` у `fixed`, `at` у
H/V и Distance, `"Start"`, `"at"`, `"middle"`, число вместо строки, строка/bool/
null вместо числа, NaN, ±Infinity, ±1e999) отвергаются до публикации. Публикация
с закрытым stdout даёт exit 7 и сохраняет читаемую валидную копию.

## Чувствительность проверок

Две узкие временные поломки **скомпилировались и дали исполнившиеся провалы**:

1. В preparation потеряна одна координата pin (`y := x.get()`):
   document gate увидел `(…, Start, −20.0, −20.0)` вместо `(…, Start, −20.0, −10.0)`
   — 0 passed / 1 failed; native CLI gate тоже провалился на измеренной позиции.
   Логи `mutation-pin-coordinate.log`, `mutation-pin-coordinate-native.log`.
2. Удаление задевает чужой UUID (вместе с названным отбрасывается первый
   constraint): **3 passed / 4 failed**, включая точные списки ID после
   замены и удаления и прежний preservation gate. Лог `mutation-removal-scope.log`.

Оба раза источник восстановлен побайтово (`cmp` с копией) и положительные
document/native gates повторены: 7 + 7 passed. Compile failure и zero tests
доказательством не считались.

## Stub, reader и рецепт

Свежий отдельный target `/private/tmp/ferrite-25g-stub-target`, env
`/private/tmp/ferrite-25g/stub-env.sh`: `OpenCASCADE_DIR` и `FCAD_PLANEGCS_DIR`
указывают в несуществующие каталоги, DYLD/LD paths сняты. Первая попытка **не
была настоящим stub**: этот `cmake` установлен в `/opt/homebrew`, поэтому его
собственный префикс попадает в поиск, и `find_package` подобрал Homebrew
OpenCASCADE 7.9 — CMakeCache показал
`OpenCASCADE_DIR:PATH=/opt/homebrew/lib/cmake/opencascade`, а бинарь
импортировал 49 OCCT-библиотек. Переменная окружения `CMAKE_IGNORE_PREFIX_PATH`
этим CMake 4.3.3 не применяется (проверено отдельным минимальным проектом).
Настоящий stub получен через honest CMake-механизм — `CMAKE_TOOLCHAIN_FILE`
(`stub-toolchain.cmake`, единственная строка `set(CMAKE_IGNORE_PREFIX_PATH
"/opt/homebrew" …)`), ничего в системе и репозитории не менялось.

Доказательства настоящего stub: build script напечатал
`Open CASCADE was not usable … Reason: configuring the bridge failed`, и
`otool -L` собранного `ferritecad` показывает **0** OCCT/PlaneGCS импортов —
только `libiconv.2.dylib` и `libSystem.B.dylib` (`stub-build.log`).

| Stub suite | Исполнено | Native skips | Лог |
| --- | ---: | ---: | --- |
| document `constraint_edit` | **7** | 0 | `stub-document.log` |
| CLI `edit_constraints` | **3** | 4 | `stub-cli.log` |
| app `constraints::tests::` | **8** | 3 | `stub-ui.log` |
| eval (весь пакет) | **123** | 39 | `stub-eval.log` |

Всего **141 исполненный** тест и **46 явных skips**, посчитанных отдельно;
ни один kernel-unavailable путь не выдал геометрический успех. Новый
`native_fixed_endpoint_pins_…` и `native_fixed_point_worker_…` честно скипнулись
здесь и честно исполнились в native кампании. Три stub-only теста, пропущенные
в native, здесь исполнены: `a_build_with_no_solver_publishes_no_facts`,
`a_build_with_no_solver_refuses_before_the_kernel`,
`a_build_with_no_solver_refuses_without_inventing_a_conflict`. Все новые
request-level отказы pin (типы, лишние/отсутствующие поля, неверный `at`,
NaN/Infinity/overflow) исполнены именно на stub, до создания kernel.

Pinned ufbx 0.23.0 reader (`/private/tmp/ferrite-25g/read_production`, собран из
`tools/unity-fbx-smoke/scripts/read_production.c` с прежним pin) прочитал шесть
опубликованных FBX — `pinned`, `pin-moved`, `pin-origin`, `unpinned`, `ui`,
`cli` — по **6 checks, 0 failures** на каждый. Список
`FCAD_SKETCH_CONSTRAINT_FBX_DIR` в `tools/check-fbx-complex.sh` дополнен
`pinned` и `pin-moved`; отдельного workflow и большого STEP-корпуса нет.

Рецепт извлечён из текущего `docs/fixed-sketch-vertex.md` и исполнен настоящим
CLI (`agent-recipe.py`, `agent-recipe.log`, exit 0). Итог:
`{"pinned_mm": [[10.0, -5.0], [70.0, 25.0]], "moved_mm": [[-25.0, 40.0],
[35.0, 70.0]], "volume_mm3": 18000.0, "dof": {"dimensioned": 2, "pinned": 0,
"unpinned": 2}}`. Рецепт включает независимое STL-измерение, ufbx и проверку
неизменности source по sha256 и mtime.

## CI-гейты нового diff

Новые exact-name/no-skip gates добавлены в **существующие** workflows на трёх ОС,
старые не ослаблены: обычный `ci.yml` получил headless widget gate
`fixed_point_widgets_…` и два document gate'а; `runtime-layout.yml` получил
native process gate `native_fixed_endpoint_pins_…` и app worker gate
`native_fixed_point_worker_…`. Все они проверяются на `skipped:` и на точную
строку `test <name> ... ok`; вывод `--nocapture` эту строку не разрывает —
дефект PR33 не воспроизведён (новые тесты ничего в stdout не печатают).
Это проверка конфигурации, не исполненный удалённый CI.

## GUI и ограничения

**Интерактивный GUI smoke не запускался и отложен по прямому указанию
пользователя**; GUI не считается проверенным. Viewer, GPU/render tests, CUA,
osascript, Unity и системные диалоги не запускались; экран не требовался.
Headless egui widget tests — не GUI smoke.

Причина прежнего OOM не установлена. Большой STEP/complex FBX-корпус, Unity,
полный GPU suite, полный workspace test suite и Linux/Windows локально не
запускались. `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался. Document schema,
native pins/FFI, dependencies и Cargo.lock не менялись. Чужие процессы не
останавливались, блокировка экрана не отключалась.

Весь diff оставлен unstaged/uncommitted; staging/commit/push/PR/merge не
выполнялись. Следующий шаг — независимое ревью. Следующий срез не начат.

## Полный состав diff

Все 16 файлов перечислены ниже; `new` — untracked, учитывается как добавление
целого файла. Индекс пуст. Ветка `fixed-sketch-vertex`, HEAD остаётся точной
базой выше.

| Файл | + | − | Статус |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 5 | 1 | modified |
| `.github/workflows/runtime-layout.yml` | 2 | 2 | modified |
| `README.md` | 3 | 1 | modified |
| `crates/ferritecad-app/src/constraints.rs` | 735 | 4 | modified |
| `crates/ferritecad-cli/src/edit_constraints.rs` | 38 | 3 | modified |
| `crates/ferritecad-cli/tests/edit_constraints.rs` | 347 | 11 | modified |
| `crates/ferritecad-document/src/lib.rs` | 3 | 3 | modified |
| `crates/ferritecad-document/src/sketch_constraints.rs` | 102 | 14 | modified |
| `crates/ferritecad-document/tests/constraint_edit.rs` | 234 | 5 | modified |
| `docs/cli-capabilities.md` | 3 | 1 | modified |
| `docs/cli-json-v1.md` | 21 | 0 | modified |
| `docs/implementation-plan.md` | 30 | 8 | modified |
| `docs/sketch-constraints-copy.md` | 43 | 6 | modified |
| `tools/check-fbx-complex.sh` | 3 | 2 | modified |
| `docs/fixed-sketch-vertex-verification.md` | 187 | 0 | new |
| `docs/fixed-sketch-vertex.md` | 201 | 0 | new |

## Независимое ревью Codex

Состояния unstaged и diffstat выше относятся к исходной передаче. При ревью
production-дефектов не обнаружено. Усилены существующие native gates:
положение проверяется у конкретного Start/End выбранной кривой, а не у любой
вершины прямоугольника; headless worker и CLI также обязаны дать побайтово
одинаковый FBX. README уточняет отдельные лимиты: длина на Line, Fixed на
профиль. Следующий §25H только записан в план, не реализован.

Логи независимого прогона: `/private/tmp/ferrite-pr35-review/`.
Последовательно повторены fmt, workspace clippy all targets/features с
`-D warnings`, build CLI/app, document/jobs/eval (372 harness passes, включая
три неприменимых здесь stub-only случая; один прежний ignored benchmark),
48 CLI и 44 headless app tests (constraints 11, sketch 21, edit 8, STL 4).
Геометрические проверки обязательного native env исполнились без skips.
После усиления assertions повторены CLI constraints 7/7 и точный Fixed worker.
Actionlint, shellcheck и whitespace — pass.

Настоящий отдельный stub: document 7, CLI 3, headless widgets/history 8
исполненных проверок; четыре CLI и три app native skips учтены отдельно.
Ни один skip не считается проверкой геометрии. Native libraries и stub imports
перепроверяются отдельно от сообщений test harness.

Из текущих исходников и проверенного по SHA-256 pinned ufbx заново собран
независимый reader. Шесть FBX (`pinned`, `pin-moved`, `pin-origin`, `unpinned`,
отдельные Fixed worker `ui`/`cli`) дали 6 checks / 0 failures каждый.
Для Fixed worker выделен отдельный каталог: другие constraint tests также
используют имена `ui`/`cli`, поэтому их артефакты не подменяют это доказательство.
Публичный Python-рецепт заново извлечён из Markdown и исполнен: положение
(10, −5) → (−25, 40), объём 18000 mm³, DOF 2 → 0 → 2.

Viewer, окна, системные диалоги, GPU/render и Unity не запускались. Причина
прежнего OOM остаётся неизвестной. CI нового опубликованного head и merge
проверяется отдельно; локальный успех не выдаётся за Linux/Windows.

Итого: **16 файлов, 1957 добавлений, 61 удаление**, включая untracked.
Снимки `final-status.log` и `final-diffstat.log` — в `/private/tmp/ferrite-25g/`.
