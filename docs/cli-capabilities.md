# Карта предметных возможностей: UI ↔ CLI ↔ общий API

Срез §24A, обновлён срезами §24B/§24C/§24D. Это описание **текущих** команд и владельцев, а не проект нового протокола.

Документ нужен агенту и человеку, которым дали задачу и публичный CLI: что уже можно получить тем же предметным результатом, что в UI, что умеет только один клиент, и где библиотечный метод ещё не является пользовательской операцией. Архитектурное правило — [§4.5 плана](implementation-plan.md): UI и CLI — два клиента общих операций; равенство считается по сохранённой модели и экспортируемым артефактам, а не по hover, жестам мыши или промежуточным кадрам.

Сборки и чертежи в beta не входят. Импортированная STEP-сборка сохраняет структуру и может быть экспортирована в FBX; это не редактор сборок.

## Как читать таблицу

- **Предметный результат** — что должно оказаться в `.fcad` или в выданном файле.
- **UI сейчас** — что пользователь может сделать в `ferritecad-viewer` без чтения исходников.
- **Публичный CLI сейчас** — подкоманда `ferritecad` из `--help`. Отсутствие строки в `--help` значит, что команды нет.
- **Общий API / владелец** — библиотечный вход, которым реально пользуется клиент. Ссылки ведут в исходники, чтобы назвать владельца, а не чтобы агент читал их ради поддерживаемой задачи.
- **Фактический пробел** — чего нет у пользователя, даже если в Rust есть тип или метод.

**Библиотечная возможность ≠ пользовательская операция.** `rebuild_cached`, `Document::write`, `ObjectPayload::Parameter` и методы `OcctKernel` вне контракта `GeometryKernel` существуют в коде и тестах. Это не команды CLI и не действия UI, пока их нет в публичном `--help` или на экране.

Помечено **измерено**, если поведение снято с настоящего CLI в этом срезе. Помечено **из кода / тестов**, если оно известно из исходников, `--help` или существующих тестов и здесь заново не воспроизводилось.

## Что этот срез сознательно не делает

§24D задаёт [JSON v1](cli-json-v1.md) только для opt-in `inspect --json` и `edit-extrude --json`. Общего JSON всех команд, DSL, RPC/MCP server, универсального dispatcher и библиотеки «всех операций» нет. Stdin, отмена CLI и пакетный ввод остаются будущими требованиями §4.5.

§24B добавил ровно одну общую операцию — создание документа — и её второго клиента (`New…` во вьюере). Новых команд и флагов у `ferritecad` нет. Остальные возможности сохраняют прежних клиентов.

## Карта текущих операций

