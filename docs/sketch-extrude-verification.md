# §25A — проверка и передача незакоммиченного diff

Дата: 2026-09-13, macOS arm64. База после fetch:
`main = origin/main = 3c4ed9637cb26c50938fa47f0d6c841442be5429`.
PR #26 — MERGED; head `042ad5672618cd43680b3b7e3541752b08809236` и merge
имеют дерево `26304ac4c7d692c9984109237136105bd293fa04`.
27 checks и шесть workflow этой базы проверены через GitHub CLI: success.
Это **CI базы**, не будущего diff. Ветка `sketch-extrude-create`; staging,
commit/push/PR/merge не выполнялись. Worktree a200 не изменялся.

Контракт и выполняемый рецепт: [sketch-extrude-create.md](sketch-extrude-create.md).
Все локальные логи/артефакты: `/private/tmp/ferrite-25a/` (также `/tmp/ferrite-25a/`).

## Архитектура и факты

UI/CLI передают один `NewDocument::SketchExtrude(PolygonExtrusion)` в
`create_document_with_kernel`. Общий create writer/transaction/Temporary/Keep
сохраняет DatumPlane, Sketch с шестью Line UUID, Blind Extrude, Body, три deps
и три topology refs. Нового document schema, IR, C++/FFI, runtime dependency
или JSON остальных команд нет. Cargo.lock/inventories/notices/SBOM не менялись:
dependency edges и их входы не изменены; проверка native inventory прошла.

Результат JSON берётся из `CreatedDocument` после publish: destination/document_id.
Общий emitter даёт 0/2/7. UI использует существующие Creates generation, worker,
диалоговый адаптер §23D и последующий обычный async Open. Черновик и его до 128
состояний отдельны от принятой сцены; числовые поля ограничены 64 символами.

## Выполнено на локальном native build

`CARGO_BUILD_JOBS=1`; тесты `--test-threads=1`. Env проверен по реальной сборке:
`native-env.sh`, `native-cmake.txt`, `native-imports.txt`. Target
`/private/tmp/ferrite-24b-native-target`, vendor OCCT 8.0.1 и vendor planegcs.
Оба REQUIRE_NATIVE флага равны 1 (OCCT/PlaneGCS), GPU unset кроме отдельного
описанного ниже GPU gate. OCCT из исходников заново не собирался.

| Проверка | Результат / лог |
| --- | --- |
| fmt и workspace clippy all targets/all features, `-D warnings` | pass; `fmt.log`, `clippy.log` |
| Свежие CLI + app, all features | pass; `build-final.log` |
| document + eval suites | 312 passed, один existing ignored timing test; `document-eval.log` |
| jobs | 49 passed, 0 skips; `jobs-final.log` |
| create/edit/import/shared import/JSON import/JSON v1/validate/STL/FBX и ранняя версия нового process suite | 67 passed, 0 skips; `cli-regression.log` |
| Финальный новый CLI suite | 4 passed, 0 skips; `sketch-cli.log` |
| UI draft/widgets + native UI/CLI parity | 3 passed, 0 skips; `sketch-ui.log` |
| Прежние create workers | 17 passed; `creates.log` |
| Свежий peer CLI → 8 edit workers | 8 passed; `edits-final.log` |
| STL workers | 5 passed (включая 4 обязательных native); `stl-workers-final.log` |
| Dialog adapter | 3 passed; `dialogs.log` |
| Accepted scene / failure / stale answer boundaries | 4 exact named tests passed; `scene-boundaries.log` |
| Complete native/imported JSON FBX + refusals | 2 exact named tests passed; `fbx-complete.log`, `fbx-refusals.log` |
| Исправленный macOS memory guard | 5 passed; `memory-guard-tests.log` |

Не складывать строки таблицы: некоторые gates повторяются. Обычные семь
validation executions этой платформы исполнены (4 process + 1 jobs + 2 snapshot),
а их 21 трёхплатформенное исполнение сохранено в CI. Старые 54 required native
gates и strict ufbx campaign в runtime workflow не удалены/не ослаблены. Локально
исполнены 17 из 18 прежних named gates для macOS; тяжёлый
`the_complex_assembly_becomes_one_fbx_that_keeps_every_definition_and_says_what_is_missing`
и его partial/delivery ufbx campaign повторно не запускались: этот срез не меняет
STEP import или FBX writer, а новый L проверен отдельно. Полный тяжёлый corpus
не запускался. Linux/Windows CI незакоммиченного diff не выполнялся.

Новые gates добавлены в существующий combined runtime workflow для каждой ОС:

