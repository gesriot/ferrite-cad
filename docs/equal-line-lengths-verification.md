# §25H — реализация и локальная проверка

2026-09-14. База `e4c240e029e4c9fe3c9fea4c88368da25bdfca36`, PR #35 MERGED
(2026-09-14T18:57:38Z, ветка `fixed-sketch-vertex`). После `git fetch --all`
HEAD/main/origin/main совпадали, дерево было чистым. Ветка `equal-line-lengths`
(без префикса `codex/`). CI точного merge SHA перепроверен: **6/6 workflows
success** (CI, runtime layout, planegcs pin, product sbom, rust sbom,
rust notices) и 27/27 check runs. Это **CI базы**; CI нового незакоммиченного
diff не запускался и базе не приписывается. Чужой detached worktree
`/Users/drt/.codex/worktrees/a200/ferrite-cad`, fixtures и чужие изменения
не затрагивались; ничего не staged, не commit, не push.

## Предметный результат

[Точный контракт](sketch-constraints-copy.md), [исполняемый рецепт](equal-line-lengths.md).

Equal length — **связь**, а не число. У неё нет `distance`, она не копирует
значение во второй `Distance` и не переписывает stored координаты. Поэтому
изменение ведущей длины меняет обе стороны, а UUID самой связи переживает
замену `Distance`. Три вещи по-прежнему разделены: stored coordinates (inputs
solver), solved coordinates (presentation одного rebuild, измеряется по телу) и
degrees of freedom (измеренный результат solve).

Document расширяет прежний managed класс существующим
`SketchConstraintRule::EqualLength { a, b }` с двумя `SketchSegmentRef`
Start→End. Request-тип стал честным: прежний `AddLineConstraint { curve, kind }`
превращён в типобезопасный enum `AddLineConstraint::Line { curve, kind }` /
`AddLineConstraint::EqualLength { a, b }`. Фиктивного UUID, скрытого второго ID
в `LineConstraintKind`, клиентской таблицы связей и пары независимых `Distance`
нет — пара выражена двумя полями. Слот занятости получил четвёртую форму
`Slot::Equal(min, max)`: `(A,B)` и `(B,A)` — один слот, а сохранённый
`SegmentRef`, записанный End→Start, читается общим `whole_line` как та же Line и
**не** переписывается канонизацией. Self-pair отказывается отдельным
структурным сообщением до solver; членство обеих Lines в выбранном Sketch
проверяется для каждой стороны. Собственного анализа транзитивности или ранга
нет. Существующий перевод EqualLength в solver (`ferritecad-eval/src/solve.rs`),
прежний narrow writer, jobs `edit_sketch_constraints_copy`, snapshot/version
guard, source/alias/no-clobber, close SQLite до публикации, cancellation,
cleanup и late-cancel policy не менялись. Runtime dependencies, схема БД, DDL,
FFI и `Cargo.lock` не менялись.

CLI request v1 аддитивно принимает точную форму
`{"rule":"equal_length","a_curve_id":UUID_A,"b_curve_id":UUID_B}`. Старые
H/V/Distance/Fixed формы, лимит 65536 bytes, 512 изменений, UTF-8 правила,
closed-pipe поведение, operation/schema v1 и коды 0/2/7 сохранены. Response
EqualLength остаётся прежним DTO `a`/`b` Segment с endpoint refs; входные
`a_curve_id`/`b_curve_id` в ответ не попадают.