| Предметный результат | UI сейчас | Публичный CLI сейчас | Общий API / владелец | Фактический пробел |
| --- | --- | --- | --- | --- |
| Пустой нативный `.fcad` | `New…` → «Empty document» → системный Save-диалог → создание → обычный Open результата. | `ferritecad create <path>` | Один маршрут: [`ferritecad_jobs::create_document`](../crates/ferritecad-jobs/src/create.rs). Внутри — `Document::create_with` в scratch, транзакция [`Document::write`](../crates/ferritecad-document/src/document.rs), закрытие соединения и атомарная публикация через [`Temporary`](../crates/ferritecad-jobs/src/publish.rs). CLI: [`create`](../crates/ferritecad-cli/src/main.rs). UI: [`creates`](../crates/ferritecad-app/src/creates.rs). | Ни у `create`, ни у UI нет замены существующего файла: занятое назначение — отказ. Пустой документ и пустое окно без документа — разные состояния. |
| Sample plate с шириной, глубиной, высотой | `New…` → «Sample plate» → W/D/H в мм → Save-диалог → создание → обычный Open. Размеры подписаны единицей; значения по умолчанию те же, что у CLI. | `ferritecad create <path> --sample --size W D H` | Тот же `create_document` с `NewDocument::SamplePlate(PlateSize)`. Построение плиты — приватная транзакция в [`create.rs`](../crates/ferritecad-jobs/src/create.rs); в CLI-крейте её больше нет. Размеры — координаты эскиза и `Expression::constant` высоты, не объекты `Parameter`. | Нет правки уже созданной плиты: `--size` действует только в момент создания. Случайные `ObjectId`/`DocumentId` при каждом создании не совпадают — это контракт, не баг. |
| Прочитать документ (метаданные, объекты, ссылки) | `Open…` читает `.fcad` read-only через `snapshot_of` → `prepare::load`, с холодным перестроением. | `ferritecad inspect <path> [--json]` | [`Document::open_read_only`](../crates/ferritecad-document/src/document.rs). Текст — `render::inspect`; JSON — общий `ExtrudeEditSource` на том же закреплённом снимке, без kernel/rebuild, миграции и записи. | JSON v1 даёт каталог Extrude для правки, а не всю БД/геометрию. UI не показывает сырой граф и topology refs. |
| Проверить документ без ядра | Нет отдельной команды. Неоткрываемый файл даёт `Open failed`; это отказ загрузки, не отчёт `validate`. | `ferritecad validate <path>` | [`Document::validate`](../crates/ferritecad-document/src/document.rs) / [`validate::validate`](../crates/ferritecad-document/src/validate.rs). Код 1 при ошибках валидации — *из кода*. | Нет JSON-отчёта. Нет UI-эквивалента «документ внутренне согласован», отдельного от «нарисовался». |
| Cold rebuild нативного графа | Не команда. Open/Export сами делают холодное перестроение как часть чтения. | `ferritecad rebuild --cold <path>` | [`rebuild_cold`](../crates/ferritecad-eval/src/cold.rs) в `ferritecad-eval` против [`OcctKernel`](../crates/ferritecad-occt/src/kernel.rs) / [`GeometryKernel`](../crates/ferritecad-kernel/src/kernel.rs). Документ — `open_read_only`. | Без `--cold` команда отказывает (exit 2, **измерено**). [`rebuild_cached`](../crates/ferritecad-eval/src/cold.rs) есть в библиотеке и тестах и **не** предлагается CLI/UI. Публичного cached-rebuild нет. |
| Граф зависимостей | Нет. Список определений во вьюере — не dump графа. | `ferritecad dump-graph <path> [--format <text\|dot>]` | [`Document::evaluation_order`](../crates/ferritecad-document/src/document.rs), [`evaluation_order`](../crates/ferritecad-document/src/graph.rs), печать — [`render::graph_text` / `graph_dot`](../crates/ferritecad-cli/src/render.rs). | `dot` — Graphviz, не JSON-контракт всех команд. `--format json` нет (**измерено**, clap exit 2). |
| Topology references: что документ назвал и держится ли это | Клик по именованной грани/ребру/углу нативного тела показывает переносимое имя в инспекторе. Это просмотр, не отчёт по всем ссылкам. | `ferritecad print-topology <path>` | Хранение — [`Document::topology_refs`](../crates/ferritecad-document/src/document.rs). Разрешение — `RebuildResult::resolve` после `rebuild_cold`. Отчёт и коды — [`topology::print_topology`](../crates/ferritecad-cli/src/topology.rs). *Из кода:* lost → exit 3, invalid → 1, unsupported/прочее → 2. | Нет команды «добавить/изменить ссылку». Роль сегмента эскиза как самостоятельной ссылки в плане помечена как граница, не упущение CLI. Импортированная топология не именуется durably. |
| STEP → новый `.fcad` (байты источника внутри) | Нет. Open принимает `.fcad`, не STEP. Отдельное продолжение после §23 это признаёт. | `ferritecad import-step <file.step> -o <out.fcad> [--name <name>] [--force]` | Чтение байтов — CLI. Ядро — [`OcctKernel::import_step`](../crates/ferritecad-occt/src/kernel.rs) → [`ferritecad_exchange::decode`](../crates/ferritecad-exchange/src/lib.rs) / `Import`. Запись — [`Document::store_step_import`](../crates/ferritecad-document/src/document.rs). Публикация файла — [`Temporary`](../crates/ferritecad-jobs/src/publish.rs). | UI не импортирует STEP. Импорт не делает сборку редактируемой. Нет STEP-экспорта (команды `export-step` нет). |
| Binary STL одного тела | Нет. | `ferritecad export-stl <path> -o <file.stl> [--solid <name-or-id>] [--linear-deflection <mm>] [--angular-deflection <rad>] [--force]` | Холодный rebuild; выбор `Body` — приватный `choose` в [`export.rs`](../crates/ferritecad-cli/src/export.rs) CLI, не публичный библиотечный API; сетка — `GeometryKernel::tessellate`; байты — [`binary_stl`](../crates/ferritecad-export/src/lib.rs); публикация — `ferritecad-jobs::Temporary`. Это **не** общий job, в отличие от FBX: второй клиент (UI) не подключён. | UI не экспортирует STL. Документ только с `ImportedStep` тел `Body` не содержит: `export-stl` отказывает «contains no bodies» (**измерено**). |
| FBX 7.4 ASCII всей модели | `Export FBX…` при принятой сцене. Замена существующего файла — вопрос окна, не `--force`. Отмена есть. | `ferritecad export-fbx <path> -o <file.fbx> [--force]` | Один маршрут: [`ferritecad_jobs::export_document_as_fbx`](../crates/ferritecad-jobs/src/fbx.rs) ← [`export_scene`](../crates/ferritecad-scene/src/export.rs) ← `prepare::load`. CLI: [`export_fbx::export_fbx`](../crates/ferritecad-cli/src/export_fbx.rs). UI: [`exports::export_into`](../crates/ferritecad-app/src/exports.rs). | Это уже два клиента одной операции. Нет JSON-отчёта. CLI не выставляет отмену; UI — выставляет. Частичный файл и exit 6 — *из README/кода*, в этом срезе на плите и `01-single-part` не наблюдался. |
| Диагностика sketch solver (что слинковано) | Не панель. Команда бинаря вьюера. | Нет у `ferritecad`. Есть `ferritecad-viewer --solver-info` (окно не открывается). | [`ferritecad_sketch_solver::provenance`](../crates/ferritecad-sketch-solver/src/lib.rs), маршрутизация — [`solver_info`](../crates/ferritecad-app/src/main.rs). Что rebuild нашёл по constrained sketch — `SketchSolveReport` в `ferritecad-eval`; во вьюере панель `Sketch solves` после Open. | CLI документа эту диагностику не печатает. Нет команды «решить эскиз отдельно от rebuild». Сборка без planegcs отвечает unavailable и exit 3 (*из README/кода*). В этом срезе solver **available**, exit 0 (**измерено**). |
| Удалить регенерируемый `.fcad-cache` | Нет. Viewer/CLI cold-путь sidecar не пишут. | `ferritecad clear-cache <path>` | [`CacheStore::discard`](../crates/ferritecad-document/src/cache.rs). | Пользовательского warm rebuild нет, поэтому после `rebuild --cold` sidecar обычно отсутствует (**измерено**: «no cache sidecar»). Библиотечный `rebuild_cached` кэш писать умеет — это не пользовательская операция. |

