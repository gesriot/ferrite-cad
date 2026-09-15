# §25K — реализация и независимое ревью

2026-09-15. База `b6d3de0b1a7c745a6e3f75fc46bfac19cda41ead`, PR #38 MERGED
(2026-09-15T10:07:17Z, ветка `circle-sketch-extrude`). После
`git fetch --all --prune` HEAD/main/origin/main совпадали, дерево было чистым,
чужого diff не было. CI точного merge SHA перепроверен с полной пагинацией:
**27/27 check runs success**, 6/6 workflows (CI, combined runtime layout,
planegcs pin, product sbom, rust sbom, rust notices). Это **CI базы**; CI
собственного незакоммиченного diff не запускался и базе не приписывается.
Ветка `edit-circle-copy` (без префикса `codex/`). Чужой detached worktree
`/Users/drt/.codex/worktrees/a200/ferrite-cad` не трогался; reset/clean/stash/
rebase/amend не выполнялись. Эти сведения о незакоммиченном состоянии относятся
к передаче реализации; публикация и CI после ревью фиксируются отдельно в PR.

## Предметный результат

[Контракт и рецепт](edit-circle-copy.md).

Пользователь открывает собственный документ с одним unconstrained XY
Sketch/`Circle` → forward literal Blind `Extrude`/`NewBody` → `Body`, численно
меняет центр и радиус и сохраняет проверенную копию. Высота и все identity
остаются. Та же операция доступна агенту по документированному CLI: UUID и
версия берутся из `inspect --json`, без чтения prose и без поиска в SQLite.

## Решения

**Одна структурная проверка.** `sketch_edit::supported` разложена на общий
`frame` (ровно четыре объекта без parent, untransformed XY DatumPlane, forward
literal Blind `Extrude`/`NewBody`, его `Body`, ровно три dependencies
plane/profile/body-tip, и lossless payload выбранного Sketch) и ветку `lines`.
Новый `circle_edit::supported` берёт тот же `frame` и требует ровно одну
неконструкционную неограниченную `Circle` внутри той же `CircleExtrusion`
policy, применённой к **сохранённой** высоте. Разделять эти проверки значило бы
позволить редакторам разойтись; класс, который один принимает, а другой
отвергает, — это класс, который никто не проверил.

**Один каталог из одного снимка.** `ExtrudeEditSource` получил
`circle_sketches: Vec<CircleChoice>` рядом с прежними `sketches` и
`constraint_sketches` — из того же `objects()` и той же `DocumentVersion`.
Файл ради DTO второй раз не открывается. Прежний `vertices: null` не
превращается в фиктивный polygon, и документный `copy_access` сохраняет
приоритет: `available` ложно при документном отказе, а факты структуры при
этом всё равно сообщаются.

**Одна подготовка и одна запись.** `replace_circle_geometry` — единственная
предметная подготовка; `CircleChoice::validate_circle` — тот же самый чек для
draft, без SQLite и kernel. UI и CLI не дублируют ни допустимость, ни запись.
`edit_circle_copy` использует прежний `edit_object_copy`: тот же
no-clobber/alias, snapshot, baseline + edited cold rebuild, повторная проверка
версии источника **перед** публикацией, закрытие SQLite и атомарная публикация.
Новый `CopyWrite::Circle` попадает в **строгую** ветку сохранности refs: правило
выражено методом `requires_resolved_references`, в котором послабление имеет
только прежний extrude-вариант, а не всё прочее по умолчанию. Узкий
`write_circle_geometry` перечитывает строку, отказывается, если Sketch изменился
между подготовкой и записью, повторяет общую предметную подготовку и сравнивает **весь** payload с
подготовленным результатом. Это отвергает подмену UUID, plane, construction и
недопустимых чисел даже при прямом вызове публичного writer. Он меняет ровно `payload` и `payload_hash`. Второго SQLite copier
нет.

**Высота не входит в запрос.** Она принадлежит `Extrude` и меняется прежним
`edit-extrude`. Запрос без неё невозможно случайно использовать как правку
высоты, а `height_mm` в запросе — лишнее поле и отвергается.

