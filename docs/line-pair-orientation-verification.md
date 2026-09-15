# §25I — реализация и локальная проверка

2026-09-14. База `00398078a5357078fe6d5e05c21a5e70b9812214`, PR #36 MERGED
(2026-09-15T02:46:45Z, ветка `equal-line-lengths`). После `git fetch --all
--prune` HEAD/main/origin/main совпадали, дерево было чистым. Ветка
`line-pair-orientation` (без префикса `codex/`). CI точного merge SHA
перепроверен: **6/6 workflows success** (CI, combined runtime layout,
planegcs pin, product sbom, rust sbom, rust notices). Это **CI базы**; CI нового
незакоммиченного diff не запускался и базе не приписывается. Чужой detached
worktree `/Users/drt/.codex/worktrees/a200/ferrite-cad`, fixtures и чужие
изменения не затрагивались. При передаче реализации ничего не было staged,
commit или push; независимое ревью описано ниже.

## Предметный результат

[Точный контракт](sketch-constraints-copy.md),
[исполняемый рецепт](line-pair-orientation.md).

Parallel и Perpendicular — **относительная ориентация**, а не направление.
У связи нет числа и нет угла в градусах, она не подменяется Horizontal/Vertical
и не переписывает stored координаты. Поэтому она держится на наклонном профиле,
а профиль остаётся свободным вращаться: контракт — длины, нормированные
cross/dot выбранных сторон, площадь и объём, а не конкретные XY и не
axis-aligned extents. Три вещи по-прежнему разделены: stored coordinates
(inputs solver), solved coordinates (presentation одного rebuild, измеряется по
телу) и degrees of freedom (измеренный результат solve).

Document расширяет прежний managed класс существующими
`SketchConstraintRule::Parallel { a, b }` и `::Perpendicular { a, b }` с двумя
`SketchSegmentRef` Start→End. Request-тип остался честным: прежний
типобезопасный `AddLineConstraint` получил третий вариант
`Relation { a, b, relation }` с новым `LineRelation::Parallel|Perpendicular`.
Фиктивного UUID, скрытой второй стороны, клиентской таблицы связей и второго
вычисленного размера нет — пара выражена двумя полями, вид связи третьим.
Слот занятости получил пятую форму `Slot::Relation(min, max)`: `(A,B)` и `(B,A)`
— один слот, а сохранённый `SegmentRef`, записанный End→Start, читается общим
`whole_line`/`related` как та же Line и **не** переписывается канонизацией.
**Политика занятости**: Parallel и Perpendicular — два ответа на один вопрос об
этой паре и делят один слот, поэтому повторный тот же ответ, другой ответ и
reversed-дубликат любого из них отказываются структурно до solver;
`EqualLength` на той же паре — отдельный вопрос, отдельный слот и отдельный
UUID. Self-pair отказывается отдельным структурным сообщением; членство обеих
Lines в выбранном Sketch проверяется для каждой стороны. Собственного анализа
транзитивности, ранга и косвенных конфликтов нет. Существующий перевод
Parallel/Perpendicular в solver (`ferritecad-eval/src/solve.rs`), прежний narrow
writer, jobs `edit_sketch_constraints_copy`, snapshot/version guard,
source/alias/no-clobber, close SQLite до публикации, cancellation, cleanup и
late-cancel policy не менялись. Runtime dependencies, схема БД, DDL, FFI и
`Cargo.lock` не менялись. Поведение redundancy/constraint conflict не менялось.

CLI request v1 аддитивно принимает точные формы
`{"rule":"parallel","a_curve_id":UUID_A,"b_curve_id":UUID_B}` и
`{"rule":"perpendicular",…}`. Старые H/V/Distance/Fixed/EqualLength формы, лимит
65536 bytes, 512 изменений, UTF-8 правила, closed-pipe поведение,
operation/schema v1 и коды 0/2/7 сохранены. Response обеих связей — прежний DTO
`a`/`b` Segment с endpoint refs; входные `a_curve_id`/`b_curve_id` в ответ не
попадают, поля `distance` у связи нет. Единственный fallible emitter прежний.