## Ограничения моделирования (чтобы агент не обещал лишнего)

Создать sample plate **не значит** произвольно редактировать параметры, эскизы и фичи существующей модели. `create --sample --size` и `New… → Sample plate` задают прямоугольник и высоту **в момент создания**. §24C позволяет изменить существующее постоянное Blind-выдавливание в новой копии (см. ниже). Команды или действия окна изменить ширину, заменить сегмент, добавить отверстие, fillet или параметр нет. В документе плиты нет объектов `Parameter`: ширина и глубина — координаты четырёх линий эскиза, высота — константа `Blind` у `Extrude` ([`create.rs`](../crates/ferritecad-jobs/src/create.rs)). Типы `Parameter` и `SketchConstraint` в `ferritecad-document` есть; пользовательской операции «задать параметр / поставить ограничение» нет.

Плита — **шаблон**, и окно называет её так. `New…` во вьюере не вводит несохранённый редактируемый документ: имя файла выбирается до создания, результат открывается обычным Open, `Save` / `Save As` для уже существующей модели по-прежнему нет.

Допустимость размеров решает документ, а не форма и не флаг. **Измерено:** ширина и глубина принимают ноль и отрицательные значения (это координаты эскиза), высота обязана быть положительной (`extrude distance must be positive`), любой не-конечный размер отказывается (`model values must be finite`). §24B этих правил не менял.

Импортированная STEP assembly **не редактор сборок**. `import-step` сохраняет definitions, placements и исходные байты. Нет команд разместить компонент, сопрячь, подавить, переименовать дерево или пересобрать сборку как нативную. `rebuild --cold` по такому документу **не пересчитывает** хранимую геометрию (**измерено**: «0 objects evaluated, 0 shapes built» и отдельная строка про imported object).

Агент **не получит** по одному заданию:

- произвольную параметрическую деталь, которой нет в sample plate и которой нельзя добиться импортом готового STEP;
- редактируемую сборку или чертёж;
- STEP наружу, DXF, ЕСКД;
- JSON-контракт всех команд, stdin-пакет, отмену CLI.

Отказ должен быть отказом, а не «почти той же» моделью.

## Что во вьюере не является разрывом предметного CLI

Орбита, pan, zoom, именованные виды, перспектива/ортогональ, hover, клик, Hide/Isolate/Show all, Frame, сетка, `Undo visibility` меняют **картинку сессии**. Они не пишутся в `.fcad` и не входят в экспорт: FBX — холодное чтение сохранённого файла, не копия GPU-снимка (README и [`ferritecad-app` exports](../crates/ferritecad-app/src/exports.rs)). Выбор именует definition и хранимые native names; GPU pick id в документ не сохраняется.