UI `Edit constraints`: строка `Equal length:` с `Equal Line A`, `Equal Line B`,
`Add Equal length` и показом выбранной пары `Pair: A <id> · B <id>`. Обе стороны
берутся из того же прежнего stored segment list — второго каталога нет. Выборы
вне истории; один Apply — один прежний bounded `History::change`. Persisted
равенство показано как `Equal length · UUID` и `Lines <a> = <b>`, не прежним
fallback-ярлыком `Coincident closure`, и удаляется по exact identity.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`, GPU unset,
loader-failure probes unset. Env/target: `/private/tmp/ferrite-25g/native-env.sh`,
`/private/tmp/ferrite-24b-native-target`. Существующие pinned OCCT 8.0.1 и
PlaneGCS переиспользованы; OCCT из исходников не пересобирался. Peer CLI собран
перед app worker gates. Команды — `/private/tmp/ferrite-25h/native-tests.sh`.

| Проверка | Результат | Лог в `/private/tmp/ferrite-25h/` |
| --- | --- | --- |
| fmt, workspace clippy all-targets/all-features `-D warnings`, `git diff --check` | pass, 0 warnings | `fmt.log`, `clippy.log` |
| document/jobs/eval `--all-features` | **373 harness-passed: 370 исполненных + 3 stub-only not-applicable**, 1 прежний ignored benchmark | `domain-tests.log` |
| CLI binary/create/edit/Sketch/JSON/constraints | **49 passed**, 0 skips | `cli-tests.log` |
| App `constraints::tests::` | **13 passed**, 0 skips | `app-constraints__tests__.log` |
| App `sketch::` | **21 passed** | `app-sketch__.log` |
| App `edits::tests::` | **8 passed** | `app-edits__tests__.log` |
| Jobs cancellation/race/cleanup gate | **1 passed** | `jobs-faults.log` |
| licence headers (336 файлов), actionlint, shellcheck изменённого скрипта | pass | одноимённые логи |

Новые критические named gates:
`native_equal_length_ties_two_lines_and_removal_restores_the_free_dimension`,
`constraints::tests::native_equal_length_worker_and_cli_tie_the_same_solved_body`,
`constraints::tests::equal_length_widgets_pick_two_lines_and_name_the_persisted_pair`,
`equal_length_pairs_are_checked_stored_and_removed_without_touching_other_ids`.
Все прежние H/V, length и Fixed gates исполнены без ослабления; правило
`geometry_checked`, требующее пустой `redundant()`, не смягчалось.

## Geometry, identity, refusal и delivery

Настоящий CLI-процесс: create 80×40×10 → H/V четырёх сторон + одна длина 60 mm
+ один Fixed (10, −5) даёт **DOF 1**; одна EqualLength соседних сторон даёт
**DOF 0** после cold reopen (`rebuild_cold` + реальный `solve_report`, а не
сообщение DTO). Независимый разбор опубликованного STL
(`/private/tmp/ferrite-25h/artifacts`):

| Публикация | triangles | extents mm | XY lo → hi | объём mm³ |
| --- | ---: | --- | --- | ---: |
| `square` (DOF 0) | 12 | 60 × 60 × 10 | (10, −5) → (70, 55) | 36000.00000 |
| `smaller` (60→45, DOF 0) | 12 | 45 × 45 × 10 | (10, −5) → (55, 40) | 20250.00000 |
| `freed` (равенство удалено, DOF 1) | 12 | 45 × 25 × 10 | (10, −5) → (55, 20) | 11250.00000 |
| `slanted-equal` (наклонные Lines) | 12 | 62.189217 × 49.208790 × 10 | — | 24907.54551 |
| `equal-ui` / `equal-cli` | 12 | 60 × 60 × 10 | (10, −5) → (70, 55) | 36000.00000 |

Проверяются именно **выбранные UUID**, а не «любые две совпавшие стороны»:
индекс каждой стороны берётся из `rule.a.from.curve_id` / `rule.b.from.curve_id`
опубликованного равенства, и обе решённые длины сравниваются между собой и с
60 (после замены — с 45). На наклонном профиле обе Lines дают евклидову длину
50 mm, причём `|dx|` и `|dy|` от 50 отличаются — это длина, а не проекция.
После удаления равенства решённая модель всё ещё держит H/V всех сторон,
ведущую длину 45 и закреплённую вершину в (10, −5); свободной осталась ровно
одна сторона (`freed`, extents 45 × 25).

Stored curves во всех копиях побайтово равны исходным: solve себя не записал.
`stored_same` проверяет metadata, dependency/topology facts, имена, parent,
ordinal и `validate`. Списки constraint UUID сравниваются целиком: равенство
добавляет ровно один UUID к прежним десяти; замена ведущей длины создаёт один
новый Distance UUID и **сохраняет UUID равенства** и порядок остальных;
удаление равенства оставляет ровно те же десять, включая Coincident closure,
H/V, длину и Fixed. Closure при удалении равенства не удаляется. Если равенство
— первое пользовательское ограничение, прежний маршрут создаёт все четыре
недостающих Coincident joints (document gate: 5 added, из них 4 closure).

Headless egui → настоящий worker отправил построенный виджетами request,
опубликовал копию, и peer CLI-процесс воспроизвёл тот же ordered request над тем
же source. UI и CLI дали **побайтово равные STL (684 B) и FBX (4137 B)**;
различаются только действительно новые constraint UUID (11 rules равны попарно,
11 id различны). Все SQL-таблицы, rowids, source claims и прочие cells равны,
кроме payload/hash/schema_version выбранного Sketch и required capability
`sketch.constraints.v1`. Артефакты worker-теста названы `equal-ui`/`equal-cli`,
чтобы не подменять прежние `ui`/`cli` других constraint tests.

Структурные отказы и настоящий solver проверены **отдельно**:

* structural (`error.kind:"input"`, без `constraint_conflict`, без output и без
  изменения каталога/source): self-pair; чужая Line второй стороной; два
  одинаковых add одной пары; add пары и её reversed-дубликата в одном запросе;
  дубликат против **сохранённого** равенства в обоих порядках;
* solver conflict (`error.kind:"constraint"` с непустым typed
  `constraint_conflict`, содержащим `equal_length`): H/V + несовместимые
  сохранённые длины 60/30 соседних сторон + равенство этой пары;
* solver redundancy (публикация проходит, `redundant_constraint_ids` = ровно
  UUID равенства): H/V + две длины 60/60 + Fixed + равенство той же пары.
  Это ответ настоящего solver, не синтетический успешный DTO.

Wire-отказы request (`a_curve_id` без `b_curve_id` и наоборот; лишние
`curve_id`/`distance_mm`/`at`; `null`, число, не-UUID строка вместо UUID;
`curve_id` вместо пары; неизвестный `equallength`) отвергаются до создания
kernel — исполнены именно на stub. Публикация с закрытым stdout по-прежнему
даёт exit 7 и сохраняет читаемую валидную копию (прежние gates).

Headless egui дополнительно проверяет, что новые поля не вытесняют
`Cancel constraints draft`, `Undo`, `Redo`, `Clear pending changes`,
`Add length`, `Replace length`, `Pin Start`/`Pin End`, `Add Fixed point`,
`Equal Line A`, `Equal Line B`, `Add Equal length` и `Save constraints copy…`
за пределы окна 988×768 при длинном списке pending additions
(`constraint_editor_keeps_actions_reachable_with_many_pending_additions`,
добавлен в CI как named gate).

## Чувствительность проверок

Две узкие направленные поломки **скомпилировались и дали исполнившиеся
провалы**; скрипты — `/private/tmp/ferrite-25h/mutate-*.py`:

1. Потеря/подмена второго curve: preparation строит
   `EqualLength { a: segment(a), b: segment(a) }`. Document gate увидел обе
   стороны на одной Line вместо двух — **7 passed / 1 failed**, сообщение
   «both sides are whole Lines, in stored order».
2. Удаление чужого constraint UUID: lookup заменён на `else { continue; }`,
   то есть removal, не принадлежащий выбранному Sketch, перестаёт отказываться —
   **6 passed / 2 failed**, включая новый §25H-кейс
   (`remove: [новый UUID]` + add пары) и прежний
   `ordered_requests_refuse_duplicates_foreign_ids_and_closure_removal_atomically`.

Оба раза источник восстановлен **побайтово** (sha256
`2f6c432694b24442fee2331b9e7d6ac3bb5640b33b61cdd46b8a9a44d571a780` до и после),
и положительные document/CLI/app gates повторены целиком. Compile failure и
zero tests доказательством не считались.

## Stub, reader и рецепт

Настоящий отдельный stub: target `/private/tmp/ferrite-25g-stub-target`, env
`/private/tmp/ferrite-25h/stub-env.sh` (`OpenCASCADE_DIR` и `FCAD_PLANEGCS_DIR`
указывают в несуществующие каталоги, DYLD/LD paths сняты,
`CMAKE_TOOLCHAIN_FILE` убирает префикс `/opt/homebrew` из `find_package`).
Проверено, а не предположено: `bridge-build/CMakeCache.txt` содержит
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` и
`CMAKE_IGNORE_PREFIX_PATH:STRING=/opt/homebrew`; build script напечатал
«Open CASCADE was not usable … Reason: configuring the bridge failed»; `otool -L`
свежего `ferritecad` показывает **0** OCCT/PlaneGCS импортов (только
`libiconv.2.dylib` и `libSystem.B.dylib`). Homebrew OCCT незаметно не подхвачен.

