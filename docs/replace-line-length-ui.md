# §25F-1 — UI-замена сохранённой длины Line

2026-09-14. База `86994d48bbbec4be2ce3d1c48cf8fac96e81b935` = PR #33 MERGED =
HEAD = main = origin/main. После fetch дерево было чистым. Ветка
`replace-line-length-ui` (без префикса `codex/`). GitHub metadata точной базы:
**27/27 checks, 0 skipped, 6/6 workflow success** —
[runtime](https://github.com/gesriot/ferrite-cad/actions/runs/34809550174),
[CI](https://github.com/gesriot/ferrite-cad/actions/runs/34809550170).
Это CI базы; удалённый CI незакоммиченного diff не запускался.
Чужой a200 (`5b20719145222befc0bd25647f47ba3d67fee66d`, detached) не трогался.

## Изменение

`Edit constraints`: рядом с полем mm действие `Replace length` для выбранного
Line и его сохранённого Distance (включая обратную ориентацию endpoints).
Показывается `Stored length N mm · UUID`. Без выбора или без сохранённой длины
кнопка недоступна; `Add length` остаётся. Один Apply собирает прежний exact
remove UUID + add Distance и один `History::change`. Повторное значение
обновляет pending запись на месте; возврат к stored длине снимает только эту
замену. Пустой draft после отмены замены не отправляется в jobs. Поле и
selection вне истории. Solver на ввод/Apply/Undo/redraw не запускается.
[Контракт](sketch-constraints-copy.md#25f-1-замена-сохранённой-длины).

CLI/request/response/schema, jobs, document policy/writer, native/FFI,
dependencies и `Cargo.lock` не менялись. Новый constraint UUID по-прежнему
выдаёт общая операция при публикации.

## Состав diff

Индекс пуст. HEAD остаётся точной базой выше.

| Файл | + | − | Статус |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 1 | 1 | modified |
| `crates/ferritecad-app/src/constraints.rs` | 675 | 5 | modified |
| `docs/cli-capabilities.md` | 1 | 1 | modified |
| `docs/implementation-plan.md` | 10 | 5 | modified |
| `docs/line-length-constraints.md` | 2 | 0 | modified |
| `docs/sketch-constraints-copy.md` | 21 | 2 | modified |
| `docs/replace-line-length-ui.md` | (этот файл) | 0 | new |

`git diff --stat` tracked: **6 files, 710 insertions, 9 deletions**. Untracked
протокол учитывается отдельно. `Cargo.lock` не изменён.

Обычный CI exact-name gate дополнен
`constraints::tests::replace_line_length_widgets_build_one_remove_add_request`.
Имена native runtime gates не добавлялись: прежние **90** native executions
сохраняются; worker длины расширен тем же именем.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`.
Env `/private/tmp/ferrite-25f/native-env.sh`, target
`/private/tmp/ferrite-24b-native-target`. Vendor OCCT 8.0.1 (`libTKernel.8.0.1.dylib`)
и PlaneGCS; OCCT из исходников не пересобирался. Peer CLI собран до worker.
Логи: `/private/tmp/ferrite-25f1/`.

| Проверка | Результат |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| workspace clippy all-targets/all-features `-D warnings` | pass |
| `constraints::tests::` | **9 passed**, 0 skip: 7 widget/history + 2 native workers |
| exact `--nocapture`: `replace_line_length_widgets_build_one_remove_add_request` | буквальная строка `test constraints::tests::replace_line_length_widgets_build_one_remove_add_request ... ok` |
| exact `--nocapture`: `native_line_length_worker_and_cli_preserve_model_and_solved_body` | буквальная строка `test constraints::tests::native_line_length_worker_and_cli_preserve_model_and_solved_body ... ok`; `skipped:` нет |
| `sketch::` | **21 passed** |
| `edits::tests::` | **8 passed** |
| `exports::stl::tests::native_` | **4 passed** |
| headless watchdog `tools/test-viewer-memory-guard.py` | **5 passed** |

Виджет-тест кликами: stored 60 → Apply 55 (exact remove+add, один Undo/Redo);
55 → 50 без дублей; повтор 50 no-op; отказ/running сохраняют Redo; возврат к 60
снимает только эту замену и оставляет pending H/удаление/другую длину; пустой
draft после отмены не показывает Save; Save Cancel/stale/ошибка сохраняют
восстановленный request; reversed Distance endpoints остаются выбираемыми;
`Replace length` виден в длинном каталоге.

## Request, identity и геометрия

Native worker после публикации 60×30 через прежний Add length собирает Replace
60→55 теми же виджетами и шлёт тот же job. Peer CLI `remove` exact UUID +
`add` distance 55 по тому же source. Остальные constraint UUID, closure, stored
curves, SQL cells/rowids и source claims совпадают; явно сопоставляется только
новый UUID заменённой длины. Source bytes 60×30 копии неизменны.

Независимый STL (12 triangles, 684 bytes): extents **55×30×10 mm**, объём
**16500 mm³**. UI и CLI STL/FBX побайтово равны. Pinned ufbx 0.23.0
`read_production --identity`: **6 checks, 0 failures** на `ui-replaced.fbx` и
`cli-replaced.fbx`. Артефакты `/private/tmp/ferrite-25f1/native-artifacts/`.
Числовое поле DTO не выдавалось за измерение.

Целевого failing-first не потребовалось: дефекта в собственной реализации при
исполнении тестов не нашлось; отдельная mutation-инфраструктура не создавалась.

## Stub

Свежий target `/private/tmp/ferrite-25f1-stub3-target`. Homebrew cmake иначе
находит OCCT 7.9; для NOTFOUND cmake вызван с
`-DCMAKE_IGNORE_PREFIX_PATH=/opt/homebrew`.
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`.
`cargo warning=Open CASCADE was not usable`. Исполненный test binary:
`otool` без TKernel/PlaneGCS.

**7 исполненных** widget/history tests, включая Replace. **2 явных native skip**
worker'ов (`skipped: constraint worker requires OCCT and PlaneGCS`); не считаются
геометрией. Exact-name Replace на stub целый.

## GUI и память — ограничение

CUA/AX/screenshot в этой сессии недоступны. Свежий bundle не собирался и
viewer не запускался, чтобы не оставлять чужой GUI-процесс без возможности
закрыть его штатно через CUA. **GUI smoke не исполнен.** Headless tests не
выдаются за проверку окна. Watchdog 5/5 — unit tests guard'а, не footprint
живого viewer.

Причина прежнего OOM не установлена. Большой STEP/Unity/полный GPU локально не
запускались. `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался. Параметрика в
целом не объявляется завершённой.

Весь diff unstaged/uncommitted; staging/commit/push/PR/merge не выполнялись.
Следующий срез не начат. Следующий шаг — независимое ревью.

## Независимое ревью §25F-1

Повторная проверка проведена на том же base SHA. Production-дефектов не найдено.
В существующий widget gate добавлен достижимый через Remove конфликт H/V:
Replace с корректным числом обязан получить отказ общего validator и сохранить
весь request/Undo/Redo. После Redo тот же Replace сохраняет pending V выбранного
Line на прежнем месте. Dense-editor regression теперь выбирает Line с реальной
stored длиной: дополнительная строка не скрывает Save/Undo/Replace.

Повторно выполнены fmt, workspace clippy all targets/features `-D warnings`,
сборка CLI/app и 42 headless app tests: constraints 9, sketch 21, edit 8, STL 4.
Native workers используют OCCT 8.0.1 и PlaneGCS; native skips отсутствуют.
После изменения только тестов повторены все 9 constraints tests и clippy.
Из fresh worker artifacts подтверждены одинаковые UI/CLI STL/FBX,
55×30×10 mm / 16500 mm³. Pinned ufbx прочитал две замены: 6 + 6 checks,
0 failures. Это headless worker, не интерактивный GUI.

Настоящий stub повторил 7 widget/history tests, включая новые assertions;
2 native worker skips учтены отдельно. CMakeCache подтверждает OCCT NOTFOUND,
`otool` исполненного тестового бинаря не содержит OCCT/PlaneGCS. Первое чтение
CMakeCache из неверного пути завершилось ошибкой; правильный путь найден и
проверен отдельно, положительный test run от этого не менялся.

По последнему указанию пользователя viewer, GUI, GPU, Unity и системные диалоги
не запускались. Пользователь и включённый экран не требовались. Сборки выполнены
последовательно с CARGO_BUILD_JOBS=1; тесты с --test-threads=1. Причина прежнего
OOM остаётся неизвестной. Логи ревью: `/private/tmp/ferrite-pr34-review/`.

Состояние unstaged/uncommitted и отсутствие CI выше описывают передачу Grok.
Следующий этап ревью — commit/публикация, CI точного head, merge и CI merge;
их результаты фиксируются отдельно в PR, без приписывания CI базы новому diff.
