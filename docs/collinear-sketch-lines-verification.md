# §31G — коллинеарность двух сохранённых Lines: verification

Короткий отчёт о том, что исполнено локально. Контракт, рецепт и окно:
[collinear-sketch-lines.md](collinear-sketch-lines.md). **CI этого diff не запускался**;
Windows, Linux, окно приложения, CUA, Unity, GPU-наборы и большой STEP/FBX корпус не
исполнялись (N/A).

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