| Stub suite | Исполнено | Native skips | Лог |
| --- | ---: | ---: | --- |
| document `constraint_edit` | **8** | 0 | `stub-document-nocapture.log` |
| CLI `edit_constraints` | **3** | 5 | `stub-cli-nocapture.log` |
| app `constraints::tests::` | **9** | 4 | `stub-ui-nocapture.log` |
| eval (весь пакет) | **139** | 39 | `stub-eval-nocapture.log` |

Всего **159 исполненных** тестов и **48 явных skips**, посчитанных отдельно;
ни один kernel-unavailable путь не выдал геометрический успех. Новые
`native_equal_length_ties_…` и `native_equal_length_worker_…` честно скипнулись
здесь и честно исполнились в native кампании. Первая версия нового
non-native CLI-кейса ошибочно ожидала `input` для self-pair, который на stub
отказывается раньше как `unsupported`: кейс перенесён в native gate, где он
измеряется против настоящего документа.

Pinned ufbx 0.23.0 reader (`/private/tmp/ferrite-25g/read_production`, собран из
`tools/unity-fbx-smoke/scripts/read_production.c` с прежним pin) прочитал шесть
новых опубликованных FBX — `square`, `smaller`, `freed`, `slanted-equal`,
`equal-ui`, `equal-cli` — по **6 checks, 0 failures** на каждый. Список
`FCAD_SKETCH_CONSTRAINT_FBX_DIR` в `tools/check-fbx-complex.sh` дополнен
`square`, `smaller`, `equal-ui`, `equal-cli`; отдельного workflow и большого
STEP-корпуса нет.

