# §26E — проверка bounded circular Cut history

2026-09-18, macOS arm64. Ветка `circular-cut-history`, без commit/stage/push/PR.
[Контракт и извлекаемый рецепт](circular-cut-history.md).

## База и границы отчёта

Fresh `git fetch origin` прошёл. Исходное дерево было чистым;
HEAD/main/origin/main = `2f9fdac6dc13183f023a64a3c61150c7cb9f2f90`, tree
`43689f10fbb4a20d7ea15caac151f387bd0265e3`. PR #47 — MERGED с head
`cd5b3245e7293cc748ff54d230c7e1fd9c7cb06f`. GitHub API подтвердил 27 успешных
check-runs **точного merge**. Это доказательство базы, не CI будущего diff.
Worktree a200 не открывался и не изменялся. Применимых AGENTS.md не найдено.

## Реализация

Один `CutHistory` проверяет 0–16 звеньев по Body.tip/Feature.previous, полный
набор объектов/рёбер, lossless payload, инструменты и попарный зазор. Discovery
строит его один раз в общем read-only snapshot и передаёт DTO без повторного
чтения файла. На 16 Add недоступен, все Edit доступны; отдельная 17-звенная
фикстура валидируется и открывается обычным reader, но editor явно отказывает
всему каталогу. Имена/SQL rows/ordinal не определяют принадлежность.

Каждый Cut после первого получает origin-имена всех предков. Появление дна
добавляет собственную ссылку и по одной на каждом последующем producer;
повтор добавляет 0. Pocket→through отказывает со всеми floor UUID. Map/archive
v3 и predecessor-qualified keys не изменены: их существующая реализация
проверяется на длинной цепочке. Writer повторно выводит предложение внутри
SQLite-транзакции и отвергает занятые/повторные UUID во всём новом наборе,
включая UUID Sketch curves. Новых feature, схемы или copier нет.

Добавлен только диагностический C ABI `fc_occt_cylinder_axis`: координаты оси
и её направление измеряются по уже разрешённому UUID handle, без поиска
identity. Проверяются также отказ для плоской грани, неизвестного индекса и
освобождённого handle. Это пересборка небольшого bridge, не OCCT/PlaneGCS.

## Геометрия, naming, SQL и cache

Прежняя матрица 1/2 сохранена. Новая — 3/4/16 Cut, первый/средний/последний
выбранные Cut, смешанные through/pocket, радиусы 1.5–2.5 mm, глубины 3–12 mm.
Слоты сетки переставлены `(i*7)%16`: порядок history не совпадает с соседством
в пространстве. Плита 80×50×12 mm. SQL rowids отрицательные, ordinal обращены,
display names одинаковы; optional capability row и посторонняя BLOB-таблица
сохраняются. Правка меняет центр на (+0.375, −0.25), radius на +0.125 и depth
на 2.75 mm. Появление раннего/среднего дна проверяется вплоть до 16/8 новых refs.

`measure_history` разрешает сохранённые UUID, затем измеряет **полученные**
handles: Cylinder radius и аналитическую ось, положения/нормали стенок и
floor, исходные caps/sides. Исторические Cut и final producer именуют каждую
поверхность ровно один раз. Исходная ручная fixture плиты хранит три base refs;
каждый Cut дополнительно называет все шесть её поверхностей. Измеряются все
исторические объёмы и final Body по `W*D*H − Σπ*r²*depth`, abs tolerance
1e-6 mm³, независимо от тесселяции.

Настоящий старый sidecar переносится к каждой edited copy. Ожидаются Hits
только у base/предков, Miss у выбранного и всех потомков; следующий проход
целиком Hit. Cold и восстановленные объёмы/refs совпадают. Это проверка
инвалидации готовых старых результатов, не тест пустого кэша.

Каждая SQL-ячейка edit сравнивается с allowlist (два payload/hash,
modified_at, ровно N−i новых refs либо 0). Все прежние ref rows сохраняются
с rowid. Каждый из 16 CLI add сравнивается с allowlist; старые capabilities,
refs и non-BodyTip dependencies сохраняют rowid. Source bytes неизменны.