UI `Edit constraints`: прежний выбор пары EqualLength переименован в
нейтральную строку `Line pair:` с `Pair Line A`/`Pair Line B` и показом
`Pair: A <id> · B <id>`, а строка действий даёт `Add Equal length`,
`Add Parallel` и `Add Perpendicular` над **одной** выбранной парой. Второго
каталога и второго выбора пары нет — это и есть переиспользование §25H, а
нейтральное имя строки убирает единственную оставшуюся неясность («Equal Line A»
как сторона Parallel). Выборы вне истории; один успешный Add — один прежний
bounded `History::change`. Persisted связь показана как `Parallel · UUID` либо
`Perpendicular · UUID` и `Lines <a> and <b>`, не прежним fallback-ярлыком
`Coincident closure`, и удаляется по exact identity.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`, GPU unset,
loader-failure probes unset. Env/target: `/private/tmp/ferrite-25g/native-env.sh`,
`/private/tmp/ferrite-24b-native-target`. Существующие pinned OCCT 8.0.1 и
PlaneGCS переиспользованы; OCCT из исходников не пересобирался. Peer CLI собран
перед app worker gates. Команды — `/private/tmp/ferrite-25i/native-tests.sh`.

| Проверка | Результат | Лог в `/private/tmp/ferrite-25i/` |
| --- | --- | --- |
| fmt, workspace clippy all-targets/all-features `-D warnings`, `git diff --check` | pass, 0 warnings | `fmt.log`, `clippy.log` |
| document/jobs/eval `--all-features` | **374 harness-passed: 371 исполненный + 3 stub-only not-applicable**, 1 прежний ignored benchmark | `domain-tests.log` |
| CLI binary/create/edit/Sketch/JSON/constraints | **50 passed**, 0 skips | `cli-tests.log` |
| App `constraints::tests::` | **15 passed**, 0 skips | `app-constraints__tests__.log` |
| App `sketch::` | **21 passed** | `app-sketch__.log` |
| App `edits::tests::` | **8 passed** | `app-edits__tests__.log` |
| Jobs cancellation/race/cleanup gate | **1 passed** | `jobs-faults.log` |
| licence headers (336 файлов), export boundary, actionlint изменённых workflows | pass | `licence.log`, `export-boundary.log` |

Новые критические named gates, исполненные по exact name с `--nocapture`
(чистая строка `test … ok`, без диагностического stdout посреди неё):
`native_line_relations_orient_two_lines_and_removal_restores_the_free_dimension`,
`constraints::tests::native_line_relations_worker_and_cli_orient_the_same_solved_body`,
`constraints::tests::line_relation_widgets_pick_two_lines_and_name_the_persisted_relation`,
`line_relations_are_checked_stored_and_removed_without_touching_other_ids`.
Все прежние H/V, length, Fixed и EqualLength gates исполнены без ослабления;
правило `geometry_checked`, требующее пустой `redundant()`, не смягчалось.

Одна прежняя проверка изменена по существу: в
`constraint_discovery_separates_document_and_feature_refusals` примером
неподдерживаемой family служил `Perpendicular` между двумя целыми Lines —
ровно то, что §25I теперь поддерживает. Пример заменён на arbitrary
point-to-point `Distance` между разными Lines, которая по-прежнему вне managed
класса; смысл проверки (локальный family refusal против документного) сохранён.

## Geometry, identity, refusal и delivery

Настоящий CLI-процесс над **наклонным** профилем
`[[-20,-10],[40,-8],[42,30],[-20,30]]`, высота 10 mm. Прямоугольник 60 × 30 mm
собран **из одних отношений**: `Parallel(L0,L2)`, `Parallel(L1,L3)`,
`Perpendicular(L0,L1)` и две длины. Ни одного H/V в этой системе нет.

Обоснование степеней свободы (не назначено заранее, а подтверждено solver):
замкнутый четырёхугольник имеет 8 плоских степеней свободы (4 вершины × 2);
две Parallel и одна Perpendicular задают форму прямоугольника — 3 независимых
условия; две длины задают размер — ещё 2. Остаётся **DOF 3**: две степени
положения и одна поворота. Независимость выбранных пяти подтверждается тем, что
настоящий solver вернул `redundant_constraint_ids: []`; когда к той же системе
добавлен четвёртый угол `Perpendicular(L1,L2)`, solver назвал именно его
избыточным и DOF остался 3.

Независимый разбор опубликованного STL (`/private/tmp/ferrite-25i/artifacts`):

| Публикация | DOF | triangles | стороны основания mm | extents mm | объём mm³ |
| --- | ---: | ---: | --- | --- | ---: |
| `relations-rect` | 3 | 12 | 60 / 30 / 60 / 30 | 60.353163 × 30.712717 × 10 | 17999.99951 |
| `relations-smaller` (60→45) | 3 | 12 | 45 / 30 / 45 / 30 | 47.548145 × 33.978452 × 10 | 13500.00004 |
| `relations-freed` (Perpendicular удалён) | 4 | 12 | 45 / 30 / 45 / 30 | 52.610277 × 33.756747 × 10 | 12574.74626 |
| `relation-ui` / `relation-cli` | 3 | 12 | 60 / 30 / 60 / 30 | 60.353163 × 30.712717 × 10 | 17999.99951 |

Объём 18000 mm³ и 13500 mm³ измерен интегрированием треугольников
опубликованного STL, независимо от solver. Axis-aligned extents **не
обещаются**: они больше 60 и 30 именно потому, что профиль остался повёрнутым,
и именно это отличает настоящую относительную ориентацию от подмены H/V.
У `relations-freed` стороны сохранены (45 и 30), а площадь меньше — угол
освободился; исходные координаты при удалении связи не обещаются и не
проверяются.

Проверяются именно **выбранные UUID**, а не «любые подходящие стороны»: индекс
каждой стороны берётся из `rule.a.from.curve_id` / `rule.b.from.curve_id`
опубликованной связи, направление каждой стороны нормируется, и предварительно
проверяется ненулевая длина (`> 1 mm`, иначе связь была бы удовлетворена
вырождением). Для Parallel измеряется `|cross| < 1e-7` и `|dot| > 1 − 1e-7`, для
Perpendicular — `|dot| < 1e-7` и `|cross| > 1 − 1e-7`. Дополнительно у каждой
стороны `|ux| > 0.01` и `|uy| > 0.01`: ни одна сторона не легла на ось.

Stored curves во всех копиях побайтово равны исходным: solve себя не записал.
`stored_same` проверяет metadata, dependency/topology facts, имена, parent,
ordinal и `validate`. Списки constraint UUID сравниваются целиком: замена
ведущей длины 60→45 создаёт один новый Distance UUID и **сохраняет UUID обеих
Parallel и Perpendicular**, а также правила и порядок всех остальных; удаление
exact `Perpendicular` оставляет ровно те же восемь UUID в том же порядке,
включая Coincident closure. Если связь — первое пользовательское ограничение,
прежний маршрут создаёт все четыре недостающих Coincident joints (document
gate: 5 added, из них 4 closure).

Headless egui → настоящий worker отправил построенный виджетами request
(`Pair Line A`/`Pair Line B` + `Add Parallel`/`Add Perpendicular` + две длины),
опубликовал копию, и peer CLI-процесс воспроизвёл тот же ordered request над тем
же source. UI и CLI дали **побайтово равные STL (684 B, sha256 `e43f7f49…`) и
FBX (6028 B, sha256 `d649720e…`)**; различаются только действительно новые
constraint UUID (9 rules равны попарно, 9 id различны). Все SQL-таблицы, rowids,
source claims и прочие cells равны, кроме payload/hash/schema_version выбранного
Sketch и required capability `sketch.constraints.v1`. Артефакты worker-теста
названы `relation-ui`/`relation-cli`, а process-gate — `relations-*`, чтобы не
подменять прежние `ui`/`cli`/`equal-*` других constraint tests.

Структурные отказы и настоящий solver проверены **отдельно**:

* structural (`error.kind:"input"`, без `constraint_conflict`, без output и без
  изменения каталога/source): self-pair обоих видов; чужая Line второй
  стороной; тот же ответ повторно в обоих порядках; **другой** ответ на ту же
  пару в обоих порядках; Parallel и Perpendicular одной пары в одном запросе;
  remove неизвестного UUID вместе с add; сохранённый reversed `SegmentRef`
  как дубликат;
* независимость свойств: `EqualLength` на паре, уже держащей ориентацию,
  публикуется и добавляет ровно один UUID к прежним;
* solver conflict (`error.kind:"constraint"` с непустым typed
  `constraint_conflict`, содержащим и `parallel`, и `perpendicular`):
  прямоугольная система + `Parallel(L1,L2)`, то есть требование, чтобы соседние
  стороны были одновременно перпендикулярны и параллельны;
* solver redundancy (публикация проходит, `redundant_constraint_ids` = ровно
  UUID четвёртого угла): прямоугольная система + `Perpendicular(L1,L2)`.
  Это ответ настоящего solver, не синтетический успешный DTO.

Wire-отказы request (`a_curve_id` без `b_curve_id` и наоборот для обоих rules;
лишние `curve_id`/`distance_mm`/`at`/`future`; `null`, число, bool, не-UUID
строка вместо UUID; `curve_id` вместо пары; `Parallel` и `perpendicularity`
вместо точного имени; `a_curve_id`/`b_curve_id` у `horizontal` и `distance`)
отвергаются до создания kernel — исполнены именно на stub. Публикация с закрытым
stdout по-прежнему даёт exit 7 и сохраняет читаемую валидную копию (прежние
gates).

Headless egui дополнительно проверяет, что новые действия не вытесняют
`Cancel constraints draft`, `Undo`, `Redo`, `Clear pending changes`,
`Add length`, `Replace length`, `Pin Start`/`Pin End`, `Add Fixed point`,
`Pair Line A`, `Pair Line B`, `Add Equal length`, `Add Parallel`,
`Add Perpendicular` и `Save constraints copy…` за пределы окна 988×768 при
длинном списке pending additions
(`constraint_editor_keeps_actions_reachable_with_many_pending_additions`).

## Чувствительность проверок

Две узкие направленные поломки **скомпилировались и дали исполнившиеся
провалы**; скрипты — `/private/tmp/ferrite-25i/mutate-*.py`:

1. Потеря второй стороны: preparation строит
   `Relation { a, b }` как `(segment(a), segment(a))`. Document gate увидел обе
   стороны на одной Line вместо двух — **8 passed / 1 failed**, сообщение
   «both sides are whole Lines, in stored order» (`mutation-1.log`).
   Та же потеря, перенесённая в перевод solver, ловится измерением: DOF
   опубликованной копии стал 5 вместо 3 (`mutation-1b.log`).
2. Неверный вид отношения: перевод solver сообщает `Parallel` вместо
   `Perpendicular` и наоборот, при неизменных stored kind, UUID и wire DTO.
   Native process gate провалился на первом же измеренном шаге: вместо
   публикации `exit 0` пришёл `exit 2` с настоящим solver conflict — такой
   системы не существует (`mutation-2.log`). Вариант, остающийся разрешимым
   (`Parallel` переведён как `EqualLength`), был пойман измерением
   избыточности: `redundant_constraint_ids` стал пуст вместо названного
   четвёртого угла (`mutation-2b.log`); при этом cross/dot-измерения
   исполнились и справедливо прошли, так как геометрия действительно осталась
   прямоугольной.

Оба раза источники восстановлены **побайтово**: sha256
`6ecaff9443547d7756961a26a1b101ad047db2b1ba596559f29da68e11f0bc00`
(`sketch_constraints.rs`) и
`8ecaa4d47e6eed0747b828bf0f1509f60d04bc3f8b2036ddb12a2a731891db8b`
(`solve.rs`) до и после, и весь положительный campaign повторён целиком.
Compile failure и zero tests доказательством не считались. Новой
mutation-инфраструктуры не создавалось.

## Stub, reader и рецепт

Настоящий отдельный stub: target `/private/tmp/ferrite-25g-stub-target`, env
`/private/tmp/ferrite-25i/stub-env.sh` (`OpenCASCADE_DIR` и `FCAD_PLANEGCS_DIR`
указывают в несуществующие каталоги, DYLD/LD paths сняты,
`CMAKE_TOOLCHAIN_FILE` убирает префикс `/opt/homebrew` из `find_package`).
Подтверждено, что stub настоящий: `CMakeCache.txt` содержит
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`, а `otool -L` собранного
`ferritecad` не показывает ни одного TK/OCC/planegcs import.