Это не пробел автоматизации модели. Если позже понадобится снимок или чертёж с заданным ракурсом, ракурс станет **явным параметром новой операции**, которой сейчас нет.

## Контракт текущего CLI

Два бинаря: `ferritecad` (предметные команды) и `ferritecad-viewer` (окно и `--solver-info`). Справка вьюера как у clap нет: лишний аргумент и `--help` — usage, exit 2 (**измерено**).

### Единицы

- Внутри модели длины — миллиметры, углы — радианы ([`Unit`](../crates/ferritecad-types/src/units.rs)).
- `create --length-unit` / `--angle-unit` — **единицы отображения** (по умолчанию `mm` / `deg`). Значения всё равно хранятся в мм и радианах.
- `--size` задаётся **в миллиметрах**, независимо от `--length-unit` (текст `--help`).
- `export-stl --linear-deflection` — мм (по умолчанию 0.01); `--angular-deflection` — радианы (по умолчанию 0.5).

### Адресация объектов

- Нативные объекты — `ObjectId` (печатаемый UUID) и необязательное имя (`XY`, `Profile`, `Extrude1`, `Plate`).
- `export-stl --solid` — имя или идентификатор **тела** (`Body`). При одном теле флаг необязателен; при нескольких — обязателен, без угадывания первого.
- Topology refs — `StableEntityId`, семантическая роль, правило выборки. Не индексы граней, не session handles.
- Импортированная деталь — `ImportedSourceId` + ключ вида `step.product_definition#5` (локален одному источнику).
- Экранные координаты, GPU picking id и порядок обхода **не** являются адресами CLI.

При совпадающих именах `--solid` отказывает и предлагает UUID. Строка, разбираемая как UUID, адресует идентификатор, даже если такое же имя есть у другого тела (*из кода*).

Идентификаторы нового `create` каждый раз другие. Рецепт не должен требовать конкретного UUID.

### Потоки и форматы

| Поток | Что туда пишется |
| --- | --- |
| stdout | Отчёты команд, включая невалидный документ (`validate`, exit 1), потерянные ссылки (`print-topology`, exit 3), замечания/отказ STEP (exit 4/5) и итог опубликованного partial FBX (exit 6). Также `--help`, `--version`, `--solver-info`. Наличие stdout не означает exit 0. |
| stderr | `error [kind]: …` и цепочка `caused by:` для `CadError`; usage при ошибке аргументов clap; `note:` если путь `create` без `.fcad`; подробности partial FBX. |

Форматы вывода сейчас:

- проза / фиксированные текстовые отчёты;
- `dump-graph --format dot` — Graphviz DOT, только эта команда;
- STL — бинарный, 80-байтовый заголовок без даты (**измерено**: `FerriteCAD binary STL. Units are millimetres. No timestamp, by design.`);
- FBX — ASCII 7.4;
- `inspect --json` и `edit-extrude --json` — один объект JSON v1 + LF, [точный контракт](cli-json-v1.md).

**Нет:** общего JSON всех команд, stdin как входа документа, пакетного списка задач, флага отмены у `ferritecad`. `inspect -` открывает файл с именем `-`, а не стандартный ввод (**измерено**). Существующий DOT не означает JSON-контракта остальных команд.

### Коды исхода `ferritecad`

| Код | Когда | Источник |
| --- | --- | --- |
| 0 | Команда сделала то, что обещала без оговорок | **измерено** на create/inspect/validate/dump-graph/rebuild --cold/print-topology (решённые ссылки)/import без диагностики/clear-cache/полном STL и FBX |
| 1 | `validate`: документ открылся, но невалиден; `print-topology`: contradictory stored ref | *из кода* |
| 2 | Не запустилась или отказала: clap usage, `CadError` по умолчанию, нет `--cold`, нет тел для STL, файл уже есть | **измерено** |
| 3 | `print-topology`: документ перестроился, имя потеряно | *из кода* |
| 4 | `import-step`: документ записан целиком, чтение что-то сообщило | *из кода / README* |
| 5 | `import-step`: ничего не записано (отказ читать); отчёт находится на stdout | **измерено при ревью** на файле, не являющемся STEP |
| 6 | `export-fbx`: файл опубликован и неполон | *из кода / README* |
| 7 | Только JSON v1: отчёт не доставлен; копия уже могла быть опубликована, автоматический повтор правки недопустим | **измерено** реальным закрытым stdout |

Скрипт не должен считать 4 или 6 нулём и не должен считать 6 ошибкой «файла нет».

`ferritecad-viewer --solver-info`: 0 — solver есть (**измерено**); 3 — нет (*из кода*); 2 — usage (**измерено**).

### Замена файлов