Отдельный binary STL parser проверяет все полости, inward winding, z-span,
floors/openings, bbox и парные directed edges. Mesh volume ограничен между
аналитическим объёмом и объёмом с радиусами r−0.05, плюс 1e-3 mm³ на f32;
angular deflection 0.1 rad. Этот допуск не используется для B-Rep.

Native jobs проверяют cancellation при progress 0.1/0.4/0.95 и настоящий
late source race после двух rebuild, освобождение всех handles, cleanup,
отсутствие публикации, no-clobber, source/hardlink/symlink aliases, неверные
UUID/version и exit 7 после публикации. Эти gates исполнены для 2 и 4 Cut.
Stub kernel-first refusal не считается исполнением позднего version guard.

## Команды

Все тяжёлые команды последовательны с `CARGO_BUILD_JOBS=1` и
`CMAKE_BUILD_PARALLEL_LEVEL=1`. Переиспользованы только
`/private/tmp/ferrite-24b-native-target` и `/private/tmp/ferrite-25j-stub-target`.
Native env: OpenCASCADE_DIR=`$PWD/vendor/install/lib/cmake/opencascade`,
CMAKE_PREFIX_PATH=`$PWD/vendor/install`, FCAD_PLANEGCS_DIR=`$PWD/vendor/planegcs`.
Для build-tree исполнения DYLD_LIBRARY_PATH содержит обе native lib dirs;
staged execution работает при unset обеих DYLD-переменных.