| Stub-проверка | Исполнено | Explicit skips |
| --- | --- | --- |
| `ferritecad-document --test constraint_edit` | 9 passed | 0 |
| `ferritecad-cli --test edit_constraints` | 3 исполнены (9 harness-passed) | **6** native-only |
| `ferritecad-app constraints::tests::` | 10 исполнены (15 harness-passed) | **5** native-only |
| `ferritecad-eval` | 139 исполнены (178 harness-passed) | **39** требуют solver |

Независимый повтор с `--nocapture` уточнил суммарный результат: **161 исполненная
проверка и 50 explicit skips**. Поимённые skips находятся в
`/private/tmp/ferrite-pr37-review/stub-{cli,ui,eval}.log`; исходный отчёт
ошибочно называл все 178 eval harness passes исполненными. Harness-passed со skip **не** считается
доказательством геометрии — геометрию доказывает native-таблица выше.

Рецепт извлечён из Markdown скриптом
`/private/tmp/ferrite-25i/extract_recipe.py` по маркеру
`FCAD_25I_AGENT_RECIPE` и исполнен настоящим CLI без разбора prose
(`agent-recipe.log`): DOF 3 / 3 / 4, 18000 mm³ и 13500 mm³, стороны и
cross/dot основания, отказы, redundancy и conflict. Свежий pinned ufbx
(`fetch_ufbx.sh`, commit `fcc5d6ba…`, ufbx 0.23.0) собран заново в
`/private/tmp/ferrite-25i/read_production` и прочитал реальные небольшие FBX:
`FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0`.