- `create` существующий путь не заменяет и **не имеет** `--force`. Текст: `already exists; creating would destroy it` (**измерено**, exit 2).
- `import-step`, `export-stl`, `export-fbx` без `--force` не заменяют назначение. Текст: `already exists; pass --force to replace it` (**измерено**, exit 2, байты назначения не меняются).
- Исходник не может быть назначением даже с `--force` (*из кода*: `refuse_source_as_destination`).
- `import-step` и оба экспорта публикуют через scratch рядом с назначением: ошибка выполнения оставляет назначение как было. UI для FBX вместо `--force` спрашивает `Replace existing file?`.
- `create` публикует через scratch рядом с назначением, как `import-step` и оба экспорта (§24B). Документ строится целиком, соединение SQLite закрывается, и только потом файл появляется по указанному пути одним атомарным no-clobber шагом. Файл, появившийся после первоначальной проверки, сохраняется, а не заменяется.
- UI на занятое назначение отвечает `already exists; choose a different file name` — своими словами и без флага, которого у окна нет. Общий `create` не заменяет файл. Системный Save-диалог macOS при выборе существующего имени может сначала показать Replace; даже после подтверждения приложение отказывает и сохраняет прежний файл.

### Чтение и возможная запись документа

`validate`, `dump-graph` и `clear-cache` используют `Document::open`, который может мигрировать старую схему и настраивает SQLite connection. Эти команды не обещают неизменность файла во всех случаях. Измерение неизменных байтов/mtime текущего документа не доказывает read-only контракт старых файлов. `inspect` (с §24C), Viewer, `rebuild --cold`, `print-topology` и оба экспорта используют `open_read_only`: старую схему или WAL-состояние отказывают, а не мигрируют (*из кода*).

### Ошибка

Ошибка выполнения экспорта или импорта (exit 2) не публикует полузаписанный STL/FBX/новый `.fcad` импорта (*из кода* и README). Это не относится к явным результатам 4 и 6: файл уже опубликован. `CadError` идёт на stderr с `ErrorKind`; отказ STEP (exit 5) идёт отдельным отчётом на stdout. Отказ Open во вьюере не подменяет картинку. Операции CLI, принимающие `OperationContext`, используют `default()` и не предоставляют отмену.

Отказ или отмена, замеченная до публикации, не оставляют нового `.fcad` и scratch (§24B, **измерено**). Поздняя отмена уже опубликованный файл не удаляет: UI сообщает Created, но не переключает принятую сцену.

**Исправленный дефект `create`, §24B.** До §24B `ferritecad create bad.fcad --sample --size NaN 50 12` возвращал exit 2 (`model values must be finite`) и оставлял по новому пути пустой `.fcad` с нулём объектов: файл создавался до наполнения, а откат `Document::write` уже созданный файл не отменял. Теперь это два разных барьера — транзакция внутри документа и атомарная публикация целого файла, — и оба обязательны. Failing-first проверки живут в [`crates/ferritecad-cli/tests/create.rs`](../crates/ferritecad-cli/tests/create.rs): они запускают настоящий процесс и падают именно на оставленном файле; проверяются оба вида отказа — до транзакции (`--size NaN 50 12`) и внутри неё (`--size 60 40 NaN`, высота строится уже после трёх записанных объектов).

## Рецепты на существующих публичных командах

Примеры ниже — shell для macOS/Linux (или Bash с подходящим окружением в Windows); это не PowerShell. Публичный `ferritecad` должен быть в `PATH`; для поставки можно добавить её каталог CLI, а для checkout-сборки настроить библиотеки по README. Каждый блок запускается в отдельном subshell, создаёт собственный временный каталог и оставляет результаты в нём. `set -eu` останавливает блок при неожиданной ошибке; ожидаемые отказы в рецепте 3 разобраны отдельно. UUID в выводе будут другими. Не нужно читать исходники. Сборка должна быть с Open CASCADE: ответ `unsupported` / «this build has no Open CASCADE» — не успех.

Исходные fixtures репозитория не трогать. Для STEP копировать файл во **свой** каталог и удалять только эту копию.

Обработка кодов: продолжать только при 0 (в этих рецептах 4 и 6 не ожидаются). 2 — остановиться и не считать файл обновлённым.

### 1. Sample plate заданного размера → проверка → cold rebuild → экспорт

```sh
(
    set -eu
    recipe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-plate.XXXXXX")
    cd "$recipe_dir"
    ferritecad create plate.fcad --sample --size 80 50 12
    ferritecad validate plate.fcad
    ferritecad rebuild --cold plate.fcad
    ferritecad export-stl plate.fcad --output plate.stl
    ferritecad export-fbx plate.fcad --output plate.fbx
    printf 'Results: %s\n' "$recipe_dir"
)
```