## Инварианты, названные до реализации

1. Меняются только центр и радиус названной окружности; в SQL — `payload` и
   `payload_hash` выбранной строки `objects` плюс `meta.modified_at` копии.
2. UUID окружности, Sketch, всех объектов, документа, порядок/имена/parents,
   dependencies, topology refs, высота и выражение Extrude сохраняются.
3. Аналитический B-Rep копии — три грани: `Cylinder` запрошенного радиуса и две
   `Plane`; объём π r² h.
4. Отказ или ранняя отмена ничего не публикуют и не трогают source/занятую цель;
   поздняя отмена не откатывает публикацию; scratch, handles и sidecars не
   остаются.
5. Line-редакторы и их JSON-контракты не меняются.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`, GPU unset,
loader-failure probes unset. Env/target: `/private/tmp/ferrite-25g/native-env.sh`
и `/private/tmp/ferrite-24b-native-target` — оба проверены на существование и
на реальные pinned библиотеки (`vendor/install` OCCT 8.0.1, `vendor/planegcs`)
до запуска; DYLD экспортирован в той же Bash-сессии. Peer CLI пересобран перед
worker-тестами. Ресурсы проверены заранее: ~19–20 GiB свободно, поэтому
переиспользованы существующие native и stub targets и новых многогигабайтных
деревьев не создавалось; чужие кеши не удалялись. Команды —
`/private/tmp/ferrite-25k/native-tests.sh`.

| Проверка | Результат | Лог в `/private/tmp/ferrite-25k/` |
| --- | --- | --- |
| fmt, workspace clippy all-targets/all-features `-D warnings`, `git diff --check` | pass, 0 warnings | `fmt.log`, `clippy.log` |
| document/jobs/eval `--all-features` | **381 executed**, 3 stub-only N/A, 1 прежний ignored benchmark (384 harness-passed) | `domain-tests.log` |
| CLI binary/edit-circle/circle/edit-sketch/sketch/extrude/constraints/JSON/validate/STL | **64 executed**, 0 skips | `cli-tests.log` |
| App `sketch::` | **25 executed**, 0 skips | `app-sketch__.log` |
| App `creates::` / `edits::tests::` / `constraints::tests::` | **17 / 8 / 15 executed**, 0 skips | одноимённые логи |
| licence headers (338 файлов), export boundary, shellcheck, actionlint | pass | — |

Новые named gates: `circle_discovery_and_protocol_without_kernel`,
`native_circle_edit_moves_and_resizes_only_the_named_circle`,
`native_circle_edit_refusals_preserve_source_and_destinations`,
`native_circle_edit_cancellation_races_and_cleanup_are_atomic`,
`circle_edit_non_utf8_paths_refuse_before_reading`,
`sketch::tests::circle_edit_widgets_change_only_the_two_numbers_and_keep_the_draft`,
`sketch::tests::native_circle_edit_worker_and_cli_publish_equivalent_copies`,
плюс четыре исходных unit-теста `circle_edit::tests::` в document. Прежние gates не
переписывались под новый успех.

## Геометрия и identity

Источник — (12, −7), r 10, h 15. Измерено на **переоткрытом** документе после
cold rebuild, а не по result DTO:

| Правка | центр | радиус | высота | грани | боковая поверхность | объём B-Rep mm³ |
| --- | --- | ---: | ---: | ---: | --- | ---: |
| центр и радиус | (−3.5, 4.25) | 6.75 | 15 | 3 | `Cylinder { radius: 6.75 }` | π·6.75²·15 = 2147.08223 |
| только центр | (−3.5, 4.25) | 10 | 15 | 3 | `Cylinder { radius: 10 }` | π·10²·15 |
| только радиус | (12, −7) | 6.75 | 15 | 3 | `Cylinder { radius: 6.75 }` | π·6.75²·15 |
| затем `edit-extrude` 15 → 25 | (−3.5, 4.25) | 6.75 | 25 | 3 | `Cylinder { radius: 6.75 }` | π·6.75²·25 |

Правки только центра и только радиуса существуют именно затем, чтобы потеря
одной компоненты не прошла незамеченной; направленная поломка ниже подтвердила,
что они это ловят.

Оба cap — `Plane`. Все три `TopologyRef` разрешаются **по сохранённым UUID**
после cold reopen; `shape_count() == 1`, и после каждого теста
`live_shape_count() == 0`. Все SQL-ячейки копии сравниваются с источником
по именам колонок; разрешённый список короткий и явный: `objects.payload` и
`objects.payload_hash` **выбранной** строки и `meta.modified_at`. Любая другая
ячейка, включая посторонние таблицы, capability rowids и source claims, обязана
совпасть. Источник остаётся побайтово цел, его mtime не меняется.

STL читается собственным parser при **явном** `--linear-deflection 0.01`.
Проверяются конечность координат, принадлежность каждой вершины основания
запрошенной окружности, собственный периметр и bound box. Для каждого
реального углового промежутка проверяется стрела хорды ≤ deflection с
допуском float32; большое число коротких хорд не скрывает один большой
пропуск. Для r 6.75 получено 82 стороны и периметр 42.401126 mm против
аналитического 2πr = 42.411501. Точное π от тесселяции не требуется.

Исходный отчёт называл две команды `rebuild --cold` проверкой кэша; это
исправлено. Независимый gate использует один настоящий CacheStore для
документов с тем же document UUID: исходник → сдвинутый центр → новый радиус.
Каждый вариант даёт ожидаемые **Miss, затем Hit**. Из фактического результата
измеряются поверхность, объём и центр геометрии; прежняя окружность из кэша
не проходит проверку. Прежний `edit-extrude` после правки окружности меняет
только высоту, сохраняя UUID, центр, радиус и все refs.

UI worker и peer CLI применили **один и тот же** запрос к одному source:
`read_semantics` совпадает, `meta.document_id`, `objects()`, `dependencies()` и
`topology_refs()` равны. STL **и FBX** совпали побайтово; оба FBX также
прочитаны pinned ufbx.

## Отказы, гонки и cleanup

* **Wire-уровень** (до открытия ядра): неизвестная версия, `schema_version`
  вместо `request_version`, отсутствующие поля, лишние `height_mm`/`vertices`,
  центр из одного или трёх чисел, строки/null вместо чисел, не-UUID `curve_id`,
  текстовые `NaN`/`Infinity`/`1e999`, запрос больше 65536 байт, non-UTF-8 path.
* **Domain-уровень** (решается против документа): нулевой, отрицательный и
  слишком большой радиус, центр вне диапазона, чужой `curve_id`, чужой Sketch
  UUID, stale `--expect-version`, занятая цель, source как собственная цель,
  hard link и symlink источника, read-only документ (триггер), Line-полигон
  вместо окружности, отсутствие ядра. В сборке без ядра эти случаи отказываются
  по более ранней причине (`unsupported`); тест это фиксирует явно, а не
  подгоняет ожидание.
* Каждое условие испытано с остальными допустимыми аргументами; ни один случай
  ничего не публикует, каталог остаётся прежним, source байт в байт цел.
* Закрытый stderr после отказа — exit 2 с JSON; закрытые оба канала — exit 7.
* Настоящая успешная публикация с закрытым stdout и с обоими закрытыми
  каналами — два отдельных процесса, каждый exit 7. Копия переоткрыта,
  запрошенная окружность/refs/остальные SQL-ячейки сохранены, source цел.
* Гонки и отмена прогнаны по фазам общей операции на **настоящем** ядре
  (`native_circle_edit_cancellation_races_and_cleanup_are_atomic`): отмена до
  начала, на snapshot, после rebuild, перед публикацией и **после** неё;
  подменённая цель; изменение источника во время работы; поздний hard link.
  Поздняя отмена не откатывает публикацию; все прочие случаи не публикуют,
  не оставляют scratch и не текут handles.

Прежние Line-контракты проверены на том же документе: `editable` остаётся
`false`, `vertices` — `null`, `constraint_edit.available` — `false` для
окружности, и наоборот `circle_edit` отказывает Line-полигону, который прежний
редактор по-прежнему принимает.

## Чувствительность проверок

Две узкие направленные поломки **скомпилировались и дали исполнившиеся
провалы**; скрипты — `/private/tmp/ferrite-25k/mutate-*.py`:

1. **Потеря нового центра**: подготовка сохраняет прежний центр и берёт только
   новый радиус. Запрос, ответ, все UUID и высота остаются верными —
   `native_circle_edit_moves_and_resizes_only_the_named_circle` провалился на
   `left: [12.0, -7.0], right: [-3.5, 4.25]` (**3 passed / 1 failed**,
   `mutation-1.log`).
2. **Обход version guard**: общий `edit_object_copy` перестаёт перепроверять
   версию источника перед публикацией.
   `native_circle_edit_cancellation_races_and_cleanup_are_atomic` провалился на
   случае `changed`, опубликовав копию из источника, изменившегося во время
   работы (**4 passed / 1 failed**, `mutation-2.log`); те же три прежних
   jobs-gate (`sketch_copy_refusals…`, `constraint_copy_refusals…`,
   `cancellation_racing_destination…`) тоже провалились.

Источники восстановлены **побайтово** (sha256
`9230363a3bbf022f44f7852b9e9493368dd2b1b7ebe0cdec58817bf7f8f5e4af` для
`circle_edit.rs` и `2da184d38d100467c7facefd48bfc970f8f34bdcff63d4663c84ae0f4aafcb3a`
для `edit.rs` до и после), затронутые положительные gates повторены. Compile
failure, ноль тестов и skip пойманной поломкой не считались. Новой
mutation/benchmark-инфраструктуры не создавалось.

## Настоящий stub

Переиспользован существующий stub target `/private/tmp/ferrite-25j-stub-target`
с env `/private/tmp/ferrite-25j/stub-env.sh`; новое дерево не создавалось.
Отсутствие env доказательством не считалось: подтверждено, что
`CMakeCache.txt` содержит `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`, а
`otool -L` собранного `ferritecad` не показывает **ни одного** OCCT/PlaneGCS
import.

| Stub-проверка | Executed | Explicit skips | Ignored |
| --- | ---: | ---: | ---: |
| `ferritecad-cli --test edit_circle` | 2 | 3 native-only | 0 |
| `ferritecad-cli --test circle_extrude` | 3 | 2 | 0 |
| `ferritecad-cli --test edit_sketch` | 3 | 2 | 0 |
| `ferritecad-document` | 150 | 0 | 1 |
| `ferritecad-jobs` | 52 | 1 | 0 |
| `ferritecad-app sketch::` | 19 | 6 | 0 |

Ключевое: `circle_discovery_and_protocol_without_kernel` **исполнился и прошёл**
в сборке без ядра — discovery и весь request-протокол работают в настоящем
stub, а применение правки там отказывается. Фикстура этого теста пишет документ
через document API именно для этого; отдельный native-блок дополнительно
правит документ, который действительно создала `create-circle-extrude`.
Harness-passed со skip доказательством геометрии не считается.

## Рецепт и CI

Рецепт извлечён из Markdown по маркеру `FCAD_25K_AGENT_RECIPE` и исполнен
настоящим CLI без разбора prose (`agent-recipe.log`): create-circle → JSON
inspect → IDs/version → edit-circle → inspect/validate/cold rebuild/STL/FBX →
edit-extrude → отказы. Он останавливается на partial export, на diagnostics
validate и на exit 7 и не повторяет запись автоматически. Свежий pinned ufbx
(commit `fcc5d6ba…`, ufbx 0.23.0) собран заново в
`/private/tmp/ferrite-25k/read_production`.

В **существующий** runtime workflow добавлены пять exact-name/no-skip gates
(три CLI и два app) в уже имеющийся circle-шаг на всех трёх ОС; новый тяжёлый
workflow не создавался. Проверено автоматически, что **ни один** прежний gate
не удалён. Три новых небольших артефакта — `circle-edited.fbx`,
`circle-edit-ui.fbx`, `circle-edit-cli.fbx` — добавлены в существующий reader
loop `tools/check-fbx-complex.sh`, доведя число ufbx-чтений на каждой ОС с 25 до
28; локально все три прочитаны (`checks=6 failures=0`).

## Что не проверялось

* **Интерактивный GUI** не запускался: ни окна, ни bundle, ни event loop, ни
  диалогов, ни CUA/osascript, ни GPU/render, ни Unity. Headless egui widget и
  worker tests за GUI не выдаются; GUI smoke честно отложен и этот срез не
  блокирует.
* **На момент передачи удалённый CI diff** не выполнялся: изменения были
  unstaged/uncommitted, push/PR не делались. Приведённые 27/27 и 6/6 — CI
  **базы** `b6d3de0`. Изменённый workflow и reader loop проверены локально
  (actionlint, shellcheck, те же команды в debug), но на трёх ОС GitHub не
  исполнялись и прошедшими не объявляются.
* **Большой STEP/complex-FBX корпус** локально не дублировался: изменение не
  выявило для него конкретного риска; прежние remote gates сохраняются.
* **OCCT из исходников** не пересобирался; pin не менялся.
* Причина исторического OOM неизвестна и исправленной не объявляется.


## Независимое ревью перед публикацией

2026-09-15, журналы `/private/tmp/ferrite-pr39-review/`. Публичный writer
принимал подменённый UUID в mutable подготовленном payload. Failing-first
`the_circle_writer_refuses_forged_payloads_and_stale_preparations` исполнился
и упал на `writer accepted forged curve identity`; после повторной общей
валидации прошёл. Дополнительные случаи сохраняют plane/construction и
отказывают неверному радиусу и устаревшей подготовке.

Форма теперь требует явный **Apply circle change**: набор полей за несколько
кадров даёт один шаг истории, Save не публикует неподтверждённые числа.
Черновик хранится до принятия опубликованной сцены; при отказе загрузки или
подготовки восстанавливаются поля и история. Headless widget/worker gates
проверяют эти переходы, без заявления об оконном smoke.

Финальный native прогон: fmt и workspace clippy all-targets/all-features
`-D warnings`; **382 исполненных domain/jobs/eval**, три stub-only N/A и один
прежний ignored benchmark (385 harness-passed); **64 CLI**, **69 headless app**
(25 sketch + 17 creates + 8 edits + 15 constraints + 4 STL), без native skips.
Все команды последовательные, один build job и один test thread; peer CLI
пересобран до worker gates.

Независимый настоящий stub: **230 исполненных**, **14 явных native skips**,
один ignored benchmark. По suite: edit_circle 2+3 skip, circle_extrude 3+2,
edit_sketch 3+2, document 151, jobs 52+1, app sketch 19+6. Фактические CMakeCache
и imports CLI/viewer подтверждают отсутствие OCCT/PlaneGCS; viewer не запускался.
Исправленная таблица выше отделяет исполнения от harness-passed исходного
прогона; дополнительный writer test даёт +1 document в финальном прогоне.

Настоящий CacheStore проверен на Miss/Hit для source, center-only и
center+radius копий с тем же document UUID; измеряется фактическая геометрия.
Два успешных процесса с закрытым stdout/обоими каналами оставили проверенные
копии и вернули 7. STL проверяет каждую реальную хорду. UI/CLI STL и FBX
совпали побайтово. Все **семь** small-circle FBX прочитаны заново собранным
pinned ufbx: каждый 6 checks, 0 failures. Текущий рецепт извлечён из Markdown
и исполнен; 82 стороны, периметр 42.40112596634743 mm, расчётный πr²h
2147.082229187774 mm³; аналитический объём измерен отдельно native gate.

На последнем локальном ресурсном замере свободно 19 GiB, viewer отсутствует,
swap 2757.19 MiB без роста. Оконные, GPU, Unity и большой STEP-корпус локально
не запускались. CI публикуемого SHA и merge SHA проверяются отдельно в PR;
результаты базы за них не выдаются.