```sh
cargo test --release -p ferritecad-document -p ferritecad-topology \
  -p ferritecad-eval -p ferritecad-jobs --features ferritecad-eval/planegcs \
  -- --nocapture --test-threads=1
cargo test --release -p ferritecad-cli --test circular_cut \
  --test edit_circular_cut --test print_topology --features planegcs \
  -- --nocapture --test-threads=1
cargo build --release --features planegcs -p ferritecad-cli -p ferritecad-app
cargo test --release -p ferritecad-app --features planegcs cuts::tests:: \
  -- --nocapture --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Scratch с полными логами и извлечёнными argv: `/private/tmp/ferrite-26e`.
В том числе `native.sh`, `packed-native.sh`, `packed-mixed.sh`, `packed-stub.sh`,
`reader-block.sh` и `recipe.py`. Workflow blocks извлечены из фактического YAML;
локально подставлен только macOS matrix name и пути окружения.

Заключительный прогон после побайтового восстановления обеих мутаций:

| Матрица | Harness passed | Исполнено | Runtime skips | Ignored |
| --- | ---: | ---: | ---: | ---: |
| document/topology/eval/jobs + eval/planegcs | 508 | 506 | 2 | 1 |
| CLI circular_cut/edit_circular_cut/print_topology + planegcs | 32 | 32 | 0 | 0 |
| app cuts + planegcs | 8 | 8 | 0 | 0 |
| Дополнительный diagnostic axis exact gate | 1 | 1 | 0 | 0 |

Итого в этой native-матрице **547 исполненных tests**, два runtime skips и
один ignored. Пропуски — `a_build_with_no_solver_publishes_no_facts` и
`a_build_with_no_solver_refuses_before_the_kernel`: этот build имеет solver.
Ignored — прежний `edit::tests::measure_extrude_catalog_read` (manual benchmark).
Повторные packaged/mutation прогоны не прибавляются к этому числу.

Release CLI/app build, workspace all-targets/all-features clippy с `-D warnings`,
fmt, actionlint обоих изменённых workflows, shellcheck reader script,
export-boundary, licence headers и `git diff --check` прошли. Header check
прочитал 358 tracked source/build files; MIT header нового untracked Rust
файла проверен отдельно. Рецепт побайтово совпадает с документированной
процедурой извлечения (marker служит delimiter).

## UI и эквивалентность

В основной native-матрице 8 app cut tests прошли без runtime skips. Новый
worker gate редактирует первый/средний/последний из трёх Cut, сравнивает оба
экспорта со свежим peer CLI, проверяет exact f64 Undo, один Apply/один шаг,
Redo, Save Cancel, stale worker failure/чужой completion и отказ async Open.
На реальном 16-Cut каталоге отдельный widget проход проверяет, что Apply,
Undo, Redo и Save находятся внутри clip rect и viewport 988×768, а Save
формирует запрос. Это headless egui evidence, не оконный GUI smoke.

В разработке исправлены compile-only ошибки новых тестов (тип массива,
видимость общих test helpers), затем ошибки их ожиданий: base fixture хранит
три собственных refs; отрицательный gap −8 у малых дисков переносил центр
за другой диск вместо пересечения; OCCT вправе параметризовать цилиндр осью
−Z, поэтому проверяется геометрическая ось ±Z. Эти исправления не выданы за
mutation evidence. Регрессионный отказ нового writer guard исправлен с
сохранением UUID в сообщении о коллизии.

## Исполненные packaged gates, stub и mixed

Новый native workflow block реально исполнен: 7 exact-name gates, 0 runtime
skips, 0 ignored. Это четыре CLI history gates, writer, app worker/widgets и
диагностический cylinder-axis gate. Существующее имя последнего уже входило
в baseline; новых required names всего **7**, не 8.

Mixed OCCT/no-solver: весь существующий расширенный workflow block исполнен —
7 tests, 0 skips/ignored. Новый `occt_without_solver_builds_and_edits_history`
действительно создаёт и меняет unconstrained history. `otool -L` именно
исполненного `circular_cut-a2ac09e525e5b280` показывает OCCT, без PlaneGCS.

Genuine stub: новый workflow block — 2 tests, плюс все 21 document cut tests;
writer повторяется в обоих, поэтому **22 различных исполненных tests**, 0
skips/ignored. В reused stub target CMakeCache bridge
`debug/build/ferritecad-occt-db51f87441ed7696/out/bridge-build/CMakeCache.txt`
содержит `OpenCASCADE_DIR-NOTFOUND` и ignore paths для Homebrew, /usr/local и
repo vendor/install. `otool -L` исполненного `circular_cut-3cb2f3330ca53ea4`
содержит только libiconv/libSystem. Discovery/writer работают без kernel и
solver; stub не засчитан как native late-race evidence.

Из YAML извлечены множества required runtime names: **147 → 154**, удалённых
имён 0 (`runtime-gates.json`). Все прежние reader calls сохранены:
**52 → 58 на ОС** после добавления шести small FBX к существующему loop.
Это проверка состава будущего CI, не заявление об исполнении всех 154 gates
или 58 reads локально. Нового тяжёлого workflow нет. Windows/Linux и remote
CI этого незакоммиченного diff не запускались.

## Независимые экспорты и рецепт

Настоящий добавленный reader loop исполнен на шести native test artifacts:
`history-3-0`, `history-3-1`, `history-3-2`, `history-4-0`, `history-4-2`,
`history-4-3`. Каждый дал `checks=6 failures=0`; итог
`FCAD_CUT_HISTORY_UFBX_EXECUTED`. Reader — прежний pinned ufbx 0.23.0,
исходники проверены существующим fetch script; C reader и ufbx собраны
последовательно. Большой STEP-корпус повторно не прогонялся.

Python-блок `FCAD_26E_AGENT_RECIPE` извлечён из контракта без редактирования и
исполнен свежим staged CLI при unset DYLD_LIBRARY_PATH и
DYLD_FALLBACK_LIBRARY_PATH. Итог `FCAD_26E_RECIPE_OK`, модели сохранены в
`/var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-cut-history-q_sunpfm`.
Рецепт добавил 16 Cut, правил first/middle/last для 2/3/4/8/16, проверил
SQL allowlist, source bytes, STL, отказ далёкого пересечения и 17-го Cut;
ещё шесть FBX прочитаны pinned reader. Первое исполнение рецепта остановилось
из-за запрета sandbox на `/usr/bin/time -l` → sysctl, хотя CLI успешно создал
плиту. После разрешения доступа к счётчикам рецепт повторён с новым temp root
и полностью прошёл. Ошибка измерительного wrapper не засчитана как CLI failure.

## Две компилируемые поломки

Временно добавлен `.take(1)` к перебору остальных дисков в `transition`.
`bounded_catalog_all_links_and_distant_conflicts_without_kernel` скомпилирован
и упал на исполненном assertion: 0 passed / 1 failed, exit 101. После
восстановления — 1 passed, 0 skips/ignored.

Затем временно ограничен список descendants `.skip(1).take(1)` при появлении
floor. `native_history_origins_floors_sql_and_old_sidecars` скомпилирован и
упал на исполненном assertion количества SQL refs: 0 passed / 1 failed,
exit 101. После восстановления — 1 passed, 0 skips/ignored.

Оба раза восстановлен тот же SHA-256 `cut_edit.rs`:
`7004b1762ab8f39789c860d4f83753f9256fd8cf9a03e8893b561148466f07d7`.
Логи `mutation-*.log`, `restored-*.log`, результаты `mutations.json` лежат
только в scratch. Новая mutation infrastructure в repo не добавлена.

## Время, память и staged runtime

Одиночные representative CLI процессы, macOS arm64, wall seconds и peak child
RSS из `/usr/bin/time -l`; время включает spawn и wrapper. Add N означает
добавление N-го Cut в историю из N−1. Edit меняет первый Cut. Это наблюдения
одного прогона, без timing assertions или stress.

| Cut после Add | Add, s / MiB | Inspect, s / MiB | Edit first, s / MiB |
| --- | --- | --- | --- |
| 2 | 0.0671 / 25.66 | 0.0568 / 19.78 | 0.0694 / 26.34 |
| 8 | 0.1128 / 27.20 | 0.0591 / 19.86 | 0.1162 / 27.61 |
| 16 | 0.2111 / 30.11 | 0.0613 / 20.13 | 0.2171 / 29.75 |

Fresh `/private/tmp/ferrite-26e/stage/FerriteCAD.app`: closure каждой программы
содержит 50 OCCT dylibs и 1 solver, unexpected=0; bundle 55 files / 95,428,067
bytes. `codesign --verify --deep --strict` прошёл. Viewer `--solver-info` без
обеих DYLD-переменных вернул available и pinned FreeCAD 1.0.1 provenance.
OCCT/PlaneGCS vendor inputs не менялись и не пересобирались. Собраны bridge и
Rust consumers в двух прежних targets. Тяжёлые builds/tests последовательны;
случайно запущенный второй экземпляр своего script, ожидавший Cargo lock,
остановлен по точным собственным PID до компиляции.

## Оконный GUI: попытка прервана guard, не pass

Сначала `cua.getState()` подтвердил наличие native CUA. Пользователь затем
сообщил, что только что открыл пустую вкладку, подтвердив работу на
разблокированном Mac. Запущен один собственный staged viewer под прежним
watchdog, limit 1536 MiB, без DYLD и без FCAD_ALLOW_LOADER_FAILURE_PROBES.
Источник — `cuts-2.fcad` из исполненного рецепта. Вызов native CUA для выбора
окна занял около 391 s. В течение ожидания watchdog зафиксировал превышение
physical footprint и завершил PID 77226: abort в 346.33 s, exit −15 в 347.34 s;
сам guard вернул 125. Максимальный наблюдённый footprint **2490.96 MiB**,
RSS **1037.09 MiB**, pressure normal (1), swap 0. Это разные метрики: лимит
проверяет footprint, включая charged unified/GPU allocations.

Полученный UI snapshot показывал `No document`; он не доказывает успешную
загрузку модели. Add/edit/Undo/Redo/Save/refusal/async Open в окне не выполнены,
GUI-result для измерения не создан. Попытка штатного Cmd+Q сообщила
`procNotFound`; последующая проверка процессов подтвердила отсутствие viewer.
Повторный viewer не запускался. Headless widget/worker evidence выше не
заменяет этот сценарий. Причина прежнего OOM и причина текущего роста
footprint не установлены; исправление OOM не заявляется.

После guard: 74 GiB свободно, swap 0, memory pressure normal. Лог
`/private/tmp/ferrite-26e/gui-memory.jsonl` содержит preflight, PID/start identity,
samples и завершение; `.stdout`/`.stderr` сохранены рядом.

### Точный оставшийся GUI-рецепт

После отдельной диагностики роста footprint, при доступном native CUA и
разблокированном Mac, повторить одним viewer под тем же 1536 MiB guard.
Не запускать второй процесс и не отключать guard. Использовать свежий bundle
с проверенными closure/codesign/solver-info и новый log path.

1. Открыть `cuts-2.fcad` из recipe root выше. Body UUID
   `01a0b5d1-040d-7be1-b0a4-017e4fff9581`; первый Cut
   `01a0b5d1-0482-7563-a134-28297ee5d559`, второй
   `01a0b5d1-0503-7291-8676-123b0f1ed2fb`.
2. Через **Cut circle** добавить `(48,43), r=2, depth=12`, Apply,
   Save copy в новый `gui-third.fcad`; дождаться publication и async Open.
   `inspect --json` должен показать 3 tools в порядке history. Сохранить UUID
   третьего; source bytes должны остаться прежними.
3. Edit второго UUID: `(67.375,18.75), r=1.875, depth=2.75`, Apply,
   Save copy `gui-middle.fcad`, дождаться async Open. Источник этого шага —
   `gui-third.fcad`, immediate predecessor — первый Cut.
4. Edit первого UUID: сначала поставить центр третьего `(48,43)` и Apply.
   Увидеть refusal с UUID третьего Cut; публикации нет. Исправить на
   `(10.375,6.75), r=1.625, depth=2.75`, Apply. Undo должен вернуть ровно
   `(10,7), r=1.5, depth=12`, Redo — новые числа. Save → Cancel сохраняет
   draft. Затем Save copy `gui-first.fcad`, publication и async Open.
5. Для каждого принятого запроса вызвать настоящий fresh peer CLI от того
   же **предшествующего GUI source**, с его JSON content_version и selected
   UUID/tool_curve_id. Для Add minted UUID могут различаться, поэтому сравнить
   retained Body и ordered numeric tools, а STL/FBX — побайтово. Для edit
   сохранить feature/tool UUID и `previous_feature_id`; SQL allowlist и bytes
   исходника проверить функциями `preserved` из извлечённого рецепта.
6. Экспортировать итог и peers с STL deflections 0.05/0.1. `measure` из
   рецепта проверяет три инструмента:
   `([10.375,6.75],1.625,2.75)`, `([67.375,18.75],1.875,2.75)`,
   `([48,43],2,12)`. Exact B-Rep volume = **47796.017370750116 mm³**;
   mesh volume проверять отдельным допуском рецепта. FBX прочитать тем же
   pinned reader. Проверить две floors и третье through opening, все UUID refs.
   Штатно закрыть собственный viewer и зафиксировать exit/peak footprint.

## Полный diffstat, включая untracked

Ветка `circular-cut-history`; HEAD/main/origin/main остаются на базе PR #47.
Индекс пуст; stage/commit/push/PR/merge не выполнялись.

Всего **26 files, +1961 / −365**.

| Файл | + | − | Состояние |
| --- | ---: | ---: | --- |
| `.github/workflows/ci.yml` | 14 | 0 | modified |
| `.github/workflows/runtime-layout.yml` | 50 | 0 | modified |
| `README.md` | 6 | 4 | modified |
| `crates/ferritecad-app/src/cuts.rs` | 166 | 67 | modified |
| `crates/ferritecad-cli/src/json.rs` | 17 | 0 | modified |
| `crates/ferritecad-cli/tests/support/circular_cut_history.rs` | 312 | 0 | untracked |
| `crates/ferritecad-cli/tests/support/edit_sequential_cuts.rs` | 38 | 22 | modified |
| `crates/ferritecad-cli/tests/support/sequential_circular_cuts.rs` | 108 | 27 | modified |
| `crates/ferritecad-document/src/cut_edit.rs` | 388 | 162 | modified |
| `crates/ferritecad-document/src/document.rs` | 94 | 64 | modified |
| `crates/ferritecad-document/src/edit.rs` | 3 | 2 | modified |
| `crates/ferritecad-document/src/lib.rs` | 4 | 4 | modified |
| `crates/ferritecad-occt-bridge/include/ferritecad_occt.h` | 7 | 0 | modified |
| `crates/ferritecad-occt-bridge/src/bridge.cpp` | 28 | 0 | modified |
| `crates/ferritecad-occt/src/ffi.rs` | 35 | 0 | modified |
| `crates/ferritecad-occt/src/kernel.rs` | 10 | 0 | modified |
| `crates/ferritecad-occt/src/unavailable.rs` | 4 | 0 | modified |
| `docs/circular-cut-history-verification.md` | 327 | 0 | untracked |
| `docs/circular-cut-history.md` | 278 | 0 | untracked |
| `docs/cli-capabilities.md` | 8 | 3 | modified |
| `docs/cli-json-v1.md` | 15 | 8 | modified |
| `docs/decisions/0004-feature-predecessor.md` | 10 | 0 | modified |
| `docs/edit-sequential-cuts.md` | 4 | 0 | modified |
| `docs/implementation-plan.md` | 17 | 0 | modified |
| `docs/sequential-circular-cuts.md` | 6 | 2 | modified |
| `tools/check-fbx-complex.sh` | 12 | 0 | modified |


## Независимое ревью 2026-09-18

Ревью повторило основной native-прогон: 547 реально исполненных проверок;
два stub-only N/A и один прежний ignored benchmark отдельно. Scratch ревью:
`/private/tmp/ferrite-26e-review`. Результаты ниже относятся к текущему diff,
не к CI базы.

Найден и исправлен дефект оконного event loop, существовавший до §26E:
`egui-winit::on_window_event` возвращает `repaint=true` также для
`RedrawRequested`. Viewer превращал это в следующий `request_redraw`, поэтому
каждый готовый кадр запускал ещё один. Теперь repaint самого RedrawRequested
удовлетворяется текущим кадром; отдельные запросы widgets, ввод и таймеры
сохранены. Exact test
`tests::completing_a_redraw_does_not_schedule_itself_again` проверяет остановку
этой цепочки и сохранение отдельного повода перерисовки; CI исполняет его на
трёх ОС. Возврат прежнего поведения скомпилировался и дал именно assertion
`idle frame scheduled another frame`; исходник восстановлен побайтово, exact
положительный тест повторён. Первая пробная команда с неполным exact-именем
дала ноль тестов и не засчитана.

До исправления собственный viewer в течение 161 s не воспроизвёл OOM
(peak footprint 191.84 MiB, exit 0), но 1-секундный `sample` показал постоянную
отрисовку и ожидание `CAMetalLayer::nextDrawable`. После исправления такой же
sample в покое целиком ждёт события в `mach_msg2_trap`, без отрисовки.
Это доказательство устранённого redraw loop, **не доказательство причины
первоначального скачка до 2490.96 MiB** и не обещание устранить любой OOM.

Свежий staged bundle ревью прошёл runtime closure, codesign deep/strict и
solver-info без DYLD. В одном PID 99080 под watchdog 1536 MiB через native CUA
исполнены: Open модели с двумя Cut → третий Cut (48,43), r2, depth12 → Save
и async Open; edit среднего (67.375,18.75), r1.875, depth2.75 → Save/Open;
edit первого с центром третьего → видимый отказ с UUID именно третьего;
затем (10.375,6.75), r1.625, depth2.75 → Apply → Undo к (10,7), r1.5,
depth12 → Redo → Save Cancel (draft сохранён) → Save/Open.

Все три опубликованные GUI-копии сравнивались с настоящим CLI от того же
предшествующего GUI source. STL и FBX побайтово равны для каждого шага;
STL независимо разобран, каждый из шести FBX прочитан pinned ufbx (6/0).
Mesh volumes соответственно 47726.009634, 47734.117951, 47796.101896 mm³.
Для обеих правок проверена SQL allowlist; различаются лишь timestamps и
новые UUID трёх добавленных floor refs, старые UUID/rows сохранены.
При первом Save абсолютный путь, введённый в поле имени, macOS превратил в
имя с двоеточиями в том же временном каталоге. Проверки использовали именно
фактически опубликованный путь; следующий Save использовал обычное basename.
Точные пути и SHA-256 источников: `gui-comparison.json` в scratch ревью.

В том же окне через системный Open открыта модель с 16 Cut. Add недоступен,
список прокручивается до последнего UUID; его форма позволяет Apply изменения
глубины 4→3.5, Undo/Redo/Save видимы. Этот дополнительный черновик отменён
без публикации. Bottom view показал 16 полостей; для трёхзвенной копии — два
кармана и сквозное отверстие. Headless проверки остаются отдельными от этого
наблюдения окна.

Viewer штатно закрыт Cmd+Q через 388.16 s, exit 0, watchdog не сработал.
Peak footprint **206.22 MiB**, перед выходом 101.14 MiB, pressure=1, swap=0
весь прогон, свободный диск ≥73.52 GiB. `fixed-memory.jsonl`,
`memory-summary.json`, оба `*.sample.txt` и GUI/CLI comparison сохранены в
scratch. Прежний неуспешный GUI-прогон выше не переименован в pass.

Удалённые проверки нового diff проверяются после публикации по точному SHA;
окончательные ссылки и состояние merge — в PR и отдельном результате ревью.

Mixed OCCT/no-solver повторён: 7 exact gates без skips. Настоящий stub снова
подтверждён CMakeCache NOTFOUND и `otool -L` без OCCT/PlaneGCS: 21 document
cut test и один отдельный CLI discovery (повторный writer exact входит в 21).
Новый packed native block исполнил все семь команд; старые required имена
не удалены (147 → 154 уникальных runtime names, осевой gate также исполняется
повторно). Fmt, workspace clippy all-targets/all-features, actionlint,
shellcheck, export boundary, licence headers, native inventory 58/58 и
публичный рецепт с новым pinned reader прошли. Рецепт создал новый private
root, шесть FBX из matrix и шесть из рецепта прочитаны независимо (6/0 каждый).
Первый перенос packed-команд в scratch ошибся путями tee/grep; после
исправления wrapper блоки повторены, неуспешная попытка не засчитана.
Тесты watchdog сначала остановились на запрете sysctl в sandbox до запуска
ребёнка; повтор с доступом к счётчикам прошёл: 5/5 tests, 8.97 s.


Первый CI head `f14e7dc` выявил устаревшее ожидание в старом JSON/STL test:
после добавления parented Body он сравнивал целиком `features`, включая
контекстную причину недоступности Cut. Прежние Extrude facts не изменились;
у нового общего каталога диагностика корректно зависит от полного Body
frame. Тест теперь сравнивает все прежние поля Extrude без изменения и
отдельно проверяет Cut availability/saved/refusal/document_refusal по общему
каталогу того же pinned snapshot. Причина отказа не закрепляется как
независимая от структуры модели. Полный JSON v1 suite повторён: native
13/13 без skips; stub 12 исполненных и один явный native skip. Clippy и fmt
повторены. Первый CI не объявлен проходом; исправление публикуется новым
review commit и требует CI нового точного head.