Ожидание при exit 0:

- `created plate.fcad (…)`
- `plate.fcad is valid (0 warnings)`
- rebuild: ядро `occt 8.0.1 …`, `4 objects evaluated, 1 shape built`, `3 of 3 stored references resolved`; у `Extrude1` в отчёте `solid, 6 named faces` (имена rebuild, не число **хранимых** ссылок — их три)
- STL: `12 triangles, 684 bytes` с плиты `Plate`, deflection `0.01 mm` / `0.5 rad`
- FBX: `complete: nothing this document holds was left out of the file`

Полезные, но необязательные проверки: `inspect`, `print-topology` (три `resolved`), `dump-graph`. `clear-cache` после этого cold-пути сообщает, что sidecar нет — это нормально.

### 2. Небольшой STEP → `.fcad` → экспорт после удаления своей копии STEP

Этот блок запускается из корня checkout и копирует маленький `fixtures/step/canonical/01-single-part.step`. Для своего STEP задайте `source_step` абсолютным путём к нему; размеры и текст отчёта тогда будут зависеть от файла.

```sh
(
    set -eu
    source_step="$PWD/fixtures/step/canonical/01-single-part.step"
    recipe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-step.XXXXXX")
    cp "$source_step" "$recipe_dir/part.step"
    cd "$recipe_dir"
    ferritecad import-step part.step --output part.fcad
    rm part.step
    ferritecad export-fbx part.fcad --output part.fbx
    printf 'Results: %s\n' "$recipe_dir"
)
```

Ожидание при exit 0:

- import: `15408 byte(s) stored whole`, `1 definition, 1 placement`, ключ `step.product_definition#5`, и честное «nothing was reported while reading it» / «that describes this reader, not the file»
- после `rm` исходный fixture на месте, своей копии нет
- FBX всё равно пишется: геометрия берётся из байтов внутри `.fcad`

Не использовать здесь `export-stl`: у импортированного документа нет `Body`, команда отвечает `contains no bodies to export` и exit 2. Это текущий контракт STL, не сбой импорта. `rebuild --cold` по такому файлу не «пересобирает солид из фич»; он сообщает, что imported geometry не пересчитывается.

### 3. Отказ перезаписать результат без `--force`

```sh
(
    set -eu
    recipe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-replace.XXXXXX")
    cd "$recipe_dir"
    ferritecad create plate.fcad --sample --size 80 50 12
    ferritecad export-fbx plate.fcad --output plate.fbx
    cp plate.fcad before.fcad
    cp plate.fbx before.fbx
    if ferritecad create plate.fcad --sample --size 1 1 1; then
        printf 'Unexpected create success\n' >&2
        exit 1
    else
        code=$?
        [ "$code" -eq 2 ] || exit "$code"
    fi
    cmp before.fcad plate.fcad
    if ferritecad export-fbx plate.fcad --output plate.fbx; then
        printf 'Unexpected export success\n' >&2
        exit 1
    else
        code=$?
        [ "$code" -eq 2 ] || exit "$code"
    fi
    cmp before.fbx plate.fbx
    ferritecad export-fbx plate.fcad --output plate.fbx --force
    printf 'Results: %s\n' "$recipe_dir"
)
```

Ожидание:

- первый create и первый export — exit 0
- второй create — exit 2, `already exists; creating would destroy it`; `--force` у `create` нет
- второй export без `--force` — exit 2, `already exists; pass --force to replace it`; `plate.fbx` не меняется
- третий, с `--force` — exit 0, файл заменён целиком

То же для `export-stl` и `import-step` на существующее назначение.

## Проверено в этом срезе

