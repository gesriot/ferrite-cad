# §24C: изменение существующего Blind Extrude в новой копии

## Контракт

`ferritecad edit-extrude <source.fcad> --feature <ObjectId> --distance-mm <n> -o <new.fcad>`
и `Edit extrusion…` вызывают `ferritecad_jobs::edit_extrude_copy`. Операция принимает
source, DocumentId/полную версию содержимого, feature UUID, расстояние в mm и output.
CLI читает версию непосредственно перед запросом; `--expect-version` дополнительно
закрепляет версию ранее выполненного публичного `inspect`. UI получает эту же версию
в `LoadedScene` из того же чтения, которое построило принятую сцену.

Результат — сохранённая изменённая версия **той же модели**. DocumentId, ObjectId,
ID сегментов, stable refs, имена, parent/dependency links, порядок, все остальные
параметры и импортированные BLOB сохраняются. Это отличается от `create`/New:
новая независимая модель там получает новые идентификаторы. Здесь изменяются только
выбранное расстояние и его `Expression`, payload hash и `meta.modified_at`;
`Document::write` штатно обновляет производный индекс capabilities. SQL page layout,
change counter и другие SQLite служебные байты не являются моделью. В сравнении
UI/CLI допускается только различное `meta.modified_at`, UUID не нормализуются.

Поддерживается существующий `Extrude` с `EndCondition::Blind` и числовым literal.
`Expression` сегодня — source text + последнее нормализованное значение, без AST,
признака literal и evaluator выражений. Разрешается только `source.trim().parse::<f64>()`,
который конечен и совпадает с `value()`. Формула, имя параметра, несовпадающие text/value
и Parameter dependency выбранной фичи дают `Unsupported`. Также отказывают
Symmetric/ThroughAll и другой тип объекта. Неверный/отсутствующий UUID и неположительное
или не-конечное новое значение дают `Input` (ошибки синтаксиса UUID ловит clap).
Display length unit исходника не масштабирует ввод mm. `reversed`, `operation`,
`target_body`, `profile` сохраняются; границы существующего evaluator остаются в силе.

## SQLite, версия и публикация

`Document::open_read_only` теперь закрепляет SQLite read transaction до закрытия.
Все чтения одного документа — metadata, objects, dependencies, refs и source BLOB —
видят согласованное состояние. WAL и старая схема по-прежнему отказывают без миграции
или создания source sidecars. `inspect` также перешёл на этот read-only путь.

`Document::snapshot_to` использует SQLite online backup API из закреплённого чтения
в приватный `Temporary` рядом с output. Обычного копирования живой базы и сериализации
только известных объектов нет. Backup сохраняет неизвестные tables/envelopes и все
страницы SQLite. Capability access проверяется до копирования и остаётся тем же
после открытия копии. Read-only connection и совместимость формата — разные факты:
просмотр совместимого документа сам по себе не запрещает создание его изменённой копии.
Будущие required capabilities не становятся поддерживаемыми от копирования.

Content version — BLAKE3 по полной SQL-схеме и всем типизированным ячейкам всех таблиц
в определённом порядке, включая неизвестные данные, BLOB, UUID и timestamps. Mtime,
размер, имена путей, страничное размещение и UUID нового документа не используются.
Версия проверяется перед копированием и повторным открытием source path непосредственно
перед publish. Последнее read connection удерживается до публикации. Изменение
данных или замена source после показа формы приводит к отказу с предложением reopen.
Изменение, случившееся после последней согласованной проверки, относится к следующей
версии source; опубликованная копия всегда построена из подтверждённого снимка.

В scratch выполняются baseline cold rebuild, одна `Document::write`, валидация и
cold rebuild изменённого документа. Ядро создаёт и уничтожает клиент в своём worker;
jobs не зависит от OCCT. Все shape handles освобождаются при успехе и ошибке.
Для каждого фактического stored ref проверяется: разрешимый до изменения не может
стать неразрешимым после него. Уже неразрешимый ref не превращает произвольный документ
в требование «3/3»; ошибки rebuild/валидации/solver всегда блокируют publish. Для sample
plate проверка действительно даёт 4 объекта, 1 shape, 3/3 refs. ImportedStep сохраняется
как stored geometry; cold evaluator не пересчитывает импорт. Неизвестный **объект**,
который существующий evaluator не умеет перестраивать, даёт атомарный отказ; backup
не теряет его envelope. Неизвестная дополнительная таблица сохраняется и при успешной
правке. Новых правил интерпретации неизвестной геометрии этот срез не вводит.

До publish scratch-документ закрывается. `Temporary::publish(Existing::Keep)` —
атомарный hard-link no-clobber. Нет `--force`, in-place Save или замены назначения.
Source/output aliases проверяет `same-file` (включая hard links), затем проверяется
занятость и повторно — source identity перед publish. Файл, появившийся в гонке,
остаётся прежним. Ошибка/отмена до publish удаляет собственный scratch и SQLite
sidecars; чужой `.fcad-cache` не удаляется. После publish поздняя отмена сохраняет
успех и файл; UI сообщает Saved и не начинает нежелательный Open.