Рецепт извлечён из текущего `docs/equal-line-lengths.md` и исполнен настоящим
CLI (`agent-recipe.py`, `agent-recipe.log`, exit 0). Итог:
`{"square_mm3": 36000, "smaller_mm3": 20250.0, "dof": {"one_size": 1,
"square": 0, "smaller": 0, "freed": 1}}` плюс UUID равенства и UUID, названного
solver избыточным. Рецепт включает независимое STL-измерение, pinned ufbx,
структурные и solver-отказы и проверку неизменности source по sha256 и mtime.

## CI-гейты нового diff

Новые exact-name/no-skip gates добавлены в **существующие** workflows на трёх ОС,
старые не ослаблены и тяжёлого workflow не появилось: обычный `ci.yml` получил
headless widget gate `equal_length_widgets_…`, gate доступности действий
`constraint_editor_keeps_actions_reachable_…` и document gate
`equal_length_pairs_…`; `runtime-layout.yml` получил native process gate
`native_equal_length_ties_…` и app worker gate `native_equal_length_worker_…`.
Все они проверяются на `skipped:` и на точную строку `test <name> ... ok`.
Локально подтверждено, что под `--nocapture` эта строка не разрывается: каждый
из пяти новых gates даёт ровно одну точную строку и ноль `skipped:`
(логи `gate-*.log`). Это проверка конфигурации, не исполненный удалённый CI.

## GUI и ограничения

**Интерактивный GUI smoke не запускался и отложен по прямому указанию
пользователя**; GUI не считается проверенным. Viewer, окна, GPU/render tests,
CUA, osascript, Unity и системные диалоги не запускались; экран не требовался.
Headless egui widget tests — не GUI smoke.