- База: `main` @ `aac7f6c72dcb6a85b708946c51eab615c985a706` (merge PR #12). Ветка документации: `cli-capability-map`.
- CLI: `/Users/drt/Desktop/github/ferrite-cad/target/release/ferritecad`, `ferritecad 0.0.1`, SHA-256 `b6c5d543530d8d2a8109b6f2b085361d18556b5c7123075abee3da592f69b1ae`. Слинкован с `@rpath/libTKernel.8.0.dylib` (OCCT 8.0.1). У этого checkout-бинаря нет `LC_RPATH`: для запуска из `target/release` нужен `DYLD_LIBRARY_PATH` на `vendor/install/lib` (и `vendor/planegcs` для вьюера). Поставочный `FerriteCAD.app/Contents/MacOS/ferritecad` библиотеки ищет сам.
- Viewer: тот же `target/release/ferritecad-viewer`. `--solver-info` → exit 0, `sketch solver: available`, provenance planegcs FreeCAD 1.0.1.
- Рецепт 1: create `--size 80 50 12` → validate → `rebuild --cold` (occt 8.0.1, 1 shape, 3/3 refs) → STL 684 байта / 12 треугольников → FBX 4104 байта, complete. Все exit 0. Sidecar после cold rebuild не появился.
- Рецепт 2: копия `01-single-part.step` (15408 байт) → import exit 0 → удалена только копия → FBX 4103 байта, complete, exit 0. Оригинал fixture не менялся. `export-stl` по этому `.fcad` — exit 2, тел нет.
- Рецепт 3: повторный `create` и экспорт без `--force` — exit 2, хеши STL/FBX не изменились; `export-fbx --force` — exit 0.
- Дополнительно **измерено**: пустой `create`; FBX пустого документа 1218 байт, complete, 0 nodes; `create` без `.fcad` пишет файл и `note:` на stderr, exit 0; clap unknown command / missing args / `--format json` — exit 2; `inspect` текущего файла байты не переписывает.

**Независимое ревью, 2026-09-06:** три shell-блока извлечены из этого документа и выполнены как написаны в отдельных временных каталогах (CLI и native libraries подготовлены в окружении). Подтверждены 684-байтовый STL, FBX 4104/4103 байта, cold rebuild 3/3 refs, отсутствие sidecar и отказ create/FBX без изменения назначения; отдельно — `inspect` с сохранением байтов/mtime, `print-topology`, DOT, `clear-cache`, обязательность `--cold`, отказ STL для imported-only, no-clobber STL/импорта и `--solver-info` (0). Все локальные ссылки существуют; shell-блоки проходят shellcheck. На входе, не являющемся STEP, измерен exit 5 с отчётом на stdout и без нового `.fcad`. Воспроизведён дефект `create` с `NaN`, описанный выше. Rust-код этот документальный срез не меняет.

Не измерялись в этом срезе и ревью (остаются *из кода/тестов/README*): exit 1/3/4/6, partial FBX, миграция старой схемы при `inspect`, побайтовое равенство UI- и CLI-FBX (зафиксировано ревью §23, здесь не повторялось), Windows/Linux CLI.

## Найденные пробелы

| Пробел | Статус |
| --- | --- |
| UI создаёт пустой документ или sample plate и меняет постоянную Blind-высоту в новой копии; нет in-place Save и STEP Open | факт |
| Нет произвольной правки параметров/эскиза/фич | Есть только изменение постоянной Blind-высоты в новой копии (§24C) |
| Нет UI/CLI STEP-экспорта | факт |
| `export-stl` только для `Body`; imported-only документ не экспортируется в STL | факт, **измерено** |
| STL, inspect, validate, graph, topology, rebuild, import, cache — один клиент (CLI), не два | факт |
| `rebuild_cached` есть в библиотеке, пользовательского warm rebuild нет | факт |
| Нет JSON всех команд, stdin/batch/отмены CLI | JSON v1 ограничен inspect/edit-extrude (§24D) |
| `create` без `--force` и с другим текстом отказа, чем publish-команды | факт |
| Случайные UUID независимых `create` не равны | обещание §4.5, не баг |
| Временная видимость и камера не в CLI | не предметный разрыв; см. выше |

Независимое ревью воспроизвело оставленный пустой файл после отказа `create --sample --size NaN 50 12` (см. «Ошибка»). Он исправлен в §24B общей операцией создания; failing-first запускает реальный CLI на старом маршруте. `export-stl` на imported STEP ведёт себя согласно своему правилу выбора тел.

## §24B: общий create и New

Реализован `ferritecad_jobs::create_document(CreateDocumentRequest, OperationContext)` →
`CreatedDocument` (путь и document ID). Типизированный `NewDocument` задаёт Empty или
SamplePlate(PlateSize); размеры хранятся в мм независимо от display units.
Построение графа больше не принадлежит бинарному CLI-крейту. Используются существующие
Document::write и Temporary: транзакция модели, затем закрытие SQLite и атомарный
no-clobber. Последняя проверка CancelToken стоит перед публикацией; поздняя отмена
не отменяет уже опубликованный файл.

New → форма → системный Save-диалог → worker → обычный асинхронный Open.
New недоступен во время Open/Export или подтверждения экспорта; Open/Export
недоступны при открытой форме New и до результата create. Cancel creation ждёт
ответ worker: успех после поздней отмены сообщается как созданный файл, но не
переключает сцену. Ошибка создания/загрузки и отмена сохраняют принятую сцену и
export source. Пустой сохранённый документ отличается от окна без документа.

Проверки запускают оба клиента: настоящий процесс `ferritecad create` и командный
маршрут UI (`ask_new` / `start_new` / `finish_create`) без диалоговых кликов.
Сравниваются полные payloads, parent/dependency links, роли и связь topology refs
с сегментами по согласованному сопоставлению UUID; самостоятельные создания
получают разные identity. Native gate делает cold rebuild с 3/3 refs, сравнивает
STL и FBX после сопоставления identity, а CLI/UI FBX одного документа — побайтово.
Отдельно проверены отказ внутри транзакции, гонка назначения перед publish,
отмена после закрытия SQLite, уборка scratch/sidecars, поздняя отмена и устаревший Open.
Обычный CI запускает CLI/app/UI/jobs/document gates; OCCT pin включает native gate.

§24 целиком не завершён. Ограниченная правка существующей модели добавлена §24C,
JSON двух команд — §24D. Stdin/batch, произвольный редактор и STEP из UI остаются недоступны.


## §24C: edit-extrude и Edit extrusion

| Операция | UI | CLI | Владелец |
| --- | --- | --- | --- |
| Изменить расстояние существующего constant Blind Extrude в новой копии той же модели | `Edit extrusion…` → явный UUID/имя, текущие mm → новое значение → `Save new file…` → обычный Open | `edit-extrude <source.fcad> --feature <uuid> --distance-mm <n> -o <new.fcad> [--expect-version <hash>] [--json]` | `ferritecad-jobs::edit_extrude_copy`; SQLite snapshot/version — document; геометрия — общий evaluator |

В JSON v1 UUID берётся из `result.features[].feature_id`, версия — `result.content_version`.
Текстовый `inspect` также печатает UUID в строке `feature.extrude` и `content version`.
При нескольких фичах UUID выбирается явно. Имена, позиции в списке и GPU IDs не адресуют
правку. Ввод всегда mm, независимо от `--length-unit` source. Сохраняются DocumentId,
ObjectId, IDs сегментов, refs и остальные данные; это версия той же модели, в отличие
от независимой модели `create`/New. Новый filename не означает новые UUID.

Source не перезаписывается и не мигрируется, output никогда не заменяется; `--force`
и in-place Save отсутствуют. Файл публикуется после одной транзакции, валидации,
cold rebuild и проверки ранее разрешимых refs. Stale source отказывает с предложением
reopen; при работе от результата прежнего inspect используйте `--expect-version`.
Версия учитывает также implicit rowid; после смены алгоритма content version
получите hash новым inspect. Если дополнительная таблица затеняет все три rowid
alias, полное чтение версии отказывает с её именем.
`inspect` теперь также read-only: старую схему/WAL отказывает. Старое описание записи
через `Document::open` выше по-прежнему относится к validate/dump-graph/clear-cache.

Текстовый CLI: успех — exit 0 и `saved …`; отказ — exit 2 с `error [kind]` на stderr.
JSON v1: успех/отказ выполнения — структурированный stdout, диагностика — stderr.
Clap usage/help остаются текстом. Потеря отчёта после публикации — exit 7 с сохранённой копией.
Неверный UUID/значение/stale/busy — input; другая фича, Symmetric/ThroughAll,
формула/Parameter dependency/future capabilities/stored SQL trigger/нестандартное mutating FK action — unsupported; kernel/topology
refusal остаётся типизированным. Отказ/отмена до publish не оставляет файла и scratch;
после publish поздняя отмена не удаляет файл и не выдаёт публикацию за отказ.
Сборка без OCCT собирается и честно отказывает операции, требующей ядра.

UI доступен по принятой сцене; форма и worker блокируют Open/New/Export. Empty/imported-only
объясняют отсутствие native extrusion. После publish Saved показывает output отдельно
от результата Open; при ошибке Open старая сцена/export source/title сохраняются.
Полный повторяемый [CLI-рецепт, контракт и проверки §24C](edit-extrude-copy-verification.md).

## §24D: JSON v1 для inspect и edit-extrude

[Схема, ошибки, ограничения путей и запускаемый рецепт агента](cli-json-v1.md)
позволяют получить UUID и непрозрачную версию из JSON, выбрать ровно нужную
поддерживаемую Extrude, изменить её высоту и проверить копию повторным inspect.
Каталог различает общий copy_access и локальные отказы; успешное чтение не обещает
геометрический успех. Правка использует ту же jobs-операцию, что текстовый CLI/UI.
Нативный process gate включён в combined runtime layout на трёх ОС вместе с восемью
существующими edit gates. Первичная сдача и независимое ревью — в [протоколе](cli-json-v1-verification.md).
