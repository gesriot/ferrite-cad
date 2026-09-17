# §25O — локальные доказательства и ограничения

Контракт среза: [параметрическое кольцо](annular-circle-constraints.md).

База: `main = origin/main = dd63718ff393ba4b7f886e717d793662b6b9a1fd` (merge
PR #42, head `5700f100c6a6c93660696e7f05d8a1249b5303da`, base
`d61afac985e5e729ed715b5f35757b0b6782a995`), дерево
`8520c10455bf1b0666f635b19bba43e8ca92ec3c`; деревья head и merge совпадают.
На этом merge, по протоколу ревью §25N, 27/27 check-runs и 6/6 workflows —
success. Ветка `annular-circle-constraints`, 0 коммитов впереди; изменения
оставлены unstaged/uncommitted. **CI нового diff не запускался и пройденным не
объявляется.**

Окружение: macOS (Darwin 24.6.0), aarch64-apple-darwin, Open CASCADE 8.0.1 и
planegcs (FreeCAD 1.0.1) из уже собранных закреплённых деревьев в `vendor/`,
`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `--test-threads=1`,
сборки последовательно. **Ни OCCT, ни planegcs, ни собственный shim не
пересобирались** — их входы не менялись: этот срез не тронул ни solver-крейт
(кроме теста), ни C ABI, ни bridge. Переиспользованы существующие targets
`/private/tmp/ferrite-24b-native-target` и `/private/tmp/ferrite-25j-stub-target`;
новых не заводилось. Диск перед прогонами 9.5 ГиБ свободно, после всех прогонов
8.7 ГиБ; swap 2709/4096 МиБ (в начале среза 2725 МиБ, роста нет). Чужие процессы
и кэши не трогались, чужой worktree `/Users/drt/.codex/worktrees/a200/ferrite-cad`
не открывался.

Логи и артефакты этого среза — в `$S =
/private/tmp/claude-501/-Users-drt-Desktop-github-ferrite-cad/f12cd650-51d7-4a48-b0a6-dde509d95c27/scratchpad`:
`$S/ci/*.log` (исполненные упакованные CI-команды), `$S/recipe.py` (рецепт,
извлечённый из Markdown по маркеру), `$S/read_production` (собственный pinned
ufbx reader), `$S/runner/annular-constraints/annular-constraint-cli.fbx`
(артефакт, прочитанный этим reader), `$S/pristine/SHA256` (контрольные суммы
двух файлов, которые ломались и восстановлены — `shasum -a 256 -c` проходит).

## Что оказалось не нужно

**Solver и C ABI не менялись.** Концентричность — это прежний
`Constraint::Coincident` двух центров, а центр окружности стал адресуемой точкой
эскиза ещё в §25N. Новый тест в `tests/circles.rs` — единственное изменение в
крейте solver'а; `planegcs_shim.cpp/.h`, `FcGcsConstraint`, `FcGcsCircle` и
`fc_gcs_session_*` не тронуты, shim не пересобирался.

**Схема и capability не поднимались.** Проверено первым, прежде чем что-то
писать: `Sketch::schema_version` ставит v3 по `needs_circle_vocabulary()`, а это
правило уже смотрит на **любой** `SketchPointSelector::Center` среди точек
правила, а не только на `Fixed`. Значит `Coincident` двух центров сам поднимает
Sketch до v3 и требует `sketch.constraints.circle.v1`. Исполняемое
подтверждение — `a_shared_centre_is_removable_and_is_the_only_removable_coincident`:
Sketch с **одной** концентричностью сообщает `schema_version() == 3` и
`required_capabilities() == [core.part.v1, sketch.constraints.v1,
sketch.constraints.circle.v1]`; после снятия — снова v1. Новое имя capability
добавило бы слово и ни одного отказа, поэтому не добавлено.

**Второго copier и второго solver нет.** Прежний
`edit-sketch-constraints-copy`, прежние discovery snapshot/version, подготовка,
writer, jobs, async worker и публикация.

## Измеренные степени свободы

Читаются у библиотеки в тесте, не сравниваются с записанной заранее константой
(`a_concentric_pair_loses_its_freedom_step_by_step_and_moves_as_one`). Две
окружности намеренно начинаются в **разных** местах — (12, −7) r10 и (40, 40)
r4 — потому что пара, уже стоявшая в одном центре, двигалась бы вместе при любой
реализации `Coincident` и ничего бы не доказала.

| Что | DOF |
| --- | --- |
| две свободные окружности | 6 |
| + `Concentric` | 4 |
| + два `Radius` | 2 |
| + `Fixed` одного центра | 0 |

Свобода возвращается при снятии, измеренная снова. Закрепление центра в двух
разных местах — (−3.5, 4.25) и (100, −50) — каждый раз приводит **оба** центра
ровно туда: закреплён один, названы оба. Сессии не текут
(`native_live_sessions() == 0`).

## Измеренная геометрия и identity

Источник: `create-annular-extrude` (12, −7) / R10 / r4 / h15. Запрос:
`concentric(R, r)` + `radius 6.75` + `radius 2.125` + `fixed center (−3.5, 4.25)`
**только у внешней окружности**. `solve.degrees_of_freedom = 0`, четыре новых
UUID, источник побайтово цел.

После close/reopen + cold rebuild копии: `solid, 4 named faces`,
`4 of 4 stored references resolved`, `Cylinder{6.75}` под прежним `ExtrudeSide`
ref внешней `Circle` и `Cylinder{2.125}` под ref внутренней, два `Plane`, объём
в пределах 1e-6 от π(R−r)(R+r)h. Сохранённый Sketch по-прежнему (12, −7)/10/4 —
solver-ответ не переписал приближение; порядок кривых, все refs, dependencies и
число объектов целы. Каждая SQL-ячейка копии сверена с явным allowlist
(payload, payload_hash и schema_version выбранной строки `objects`, новые строки
`capabilities` и `meta.modified_at`); **сохранённая геометрия обеих окружностей
в allowlist не входит**, поэтому её неизменность проверена, а не предположена.

Роли после решения сохранены: discovery копии называет ту же окружность
`boundary` и ту же `bore` под теми же UUID.

Независимый разбор binary STL при `--linear-deflection 0.05 --angular-deflection
0.1` проверяет замкнутость (каждое направленное ребро ровно раз, у каждого есть
обратное), обе окружности вокруг **решённого** центра по фактическим хордам,
ориентацию стенки отверстия внутрь и внешней наружу, отсутствие треугольника,
накрывающего ось, и объём против площади, которую замыкают его **собственные**
измеренные периметры — аналитического π от меша не требуется. Рецепт печатает
`FCAD_25O_RECIPE_OK 1933.486848 1569.16405`: первое — mesh-объём кольца
6.75/2.125 (точный π(R−r)(R+r)h = 1934.28), второе — после замены отверстия на
3.5 (точный 1569.94). Оба ниже точного тела, как и положено вписанной призме, и
далеко ниже сплошного цилиндра.

FBX прочитан **собственным** pinned ufbx (сборка из
`tools/unity-fbx-smoke/scripts/read_production.c` с пином `fetch_ufbx.sh`,
`fcc5d6ba…`): `checks=6 failures=0`. Через прежний reader loop
`tools/check-fbx-complex.sh` — тот же файл, маркер
`FCAD_ANNULAR_CONSTRAINT_UFBX_EXECUTED`, и все прежние маркеры на месте.

Документ с **переставленными** stored curves (отверстие записано первым) правится
с теми же UUID в тех же ролях и даёт ту же геометрию
(`native_stored_order_does_not_decide_the_roles`).

Замена одного радиуса (2.125 → 3.5) — один атомарный remove/add: концентричность,
pin и все прочие UUID целы, геометрия становится другой, настоящие cache
Miss/Hit на одном `CacheStore` дают новую геометрию, а не запись предыдущей.
`edit-extrude` 15 → 25 сохраняет все четыре ограничения и решённые радиусы.

## Снятие ограничений

Концентричность показана отдельной строкой и снимается по точному UUID. Что
решает, можно ли её снять, — остаётся ли результат поддерживаемым кольцом:

* снять её, оставив pin, который увёл центр внешней окружности из сохранённого
  приближения, — **атомарный отказ** без публикации (решённые центры разъезжаются);
* снять её **вместе с pin'ом** — публикация, `degrees_of_freedom = 4`, оба центра
  возвращаются к общему сохранённому приближению;
* снять всё — `solve: null`, `annulus_edit.available: true`, геометрия — в точности
  сохранённое приближение. Baseline cold rebuild источника и strict-проверка refs
  выполняются и здесь: это общий copy job, и OCCT без solver не может редактировать
  ограниченный источник (измерено в §25N и снова здесь).

## Отказы без публикации

Каждый проверен настоящим процессом; ни один не оставил файла, и источник после
каждого побайтово цел.

Структурные (исполняются **без ядра и без solver**, фикстура через Document API,
оба порядка хранения): ноль и отрицательный радиус, два радиуса на одной
окружности, `(A,B)` и `(B,A)` в одном запросе (один слот), концентричность с
самой собой, чужая кривая в паре, чужой радиус, два pin, Line-правило на кольце,
Line-длина, endpoint-pin на окружности, удаление отсутствующего UUID, пустой
запрос, `request_version: 2`, `concentric` без полей, с лишним полем, с
`curve_id` вместо пары, `1e999`. Плюс exit 7 при закрытом stdout на отказанной
операции.

Числовые и solver-зависимые: переход радиусов через друг друга, стенка тоньше
`MIN_WALL_MM`, неконцентричный результат (pin без концентричности), радиус за
пределом 1 000 000 mm.

**Настоящий solver conflict в этом срезе структурно недостижим через product
request**, и это названо, а не обойдено: два разных радиуса на одной окружности
— занятый слот, отказ подготовки; два одинаковых — тоже. Пара, которую solver
назвал бы конфликтующей, требует набора, который managed-класс не принимает.
Настоящий `Outcome::Conflicting` на окружностях измеряется там, где его можно
поставить, — на solver boundary (`two_different_radii_on_one_circle_conflict…`,
§25N), и product request ради теста не расширялся.

## Writer

Правило «каждая Coincident-связь, которая была, осталась» уточнено до того, что
оно охраняет — **замыкание Line-профиля** — и одновременно усилено: сохранённая
закрывающая связь должна вернуться под своим UUID **и с тем же правилом**.
`a_forged_plan_may_not_drop_or_rewrite_a_line_closure_link` выполняется внутри
крейта (снаружи `PreparedSketchConstraints` не даёт подменить payload) и
отказывает трём подделкам: связь удалена; связь под тем же UUID переписана как
концентричность двух центров, то есть в снимаемый вид; связь под тем же UUID
переставлена на два других конца. Честный план после каждой попытки пишется, и
документ между попытками не меняется.

## Исполненные проверки

Всё ниже — native сборка (OCCT + planegcs), `--test-threads=1`.

`ferritecad-sketch-solver` — 4 suites, 34 теста (33 прежних + 1 новый в
`tests/circles.rs`), без skips. `ferritecad-document` — 6 suites: lib 56
(+3 новых `constraint_write_tests`), `constraint_edit` 9, `document` 32,
`imported_step` 29, `sketch_constraints` 30, `snapshot` 9.
`ferritecad-eval` — 10 suites, 185. `ferritecad-jobs` — 53.
`ferritecad-app` — 315 (+2 новых). `ferritecad-ui` — 102.
`ferritecad-scene`/`export`/`occt`/`topology`/`types`/`kernel`/`exchange` — все
зелёные, без изменений.

`ferritecad-cli`: новый `annular_constraints`, 5 тестов, из них 4 исполнены в
native сборке:

* `annular_constraint_discovery_and_protocol_without_native` — фикстура через
  Document API, **оба** порядка хранения; discovery, роли, все структурные
  отказы и отвергаемые формы запроса, exit 7. Ни один не написал байта.
* `native_concentric_radii_and_a_pinned_centre_drive_the_hollow_solid` — вся
  измеренная история выше, включая SQL allowlist, cache Miss/Hit, замену
  радиуса, отказ и разрешённое снятие концентричности, снятие всего и
  `edit-extrude`.
* `native_stored_order_does_not_decide_the_roles`.
* `native_annular_constraint_refusals_are_atomic`.
* `occt_without_solver_builds_plain_annuli_and_refuses_annular_constraints` —
  исполняется в смешанной сборке, см. ниже.

Прежние CLI suites повторены: `circle_constraints` 4, `edit_constraints` 9,
`edit_annular` 6, `annular_extrude` 8, `circle_extrude` 5, `edit_circle` 5,
`edit_sketch` 5, `edit_extrude` 2, `create` 9, `export_stl` 9, `export_fbx` 9,
`print_topology` 8, `rebuild` 9, `sketch_extrude` 4, `validate` 4.

Headless UI — два новых gate через **настоящие** widgets и клики:

* `annulus_widgets_name_both_roles_and_build_one_parametric_request` — форма
  называет обе окружности их ролями и UUID и не предлагает ни Line-строки, ни
  `Add Horizontal`/`Add length`/`Pin Start`/`Pair Line A`; без пары
  концентричность не добавляется; `(B,A)` отказывается правилом **документа**;
  радиус адресует выбранную окружность; один Apply — один шаг; Undo/Redo не
  обращается ни к одному job; Save отдаёт ровно ту запись, которую собрали
  widgets. Затем запрос **записывается в документ**, и форма открывается заново:
  `Concentric · <UUID>`, обе строки `Radius … · <UUID>` и `Fixed centre … ·
  <UUID>` нарисованы, строки `Coincident closure` нет, Remove на точных UUID
  работает, замена радиуса едет в том же запросе, Undo/Redo её сохраняет.
* `native_annular_constraint_worker_and_cli_publish_the_same_ring` — один проход
  формы → настоящий async worker → тот же request в **отдельном процессе** CLI;
  document_id, refs, dependencies, все объекты и оба экспорта (STL и FBX)
  побайтово равны, различаются только новорождённые constraint UUID. Затем
  **второй** проход по уже сохранённым строкам: Remove концентричности и pin'а
  через widgets → worker → тот же CLI-запрос, снова равные публикации
  (`degrees_of_freedom = 4`). Это тот пробел, который отметило ревью §25N:
  проверено не только добавление в пустой черновик.

Окно не запускалось.

`cargo fmt --all -- --check` — чисто. `cargo clippy --workspace --all-targets
--all-features -- -D warnings` — чисто.

## Упакованные CI-команды исполнены

Не `bash -n`, а настоящий argv, извлечённый из YAML и запущенный:

* шаг «Build circles with OCCT and refuse constraints without the solver» —
  исполнены **оба** gate: прежний circle и новый
  `occt_without_solver_builds_plain_annuli_and_refuses_annular_constraints`, без
  skips, в том же target и без третьего большого target;
* новый шаг «Constrain a saved ring's radii, centre and concentricity through
  CLI and worker» — четыре CLI gate, три writer-gate, solver-gate и два UI-gate,
  каждый по точному имени и с проверкой на `skipped:`; шаг дописал
  `FCAD_ANNULAR_CONSTRAINT_FBX_DIR` в `$GITHUB_ENV`;
* шаг «Publish complete and partial JSON FBX and read them with pinned ufbx» —
  `tools/check-fbx-complex.sh` целиком: прежние маркеры и новый
  `FCAD_ANNULAR_CONSTRAINT_UFBX_EXECUTED`.

Здесь тоже нет таблицы с необязательным полем: имена gate перечислены
построчно, как их починило ревью §25M.

## Сборки без ядра и без solver

**Смешанная (OCCT есть, solver нет).** Оба gate исполнены, названы точно, без
skips. Кольцо создаётся и cold-rebuild'ится, `analytic_matches` подтверждает
4 грани и оба `Cylinder`; добавление ограничения отказывает `unsupported` без
публикации и без изменения источника.

**Настоящий stub (нет ни ядра, ни solver).** Доказан:
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` в `CMakeCache.txt`, и в imports
тест-бинаря (`otool -L`) **ноль** `libTK*` и **ноль** `planegcs`. В такой
сборке: `annular_constraints` — 1 исполненный, 4 честно пропущенных;
`circle_constraints` — 2 и 3; solver `circles` — 2 исполненных, 5 пропущенных;
document — 6 suites, 165 исполненных и 1 ignored benchmark; app constraints —
19 тестов, 10 исполненных и 9 честно пропущенных. Заведомо сломанный бинарь ради пробы загрузчика не запускался;
`FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался.

## Две направленные поломки

Обе компилировались и провалились на исполненных assertions; исходники
восстановлены побайтово (`shasum -a 256 -c` проходит), положительные гейты
повторены и зелёные.

### 1. Роли перечитаны из решённых радиусов

`jobs/edit.rs`: после решения роли выводятся заново по решённым числам вместо
сохранённых. Запрос, который меняет радиусы местами (граница 2, отверстие 10),
начинает **публиковаться**:

```
native_annular_constraint_refusals_are_atomic
  the two radii crossed over each other
  left: Some(0)   right: Some(2)
```

то есть exit 0 вместо 2, с настоящим ответом `"ok":true` и `"radius":10.0` у
кривой, которая была отверстием. Это ровно то, что контракт запрещает:
переименование ролей легализовало бы обмен.

### 2. Guard writer'а снова защищает любую Coincident

`document.rs`: предикат «это замыкание» возвращается к «любая Coincident».
Снятие концентричности перестаёт быть возможным:

```
a_shared_centre_is_removable_and_is_the_only_removable_coincident
  write: Input { message: "constraint editing retains persisted closure,
                 and this would drop 01a0abae-…" }
```

## Рецепт

Извлечён из `docs/annular-circle-constraints.md` регулярным выражением по
маркеру `FCAD_25O_AGENT_RECIPE` (без разбора прозы) и исполнен настоящим
release-CLI и настоящим собственным pinned reader: create → inspect (явные UUID,
роли и version) → constraints copy → validate/reopen/cold rebuild/print-topology
→ STL/FBX → замена радиуса → отказ на снятии одной концентричности → разрешённое
снятие вместе с pin'ом → снятие всего → height edit → семь отказов. Вывод:
`FCAD_25O_RECIPE_OK 1933.486848 1569.16405`.

## Ограничения

* **Оконный GUI smoke не исполнен.** В этой сессии нет инструмента управления
  нативным интерфейсом macOS: доступны Bash, файловые инструменты, браузерный
  Chrome-скилл и терминальный `agterm`; ни один из них не управляет окном
  FerriteCAD, а наличие `osascript` само по себе доступом к окну не является.
  Экран при этом **не заблокирован** (`ioreg` не показывает
  `CGSSessionScreenIsLocked`, console user `drt`), то есть препятствие —
  инструментальное, а не блокировка. Окно и viewer event loop не запускались,
  watchdog `tools/watch-viewer-memory.py` не запускался, свежий bundle не
  собирался (диск 8.7 ГиБ). Headless widgets/worker проверкой окна не считаются
  и здесь так не называются.
* Большой STEP, GPU/pixel и полный релизный пакет локально не повторялись.
* CI нового diff не запускался; CI базы учитывается отдельно.
* Оригинальный OOM исправленным не объявляется.


## Независимое ревью Codex — 2026-09-16

Найден и воспроизведён дефект нового mixed-gate: полный native suite с
`FERRITECAD_REQUIRE_PLANEGCS=1` исполнял тест OCCT/no-solver и падал на
`assert_ne!(Ok("1"), Ok("1"))`; четыре предметных annular-теста проходили.
Тест ограничен `cfg(not(feature = "planegcs"))`, как прежний circle-gate.
Внутри него теперь отдельно проверяются обязательный OCCT и отсутствие solver.
Полный native suite после исправления — четыре исполненных annular-теста;
оба exact mixed-gate отдельно исполнены без skips.

Исправлены подсказки редактора: выбор относится к Line или Circle, а сообщение
о сохранении Line-замыканий показано только Line-профилю. README и общий JSON v1
контракт дополнены `concentric` и discovery `role`; request/response не менялись.

Повторено локально последовательно, в прежних targets: fmt, workspace clippy
all targets/all features `-D warnings`; solver 34; document/eval/jobs 401
исполненный + 2 прежних stub-only N/A + 1 ignored benchmark; CLI 96 без skips;
app 78 headless без skips; UI 102. True stub подтверждён CMakeCache и `otool`;
его native skips отдельно записаны в `/private/tmp/ferrite-pr43-review/stub-summary.json`.
Markdown recipe исполнен свежим debug CLI: `FCAD_25O_RECIPE_OK 1933.486848 1569.16405`.
Текущий pinned reader прочитал новый FBX: 6 checks, 0 failures. Shellcheck,
actionlint, licence headers, solver ownership, export boundary и diff check прошли.
Большой STEP/GPU pixel корпус локально повторно не запускался.

**Реальный macOS GUI smoke исполнен после явного разрешения пользователя.**
Свежий bundle проверен подписью и `--solver-info` без DYLD environment.
Собственный PID 61444 запущен под watchdog 1536 MiB. Через настоящее окно:
выбраны boundary/bore, добавлены Concentric, R6.75/r2.125 и Fixed(-3.5,4.25);
Undo/Redo восстановили точный запрос; системный Save Cancel сохранил draft;
публикация `gui-parametric.fcad` и async Open дали Fully constrained / 0 DOF.
Повторно открытый редактор показал сохранённые Radius/Concentric/Fixed с UUID и
Remove. Удаление только Concentric отказало без `gui-refused.fcad`, сохранив
черновик и принятую сцену. Добавление к удалению Fixed позволило опубликовать
`gui-freed.fcad`; async Open показал Underconstrained / 4 DOF.

Обе GUI-копии сопоставлены с настоящим peer CLI на том же исходнике и запросе.
Правила, stored circles, document/objects/dependencies/refs совпали; SQL-клетки
различаются только timestamp и payload/hash с новыми constraint UUID первой
публикации. При снятии ограничений UUID совпадают полностью. STL и FBX в обеих
парах побайтово равны. Независимый STL: 1008 triangles, радиусы 6.75/2.125, h15;
центры (-3.5,4.25) и (12,-7), mesh volume 1933.486848 / 1933.486880 mm³,
аналитический ориентир 1934.288414 mm³. Оба FBX прочитаны pinned ufbx, 6/0 каждый.

Watchdog: peak footprint **188.08 MiB**, pressure normal, swap 2709.19 MiB
без роста, штатное закрытие единственного окна и exit 0, aborted=false.
Доказательства: `/private/tmp/ferrite-pr43-review/gui-memory.jsonl`,
`gui-parity.json`, временные .fcad/STL/FBX, failing и положительные логи.
Причина прежнего OOM не установлена и не объявляется исправленной.

CI этого review diff будет проверен после публикации; CI базы отдельно подтверждён.