Причина прежнего OOM не установлена. Большой STEP/complex FBX-корпус, Unity,
полный GPU suite, полный workspace test suite и Linux/Windows локально не
запускались. `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался. Coordinate drag и
Snap constrained Sketch остаются запрещены; окружностей, дуг, отверстий,
Parallel, Perpendicular, live solver drag, in-place Save, persistent undo, новой
схемы и FFI в срезе нет. Чужие процессы не останавливались, настройки сна и
блокировки не менялись.

Весь diff оставлен unstaged/uncommitted; staging/commit/push/PR/merge не
выполнялись. Следующий шаг — независимое ревью. Следующий срез не начат.

## Полный состав diff

Все 16 файлов перечислены ниже; `new` — untracked, учитывается как добавление
целого файла. Индекс пуст. Ветка `equal-line-lengths`, HEAD остаётся точной
базой выше.

| Файл | + | − | Статус |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 2 | 2 | modified |
| `.github/workflows/runtime-layout.yml` | 2 | 2 | modified |
| `README.md` | 8 | 3 | modified |
| `crates/ferritecad-app/src/constraints.rs` | 626 | 77 | modified |
| `crates/ferritecad-cli/src/edit_constraints.rs` | 17 | 3 | modified |
| `crates/ferritecad-cli/tests/edit_constraints.rs` | 387 | 0 | modified |
| `crates/ferritecad-document/src/sketch_constraints.rs` | 122 | 44 | modified |
| `crates/ferritecad-document/tests/constraint_edit.rs` | 252 | 4 | modified |
| `crates/ferritecad-jobs/src/edit.rs` | 2 | 2 | modified |
| `docs/cli-capabilities.md` | 3 | 1 | modified |
| `docs/cli-json-v1.md` | 24 | 0 | modified |
| `docs/implementation-plan.md` | 37 | 12 | modified |
| `docs/sketch-constraints-copy.md` | 54 | 12 | modified |
| `tools/check-fbx-complex.sh` | 1 | 1 | modified |
| `docs/equal-line-lengths.md` | 246 | 0 | new |
| `docs/equal-line-lengths-verification.md` |      275 | 0 | new |

Итого: **16 файлов, 2058 добавлений, 163 удаления**, включая untracked.
Снимки `final-status.log` и `diffstat-rows.txt` — в `/private/tmp/ferrite-25h/`.

## Независимое ревью §25H

Повторная проверка Codex в `/private/tmp/ferrite-pr36-review/` не выявила
дефекта production-маршрута. Дополнены два существующих теста: после замены
ведущего Distance сравниваются все retained constraints целиком (UUID, rule,
порядок), а reversed SegmentRef сохраняется также при успешном добавлении и
удалении соседнего H/V. Исправлены счётчик CI базы (27/27) и учёт трёх
stub-only not-applicable в domain suite.

Последовательно исполнены fmt, workspace clippy all-targets/all-features с
`-D warnings`, свежая сборка CLI/app, domain 373 harness-passed (370 исполненных,
3 stub-only not-applicable, 1 старый ignored benchmark), CLI 49 и headless app
46 (constraints 13, sketch 21, edit 8, STL 4). После усиления assertions повторены
exact document gate, весь CLI constraint suite 8/8, exact EqualLength worker,
fmt и clippy. Новые обязательные native gates без skips. Окна не запускались.

Новый reader собран из текущего `read_production.c` и проверенного pin ufbx;
`square`, `smaller`, `freed`, `slanted-equal`, `equal-ui`, `equal-cli` прочитаны
по 6 checks, 0 failures. Текущий Python-рецепт извлечён из Markdown и исполнен
настоящим CLI: 36000 → 20250 mm³, DOF 1 → 0 → 0 → 1. GUI/CLI здесь означает
headless widget/worker и CLI, а не интерактивный GUI smoke.

Отдельная настоящая stub-сборка: 20 исполненных проверок (document 8, CLI 3,
headless app 9) и 9 явных native skips. Текущий CMakeCache содержит
`OpenCASCADE_DIR-NOTFOUND`, imports свежего CLI — только libiconv/libSystem.

CI опубликованного head и merge проверяется отдельно после публикации;
локальные результаты не заменяют Linux/Windows. Следующий §25I записан pending.