## Окно

Источник формы — `LiveScene.document` и `edit_source`, принятые вместе с картинкой,
а не последний запрошенный `App.document`. Выбор UUID явный даже для одной фичи;
форма показывает имя, UUID, текущую высоту и mm. Пустой/imported-only документ
объясняет отсутствие native extrusions; неподдерживаемые фичи объясняют отказ.

Edit доступен после завершения Open/New/Export и подтверждения замены экспорта.
При форме/worker edit Open/New/Export недоступны; камера остаётся доступной.
Cancel формы и Save ничего не запускают; отменённый Save сохраняет введённое число.
Shutdown отменяет и join-ит edit worker. Несвоевременный ответ не принимается.

Saved output и результат Open — два факта. После publish используется обычный
асинхронный Open; только успешный `prepare_load`/`commit_scene` меняет сцену,
export source, edit version и заголовок. Ошибка Open сохраняет прежнюю сцену и экспорт,
при этом Saved продолжает показывать путь существующего output.

## Воспроизведение через публичный CLI

Нужен binary с OCCT. Команды используют миллиметры даже при display unit `in`:

```sh
ferritecad create 'source plate.fcad' --sample --size 80 50 12 --length-unit in
ferritecad inspect 'source plate.fcad'
# В objects найдите строку `UUID feature.extrude Extrude1`.
# Для этой созданной команды ровно одна фича; в произвольной модели выберите нужный UUID явно.
feature=$(ferritecad inspect 'source plate.fcad' | awk '$2 == "feature.extrude" {print $1}')
version=$(ferritecad inspect 'source plate.fcad' | awk '$1 == "content" && $2 == "version" {print $3}')
ferritecad edit-extrude 'source plate.fcad' --feature "$feature" --distance-mm 27 \
  --expect-version "$version" -o 'edited plate.fcad'
ferritecad inspect 'edited plate.fcad'
ferritecad validate 'edited plate.fcad'
ferritecad rebuild 'edited plate.fcad' --cold
ferritecad export-stl 'edited plate.fcad' -o 'edited plate.stl'
ferritecad export-fbx 'edited plate.fcad' -o 'edited plate.fbx'
```

В UI: Open source → Edit extrusion → выбрать строку Extrude1 → 27 mm →
Save new file → другое `.fcad` → дождаться нового заголовка → Export FBX.
STL в окне по-прежнему не предлагается; STL двух сохранённых результатов проверяется
через существующий CLI writer path, а FBX одного результата — обоими клиентами.

## Evidence и границы измерения

