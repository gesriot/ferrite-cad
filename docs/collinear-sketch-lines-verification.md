# §31G — коллинеарность двух сохранённых Lines: verification

Исходный локальный отчёт ниже относится к передаче незакоммиченного среза.
Тогда CI diff и окно не запускались. Результаты независимого ревью и настоящего
окна добавлены отдельно в конце. Контракт и рецепт:
[collinear-sketch-lines.md](collinear-sketch-lines.md).

## Точная база

* `bdf25e535cdb66d5ece09912c10ecc6bc3309702` — merge PR #109 (§31F), `main` = `origin/main`
  на начало и на конец работы (`git fetch`), ветка `collinear-sketch-lines` от него; чужой
  worktree `a200` не тронут.
* CI точной базы (не CI ветки): `CI` 38581358049, `planegcs pin` 38581358051, `rust notices`
  38581357837, `rust sbom` 38581357978, `product sbom` 38581357848, `combined runtime layout`
  38581358105 — все **success** (проверено до работы и после).
* Дерево на старте чистое. Весь diff оставлен unstaged/uncommitted; индекс не тронут
  (`git diff --cached` пуст).

## Изменённые и новые файлы

Modified (32):

* `.github/workflows/ci.yml`
* `.github/workflows/planegcs-pin.yml`
* `.github/workflows/runtime-layout.yml`
* `README.md`
* `crates/ferritecad-app/src/constraints.rs`
* `crates/ferritecad-app/src/constraints/tests/angle.rs`
* `crates/ferritecad-app/src/main.rs`
* `crates/ferritecad-app/src/sessions.rs`
* `crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs`
* `crates/ferritecad-cli/src/edit_constraints.rs`
* `crates/ferritecad-cli/src/json/constraints.rs`
* `crates/ferritecad-document/src/chamfer.rs`
* `crates/ferritecad-document/src/document.rs`
* `crates/ferritecad-document/src/lib.rs`
* `crates/ferritecad-document/src/model.rs`
* `crates/ferritecad-document/src/schema.rs`
* `crates/ferritecad-document/src/sketch_constraints.rs`
* `crates/ferritecad-document/tests/sketch_constraints.rs`
* `crates/ferritecad-eval/src/solve.rs`
* `crates/ferritecad-sketch-solver/planegcs-bridge/planegcs_shim.cpp`
* `crates/ferritecad-sketch-solver/planegcs-bridge/planegcs_shim.h`
* `crates/ferritecad-sketch-solver/src/contract.rs`
* `crates/ferritecad-sketch-solver/src/error.rs`
* `crates/ferritecad-sketch-solver/src/lib.rs`
* `crates/ferritecad-sketch-solver/src/planegcs.rs`
* `crates/ferritecad-sketch-solver/src/prepared.rs`
* `crates/ferritecad-sketch-solver/src/residual.rs`
* `crates/ferritecad-sketch-solver/tests/boundary.rs`
* `docs/cli-capabilities.md`
* `docs/cli-json-v1.md`
* `docs/implementation-plan.md`
* `tools/check-fbx-complex.sh`

Untracked (6, включая этот файл):

* `crates/ferritecad-app/src/constraints/tests/collinear.rs` (777 строк)
* `crates/ferritecad-cli/tests/collinear.rs` (1433 строк)
* `crates/ferritecad-document/tests/collinear.rs` (429 строк)
* `crates/ferritecad-sketch-solver/tests/collinear.rs` (507 строк)
* `docs/collinear-sketch-lines-verification.md` (158 строк)
* `docs/collinear-sketch-lines.md` (607 строк)

`git diff --stat` (tracked): 32 files changed, 1242 insertions(+), 67 deletions(-). Логи, скрипты запусков, замороженный
baseline и FBX — в игнорируемом `target/31g-check/`; модели окна нет.

## Что и как исполнено

Среда: macOS arm64, `jobs=1`, последовательные сборки, существующие targets (native — `target/`,
true stub — прежний `/private/tmp/ferrite-25j-stub-target`); перед запусками давление 1, swap 0,
диск 145 GiB свободно. Сборки не перестраивали OCCT/Boost/PlaneGCS (их входы не менялись).

### native (OCCT + PlaneGCS 1.0.1; `otool -L`: 50 `libTK`, 1 `planegcs`)

