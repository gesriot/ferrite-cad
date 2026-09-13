# §25C — проверка выбора и drag Line-вершин

Дата: 2026-09-13. База/HEAD `f932dcc8703b717af9b18f57b276ac575617ac48`,
ветка `sketch-vertex-drag`. Изменения **не staged и не закоммичены**.
Следующий шаг — независимое ревью. Commit/push/PR/merge не выполнялись.

## База и границы

В начале выполнен fetch, `main = origin/main`, дерево чистое. PR #28 — MERGED;
head `da8062c3fa3ef7764552cd3ee6d13f5f25d9394a` и merge имеют одинаковое дерево
`ccf5cd73a54308c3c6c16c33c8b5584583e04756`. Проверены 27/27 success checks,
6/6 success workflow, 23 exact native имени на каждой ОС = 69/69 executions.
[Runtime базы](https://github.com/gesriot/ferrite-cad/actions/runs/34759108630),
[CI базы](https://github.com/gesriot/ferrite-cad/actions/runs/34759108641).
Аудит: `/private/tmp/ferrite-25c/base-runtime.log`, `base-gates.json`, `base-runs.json`.
Это CI базы; **CI незакоммиченного diff не выполнен**.

AGENTS.md в репозитории и родительских каталогах не найден. Чужой a200 и
исходные fixtures не изменялись. Все тестовые документы находятся в private tmp.
Jobs/document/CLI/API/schema/C++/FFI/dependencies не менялись; inventories не
требуют генерации. Большой STEP/partial FBX и полный GPU suite локально не запускались.

## Реализованное поведение

[Протокол](sketch-vertex-drag.md): прежний canvas и один draft для создания и
сохранённого Sketch. Hit radius 9 экранных points, nearest + stable order,
видимый выбор/hover/номер/точные X/Y mm. Захват сохраняет offset и исходные строки
незатронутых координат. Один release — один undo checkpoint, no-op — ни одного;
Escape/focus loss/PointerGone отменяют до исходного State. Release за canvas
завершает жест. Fit/Reset меняют только вид; история ограничена 128 состояниями.

Сохранены свободный click нового контура и максимум 256 точек. Общие
PolygonExtrusion/SketchChoice проверяют preview; invalid draft остаётся видимым,
Save недоступен. Сохранённые curve IDs/число/порядок сегментов неизменны.
Новых kernel вызовов на pointer events или idle repaint loop нет.

GUI обнаружил дополнительный случай: press/move/release приходят одним кадром,
а egui уже очистил `press_origin()` и не выставляет drag response. Failing-first
`single_frame_drag_uses_press_and_release_even_after_later_hover` реально исполнился
и упал (60 вместо 80). Исправление использует события press/release и видимую
область/topmost layer canvas. Тест перекрывающего окна доказывает отсутствие
правки насквозь; hover после release не сдвигает результат. Временный trace
удалён, production сборка повторена. Лог: `single-frame-failing-first.log`.

## Локальные проверки diff

Все логи ниже в `/private/tmp/ferrite-25c/`. Cargo builds: `CARGO_BUILD_JOBS=1`;
тесты serial `--test-threads=1`. Не было параллельных builds/viewers.

| Проверка | Исполненный результат | Лог |
|---|---|---|
| fmt, workspace clippy all targets/features `-D warnings` | pass, повторены после исправления и mutations | `final-fmt.log`, `post-mutation-clippy.log` |
| document + jobs | 186 passed, 1 старый ignored timing test | `document-jobs.log` |
| 11 CLI suites: create/edit/import/shared-import/JSON/validate/STL/FBX/Sketch | 73 passed, без geometry skips | `cli-regression.log` |
| complete JSON FBX native + imported, 8 delivery cases | 1 exact test passed | `fbx-complete.log` |
| JSON FBX refusals | 1 exact test passed | `fbx-refusals.log` |
| Fresh native CLI + app | pass; peer CLI собран до workers | `post-mutation-build.log` |
| Sketch widgets + native workers/peer CLI | 13 passed: 10 pure + 3 native | `post-mutation-sketch.log` |
| Старые edit workers | 8 passed | `ui-edits__tests.log` |
| Старые STL workers | 4 native passed; ещё 1 stub-only early return не засчитан native | `ui-exports__stl__tests.log` |
| Create state machine/workers; dialogs | 17 + 3 passed | `ui-creates__tests.log`, `ui-dialogs__tests.log` |
| Memory watchdog | 5 passed | `watchdog-tests.log` |
| actionlint, shellcheck, licence headers, export boundary, whitespace | pass | `actionlint.log`, `shellcheck.log`, `headers.log`, `export-boundary.log` |

Единственный ignored — прежний `edit::tests::measure_extrude_catalog_read`.
13 Sketch tests включают реальное egui input: click/no-op, offset, multi-move,
coalesced press/move/release, outside release, Escape/focus/PointerGone,
повторный drag, Undo/Redo, numeric edit, scrolling, free add, tie/nearest,
invalid crossing/coincidence/winding, bounded history и idle repaint.

Native env: `native-env.sh`, target `/private/tmp/ferrite-24b-native-target`,
vendor OCCT 8.0.1 и vendor/planegcs, GPU unset. CMake/imports подтверждены в
`native-cmake.log`, `native-imports.log`. OCCT из исходников не пересобирался.

Отдельная stub-сборка: `/private/tmp/ferrite-24j-stub-target`,
`/private/tmp/ferrite-24j/stub-env.sh`. Fresh CLI/app — `final-stub-build.log`.
`final-stub-sketch.log`: 13 harness passes = **10 pure + 3 explicit native skips**.
`stub-jobs.log`: 10 mock edit tests passed. `stub-cli.log`: 22 harness passes =
18 исполненных nongeometry tests + 4 explicit geometry skips. Skips не выдаются
за native. `stub-cmake.log`: OpenCASCADE_DIR NOTFOUND; `stub-imports.log`: нет
OCCT/PlaneGCS imports (только libiconv/libSystem), PlaneGCS build warning подтверждает stub.
Imports повторно проверены после последней сборки: `final-stub-imports.log`.

Две временные компилируемые поломки повторены на окончательном коде:
checkpoint на каждом move и изменение соседней вершины. Каждая дала **1 executed
assertion failure**, не compile failure/zero tests. `mutations.log`,
`mutant-frame-undo.log`, `mutant-wrong-vertex.log`. Исходники восстановлены,
13 положительных Sketch tests, build и clippy повторены.

## Геометрия, identity и workflow

Новый exact gate:
`sketch::drag_tests::native_dragged_sketch_worker_and_cli_preserve_identity_and_geometry`.
Два реальных headless drag при scale 4.75 дают **ровно x=80** из x=60, без
последующей числовой коррекции. Через прежний saved-Sketch worker и peer CLI
сравнены все SQL cells/rowids, UUID, attached source claims, extension rows,
невыбранные payloads. Source bytes не изменились. Cold rebuild разрешил cap
z=0/z=10 и side y=0/x=80 по прежним persisted references/curve IDs.

UI/CLI STL и FBX побайтово совпали на macOS. Независимый binary STL parser:
20 triangles, 1084 bytes, bounds [0,0,0]–[80,40,10], volume **20000 mm³**;
прежний допуск 1 ppm не расширялся. Pinned ufbx 0.23.0,
commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`, strict identity **6 checks,
0 failures**. Артефакты: `final-native-artifacts/`; `final-native-ufbx.log`.

Combined runtime workflow сохраняет все 69 прежних executions и добавляет
3 исполнения нового native имени, итого **72 ожидаемых**, с exact-name/anti-skip.
21 validation и 5 watchdog checks сохранены. Прежняя FBX campaign переиспользует
pinned reader для нового drag.fbx без второго complex import; workflow требует
маркер чтения. Добавленный reader hook исполнен локально (`reader-hook.log`).
Полная трёхплатформенная campaign нового diff последует после ревью/публикации.

## GUI и память

Цельный bundle собран штатным `stage-runtime-layout.sh`: 51 library
(50 OCCT + PlaneGCS), все @rpath targets присутствуют, подпись проходит
`codesign --verify --deep --strict`. UUID native/staged viewer совпадает:
`095430D9-FBE7-3F31-894B-D31A3ECBFD50`. Bundle: `final-gui/FerriteCAD.app`;
логи `final-stage.log`, `final-signature.log`, `final-uuids.log`.
Никакие libraries не удалялись; loader-failure probes/opt-in не запускались.

После исправления реально выполнены: два drag saved L, выбор/номер/X/Y,
Undo только последнего drag, Redo, numeric refinement до 80, crossing preview
с красной общей диагностикой и без Save, Undo, Cancel Save с сохранением draft,
Save новой копии и async Open изменённой сцены. В новом незамкнутом контуре:
3 свободных click, drag существующей третьей вершины без четвёртой строки,
Undo до прежних точных строк, Cancel draft сохраняет принятую сцену.

Целые screenshot-координаты CUA при GUI масштабе дали 79.85636179070724 и
80.07456568667763 mm; до точных 80 доведены **полями**, а не ослаблением сравнения.
`final-gui.fcad` совпал с peer CLI по всем SQL cells; source SHA256 и mtime
неизменны. Validate/cold rebuild успешны, STL/FBX совпали побайтово; независимый
STL дал 20000 mm³, strict ufbx — 6/6. `final-gui-verification.log`,
`final-gui-ufbx.log`, `gui-evidence/final-verification.json`.

Финальный viewer PID **51308**, watchdog cap 1536 MiB, peak footprint
**328.75 MiB** (344720608 bytes), peak RSS 225.22 MiB; pressure всегда 1 (normal),
swap used 3595829248 bytes до/после, delta 0. Exit 0 за 197.63 s, aborted=false,
PID исчез. `final-watch.jsonl`, `gui-evidence/final-memory.json`.
До него последовательно запускались исходный smoke PID 40396 (exit 0) и
диагностический PID 47198 (exit 0), тоже только под guard. После Close app API
не вызывались; выход проверялся по PID/watchdog. Исходный пользовательский OOM
**не объявляется исправленным**; небольшой smoke не является долгим memory soak.

## Файлы и передача

| Файл | + / − | Статус |
|---|---:|---|
| `.github/workflows/runtime-layout.yml` | 30 / 0 | tracked |
| `README.md` | 2 / 0 | tracked |
| `crates/ferritecad-app/src/sketch.rs` | 347 / 128 | tracked |
| `crates/ferritecad-app/src/sketch/drag_tests.rs` | 763 / 0 | new / untracked |
| `docs/edit-sketch-copy.md` | 3 / 0 | tracked |
| `docs/implementation-plan.md` | 10 / 5 | tracked |
| `docs/sketch-extrude-create.md` | 3 / 0 | tracked |
| `docs/sketch-vertex-drag-verification.md` | 169 / 0 | new / untracked |
| `docs/sketch-vertex-drag.md` | 61 / 0 | new / untracked |
| `tools/check-fbx-complex.sh` | 11 / 0 | tracked |

Итого: **10 файлов, +1399 / −133 строк**, включая этот отчёт.

Новые tests/protocol/report включены в diffstat как untracked; git index пуст.
Публичные CLI requests/JSON не изменились: [рецепт L 60→80](edit-sketch-copy.md#исполняемый-рецепт)
и [создание контура](sketch-extrude-create.md) выражают тот же итог точными числами.
Python-блок §25B извлечён из Markdown и исполнен свежим bundled CLI: `recipe.log`,
`recipe/`. UUID обнаружены из JSON; source bytes/mtime сохранены; STL volume 20000,
independent pinned ufbx прочитал FBX 7.4 без warnings.
Нет snapping/constraints/new curves, смены числа/порядка saved segments, in-place
Save или persistent undo. Следующий срез не начат. Готово к независимому ревью.

## Независимое ревью, 2026-09-13

Воспроизведён дефект порядка событий: release активного drag и press следующей
вершины в одном кадре завершали первый жест, но теряли второй. Новый exact egui
тест `release_then_next_press_in_one_frame_keeps_the_next_drag_active` до исправления
компилировался, исполнялся и падал на `next press starts its own drag` (exit 101).
Обработка первых press/release кадра заменена последовательным разбором событий.
Каждый changed release фиксирует свой checkpoint в момент завершения; сравнение
с итоговым draft всего кадра больше не стирает цепочку A→B→A. Preview/cancel и
128 состояний сохраняются. Дополнительные реальные egui tests проверяют no-op
перед двумя жестами A→B→A, Undo/Redo и отмену до release в одном кадре.

После исправления: fmt, workspace clippy all targets/features `-D warnings`,
свежие CLI/app, document/eval/jobs/UI suites (один прежний timing-test ignored),
73 CLI tests, 16 sketch tests, 17 create, 8 edit и 4 native STL workers — pass,
без native skips. Pinned ufbx strict 0.23.0 прочитал drag publication: 6 checks,
0 failures. В отдельном stub target свежие CLI/app не импортируют OCCT/PlaneGCS;
из 16 sketch harness passes 13 исполнены, три native workers явно skipped.

Свежий staged GUI имеет UUID `B98258EF-14BD-3DB0-9499-E2BFDD0DF864`, совпадающий
со свежим executable; deep strict codesign пройден. Один PID 80695 под watchdog
1536 MiB: две вершины сохранённого L перемещены мышью, Undo отменяет только второй
жест, Redo восстанавливает. Пиксельный drag дал X=80.07456568667763; оба X явно
уточнены полями до 80 mm. Save Cancel оставил draft, повторный Save опубликовал
`gui-edited.fcad`, обычный async Open принял его и обновил title. Окно закрыто
штатно, exit 0, PID отсутствует; после закрытия CUA к нему не обращался.
Peak physical footprint 186.80 MiB за 144.43 s, pressure=1, swap не вырос.
Это наблюдение малого L-профиля, не доказательство исправления прежнего OOM.
Намеренные dyld missing-library probes не запускались.

Фактический GUI-файл и независимая CLI-копия имеют одинаковые SQL-ячейки, включая
rowid и attached source claims. Source bytes сохранены. Обе копии прошли validate
и cold rebuild, STL/FBX совпали побайтово; независимый binary STL разбор дал
20 triangles, bounds 80×40×10 mm и volume 20000 mm³. GUI FBX отдельно прочитан
pinned strict ufbx (6 checks, 0 failures). Логи ревью, failing-first, native/stub,
GUI/reader и память: `/tmp/ferrite-pr29-review/` (вне репозитория).
Тяжёлая complex/partial FBX кампания локально не повторялась; её обязательный
прогон вместе с новым drag artifact остаётся в существующем CI трёх ОС.
Результаты CI опубликованного head/merge фиксируются в PR после публикации.