База после fetch: `93d2f78a6757194802138f95b0b9c9c70fbd7218`, чистый worktree;
содержит §24B `eeb26dc3cbe5304932d8c8b7ef6e6f7726ad6b19` и §23C
`1abbebac710d9bb62b557ce00bed4d68850bd146`.
PR #15 merged в этот SHA. Проверены 15/15 checks head PR #15 и 27/27 checks merge:
[CI](https://github.com/gesriot/ferrite-cad/actions/runs/34144429790),
[runtime layout](https://github.com/gesriot/ferrite-cad/actions/runs/34144429729),
[planegcs](https://github.com/gesriot/ferrite-cad/actions/runs/34144429684),
[product SBOM](https://github.com/gesriot/ferrite-cad/actions/runs/34144429687),
[rust SBOM](https://github.com/gesriot/ferrite-cad/actions/runs/34144429709),
[rust notices](https://github.com/gesriot/ferrite-cad/actions/runs/34144429909).
Это evidence базы, не нового head; CI точного head записывается в PR.

Native tests UI/CLI используют один source, созданный реальным CLI, и UUID из stdout
`inspect`. Полное сравнение всех SQL-ячеек UI/CLI исключает только meta.modified_at.
Отдельно сравниваются source/output identity, unselected payload bytes, parent links,
deps и refs. Две модели: 80×50×12 и 91×53×17 → высота 27. Native cold rebuild и
reopen исполняются; из STL треугольников независимо вычисляются X/Y/Z и объём до/после.
STL/FBX двух копий равны побайтово; UI/CLI FBX одного output равны побайтово.
Смешанный документ дополнительно содержит импортированный реальным OCCT STEP,
проверяются полный BLOB, source claims, payloads и неизвестная дополнительная таблица.

Негативные контроли проверяют тип отказа, состояние файлов и отсутствие живых shape
handles. Детерминированные progress/channel barriers проверяют source change,
занятое/racing назначение, cancel до/после publish и несвоевременный ответ.
Подмена kernel boundary отдельно отказывает только изменённой геометрии и отдельно
теряет end-cap ref; ни один случай не публикуется. Такие инъекции — контроль
обработки отказа, не свидетельство исполнения OCCT; OCCT подтверждён отдельными
native parity tests и GUI.

Четыре временные мутации production jobs кода действительно скомпилированы и
запущены по одной: расстояние осталось 13 вместо 27; изменена первая другая фича;
записан новый UUID; проигнорирован отказ изменённого rebuild. Каждая запустила один
тест и упала на проверке поведения, а не компиляции. После восстановления все восемь
edit jobs tests прошли. Логи — `/private/tmp/ferrite-24c-evidence/mutant-*.log`.

Локальный `cargo test --workspace --all-features` с обязательными OCCT, PlaneGCS и
GPU: 1707 passed, 0 failed, 1 ignored в 93 test/doc suites. Единственный ignored —
существующий `regenerate_the_committed_manifest`, намеренно переписывающий fixture
manifest; геометрические проверки не пропущены. Запуск выполнен вне sandbox для
доступа к реальному Metal: первый запуск внутри sandbox отказал на недоступном
адаптере и не считается успешным. После него добавлены ещё три проверки:
parameter/future-capability refusal, cleanup собственных sidecars и shutdown/join;
восемь jobs tests и отдельный native shutdown test прошли на конечном коде.
No-OCCT build обоих клиентов и 11 CLI process tests также прошли, включая реальный
отказ edit при отсутствующем OCCT без нового файла.

Успешны workspace build всех targets/features, fmt, clippy с `-D warnings`,
export boundary, solver ownership, licence headers и actionlint. Изменённые runtime
dependencies отражены штатными генераторами: Rust SBOM (25 checks), native inventory
(58), product SBOM (40), Rust notices (68); все проверки прошли.
Полный native log — `/private/tmp/ferrite-24c-native-tests-unsandboxed.log`.

Два устранённых пробела общего файлового слоя: canonical-path сравнение ранее
не распознавало hard-link aliases; отдельные чтения read-only connection не держали
один SQLite snapshot. Теперь это проверяется реальными hard links и backup при
конкурирующей незакоммиченной записи, а не временем/размером файла.

Реальный macOS GUI smoke 2026-09-07: свежий debug executable staged штатным
`stage-runtime-layout.sh` в `/private/tmp/ferrite-24c-gui/FerriteCAD.app`;
Mach-O UUID viewer — `FB148CD6-FF3E-3987-80DA-7C685CF7EBBB`, SHA-256 —
`ae906967421690b5fcd9e18522ecde185920ae1ffbd6b2d36b346c2de1683092`.
Bundled CLI SHA-256 — `43fdf8919bd658978ccb72a0efb13f867315e550d16fac266dde2012e42bff5d`.
После staging менялись только комментарии production кода, дополнительные тесты, docs и workflows;
модели в `/private/tmp/ferrite-24c-evidence/Модели с пробелами`, вне checkout.
Через native Open принят `Плита 80×50×12.fcad`, через форму выбран Extrude1 с UUID
`01a07cdf-cb6d-7571-8e6b-c41023818d71`, введено 27, Save создал
`Изменённая плита 27.fcad` (macOS нормализовал имя в NFD). Наблюдались увеличенная
модель, новый заголовок и isometric view. GUI Export FBX — 4103 байта, побайтово
совпали с CLI из этого же bundle. `validate`: 0 warnings; cold: 4/1/3-of-3.

SHA-256 source: `02bf32540f38ab365af727d7b02c9db41fd94c28edcf2cf1048a40a03c9107b3`;
GUI output: `c32c98bdbd4ae3d542a8d1beefd038cd0b0dd24ea80f985ee7a3fe7c79df217a`;
FBX: `3e2e0adbae0d4d65b5452c9ba3b81efd7e895ad2c949e1cc997abd4aa4a2a2f7`.
Отмена формы и системного Save наблюдались. Проверка Replace исходной плиты была
отклонена автоматическим approval review и отменена. Более безопасная проверка
занятого назначения выполнена на отдельном созданном нами disposable sentinel:
системный Replace → приложение отказало `output already exists`, его bytes, старые
source/output hashes и заголовок сохранились; повторный GUI export побайтово совпал
с прежним. Scratch не остался. Это реальная проверка диалогов, не тест форматтера.

Windows/Linux GUI вручную не наблюдались. Все шесть native edit tests обязательно
исполняются в обычном combined runtime layout workflow на трёх платформах, с обоими
нативными компонентами и release-клиентами; имена каждого теста проверяются в логе,
skip и `--no-run` не считаются прохождением. Ручной OCCT pin также содержит три
ключевых edit gates. Его дополнительный запуск на первом коммите PR был остановлен
во время сборки OCCT из исходников после переноса обязательной проверки в обычный
workflow; он не является успешным evidence. Точный head и ссылки CI записаны в PR.
Новых release/install/association гарантий, in-place Save, dirty state, sketch/parameter
editor, STEP UI, batch/JSON/RPC/DSL, assemblies или drawings этот PR не добавляет.