| Что | Результат |
| --- | --- |
| solver-пакет (`--features planegcs`) | 7+9+15+7+9+14 = 61 passed (в т.ч. `tests/collinear.rs` 9, `boundary` +2, residual unit, shim) |
| eval, scene, solver-lab, document, topology, kernel, types, jobs, export | 0 failed; document 160+9+4+9+4+32+29+30+10, eval lib 91 (+1), jobs 126 |
| CLI-наборы (29 файлов, без трёх `*_complex` и двух GPU-pixel) | 0 failed; `collinear` 9 passed (1 skip формы mixed) |
| итого вне viewer | 1512 passed, 0 failed (CLI `collinear` учтён в итоговой версии, 9 тестов) |
| viewer целиком, под watchdog 1536 MiB | **720 passed, 2 ignored** (было 714); пик 291.3 MiB, давление 1, swap 0; 23 строки `skipped:` (тесты других форм) |
| реальный блок `ci.yml` (15 ворот §31G, mixed-сборка) | все ворота: `1 passed`, маркер на месте, 0 skip |
| реальный шаг `planegcs-pin.yml` на release-прогоне solver-пакета и solver-lab | `every planegcs gate ran…`; без имени ворот / маркера шаг падает (контроль) |
| реальный шаг `runtime-layout.yml` «Bring two saved Lines…» (release) | 6 CLI-ворот, 2 ворота окна (worker↔CLI, session), writer guard, рецепт → `FCAD_31G_RECIPE_OK offset_before=2.000 collinear=0.00010 parallel=2.000 volumes=7867.85/7900.0 conflict_reasons=10 redundant=1` |
| pinned ufbx 0.23.0 | `collinear-u`, `collinear-z`, `collinear-ui`: `checks=6 failures=0`; усечённая копия `failures=2`; `FCAD_COLLINEAR_UFBX_EXECUTED` |
| прежние angle/parallel/equal/circle и session gates | в составе наборов выше (CLI `directed_angle`, `circle_constraints`, `edit_constraints`; viewer `constraints::tests`, `sessions::tests`) |

### true stub (нет OCCT, нет solver; build warning `Open CASCADE was not usable`; 0 `libTK`, 0 planegcs, без `DYLD`)

| Что | Результат |
| --- | --- |
| реальный блок `ci.yml` (15 ворот §31G) | все `1 passed`, маркеры, 0 skip; ворота окна — под watchdog |
| рецепт | `FCAD_31G_RECIPE_NO_KERNEL` |
| наборы (solver, document, eval, jobs, scene, export, topology, kernel, types, solver-lab + CLI) | 1492 passed до правки теста; единственный FAIL — первая версия `a_collinear_request_is_strict…`: два структурных случая («одна Line дважды», чужая Line) в stub упираются в «no Open CASCADE», потому что команда читает источник раньше. Они перенесены в kernel-gated `the_pair_and_the_lines_of_a_request…`; stub-прогон `tests/collinear.rs` после правки: 9 ok, из них исполнено 2 (strict, discovery), **7 skips** |
| viewer целиком, под watchdog | **720 passed, 2 ignored**; пик 34.4 MiB; 210 строк `skipped:` (ядро/solver отсутствуют) |

### mixed (OCCT без solver, `--no-default-features`; 50 `libTK`, 0 planegcs)

| Что | Результат |
| --- | --- |
| реальный блок `ci.yml` | 15 ворот ок |
| реальный шаг `runtime-layout.yml` «Refuse a collinearity…» (release) | `FCAD_31G_NOSOLVER_EXECUTED`, `FCAD_31G_CLI_REQUEST_SLOT_EXECUTED`, `FCAD_31G_RECIPE_NO_SOLVER` |
| `tests/collinear.rs` | 9 ok: исполнено 4 (strict, discovery, request-slot, no-solver), **5 skips** (native-only). Constrained маршрут отказывает без публикации (`export-stl` и правка → `unsupported`), обычный unconstrained остаётся рабочим |

### Статические проверки

`cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`
(на финальном дереве), `check-licence-headers.sh` (499 файлов), `check-solver-ownership.sh`,
`check-planegcs-pins.sh`, `check-export-boundary.sh`, `actionlint`, `shellcheck`,
`git diff --check` — чисто. `tools/check-actions-run-size.py`: **210 блоков, максимум 20432 B
(не изменён), packed argv максимум 20463 B при пределе 21000**; блоки §31G: `ci.yml` 9562 B
(расширенный шаг), `planegcs-pin.yml` 6397 B, новые шаги `runtime-layout.yml` 2104 + 3464 B. Старые gates не удалены; единственная правка старого теста по смыслу —
ширина строки §31F в `constraints::tests::angle` (см. контракт) и список «неизвестных видов»
в старом shim-тесте.

