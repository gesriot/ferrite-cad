# §28D-1 — проверка Restore saved vertices

2026-09-27. База `5ea113bbe5df382d738bc747499318b53f03dc43`, PR #65 MERGED,
дерево `6eb0da6f6ee657f5413eda83d999d4ad60a00fd9`. После `git fetch` HEAD, main
и origin/main совпадали, дерево было чистым. Ветка
`restore-sketch-draft-vertices` создана от этой базы и не двигалась: конечный
HEAD тот же. Изменения не staged и не закоммичены.

На старте у merge commit не было завершённого отказа: часть post-merge checks
ещё шла, часть уже была success. Этот прогон их не дожидался и не называет
merge CI пройденным. CI базы PR #65 (7/7 до merge) — не проверка этого diff.
Удалённый CI незакоммиченного diff не запускался.

## Изменение

В форме **Edit saved Sketch — new copy** рядом с Undo/Redo есть **Restore
saved vertices**. Она пишет все строки координат обратно в `to_string()` тех
вершин, которые `begin_edit` уже держит в `EditSketchRequest`. Второй копии
снимка и нового предметного API нет. Совпадение строк — не шаг истории, Redo
сохраняется; `33.0` и `33` различны. Невалидный текст восстанавливается без
parsing. Один Undo возвращает весь прежний draft. Feature, высота или угол,
замыкание, UUID и порядок Line, source, expected version, выбранная вершина и
вид не меняются; Fit/Reset не вызываются. Кнопка выключена при running job и
при уже захваченном pointer gesture, тем же кадром, что Undo. Её нет у нового
контура, Circle, Annulus, редактора угла и форм constraints/cuts. Нажатие не
создаёт job, файл или публикацию. Save edited copy и правила завершения
истории после публикации прежние.

Чтобы тесты открыли уже существующие лёгкие документы, `plate` и
`write_sector`/`reading` стали `pub(crate)` внутри `cfg(test)`. Нового
генератора геометрии нет. Domain, jobs, kernel, FFI, CLI/JSON, схема,
зависимости, Cargo.lock и workflows не менялись.

## Локальные проверки

