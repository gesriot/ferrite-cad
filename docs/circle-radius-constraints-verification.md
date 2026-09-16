# §25N — локальные доказательства и ограничения

Контракт среза: [параметрическая окружность](circle-radius-constraints.md).

База: `main = origin/main = d61afac985e5e729ed715b5f35757b0b6782a995` (merge
PR #41, head `0ca347aeff17b1638954a25489ae2d6a00097bd7`, base `2487b7c`), дерево
`f2a8b3bd7de96a3645a5a1fa45cc8d95d69d2f99`; деревья head и merge совпадают.
На этом merge 27/27 check-runs и 6/6 workflows — success (`CI`, `combined
runtime layout`, `planegcs pin`, `product sbom`, `rust notices`, `rust sbom`).
Ветка `circle-radius-constraints`, 0 коммитов впереди; изменения оставлены
unstaged/uncommitted. **CI нового diff не запускался и пройденным не
объявляется.**

Окружение: macOS (Darwin 24.6.0), aarch64-apple-darwin, Open CASCADE 8.0.1 и
planegcs (FreeCAD 1.0.1, архив sha256 `f62bc07c…` по `tools/planegcs/pin.env`)
из уже собранных закреплённых деревьев в `vendor/`,
`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `--test-threads=1`, сборки
последовательно. **Пересобран собственный shim** `planegcs_shim.cpp`/`.h` (его
входы изменились); OCCT и planegcs не пересобирались — их входы не менялись.
Переиспользованы существующие targets `/private/tmp/ferrite-24b-native-target` и
`/private/tmp/ferrite-25j-stub-target`; новых не заводилось. Диск перед
тяжёлыми прогонами 12–15 ГиБ свободно, после всех прогонов 11 ГиБ; swap
2733/4096 МиБ. Чужие процессы и кэши не трогались, чужой worktree
`/Users/drt/.codex/worktrees/a200/ferrite-cad` не открывался.

Логи и артефакты этого среза — в `$S =
/private/tmp/claude-501/-Users-drt-Desktop-github-ferrite-cad/e2435d19-1587-43ca-87aa-568b8a140bbc/scratchpad`:
`$S/ci/*.log` (исполненные упакованные CI-команды), `$S/recipe.py` (рецепт,
извлечённый из Markdown), `$S/cc-fbx/circle-constraint-cli.fbx` (артефакт,
прочитанный pinned ufbx), `$S/pristine/SHA256` (контрольные суммы двух файлов,
которые ломались и восстановлены — `shasum -a 256 -c` проходит).

## Архитектура

**Solver.** Окружность — три неизвестных. Центр это `PointId`, то есть обычная
точка эскиза, поэтому его закрепляет прежний `Constraint::Fixed` и второго
семейства «закрепить центр» нет. Радиус — `Constraint::Radius { circle: CircleId,
radius }`, собственный скаляр: окружность из центра и точки на ободе несла бы
четыре параметра при трёх степенях свободы. `PointId` и `CircleId` — раздельные
пространства; `Constraint::points()` и `Constraint::circles()` — два списка.
`Solution::circle()` даёт решённый радиус, центр читается и как точка. Residual
считается по **полученному** радиусу. Отрицательный или нечисловой ответ
становится `DidNotConverge` без координат.

**C ABI.** `FcGcsConstraint` получил отдельный `int32_t circles[2]`; `points[4]`
по-прежнему только точки, и shim проверяет, что неиспользуемый массив пуст.
Новые `FcGcsCircle`, параметры `circles`/`circle_count` и отдельный
`fc_gcs_session_radii`. Layout сверяется `static_assert` в C++ и
`assert_abi_layout()` в Rust перед первым вызовом каждой сессии — и это уже
поймало ошибку при разработке: первая версия объявила `sizeof(FcGcsConstraint)
== 40`, компилятор ответил `48 == 40`, потому что я забыл `value2`.

**Документ.** `SketchPointSelector::Center` — своё слово; `Radius { curve,
radius }` адресует кривую. Sketch с ограничением про окружность — payload v3 и
новая capability `sketch.constraints.circle.v1` (v3 нет в
`readable_schema_versions` прежней сборки — она сохраняет объект побайтово и
открывает документ read-only; capability делает причину читаемой). Line-профиль
остаётся v2, пустой Sketch — v1. SQL-схема не поднята.

**Общие типы, разделённые по геометрии.** `AddLineConstraint` сохранил своё
точное имя и смысл; добавлен `AddSketchConstraint { Line(..), Circle{..} }`, и
`SketchConstraintEdits.add` стал его вектором. Переименования на 89 упоминаний
не делалось: диффа было бы больше, чем сути. `Slot` получил `Radius(curve)`;
`Slot::Pin` общий, потому что профиль держит один pin, какой бы геометрией он ни
был, а два семейства не делят один профиль. `managed`/`retained`/`prepare`
получили `Family`, читаемую из кривых.

## Измеренные степени свободы

Каждое число прочитано из библиотеки в тесте, не сравнено с записанной заранее
константой (`a_free_circle_has_three_degrees_of_freedom_and_each_constraint_removes_its_own`):

| Что | DOF |
| --- | --- |
| свободная окружность | 3 |
| + `Radius` | 2 |
| + `Fixed` центра (без радиуса) | 1 |
| + оба | 0 |

Свобода возвращается при снятии ограничения, измеренная снова, а не принятая
равной прежнему числу. Два разных радиуса на одной окружности — настоящий
`Outcome::Conflicting` с UUID вызывающего; два одинаковых — `redundant` и
решение. Независимые окружности отвечают за себя в **обоих** порядках
добавления, и незакреплённая окружность не сдвигается — это и снимает опасение,
что ответ проходит за счёт `ordinal = 0`.

## Измеренная геометрия и identity

Источник: `create-circle-extrude` (12,−7)/r10/h15. Запрос: `radius 6.75` +
`fixed center (−3.5, 4.25)`. `solve.degrees_of_freedom = 0`, два новых UUID.

После close/reopen + cold rebuild копии: **3 грани**, два `Plane`,
`Cylinder{6.75}` именно под прежним `ExtrudeSide` ref исходной `Circle`, объём
в пределах 1e-6 от π·6.75²·15. Сохранённый Sketch по-прежнему (12,−7)/10 —
solver-ответ не переписал приближение. Все refs и identities целы, `4 objects`,
`3 of 3 stored references resolved`.

Независимый разбор binary STL при `--linear-deflection 0.05
--angular-deflection 0.1`:

```
triangles 500
directed edges !=1: 0        (замкнута и одинаково ориентирована)
missing opposite: 0
signed volume 2146.192485   pi*R^2*h 2147.082229   ratio 0.999586
radii about the SOLVED centre: [6.75]      z: [0.0, 15.0]
extents: [13.5, 13.4958, 15.0]  bbox x [-10.25, 3.25]  y [-2.4979, 10.9979]
```

Центр bbox — (−3.5, 4.25), то есть решённый; радиус измерен по фактическим
хордам с допуском тесселяции, аналитического π от меша не требуется. FBX
прочитан pinned ufbx `fcc5d6ba…`: `checks=6 failures=0`,
`FCAD_IDENTITY_SUMMARY nodes=1 definitions=1 occurrences=1 meshes=1`.

Замена 6.75 → 8.125 — один атомарный remove/add: pin и все прочие UUID целы,
геометрия становится `Cylinder{8.125}`. Настоящие cache Miss/Hit на одном
CacheStore до и после замены дают новую геометрию, не запись предыдущей.
Удаление обоих ограничений возвращает `circle_edit.available: true`, и геометрия
возвращается к **сохранённому приближению** (10 мм) — сохранения последнего
решённого радиуса этот срез не обещает и не изображает. `edit-extrude` 15 → 25
сохраняет оба ограничения и решённый радиус.

Каждая SQL-ячейка копии сверена с явным allowlist: payload, payload_hash и
schema_version выбранной строки `objects`, новые строки `capabilities` и прежний
`meta.modified_at`. **Сохранённая геометрия окружности в allowlist не входит** —
именно поэтому её неизменность проверена, а не предположена. Источник побайтово
цел после каждого прогона.

## Что нашли проверки во время разработки

Четыре настоящих дефекта, каждый поймали исполняемые проверки, а не чтение:

1. **`sizeof(FcGcsConstraint)`** — `static_assert` ответил `48 == 40`.
2. **Capability не была зарегистрирована.** Новая `sketch.constraints.circle.v1`
   отсутствовала в `SUPPORTED_CAPABILITIES`, и первая же публикация отказала
   «this build does not implement». Исправлено регистрацией.
3. **Guard writer'а был сформулирован не про то, что защищает.** Правило
   «constraints не пусты» было тем же правилом для Line-профиля и неверным для
   окружности: у окружности нет стыков, и снятие последнего размера законно
   оставляет пустой список. Переписано как то, что оно охраняет: **каждая
   Coincident-связь, которая была, осталась** — проверка строго сильнее прежней.
4. **`solve` был обязательным.** Снятие последнего ограничения оставляет эскиз
   без системы, и требование отчёта заставило бы неограниченный эскиз требовать
   solver. `EditedSketchConstraints.solve` и JSON `result.solve` стали
   nullable; для Line-профиля это недостижимо, поэтому прежние ответы не
   изменились.

И один дефект в **упакованной CI-команде**, найденный исполнением её настоящего
argv, а не `bash -n`: та же ловушка, что отметило ревью §25M. Таблица
`'ferritecad-document  sketch_constraints'` с пустым средним полем схлопывалась
в `read -r package features gate`, и имя теста попадало в `--features`.
Исправлено устранением необязательного поля: два отдельных вызова вместо цикла
по таблице. Проверено запуском обоих настоящих циклов.

## Исполненные проверки

`ferritecad-sketch-solver` — 4 suites, 33 теста, включая 6 новых в
`tests/circles.rs`. Прежние восемь семейств, drag-жесты, диагностика,
`neither_send_nor_sync` и lifetime-счётчики не изменились; `native_solves`
растёт, `native_live_sessions` возвращается к нулю.

`ferritecad-document` — 6 suites. Новое: `annulus`-независимые тесты словаря
окружности, два новых теста
`a_circle_constraint_declares_the_circle_vocabulary_and_cannot_under_declare_it`
(negotiation в обе стороны: v2-заголовок над круговым payload, v3 без
capability, v1 над любым ограничением — все отказываются; и payload под
capability **новее** этой сборки открывается read-only и round-trip'ится
побайтово) и `a_radius_naming_no_circle_is_refused_by_the_persistence_boundary`.

`ferritecad-eval` — 10 suites, 185 тестов; обновлён один, чья посылка устарела
(`a_circle_offers_no_point_to_solve_for` → окружность теперь заявляет центр
точкой и радиус скаляром). `ferritecad-jobs` — 53. `ferritecad-scene` — 175,
`ferritecad-export` — 60, `ferritecad-occt` — 129, `ferritecad-app` — 315
(с новым headless gate), `ferritecad-viewport-gpu` — 131 (без окна и без GPU
теста), `ferritecad-solver-lab` — 17.

`ferritecad-cli` — 19 suites, все зелёные. Новый `circle_constraints`, 4 теста:

* `circle_constraint_discovery_and_protocol_without_native` — фикстура через
  **Document API**, поэтому discovery и отказы исполняются без ядра и без
  solver: 12 структурных/числовых отказов (ноль и отрицательный радиус, выход за
  policy, два радиуса, два pin, чужая кривая, Line-правило на круговом профиле,
  Line-длина, Line-pin, удаление отсутствующего), 7 отвергаемых форм запроса и
  3 сырых текста NaN/Infinity/1e999, usage, exit 7 при закрытом stdout на
  отказанной операции. Ни один не написал байта.
* `native_circle_radius_and_pinned_centre_drive_the_solid` — вся измеренная
  история выше.
* `native_circle_constraint_refusals_and_late_delivery_are_atomic` — stale
  version, чужой Sketch, занятый output, source как destination, hard link,
  symlink, и exit 7 после **состоявшейся** публикации (копия цела и измерена).
* `circle_constraint_non_utf8_paths_refuse_before_reading`.

Headless UI — `circle_widgets_add_a_radius_and_a_fixed_centre_as_one_request_each`
через **настоящие** widgets и клики: форма называет окружность её UUID и
сохранёнными числами и не предлагает Line-строку; без выбора ничего не
добавляется; четыре плохих радиуса отказываются в форме и не меняют историю;
один Apply — один шаг; второй радиус отказывается правилом **документа**;
центр-pin использует те же X/Y-поля и попадает как круговое добавление с
селектором центра; Undo/Redo двигают запрос и не обращаются ни к одному job;
Save отдаёт ровно ту запись, которую собрали widgets. Окно не запускалось.

`cargo fmt --all -- --check` — чисто. `cargo clippy --workspace --all-targets
--features planegcs -- -D warnings` — чисто. `tools/check-licence-headers.sh` —
346 файлов, все MIT.

## Рецепт

Извлечён из `docs/circle-radius-constraints.md` регулярным выражением по маркеру
`FCAD_25N_AGENT_RECIPE` (без разбора прозы) и исполнен настоящим CLI и настоящим
pinned reader: create → inspect (явные UUID и version) → constraints copy →
validate/reopen/cold rebuild/print-topology → STL/FBX → replacement → удаление
обоих → height edit → отказы. Вывод: `FCAD_25N_RECIPE_OK 2146.192485
3109.623888`.

## Сборка без ядра и без solver

Настоящий stub доказан: `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` и
`CMAKE_IGNORE_PREFIX_PATH:STRING=/opt/homebrew` в `CMakeCache.txt`; в imports
бинаря (`otool -L`) **ноль** `libTK*` и **ноль** `planegcs`. В такой сборке:
`circle_constraints` — 2 исполненных, 2 честно пропущенных; solver `circles` — 2
исполненных (контрактные отказы, которым библиотека не нужна), 4 пропущенных;
document `sketch_constraints` — 30 исполненных. Публикации нет: применение
правки без solver отказывается `unsupported`. Заведомо сломанный бинарь ради
пробы загрузчика не запускался; `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался.

## Две направленные поломки

Обе компилировались и провалились на исполняемых assertions; исходники
восстановлены побайтово (`shasum -a 256 -c`), положительные гейты повторены.

### 1. Решённый радиус проигнорирован

`solve.rs`: `apply` перестаёт писать `answered.radius`. Solver по-прежнему решает
(DOF 0), но опубликованное тело берёт сохранённое приближение. Поймали два
независимых измерения:

```
native_circle_radius_and_pinned_centre_drive_the_solid
  the solved radius is the cylinder's
  left: [Cylinder { radius: 10.0 }]  right: [Cylinder { radius: 6.75 }]
```

и второе, показывающее, что policy на **решённой** геометрии несущая: запрос с
радиусом 2e6 стал публиковаться (exit 0 вместо 2), потому что проверяемая
геометрия перестала отражать запрос.

### 2. Потеря capability

`document.rs`: узкая запись объявляет только прежний словарь. Копия хранит
ограничение про окружность, не анонсируя его:

```
the copy stores a circle constraint without declaring
sketch.constraints.circle.v1: ["core.part.v1", "sketch.constraints.v1"]
```

Эта поломка **нашла пробел в проверке**: allowlist разрешал добавление строк
`capabilities` и запрещал потерю, но не требовал присутствия новой. Assertion
добавлена, после чего поломка ловится; в отчёт это входит как усиление, а не как
проверка, написанная под ответ.

## CI

`planegcs pin` уже прогоняет весь пакет solver'а, поэтому `tests/circles.rs`
попадает в него автоматически под `FERRITECAD_REQUIRE_PLANEGCS=1`. Компиляция
пакета сама по себе ничего не доказывает, поэтому добавлены **шесть именных**
проверок `test <gate> ... ok` — по одной на каждый новый solver-гейт — и третий
вариант строки пропуска (`skipped: this build has no planegcs`) в прежний
no-skip список.

В `combined runtime layout` добавлен шаг `Constrain one saved circle's radius and
centre through CLI and worker`: три CLI-гейта по точным именам с no-skip
проверкой, прогон `ferritecad-document --test sketch_constraints` с двумя
именными проверками capability, и UI-гейт по полному пути
`constraints::tests::…`. Итого **6 новых именных executions на ОС** здесь плюс
**6 в `planegcs pin`**. Один малый FBX добавлен в прежний reader loop
`tools/check-fbx-complex.sh`, и маркер `FCAD_CIRCLE_CONSTRAINT_UFBX_EXECUTED`
проверяется рядом с прежними — 36-е строгое чтение на ОС.

Ни один прежний гейт не удалён и не ослаблен: прежние 213 именных executions и
35 чтений на ОС остаются на месте, новые только добавлены. **Упакованные команды
исполнены**, а не только проверены `bash -n`/YAML: именно исполнение нашло
схлопывание пустого поля, которое отметило ревью §25M.

## N/A и неисполненное — отдельный учёт

| Что | Статус | Почему |
| --- | --- | --- |
| Интерактивный GUI smoke | **отложен, не пройден** | по указанию пользователя: полностью headless. Форма, отказы, история и запрос проверены через настоящие widgets |
| GPU/render/pixel тесты | **не запускались** | по тому же указанию; `ferritecad-viewport-gpu` прогнан без них |
| Случай «OCCT есть, solver нет» | **исполнен при независимом ревью** | исходная передача его не исполняла; `planegcs pin` этого случая не покрывал. Ревью добавило отдельный no-feature gate в `runtime-layout` и исполнило его локально в прежнем target; подробности ниже |
| Большой STEP/complex-FBX/pixel корпус | **локально не дублировался** | изменённый путь в него не входит; `ferritecad-scene`, `ferritecad-export`, `ferritecad-occt` и `ferritecad-topology` прогнаны целиком |
| Release-сборка CLI для рецепта | **не делалась** | рецепт исполнен debug-бинарём, чтобы не тратить дефицитный диск; argv и поведение те же |
| Пересборка OCCT/planegcs | **N/A** | их входы не менялись; пересобран только собственный shim |
| `FCAD_ALLOW_LOADER_FAILURE_PROBES` | **не включался** | ни разу |
| Windows и Linux | **N/A локально** | измерения на macOS; новые гейты добавлены в прежние трёхплатформенные workflows |
| CI нового diff | **не запускался** | изменения не закоммичены; пройденным не объявляется |

## Ограничения, остающиеся после среза

* Только `Radius` и `Fixed` центра, только одна неконструкционная аналитическая
  `Circle` в прежнем frame §25K. Diameter, Tangent, EqualRadius, Arc, смешанные
  профили, окружности с отверстиями (§25L/M сохраняют свои контракты), несколько
  окружностей в одном профиле — не входят.
* Геометрия окружности мышью не рисуется и не тащится; live solve отсутствует —
  solver работает только при сохранении копии.
* Радиус только literal mm: выражений вместо числа нет.
* Сохранения последнего решённого радиуса при снятии размера **не обещано**:
  геометрия возвращается к сохранённому приближению, как у Line-ограничений.
* Прежний OOM не объявляется устранённым: сборки последовательные, один build
  job, один test thread; память и диск проверялись перед прогонами, диск
  остаётся узким местом (11 ГиБ свободно).

## Независимое ревью Codex — 2026-09-16

База повторно проверена по точному SHA `d61afac985e5e729ed715b5f35757b0b6782a995`:
27/27 check runs и 6/6 workflows success. Это проверка базы; CI публикации
ревью учитывается отдельно.

Найдены и исправлены:

1. **Сохранённый Radius был недоступен для удаления в UI.** Ветка отрисовки
   не знала Radius и показывала его как `Coincident closure`, без `Remove`.
   Исполняемый regression сначала упал на неверной подписи. Теперь Radius и
   Fixed centre именуют Circle и собственный UUID; проверены удаление,
   атомарный remove/add для 6.75→8.125, Undo/Redo и снятие обоих ограничений.
   Неприменимые Line-действия скрыты у окружности; сообщение о добавлении
   closure тоже относится только к Line-профилю.
2. **Сборка OCCT без solver не была покрыта заявленным CI.** `planegcs pin`
   доказывает присутствие solver, а не его отсутствие. Добавлен отдельный
   exact-name/no-skip gate в `runtime-layout`, до штатной native-сборки.
   Локально он исполнен в прежнем target без feature `planegcs`; `otool -L`
   подтверждает libTKernel и отсутствие planegcs. Реальные цилиндр и кольцо
   создаются и cold-rebuild'ятся, circle constraints отказывают без файла и
   scratch. Снятие последнего ограничения из ограниченного источника тоже
   требует solver: прежний baseline rebuild сохранён. Первоначальное ожидание
   успеха в новом review-тесте было исправлено после наблюдения этого отказа;
   инвариант проверки исходника не ослаблялся.
3. Убран однопроходный shell loop нового ufbx чтения, на который ShellCheck
   выдавал SC2043. Чтение осталось тем же.

Новый `native_circle_constraint_worker_and_cli_publish_the_same_solid` исполняет
настоящие widgets → прежний async worker → публикацию и peer CLI с тем же
запросом. DOF=0; исходник неизменён, stored Circle и refs/dependencies/UUID
сохранены, различаются только новые constraint UUID. STL/FBX двух копий
побайтово совпадают. Этот gate включён в существующий runtime workflow.

Проверки итогового кода локально (последовательные сборки, jobs=1):

- fmt и workspace clippy all targets/all features `-D warnings`;
- solver: 33 исполненных; domain/kernel/topology/export: 684 исполненных,
  отдельно 2 stub-only N/A и прежний ignored timing benchmark;
- CLI: 92 исполненных; app: 76 headless workers/widgets, включая 17 constraint
  tests; UI library: 102; native geometry skips в этих маршрутах нет;
- настоящий stub: 40 исполненных негeометрических проверок, 20 явных native
  skips отдельно; CMakeCache NOTFOUND, imports без libTK/planegcs;
- отдельный mixed OCCT/no-solver process gate: 1/1;
- Markdown-рецепт: `FCAD_25N_RECIPE_OK 2146.192485 3109.623888`;
  pinned ufbx: 6 checks, 0 failures;
- actionlint, shellcheck, solver/export boundaries, 11 notice ownership checks,
  license headers и `git diff --check`.

Итоговая runtime матрица сохраняет прежние 71 exact execution на ОС и добавляет
7 circle-constraint executions плюс 1 mixed-build execution: **79 на ОС,
237 на три ОС**. Строгих ufbx чтений — **36 на ОС**. Это ожидаемый состав;
факт удалённого исполнения определяется логами опубликованного SHA.

Оконный smoke на момент публикации ревью не исполнен: пользователь разрешил
GUI, но два вызова CUA сообщили `Mac is locked`; запрос разблокировки отправлен.
Viewer не запускался. Headless результаты не объявляются GUI. GPU/pixel, Unity
и большой STEP корпус локально не дублировались. `FCAD_ALLOW_LOADER_FAILURE_PROBES`
не включался. Свободная память 49–53%, swap 2733.19 MiB без роста, диск около
11 GiB; новый target не создавался. Логи ревью: `/private/tmp/ferrite-pr42-review/`.