## Мутации

Две требуемые и две дополнительные: каждая внесена, прогнана против ворот, обязанных её
поймать (`compiled && rc != 0 && "test result: FAILED" && "panicked at"`), восстановлена
**побайтово** (SHA-256 совпал), затем те же ворота зелёные. Скрипт — прежний из ignored
`target/`, в репозитории инфраструктуры нет; ворота окна — под watchdog.

| Мутация | Падают |
| --- | --- |
| смещение потеряно: evaluator переводит Collinear в один `Parallel` | eval-перевод (`left: Parallel…`), CLI U-тест (`15` вместо `14`), worker окна (`Some(Some(15))` вместо `14`) |
| capability потеряна (`SUPPORTED_CAPABILITIES`) | документ read-only (`the new capability is supported`), CLI replace/remove (`unsupported: document cannot be edited`) |
| обход writer (`rederive` без `managed`) | оба forged-plan теста |
| shim кодирует вид 10 как Parallel | 7 из 9 solver-тестов; `expected a solution, got DidNotConverge { worst_residual: Some(5.0) }` |

## Старая сборка

Замороженный CLI чистого `bdf25e5` (`target/31g-check/baseline/ferritecad-main-bdf25e5`,
SHA-256 `a781ea805a8d31ea…`): файл с Collinear (payload v5) остаётся побайтово целым (хэш и
mtime), `inspect` перечисляет ноль Sketch, правка и экспорт отказывают
(`requires sketch.constraints.collinear.v1, which this build does not implement`, «preserves
but cannot rebuild»), ни Parallel, ни Perpendicular не появляется, выходных файлов нет;
новая сборка читает файл. `FCAD_31G_BASELINE_REFUSAL_OK`.

## Ресурсы и отклонения

* Каждый запуск viewer/widget — через `tools/watch-viewer-memory.py`, 1536 MiB, ограниченное
  время; watchdog не срабатывал. Пики: native 291.3 MiB, stub 34.4 MiB; давление 1, swap 0.
* **Отклонение:** единственный временный пробный widget-тест (измерение ширины строк, затем
  удалён) один раз запущен прямым `cargo test`, до того как я применил watchdog; дальше все
  запуски идут через него. Реальные строки workflow для ворот окна выполнены с тем же
  аргументным списком, но test-exe запускался под watchdog, а не прямым `cargo test`.
* Первый запуск native-наборов остановлен до конца (код менялся: невязка стала отвергать
  нечисло), собственные процессы завершены; итоговый прогон выше сделан на замороженном коде.
  Одна ошибка сопровождения: сборка mixed перезаписала общий `target/debug/ferritecad`
  без solver, и native-ворота worker↔CLI упали (30 тестов); после пересборки peer CLI тот же
  набор — 720 passed. Итоговые бинарники `target/{debug,release}/ferritecad` — native.
* GPU-наборы и окно не запускались; чужие процессы и каталоги не трогались; фоновых
  процессов не осталось.

## Не закрыто

Hover-опоры §31C/§31D, центр окружности, §30, Milestone 5C, продукт целиком и расследование
OOM; окно (сценарий в контракте) проверит ревьюер.

## Независимое ревью, 2026-10-14 — PR #110

Проверен код `586339f97f6ba11a968615b2f3e28641f7ae5c71` от точной базы
`bdf25e535cdb66d5ece09912c10ecc6bc3309702`. На базе отдельно подтверждены
27 успешных check runs. Исправлений кода по результатам ревью не потребовалось;
последующий коммит дополняет только документацию. Логи ревью лежат в игнорируемом
`target/31g-review/`, исходные и полученные оконные модели — вне checkout.

Повторено независимо на macOS arm64 с OCCT и PlaneGCS:

* solver — 61 passed; document `--test collinear` — 4 passed;
* CLI `--test collinear` — 8 исполненных тестов и один условный пропуск теста,
  предназначенного именно для сборки без solver (9 harness passes);
* под watchdog — 3 теста формы Collinear, 3 прежних теста DirectedAngle и
  `native_collinear_of_a_u_is_applied_like_the_command_line`: 7 passed, без skips;
* свежая сборка обоих клиентов и relocatable arm64 bundle, fmt, workspace clippy
  all-targets/all-features `-D warnings`, licence (503 файла), solver ownership,
  pins, export boundary, actionlint, shellcheck и whitespace — pass;
* Actions: 210 блоков, максимум 20432 B, packed argv 20463 B при пределе 21000.

