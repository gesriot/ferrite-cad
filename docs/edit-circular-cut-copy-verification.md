# §26B — локальные доказательства и ограничения

Первоначальный отчёт реализации приведён ниже; результаты независимого ревью
и исправления отделены в последнем разделе. CI базы не заменяет CI нового head.

Контракт среза: [правка параметров сохранённого circular Cut](edit-circular-cut-copy.md).
Срез, который этот Cut создаёт: [§26A](circular-cut-copy.md).
Решение об истории: [ADR 4](decisions/0004-feature-predecessor.md).

База: `main = origin/main = 916cb3e43e08c37e3e473639f2003540aab96733` (merge
PR #44, head `df14aed`, base `0780dc0`), дерево
`5c383e1c6dfb105914413ef609504a7fbbe05fb4`. На этом merge в этой сессии
независимо подтверждены **27/27** check-runs success и **6/6** workflows
success (`gh api .../commits/916cb3e.../check-runs`, `gh run list --commit`).
Ветка `edit-circular-cut-copy` создана от этого merge и **0 коммитов впереди**;
изменения оставлены unstaged/uncommitted. **CI нового diff не запускался и
пройденным не объявляется.**

Окружение: macOS (Darwin 24.6.0), aarch64-apple-darwin, Open CASCADE 8.0.1 и
planegcs (FreeCAD 1.0.1) из уже собранных закреплённых деревьев в `vendor/`,
`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `--test-threads=1`,
тяжёлые сборки последовательно. **Ни OCCT, ни planegcs, ни собственный C++
bridge не пересобирались** — их входы этот срез не менял.

Переиспользованы прежние target: `/private/tmp/ferrite-24b-native-target`
(native и mixed) и `/private/tmp/ferrite-25j-stub-target` (stub). Третий большой
target не заводился; чужой worktree `/Users/drt/.codex/worktrees/a200/ferrite-cad`
не открывался и не изменялся; чужие каталоги и кэши не чистились. Логи и
артефакты этого среза — в
`/private/tmp/claude-501/-Users-drt-Desktop-github-ferrite-cad/a3bf5114-6b1f-429b-8a66-55a657d51289/scratchpad`.

## Что именно меняется

Исполнено `native_the_centre_radius_and_depth_move_alone_and_together` и
`cut_edit::tests::editing_a_cut_changes_its_numbers_and_nothing_it_is_made_of`.

В SQL сверена **каждая ячейка каждой таблицы** источника и копии против явного
allowlist: `payload`/`payload_hash` ровно двух строк `objects` (Sketch
инструмента и Cut-фича), `meta.modified_at`, и — только при переходе
насквозь→карман — одна добавленная строка `topology_refs`. Ни одна строка ни
одной таблицы не исчезла и не изменилась иначе; `objects` не выросла;
`kind`, `schema_version`, `parent_id`, `ordinal` и `name` обеих правленых строк
прежние.

Сохранены и проверены по идентичности: document UUID, все шесть object UUID,
UUID окружности инструмента, `Body.tip_feature`, `Extrude.previous`,
`operation`, `profile`, отсутствие `target_body`, исходные Sketch и Extrude
детали целиком, все шесть dependencies и **каждая** сохранённая topology ref под
своей identity (сравнение `refs_before ⊆ refs_after` по значению, плюс точный
счёт). Посторонняя таблица `extra_data` с собственными строками переживает
правку побайтово (`native_unknown_tables_survive_and_a_stored_trigger_stops_the_edit`).
Источник после каждого прогона побайтово цел.

## Идентичности берутся из связей, не из имён

`cut_edit::tests::a_saved_cut_is_found_through_links_and_types_and_never_by_name_or_order`
переименовывает **оба** Sketch в одно и то же слово `Sketch` и переставляет
ordinal так, что Sketch инструмента идёт первым, — и каталог отвечает то же
самое: тот же `tool_sketch`, тот же `profile_sketch`, та же окружность, тот же
центр. Исходная `NewBody`-фича в этом же тесте отказывается для §26B и
по-прежнему принимается `editable_extrude`.

`cut_edit::tests::a_cut_whose_names_are_not_the_ones_this_build_gives_is_refused`
добавляет Cut одну лишнюю ссылку `ExtrudeCap{Start}` — то есть имя, которое этот
модуль никогда не выдаёт, — и весь документ перестаёт быть поддержанным классом.

## Политика pocket ↔ through

Проверены **отдельно** и в обе стороны
(`native_a_hole_may_be_shortened_and_a_pocket_may_not_be_cut_through`,
`cut_edit::tests::a_saved_pocket_floor_may_not_be_cut_away_and_a_hole_may_gain_one`):

| Сохранено | Запрошено | Результат |
| --- | --- | --- |
| карман h4 | карман 0.5 / 4 / 9.9 / 9.999 | поддержано, ни одного нового имени |
| карман h4 | насквозь 10 | **отказ** `unsupported`, сообщение содержит UUID сохранённой ссылки на дно; публикации нет |
| отверстие h10 | карман 3 | поддержано, **+1** ссылка `ExtrudeCap{End}` с owner/producer самой Cut |
| отверстие h10 | отверстие 10 | поддержано, ни одного нового имени |

После укорачивания отверстия в карман: 7 → **8** аналитических граней, объём
24000 − π·25·3, `11 of 11 stored references resolved`, и **два разных плоских
имени** различимы — `ExtrudeCap{End}` самой Cut (дно на z = 3) и
`CarriedCap{End}` (дальняя сторона детали на z = 10), каждое ровно на одну
грань. Это и есть причина, по которой одной роли на оба было бы недостаточно.

## Измеренная геометрия

Источник: плита 60 × 40 × 10 с сохранённым карманом r5 в (20, 15) глубиной 4,
сделанная штатными `create-sketch-extrude` и `cut-circular-copy`.

| Что правится | Новые числа | Граней | Объём (аналитический B-Rep) |
| --- | --- | ---: | --- |
| только центр | (30, 20) r5 d4 | 8 | 24000 − π·25·4 |
| только радиус | (20, 15) r7.5 d4 | 8 | 24000 − π·56.25·4 |
| только глубина | (20, 15) r5 d6.5 | 8 | 24000 − π·25·6.5 |
| всё сразу | (35, 22.5) r8.25 d2.75 | 8 | 24000 − π·68.0625·2.75 |

В каждом случае стенка отверстия — `Cylinder { radius: r }` **под ссылкой
собственной `Circle` инструмента** (`side of <cut UUID>`, а не планарные `side`
исходной экструзии), объём сходится с аналитическим в пределах 1e-6, а
`validate` даёт `valid: true` без диагностик.

**Повторная правка уже отредактированной копии** — обычный случай, а не
особый: после каждой из четырёх правок копия правится ещё раз обратно на
(20, 15) r5 d4, и `feature_id`/`tool_curve_id` в отчёте — по-прежнему
идентичности **исходного** документа, а объём совпадает с исходным.

## Независимый разбор экспорта

Собственный parser binary STL при `--linear-deflection 0.05
--angular-deflection 0.1` проверяет на каждой копии: замкнутость (каждое
направленное ребро ровно раз, у каждого есть обратное), стенку отверстия по
фактическим хордам вокруг запрошенного центра и радиуса, **ориентацию полости
внутрь** (иначе это выступ, а не отверстие), z от 0 до запрошенной глубины,
наличие дна и закрытой дальней стороны у кармана против открытой у сквозного,
габариты детали и объём в границах, которые вписанная призма может дать. Это
объём тесселяции; аналитический объём B-Rep измеряет ядро, и они названы
раздельно.

FBX всех пяти опубликованных копий прочитаны **собственным** pinned ufbx
reader, собранным из `tools/unity-fbx-smoke/scripts/read_production.c` с pin из
`fetch_ufbx.sh`: `checks=6 failures=0` на каждом. `export-fbx` сообщает
`geometries: 1` — экспортируется один конечный Body, не инструмент и не
промежуточный solid. Новый большой корпус ради этого среза не заводился:
использован прежний reader loop `tools/check-fbx-complex.sh`, который напечатал
новый маркер `FCAD_CUT_EDIT_UFBX_EXECUTED`.

## Кэш

`native_the_edited_cut_is_keyed_by_its_own_numbers`: один документ, один
sidecar. Miss/Miss → Hit/Hit (та же геометрия и то же число разрешённых граней,
что и cold). Затем инструмент сужается **на месте** в том самом документе,
которому принадлежит sidecar: архив исходной фичи законно попадает
(`plate` Hit), запись Cut обязана промахнуться (`cut` Miss), и объём равен
24000 − π·9·6 — то есть **старая полость из кэша не вернулась**. Cold rebuild
того же файла после этого даёт то же число.

## Клиенты

CLI `edit-circular-cut`: строгий request v1 из пяти полей,
`deny_unknown_fields`, bounded чтение 65536 байт, UTF-8 preflight,
`--expect-version`, прежний envelope, коды 0/2/7. Discovery — аддитивное
`circular_cut_edit` у каждой строки `features`; работает **без ядра** и без
окна. Прежние `editable`/`distance_mm` по-прежнему отвечают про
`edit-extrude`, `editable` у Cut — `false`, а у исходной `NewBody` — `true`, и
её локальный отказ для §26B прямо называет `edit-extrude`. Все четыре Sketch-
редактора (`editable`, `circle_edit`, `annulus_edit`, `constraint_edit`) и
`bodies[].cut_edit` на документе с Cut отказывают — в том числе `circle_edit`
на одноокружностном Sketch инструмента.

UI: действие только у фичи, которую документ разрешил править; форма
открывается на **сохранённых** числах (проверено посимвольно), называет
плоскость, +Z, размеры детали, выбранный Cut, Sketch инструмента и окружность,
говорит, карман это или отверстие, и показывает разные подсказки для
`through_allowed` true/false. Один подтверждённый `Apply cut` — один шаг
bounded Undo/Redo на все четыре числа; изменение числа после Apply снова
закрывает Save; Undo/Redo не обращается ни к одному job; отказ документа
(включая «дно нельзя срезать») не трогает историю.
`native_cut_edit_worker_and_cli_publish_the_same_part` — один проход формы →
настоящий async worker → тот же запрос отдельным процессом CLI: один
document_id, шесть объектов, **все шесть совпадают целиком** (эта операция не
создаёт ни одного объекта, поэтому исключений из сравнения нет), `topology_refs`
обеих копий равны под теми же identity, а STL и FBX побайтово равны. Отказ
async Open после публикации восстанавливает точные `typed`/`applied`/`history`
через общий `draft_published`/`draft_load_finished`; принятая сцена черновик
завершает, и воскресить его повторно нельзя.

Окно не запускалось.

## Отказы без публикации

Каждый проверен с остальными корректными аргументами; ни один не оставил файла,
и источник после каждого побайтово цел.

Синтаксические, структурные и числовые случаи: `request_version: 2`,
`schema_version` вместо него, пропущенные `tool_curve_id` и `depth_mm`, лишнее
поле `plane_id`, трёхкомпонентный центр, `tool_curve_id`, который не UUID,
чужой `tool_curve_id`, нулевой и отрицательный радиус, нулевая глубина, глубина
больше высоты детали, инструмент **точно касающийся** стенки, свисающий за край
и мимо детали, срезание сохранённого дна, `1e999`, запрос больше 65536 байт,
чужой и не-Cut `--feature`, устаревшая версия, источник как собственное
назначение, занятый выход.

Уточнение независимого ревью: синтаксис request проверяется CLI до открытия
ядра. Структурные/числовые проверки внутри job реально исполняются в native
процессе и напрямую в document tests; stub CLI отказывает раньше них при
создании kernel. Его exit 2 и отсутствие файла не засчитываются как проверка
конкретного более позднего правила.

Через ядро (`native_cut_edit_refusals_are_atomic`): источник как назначение,
занятый выход, **hard link** и **symlink** на источник, не-UTF-8 путь в
JSON-режиме. Отдельно: потеря stdout на отказанной операции даёт **exit 7** без
публикации, а потеря stdout **после** состоявшейся публикации
(`native_a_lost_report_after_publication_leaves_the_copy_whole`) тоже даёт
exit 7 — и копия при этом цела, содержит новые числа и прежние identity.
Публикация после exit 7 не откатывается, и слепой повтор не рекомендуется.

Документ с сохранённым SQL-триггером отказывается **до** попытки правки:
`document_refusal` называет триггер, `available` становится false, но факты о
Cut по-прежнему сообщаются — отказано редактирование, не наблюдение.

## Сборки

**Native (OCCT + planegcs), релиз, `--test-threads=1`.** Полный проход по всем
крейтам и всем targets (`cargo test --release --workspace --all-targets
--features planegcs`): **1959 исполненных, 0 падений, 0 skips**, 2 прежних
`ignored` (ручной timing benchmark и регенерация fixture manifest). Крупнейшие
наборы: app 320, headless 130, ui 102, smoke 99, kernel 92, eval 86, scene 75,
document 72, jobs 53. Новое в этих числах: document `cut_edit` **16** (6 новых),
CLI `edit_circular_cut` **8**, app `cuts` **5** (3 новых).

Числа взяты из **последнего** прогона, после всех правок и после восстановления
обеих направленных поломок; промежуточные прогоны в отчёт не переносятся.

**Настоящий stub (нет ни ядра, ни solver).** Доказан:
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` в `CMakeCache.txt` bridge-build
и **ноль** `libTK*` и **ноль** `planegcs` в `otool -L` тест-бинаря
(`edit_circular_cut-f99d9c115ffb7182`). В такой сборке: `edit_circular_cut` —
**1 исполненный** (`cut_edit_discovery_and_protocol_without_native`: discovery,
весь request-протокол и все структурные отказы), **6 честно пропущенных** по
отсутствию ядра и **1 пропущенный** как предназначенный смешанной сборке;
document lib — **72 исполненных**, включая все 16 `cut_edit`; app `cuts` — **5
честно пропущенных**. Заведомо сломанные loader probes не запускались,
`FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался.

Чтобы discovery и протокол были исполнимы без ядра, фикстура
`Fixture::drawn` пишет те же шесть объектов напрямую через
`prepare_circular_cut`/`write_circular_cut` — то есть через **тот же** код, что
и штатная команда; ядро нужно только самому boolean, и здесь оно не
запрашивается.

**Найденный при этом честный факт.** Команда открывает сессию ядра до вызова
job, поэтому в сборке без ядра **любой** вызов отказывается раньше version
guard и по другой причине. Формулировка отказа version guard поэтому
проверяется только там, где ядро есть; exit 2, отсутствие публикации и
сохранность источника проверяются в обеих сборках. Это свойство команды, а не
guard, и оно названо, а не обойдено.

**Смешанная (OCCT есть, solver нет), в том же прежнем target.**
`occt_without_solver_edits_an_unconstrained_cut` исполнен по точному имени, без
skips, рядом с тремя прежними mixed-gate. Тест обязательно помечен
`cfg(not(feature = "planegcs"))`, поэтому полный native suite его не исполняет с
противоположным требованием.

## Упакованные CI-команды исполнены

Не `bash -n`, а настоящий argv, извлечённый из YAML (`ruby -ryaml`) и
запущенный; подставлены только выражения самого runner'а (`matrix.name`,
`RUNNER_TEMP`, `GITHUB_ENV` и пути к OCCT/planegcs, которые на CI выставляют
предыдущие шаги).

* новый шаг **«Edit a saved circular cut through CLI and worker»** — exit 0,
  **26** исполненных gate по точным именам (6 CLI, 16 document, 3 UI, плюс
  suite-строки), каждая с проверкой на `skipped:`; шаг дописал
  `FCAD_CUT_EDIT_FBX_DIR` в `$GITHUB_ENV` и оставил все пять FBX;
* шаг **«Build circles with OCCT and refuse constraints without the solver»** —
  четыре mixed-gate, включая новый;
* шаг **«Publish complete and partial JSON FBX and read them with pinned
  ufbx»** — `tools/check-fbx-complex.sh` целиком под `bash` (полный корпус:
  `definitions=46 nodes=140 geometries=34 triangles=986873`), затем новый
  маркер `FCAD_CUT_EDIT_UFBX_EXECUTED`. Прежние маркеры этого шага требуют
  каталогов, которые выставляют более ранние шаги workflow; здесь они не
  выставлялись, и их отсутствие отмечено, а не выдано за прохождение.

Первый прогон реально нашёл дефект **в собственной оснастке**, а не в продукте:
`source tools/check-fbx-complex.sh` из zsh оставляет `BASH_SOURCE[0]` пустым, и
скрипт вычисляет корень репозитория мимо. Прогон остановлен и повторён под
`bash`, как это делает workflow.

## Три направленные поломки

Все три компилировались и провалились на **исполненных** assertions; compile
failure и zero-test за воспроизведение не засчитывались. Исходники
восстановлены побайтово (`shasum -a 256 -c` для `cut_edit.rs` и `document.rs`
проходит), затронутые положительные гейты повторены и прошли, а окончательные
числа полного прогона сняты **после** восстановления. Постоянная
mutation-инфраструктура не добавлялась.

### 1. Новый центр и радиус не доходят до диска

`document.rs`: писатель для Sketch кладёт байты, которые уже лежат в строке,
вместо подготовленных. `rederive_parameters` при этом честно проходит — план
правильный, а записывается не он. (Более грубый вариант — не применять новую
геометрию в `prepare_cut_parameters` — ловится раньше, самим writer guard:
`the prepared cut edit does not describe the document it is being written to`.
Приведён вариант, который guard пройти **может**, потому что он проверяет
именно измеренную геометрию.)

```
native_the_centre_radius_and_depth_move_alone_and_together
  left:  Array [Number(20.0), Number(15.0)]
  right: Array [Number(30.0), Number(20.0)]
```

### 2. Политика pocket → through снята

`cut_edit.rs`: случай «дно есть, дна не будет» возвращает `Kept` вместо отказа.

```
native_a_hole_may_be_shortened_and_a_pocket_may_not_be_cut_through
  left:  String("topology")
  right: String("unsupported")
```

Это и есть смысл политики: без неё запрос всё равно отказывается, но **общей**
проверкой refs и с сообщением, которое не называет защищаемую ссылку. Отказ
производит политика, а не запасной рубеж.

### 3. Писатель верит публично изменяемому payload

`document.rs`: результат `rederive_parameters` игнорируется.

```
cut_edit::tests::the_writer_refuses_a_prepared_cut_edit_the_document_would_not_have_made
  panicked at ...: refused: ()
```

## Проверки

`cargo fmt --all`, workspace `cargo clippy --all-targets --all-features
-D warnings`, `tools/check-licence-headers.sh` (**356/356** файлов, включая два
новых Rust-файла), `git diff --check`. Публичный
Markdown-рецепт заново извлечён из документа по маркеру `FCAD_26B_AGENT_RECIPE`
и исполнен: `FCAD_26B_RECIPE_OK`.

## Расход ресурсов

Тяжёлые сборки и прогоны шли по одному, `CARGO_BUILD_JOBS=1` и
`CMAKE_BUILD_PARALLEL_LEVEL=1` на каждом. Свободного диска на корне не меньше
75 ГиБ на всём протяжении; `vm.swapusage` сообщает **0** до и после, роста
свопа нет; свободная память не опускалась ниже ~70 %. OCCT и planegcs не
пересобирались; собственный C++ bridge тоже — этот срез его входы не менял.
Использованы два прежних target (native 8.0 ГиБ, stub 3.4 ГиБ); третий большой
target не заводился, чужие каталоги и кэши не чистились.

## Ограничения

* **Оконный GUI smoke не исполнен.** В этой сессии нет инструмента управления
  нативным интерфейсом macOS: доступны Bash, файловые инструменты и браузерный
  Chrome-скилл. Браузерный инструмент нативным UI-доступом не является и так не
  называется. Bundle не собирался, viewer и watchdog не запускались. Headless
  widgets/worker проверкой окна не считаются. Рецепт для Codex — ниже.
* Второй Cut, face attachment, произвольные плоскости, Add/Intersect, Fillet,
  ThroughAll, constrained-инструмент, live preview, правка Cut в середине более
  длинной истории и in-place Save в срез не входят и отказываются.
* Настоящий solver conflict к этому срезу отношения не имеет: деталь и
  инструмент неограниченные, и правка Cut solver не спрашивает.
* Большой STEP и GPU/pixel локально не перепроверялись сверх того, что вошло в
  полный прогон; удалённый строгий reader campaign проверяется отдельно на
  точном публикуемом SHA.
* CI нового diff не запускался; CI базы учитывается отдельно.

## Рецепт GUI smoke для Codex

Собрать свежие release CLI/viewer, поставить их штатными
`tools/runtime-closure.sh` и `tools/stage-runtime-layout.sh`, проверить bundle
`codesign --verify --deep --strict` и `--solver-info` без DYLD. Затем под
`tools/watch-viewer-memory.py` (limit 1536 MiB, один собственный viewer,
временные модели, без тяжёлого STEP):

1. `create-sketch-extrude` плиту 60 × 40 × 10 и `cut-circular-copy` карман
   r5 в (20, 15) глубиной 4 — это источник;
2. открыть его в viewer, нажать `Edit cut Cut — <UUID>…`;
3. убедиться, что поля уже содержат `20`, `15`, `5`, `4`, а подсказка говорит
   «must stay below the part's height»;
4. ввести глубину `10` → `Apply cut` → **видимый отказ**, называющий UUID
   сохранённой ссылки на дно; поля сохранены, история пуста;
5. ввести (30, 20) r7.5 d6 → `Apply cut` → `Undo` (вернулись сохранённые
   числа) → `Redo` → Save Cancel (draft сохранён) → публикация
   `GUI edited.fcad` → async Open, новый title, карман смещён и шире;
6. отдельно повторить с источником-отверстием (глубина 10): подсказка «may run
   to the part's height», укорачивание до 3 публикуется;
7. peer CLI тем же request → побайтово равные STL и FBX;
8. закрыв viewer, **не** вызывать инспектор, который сам вновь запускает
   приложение; проверить inventory и exit 0.


## Независимое ревью, 2026-09-17

Исправлены пять дефектов; все проверки ниже относятся к исправленному diff.

- Первый Undo в Edit cut возвращал пустые поля. История теперь начинается с
  сохранённых четырёх чисел; прежний exact widget gate проверяет Undo и Redo.
- Writer пересоздавал capabilities, теряя необязательные записи и rowid.
  Узкая транзакция сохраняет их и прочие SQL-данные; прежний process gate
  дополнен необязательной capability и нестандартными rowid.
- Новый UUID ссылки на дно мог заместить уже существующую ссылку через upsert.
  Внутри транзакции занятый UUID теперь отказывается без изменений документа.
- Discovery принимал сохранённые числа инструмента за пределами Cut policy.
  Каталог проверяет сохранённый центр, радиус и глубину общей функцией validate.
- Новый CLI test безусловно импортировал Unix API. Symlink-case ограничен Unix,
  Windows проверяет невалидный UTF-16 путь своим API; общие hardlink/atomicity
  проверки сохраняются на всех ОС. Исполнение Windows относится к удалённому CI.

Четыре поведенческих дефекта воспроизведены compiled failing-first assertions
до исправлений (Undo, потеря capability, замещение ссылки, discovery).
Первоначальная неудачная fixture ссылки и недопустимый нулевой radius в fixture
не засчитаны доказательствами; fixtures исправлены до положительного прогона.

Положительные проверки ревью: native release document/jobs **236 passed**, один
прежний timing benchmark ignored; шесть CLI suites **38 passed**; app cuts,
edits, STL, sketch, constraints **49 passed**. Native skips отсутствуют.
Mixed OCCT/no-solver: exact `occt_without_solver_edits_an_unconstrained_cut`
**1 passed**, без skips. Затем восстановлены CLI/app с all-features.
Настоящий stub: CMakeCache NOTFOUND, `otool -L` теста содержит только системные
библиотеки; document cut_edit **18 passed**, CLI **1 исполненный** и **7 явных
skips** (6 geometry, 1 mixed). Stub-отказ до job не доказывает исполнение
предметной политики или version guard.

Fmt, workspace clippy all-targets/all-features с `-D warnings`, export boundary,
actionlint, shellcheck и diff whitespace прошли. Пять новых FBX прочитаны
свежесобранным pinned ufbx (по 6 checks, 0 failures). Рецепт извлечён из Markdown
и исполнен настоящим release CLI с этим reader: `FCAD_26B_RECIPE_OK`.

Инвентарь runtime workflow: **131** обязательное точное имя на ОС, прежние
**112** сохранены. Добавлены два document gate ревью; пяти новым FBX соответствует
**44** чтения на ОС. Это ожидаемые исполнения, а не заявление об уже прошедшем
CI. Результаты конкретных опубликованных head/merge фиксируются отдельно в PR.

Оконный GUI smoke ревью пока не выполнен: CUA дважды сообщает, что Mac
заблокирован; пользователь уведомлён. Viewer не запускался, headless widgets
проверкой окна не объявляются. В ресурсном снимке ревью swap 0, pressure normal,
свободно 75 GiB; это снимок, не измеренный пик. Тяжёлые сборки последовательны,
использованы прежние targets. Причина исторического OOM не установлена.
Локальные логи ревью: `/private/tmp/ferrite-26b-review/`.