## Что не проверялось

* **Интерактивный GUI** не запускался по указанию пользователя: ни окна, ни
  event loop, ни bundle, ни CUA/osascript, ни GPU/render tests. Headless egui
  widget tests за GUI не выдаются.
* **При передаче реализации CI нового diff не запускался.** Приведённые
  6/6 workflows относятся к базе `0039807`. Удалённые результаты опубликованного
  head и merge фиксируются отдельно в PR; локальные проверки за них не выдаются.
* **Большой STEP/complex FBX корпус** локально не повторялся; существующий
  remote runtime workflow остаётся владельцем этой кампании.
* Причина прежнего OOM неизвестна и здесь не исследовалась.


## Независимое ревью

Ревью на той же базе проверило route от request через сохранённые refs к solver,
shared job и ответу CLI. Ошибок предметной реализации не найдено. Исправлены
два пробела проверки и отчёт:

- CLI gate теперь прямо сравнивает вид и обе стороны каждой опубликованной
  связи с исходным request, прежде чем измерять cross/dot. Самосогласованного
  ответа про другую допустимую пару недостаточно. Response также проверяется
  на отсутствие обоих request-only полей `a_curve_id`/`b_curve_id`.
- Прежняя reader-кампания CI не читала пять новых FBX. В существующий список
  добавлены `relations-rect`, `relations-smaller`, `relations-freed`,
  `relation-ui`, `relation-cli`; новый тяжёлый импорт или workflow не создавался.
  Теперь список constraint FBX содержит 16 файлов; с прежними production и
  drag публикациями runtime должен выполнить 21 strict ufbx чтение на каждой ОС.