Логи: `/private/tmp/ferrite-grok47-trial-1/`. Сборки последовательные,
`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`. Native target
`/private/tmp/ferrite-24b-native-target`, release. OCCT/PlaneGCS/Boost заново
не собирались. `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался. Это headless
egui, не оконный GUI. Оконный viewer/GPU smoke не запускался.

```bash
source /private/tmp/ferrite-26g/env.sh
cargo fmt --all -- --check
cargo clippy --offline --workspace --all-targets --all-features -- -D warnings
cargo test --offline --release -p ferritecad-app --all-features --bin ferritecad-viewer -- --list
cargo test --offline --release -p ferritecad-app --all-features --bin ferritecad-viewer sketch:: -- --test-threads=1 --skip native_ --nocapture
export FERRITECAD_REQUIRE_PLANEGCS=1 FERRITECAD_REQUIRE_OCCT=1
cargo build --offline --release -p ferritecad-cli --all-features
cargo test --offline --release -p ferritecad-app --all-features --bin ferritecad-viewer native_saved_sketch_draft_and_cli_preserve_same_model -- --test-threads=1 --nocapture
```

Fmt check и clippy `-D warnings` прошли (dev profile, 27.75s, без warnings).
`--list` показал тесты viewer; ноль тестов не засчитывался.

**29/29 исполненных headless sketch/widget/drag tests, 0 failed, 0 ignored.**
Фильтр `--skip native_` их не запускал: это не skip внутри теста, а отдельный
прогон ниже. Лог `sketch-widgets-drag.log`.

Новые проверки настоящими виджетами:

- несколько вершин, включая `33.0`, `-`, пустую строку и `1.`, → Restore →
  undo вырос на 1, не на число вершин; один Undo возвращает все эти строки;
  Redo возвращает снимок `to_string()`;
- no-op при уже совпавших строках не очищает Redo;
- running job не меняет draft и стеки; после него Restore работает;
- жест: один drag — один Undo, release над кнопкой не добавляет второй шаг,
  press по кнопке во время жеста не подменяет draft снимком и не публикует;
- после Restore обычный Save edited copy сохраняет source, version, UUID и
  порядок; сам Restore request не создаёт; отказ worker и непринятый Open
  возвращают draft с историей; принятый Open её заканчивает; файл источника
  не меняется;
- сохранённый partial Revolve и база одного Cut (лёгкие `write_sector` и
  `plate` + один `prepare_circular_cut`) — та же одна контрольная точка;
- кнопки нет у нового контура, Circle, Annulus, угла, constraints и cut form.

**Native edit worker, отдельно: 1/1 исполнен, 0 skipped.**
`native_saved_sketch_draft_and_cli_preserve_same_model` после пересборки peer
CLI (`FERRITECAD_REQUIRE_PLANEGCS=1`). Лог `native-edit-worker.log`. Остальные
native sketch/drag tests в этот прогон не входили и успехом не считаются.

## Ресурсы

До clippy target был 2.3G, после — 2.4G. Свободно на томе данных около 141G.
Swapins и swapouts за время проверки — 0. Своих фоновых процессов после
прогона не осталось. Чужой worktree `a200` не открывался.

## Независимое ревью, 2026-09-27

Предыдущие разделы фиксируют передачу незакоммиченного diff. При ревью прочитан
весь код кнопки и новые widget/drag tests; функционального дефекта не найдено,
production-код не исправлялся. Добавлен этот протокол проверки.

На том же native target последовательно пересобраны **release CLI и viewer**
с `--all-features`, `FERRITECAD_REQUIRE_OCCT=1` и
`FERRITECAD_REQUIRE_PLANEGCS=1`. Полный фильтр `sketch::` без исключений:
**44 passed**, включая native/drag; `edits::tests::`: **11 passed**.
Нулевых исполнений, native skips, failed или ignored в этих двух прогонах нет.
Повторены fmt и workspace release clippy со всеми targets/features и
`-D warnings`. Свежий arm64 bundle прошёл `codesign --verify --deep --strict`
и loader/solver-info probes без `DYLD_*`. Native библиотеки не пересобирались.

**Оконный macOS smoke исполнен независимо через CUA**, один свежий viewer,
PID 81530, под `watch-viewer-memory.py` с лимитом 1536 MiB. Временная модель —
плита со скруглением из прежнего лёгкого fixture, вне checkout.

- Исходные строки `33`, `3.25`, `-4.5` заменены на `33.0`, `-` и пустую строку.
  Restore вернул весь контур; один Undo вернул все три исходных текста правки,
  включая оба невалидных; Redo снова восстановил сохранённые вершины.
- Реальный drag изменил выбранную вершину. Undo вернул её; клик по выключенной
  Restore сохранил доступный Redo. Redo вернул жест, последующая Restore — снимок.
  Выделение и вид сохранились. Формат `33.0` отдельно включал кнопку.
- После Restore координаты изменены в валидный прямоугольник
  `(-4.5,1.25)..(36.5,15.5)`. Save Cancel сохранил форму и значения.
  Повторный Save опубликовал `gui.fcad`, async Open принял его; окно показало
  изменённую плиту со скруглением. STL и FBX экспортированы из этого же окна.
- CLI выполнил ту же правку из того же источника. Сравнены **129 SQL-ячеек**:
  GUI и CLI равны с допустимым исключением `meta.modified_at`; относительно
  источника изменились только payload/hash нужного Sketch. UUID, refs,
  высота 6.75 mm и радиус Fillet 2.375 mm сохранены; SHA-256 источника прежний.
  GUI/CLI STL и FBX равны побайтово. Pinned ufbx 0.23.0: **6 checks, 0 failures**;
  независимый join STL/FBX подтвердил **64 ориентированных треугольника**, худшее
  расхождение `6.94e-18 m` при допуске `1e-9 m`.

Watchdog: sampled peak **207.251 MiB**, pressure 1, swap 0, штатный exit 0,
`aborted=false`. После Quit проверен только PID; обращения CUA к закрытому
viewer не делались. Этот smoke не устанавливает причину прежнего OOM.

Логи, fixture, SQL/export comparison и memory samples:
`/private/tmp/ferrite-grok47-review-1/`. Независимо выполненные команды:

```sh
source /private/tmp/ferrite-26g/env.sh
export FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1
cargo build --offline --release -p ferritecad-cli -p ferritecad-app --all-features
cargo test --offline --release -p ferritecad-app --all-features --bin ferritecad-viewer sketch:: -- --test-threads=1 --nocapture
cargo test --offline --release -p ferritecad-app --all-features --bin ferritecad-viewer edits::tests:: -- --test-threads=1 --nocapture
cargo fmt --all -- --check
cargo clippy --offline --release --workspace --all-targets --all-features -- -D warnings
```

Новый stub target и тяжёлый STEP-корпус для UI-кнопки локально не создавались.
Remote CI публикуемого commit учитывается отдельно от этих проверок и CI базы.