Два незавершённых пробных запуска не засчитаны: первый запросил test executable
до окончания его сборки; второй, более широкий набор widgets, остановлен watchdog
из-за двухсекундного timeout системной команды `top`, не превышения памяти.
Затем перечисленные выше точечные наборы завершились под тем же watchdog.
Полные авторские матрицы и mixed/stub здесь не выдаются за независимый повтор.

### Настоящее окно и независимо проверенный результат

Свежий bundle открыл приватный U-профиль с разнесёнными параллельными ободьями.
В одном PID 68863 реально выполнены:

1. Pair A/B для сегментов 1/5, Add Collinear, переключение обоих направлений;
   направления не меняют связь. Draft Undo/Redo снимает и возвращает одно действие.
2. Add Parallel на занятой паре показывает отказ и сохраняет draft. Отмена
   системного Save возвращает ту же форму.
3. Apply создаёт unsaved-модель с DOF 14. Document Undo возвращает исходную
   геометрию и чистое состояние, Redo — связь и dirty-состояние.
4. STL и FBX экспортированы **до Save**, затем Save As записал `gui-applied.fcad`.
5. Удалён точный сохранённый UUID Collinear, добавлен Parallel той же пары;
   Save copy и async Open открыли `gui-replaced.fcad`, DOF 15.
6. После уменьшения окна примерно до 990×771 логических points все кнопки
   отношений и нижние Apply/Save доступны. EqualLength допускается отдельно от
   сохранённой Collinear. Переключение вкладок сохраняет её незавершённую форму
   только в исходной вкладке; Cancel закрывает только эту форму.

Ширина строки отношений (до 671 pt) принята после этого оконного наблюдения и
прежних widget gates доступности. Z-профиль и остальные структурные отказы
проверены native-тестами, но не выдаются за дополнительные оконные действия.

Из того же исходного `.fcad` независимый CLI создал Collinear-копию; из оконной
копии — точную замену Collinear на Parallel. Сравнение всех SQL-таблиц допускает
только новые constraint UUID в payload/hash и время модификации: фактически
различались только `objects.payload/payload_hash`; после нормализации новых UUID
payload совпал. Document UUID, curves, dependencies и refs сохранены. Validate и
cold rebuild оконных копий прошли, все сохранённые refs разрешены, исходные файлы
и оконные копии остались побайтово целыми после чтения и экспорта.

Несохранённые оконные STL/FBX совпали побайтово с экспортами CLI и повторно
открытой оконной копии. Независимый binary-STL parser проверил замкнутость через
направленные рёбра, восемь вершин основания и высоту 10 mm:

| Результат | Треугольники / байты STL | Объём mm³ | Четыре поперечных смещения |
| --- | --- | --- | --- |
| Collinear из окна | 28 / 1484 | 7859.4118748 | не более 2.7e-6 mm по модулю |
| Замена на Parallel | 28 / 1484 | 7900 | −2, −2, +2, +2 mm |
| Исходный профиль (контроль метрики) | 28 / 1484 | 7900 | −2, −2, +2, +2 mm |

Оконный FBX (7517 B) прочитан pinned ufbx: `checks=6 failures=0`.
Итог сравнения — `FCAD_31G_WINDOW_COMPARISON_OK`; скрипт и протокол сохранены
в ignored review-каталоге и приватном каталоге моделей.

Watchdog: лимит 1536 MiB, максимум выборки **190.376 MiB**, pressure 1, swap 0;
384.2 секунды, штатный exit 0, `aborted:false`. После Quit завершение проверено
по PID и журналу watchdog, без повторного обращения CUA к закрытому приложению.
Причина прежнего OOM этим не установлена.

### Границы вывода

Смещение и направление проверяются совместно; общая невязка с пределом 1e-6
требует пройти и безразмерному синусу, и расстояниям в mm. Ограничение ранжирования
зависимых уравнений по стартовому Якобиану закреплённой PlaneGCS остаётся явным:
правильная геометрия не даёт основания обещать точные DOF/redundancy во всех
зависимых системах. Новый собственный анализ транзитивности не добавлен.

Удалённый CI кода отслеживается отдельно в PR #110: CI 38638754599,
planegcs pin 38638694926 и combined runtime layout 38638694876.
CI и pin завершились успешно (7/7 и 4/4 jobs). По их логам независимо
пересчитаны 15 точных CI-gates, 10 solver-тестов pin и 9 маркеров pin §31G
на каждой из Linux/macOS/Windows. Runtime-матрица на момент записи этого
дополнения ещё выполняется; её итог проверяется перед слиянием, без подмены CI базы.