- Уточнены исполненные domain/stub проверки. Переданный diffstat +1592/−106
  относился к 13 tracked файлам; два новых документа добавляли ещё 576 строк.

Независимый native повтор: fmt, workspace clippy всех targets/features,
371 исполненная domain-проверка (3 stub-only not-applicable и 1 прежний ignored
benchmark отдельно), 50 CLI, 48 headless app (15 constraints + 21 sketch +
8 edit + 4 STL), без native skips в обязательных gates. После усиления assertions
повторены exact CLI и worker gates и clippy. Actionlint, shellcheck и whitespace
прошли. Native libraries переиспользованы; peer CLI собран перед worker.

Настоящий stub проверен свежей сборкой: CMakeCache NOTFOUND, `otool -L`
показывает только системные библиотеки. Исполненные проверки и skips приведены
выше. Рецепт заново извлечён из текущего Markdown и выполнен настоящим CLI:
DOF 3/3/4, объёмы 18000 и ≈13500 mm³, реальные redundancy/conflict.
Свежий pinned reader независимо прочитал все 16 небольших constraint FBX,
каждый — 6 checks, 0 failures. Логи и артефакты независимого повтора:
`/private/tmp/ferrite-pr37-review/`.

Локально viewer, окна, GPU, Unity и большой STEP-корпус не запускались.
Причина прежнего OOM остаётся неизвестной. Публикация и CI выполняются после
этого локального ревью и учитываются по точным SHA отдельно.