- `native_sketch_extrude_process_cold_geometry_and_delivery`
- `sketch::tests::native_sketch_draft_and_cli_publish_equivalent_models`

Оба исполняются с OCCT/PlaneGCS обязательными, exact name, `--test-threads=1`;
проверяются строки `test … ok` и отсутствие `skipped:`. Это не `--no-run`.
Локальный matcher проверен на фактических логах. В native pipe-тесте stderr
собирается и проверяется отдельно, чтобы диагностика не разрушала named marker.

## Что доказывают новые gates

Настоящий egui UI получает mouse/keyboard events: вход в редактор, шесть точек,
Close contour, Undo/Redo, числовая правка высоты, NaN в координате и отмена этой
правки, Submit. Видимые bounds canvas/текста берутся из результата layout; нет
подмены результата рисования готовым DTO. Затем исполняется штатный UI start_new
и worker. No-selection после диалога, занятое назначение и повторный submit
сохраняют draft, не стартуют лишний worker; успешный ответ вызывает обычный Open.
Отдельные старые gates доказывают сохранение accepted scene и отбрасывание stale
ответов. Это исполнение UI-пути без OS окна, не утверждение о ручном GUI smoke.

Peer CLI получает тот же L через public JSON request. Сравниваются полные
сохранённые payloads/deps/refs с последовательным сопоставлением независимых UUID;
ID независимых документов различаются. STL двух клиентов побайтово одинаковы.
CLI process suite независимо разбирает binary STL: 20 triangles, bounds
60×40×10, volume 16000 mm³, допуск 0.016 mm³ (1 ppm, координаты целые/float32).
Он удаляет только приватный request, делает два cold rebuild, сверяет segment/ref
identities и source bytes, проверяет complete JSON FBX и точный состав каталога
без scratch/cache/SQLite sidecars. Отдельный реальный CLI запуск clockwise L дал
те же bounds и volume (`clockwise.*`).

Отказы: версия/неизвестные поля, пересечение/повторы/коллинеарность/NaN/плохая
высота, missing request, busy output, source/hardlink/Unix symlink, не-UTF-8 OS
путь, help/usage/файл `--json`. Неподдерживаемая версия и ошибка ввода остаются
разными ErrorKind. Для плохого request/no-clobber состав каталога и bytes
сравниваются. Windows invalid UTF-16 case добавлен, локально на macOS не выполнен.

Реальные OS pipes через прежний `support/pipe.rs`: закрытый stdout после
публикации и оба закрытых канала дают 7; новый FCAD остаётся читаемым с четырьмя
objects. Отказ с закрытым stderr доставляет JSON через stdout. Оба закрытых
канала при отказе дают 7 без panic и без публикации. Перезаписи у новой операции
нет, поэтому force replacement не является её исходом.

Общий jobs gate использует borrowed MockKernel только для фазовых отказов и
счётчика ресурсов: pre-cancel, cancel после cold geometry, cancel после closed
scratch, build refusal, racing destination, late cancel и success. После каждого
исхода live_shape_count=0 **до destructor kernel**; каталог содержит только
ожидаемый destination или пуст. Прежние transaction/storage-refusal gates сохранены.

Две временные production-поломки исполнены отдельно и восстановлены:

1. `built.release_all(kernel)` → `drop(built)`: gate падает с live shapes = 1,
   «shapes must be freed before dropping the kernel» (`mutation-cleanup.log`).
2. Publish Keep → Replace: gate падает на «racer: Ok» вместо отказа
   (`mutation-publication.log`).

Оба падения — один реально исполненный тест, не compile failure/zero tests.
Восстановленный точный gate прошёл (`mutation-restored.log`), затем jobs suite
прошёл полностью. В процессе разработки исправлены обычные compile/clippy
замечания и assertion теста, который сначала не учитывал fade цвета egui окна;
эти промежуточные неуспехи не засчитываются как mutation evidence.

## Отдельная stub-сборка

Target `/private/tmp/ferrite-24j-stub-target`, invalid native config paths,
REQUIRE_OCCT/PLANEGCS=0, GPU и DYLD unset. Старый wrapper path из env уже отсутствует;
его наличие не предполагалось доказательством. Фактические `stub-cmake.txt`
и `stub-imports.txt`: `OpenCASCADE_DIR-NOTFOUND`, у CLI только libSystem/libiconv,
нет OCCT/PlaneGCS imports. Это rebuild изменённого кода в отдельном target.

`stub-final.log`: три содержательных новых process tests прошли; один native
geometry test напечатал явный skip (harness формально пишет 4 passed, **геометрия
не засчитана**). Исполнен valid polygon request → structured unsupported/exit 2,
тот же refusal с закрытым stderr и обоими каналами/exit 7, без публикации/scratch.
Четыре validation process tests — без skips; validation jobs gate и 9 snapshot
tests — pass. Два draft/widgets теста — pass без ядра (`stub-widgets-final.log`).

## Рецепт, упаковка и память

Python-блок извлечён из [Markdown](sketch-extrude-create.md) в `recipe.py` и
выполнен настоящим CLI. Результат `recipe.log`: создание 0.439 s, cold STL export
0.072 s, 20 triangles, volume 16000 mm³. SHA-256 созданного FCAD неизменён; private JSON
удалён перед validate/inspect/export. Существующий `read_fixture.c` собран с
pinned ufbx commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`, hashes проверены
штатным fetch helper. Strict reader: ASCII 7400, 0 warnings, 1 model/1 geometry,
счётчики совпадают с JSON (`recipe/L.fbx`). Unity не запускался.

Для будущего GUI подготовлен штатный bundle
`/private/tmp/ferrite-25a/gui-final/FerriteCAD.app`: runtime-closure → staging,
RPATH/подпись штатными tools. Приватные копии debug binaries обработаны `strip -S`:
первый package probe закономерно нашёл build paths в debug information
(`package.log`). После strip probe прошёл (`package-final.log`): без доступа к
vendor/build путям bundle загружает собственные libs, cold rebuild успешен,
пропавшие bundled libraries отказывают, source неизменён. Native inventory —
10 checks passed; исходные binaries/vendor не менялись ради упаковки.

Все GPU/измерительные эксперименты выполнялись только исправленным watchdog,
1536 MiB, по одному процессу; чужие процессы не завершались. Пять тестов самого
guard прошли. Первая попытка headless замера через системный `/usr/bin/time`
потеряла DYLD и завершилась loader error/134 (`headless-worker-memory.*`), **не pass**.
Повтор через явный env после time прошёл. Окно при этом не открывалось.

| Эксперимент | Факты |
| --- | --- |
| Финальные egui widgets → native worker → peer CLI | 0.34 s; time max RSS 37 093 376 bytes, peak footprint 5 947 944 bytes; watchdog exit 0 (`sketch-widgets-worker-memory.*`) |
| Существующий GPU `twenty_frames_of_one_document_read_nothing_and_upload_nothing` | 1 executed/pass, 0.86 s; time peak footprint 166 118 096 bytes (~158.42 MiB), sampled process-tree peak 162 530 216 bytes (~155.00 MiB), watchdog exit 0 (`gpu-frames-memory.*`) |

Pressure во всех samples = 1 (normal), swap used 4 099 211 264 bytes до/после
без прироста. Короткий headless worker быстрее периода sampler: его sampled
peak 0.75 MiB относится лишь к пойманному time wrapper и **не** является peak
worker/всех descendants; для worker приведены lifetime метрики time. GPU gate
проверяет прежний renderer/cache invariant, не новый интерактивный L-сеанс.
Ни один exit 125 не засчитан как pass. Причина пользовательского OOM не установлена.

**Интерактивный macOS GUI smoke не выполнен.** CUA inventory ответил:
`The Mac is locked and automatic unlock could not unlock it. Ask the user to unlock the Mac manually before continuing.`
Отправлен async запрос разблокировки; до ответа viewer как окно не запускался.
Linux/Windows GUI также не проверялся. Headless widgets/native worker и GPU gate
не выдаются за OS Save dialog smoke или замер памяти интерактивного редактора.

Процедура после разблокировки: запустить ровно один `gui-final` viewer через
`tools/watch-viewer-memory.py --limit-mib 1536`, создать L мышью/точными полями,
проверить Close/Undo/Redo/invalid input, Save Cancel и новый private FCAD;
сверить его через inspect/validate/STL с тем же рецептом. Затем открыть прежнюю
модель, начать/отменить черновик и убедиться, что сцена осталась; закрыть viewer,
проверить natural exit и peak/pressure/swap guard. При падении сначала проверить
PID, до нового CUA вызова; exit 125 означает остановленный эксперимент.

## Границы передачи

Все изменения незакоммичены и не staged. Полный список и diffstat, включая
untracked: `/private/tmp/ferrite-25a/diffstat.txt`. Новые runtime dependencies,
FFI, schema, UI STEP import, constraint editor, in-place Save, persistent undo,
holes/booleans/другая геометрия и следующий срез не добавлялись.

Следующий шаг — независимое ревью Codex. CI нового diff последует только после
ревью и публикации; интерактивная macOS проверка остаётся открытой из-за состояния
экрана и не объявлена пройденной.

## Независимое ревью и интерактивная проверка, 2026-09-13

Следующие результаты относятся к независимому ревью после первичной передачи
выше. Mac был доступен; незавершённый интерактивный smoke теперь выполнен.
Дефектов production-кода, требующих исправления, не найдено. Проверены общий
writer, фазовая отмена/закрытие SQLite, release handles перед выходом из cold
проверки, Keep publication, bounded input и сохранение идентификаторов.
Подозрение на выход линий за canvas не подтвердилось: закреплённый egui 0.36.1
уже пересекает clip rect с response rect в allocate_painter. Временная headless
проверка прошла; лишний дублирующий тест и изменение painter не включены в diff.

Независимый повтор native проверок, последовательно с CARGO_BUILD_JOBS=1 и
test-threads=1: fmt, workspace clippy all targets/features -D warnings;
document/eval/jobs/UI — 463 passed, один прежний timing test ignored; CLI —
63 passed; sketch widgets/worker — 3; create workers — 17; edit — 8; native STL
workers — 4. Native skips отсутствуют. Весь прогон под watchdog завершился
exit 0, aborted=false, 77.24 s, sampled peak группы 476.77 MiB, pressure=1.
Actionlint, export boundary, licence headers (320), diff whitespace — pass.

Отдельный stub target пересобран: новая CLI suite имеет три действительно
исполненных refusal tests и один явный geometry skip (harness пишет 4 passed).
Повторены unsupported kernel и closed-pipe отказы без публикации. У свежего CLI
imports только libSystem/libiconv; OCCT/PlaneGCS не подхватились из Homebrew.

Интерактивный smoke использовал production bundle gui-final из передачи:
Mach-O UUID E244E426-A1C4-3DD0-9CD8-1A8EDADB091A совпадает со свежим native
viewer; production sources в ревью не менялись. Единственный экспериментальный
PID 49004 запущен через исправленный watchdog, cap 1536 MiB. В окне выполнены:

- пустой старт → Create sketch + Extrude;
- шесть вершин L мышью, затем все точные координаты через поля mm;
- Close contour, Undo (контур снова открыт), Redo;
- ошибочный текст высоты → видимый отказ без Save, восстановление 10 mm;
- системный Save → Cancel: все точки/высота/замыкание сохранены;
- повторный Save → новый приватный Review L.fcad → принятая объёмная L-модель
  в Iso, правильный заголовок и доступные exports;
- новый непустой черновик при принятой модели → Cancel draft: модель и title
  сохранены; штатное закрытие окна.

Этот PID завершился exit 0, aborted=false, 342.83 s; sampled peak footprint
189.13 MiB, все samples pressure=1, swap не вырос. Причина прежнего OOM остаётся
неизвестной; это малый сценарий, не доказательство отсутствия утечки.
После закрытия запрос AX через CUA автоматически открыл ещё одно пустое окно.
Оно сразу закрыто и не засчитано как тест или измеренный сеанс; процессов viewer
после этого не осталось. При завершении будущих экспериментов не запрашивать AX
закрытого app: подтверждать выход по PID/журналу guard без повторного UI вызова.

Файл, созданный именно этим GUI, проверен отдельно: validate, inspect, два cold
rebuild, неизменный SHA-256 исходника, binary STL 20 triangles, bounds 60×40×10,
volume 16000 mm³ с прежним допуском 0.016 mm³. STL побайтово равен результату
публичного CLI-рецепта, заново извлечённого из Markdown и выполненного в новом
приватном каталоге. Создание в рецепте 0.063 s, cold STL export 0.068 s; это
единичные локальные наблюдения, не performance threshold. Оба FBX прочитаны заново
собранным reader из проверенных pinned ufbx sources: strict=true, ASCII 7400,
0 warnings, 1 model/1 geometry. Source bytes обоих FCAD не изменены.

Логи независимого ревью, GUI/CLI модели и JSONL: /tmp/ferrite-pr27-review/.
Полный тяжёлый STEP/partial-FBX corpus локально повторно не запускался; его gates
сохранены в CI. Новый runtime workflow должен подтвердить прежние 54 и шесть
новых native executions (по два на ОС). CI опубликованного head и merge будет
зафиксирован отдельно; результаты выше не являются CI этого diff.
