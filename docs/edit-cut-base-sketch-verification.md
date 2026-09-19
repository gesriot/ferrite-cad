# §26F — проверка координат базы Cut-history

2026-09-19, macOS arm64, ветка `edit-cut-base-sketch`. Изменения оставлены
unstaged/uncommitted для независимого ревью. [Контракт и рецепт](edit-cut-base-sketch.md).

## Точная база

Fresh fetch прошёл; исходное дерево чистое. `main = origin/main = HEAD` при
старте: `dd32ae36d524b1f22752fbc11bc4b49b03a8dd41`. Родители merge:
`2f9fdac6dc13183f023a64a3c61150c7cb9f2f90` и
`d8b685c10062ca9123addaa59f6e83e9eb99abf5`. Tree merge и reviewed head совпали:
`89495c289b50a3f8a0e20cfdd1435a7ec9145b87`. PR #48 MERGED.

GitHub API: merge — 27/27 check-runs, 6/6 workflows success; reviewed head —
15/15 и 3/3. Заново скачаны полные runtime logs
[merge](https://github.com/gesriot/ferrite-cad/actions/runs/35392387112) и
[reviewed head](https://github.com/gesriot/ferrite-cad/actions/runs/35388394318).
Для каждого из двух SHA и каждой ОС macOS/Linux/Windows сверены **все 154**
обязательных test names, без skips, и **58** ufbx reads с 0 failures.
Это CI **базы**, не CI незакоммиченного diff.

Применимых AGENTS.md не найдено. Чужой worktree `a200` не менялся. Staging,
commit, push, PR, merge, reset/clean/stash/rebase/amend не выполнялись.

## Архитектура и данные

Тот же `CutHistory` читает полные objects/edges и проверяет всю цепочку один раз
для каталога. Из него выводится дополнительный typed `SketchCutHistory` только
для исходной базы. Старый `sketch_edit::frame` и operation-first refusal NewBody
не менялись. Нет второго traversal, copier, feature или расширения лимита.

Polygon UUID/order/closure/winding/numeric policy остаётся прежней. Проверка
прямоугольника и каждого сохранённого диска берётся из Cut policy; её вызывают
и draft, и prepare, и writer. Writer извлекает числа из payload и повторно
выводит разрешённую правку против текущего документа внутри write transaction.
Job передаёт ему именно prepared payload вместо независимого второго массива
координат. Strict-reference guard не ослаблен.

Новая форма использует прежние поля/canvas/Undo/Redo; показывает абсолютность
инструментов и сохраняет недопустимые промежуточные координаты. Добавлен общий
ограниченный scroll списка действий. Новый тест выявил прежний `finish_edit`,
который сразу dismiss-ил Line draft после публикации; он подключён к существующему
`draft_published`/`draft_load_finished`, чтобы неуспешный async Open возвращал draft.

SQL allowlist проверяется по каждой ячейке, включая rowids: только payload/hash
базы. `meta.modified_at` также остаётся прежним, как требовал старый координатный
editor и его строгие equality-тесты. Source bytes, остальные payloads, все refs,
edges, UUID, capabilities и extra BLOB table сохраняются. Новых refs правка не
создаёт. Два прежних Cut-теста изменены только в ожидаемой доступности **базы**;
отказы tool/circle/annulus/constraint editors и строгие JSON equality сохранены.

## Геометрическая матрица и защиты

1/2/4/16 Cut; плита 80×50×12, радиусы 1.5–2.5, смешанные through/pocket, глубины
3–12, переставленные пространственные слоты `(i*7)%16`. SQL rowids отрицательные,
ordinal обращены, имена совпадают. Матрица измерений именует все шесть родных
base faces; случай с двумя Cut хранит противоположный winding. Поддержаны:

* расширение до `[[-4,-3],[86,56]]`;
* сужение до `[[2,2],[73,48]]`;
* сдвиг до `[[3,1],[83,51]]`.

Каждый исторический producer и итоговый Body cold-rebuild измеряются по формуле
`W*D*H − Σπr²depth`, abs tolerance 1e-6 mm³. По сохранённым UUID разрешаются
native/Carried/Origin refs, затем измеряются координаты/нормали caps/стен,
аналитические радиус/ось bore и положение/нормаль floor. Сопоставления по близости,
порядку граней или display name нет. Старый настоящий sidecar даёт Miss у базы и
всех Cut; следующий проход — Hit у всех. Cold/Miss/Hit совпадают по geometry/refs.

Отдельный binary STL parser проверяет новую bbox, замкнутость, directed edges,
внутренний winding bore, абсолютные центры/радиусы, z-span, floors и openings.
Mesh volume ограничен аналитическим объёмом и объёмом при радиусах `r−0.05`,
плюс 1e-3 mm³ на f32; angular deflection 0.1. B-Rep и mesh допуски не смешаны.

Отказы: missing/reordered/foreign curve IDs, foreign Sketch, nonrectangle,
degenerate/nonfinite/out-of-range coordinates, winding reversal, constraints,
cycle/fork/missing edge/shared history. Для первого/дальнего/последнего диска
отдельно проверены касание, половина WALL_CLEARANCE и пересечение новой стеной;
refusal содержит UUID именно затронутого Cut. Ни один файл не опубликован.
Writer отвергает шесть структурных подделок, числовую подделку и изменение дальнего
tool после preparation при неизменной базе.

Общий сценарий late guards переиспользован для новой геометрии: cancellation
при progress 0.1/0.4/0.95, настоящий source race после обоих rebuild, cleanup,
нулевое число live handles, no-clobber, source/hardlink/symlink aliases, stale CLI
version и exit 7 после состоявшейся публикации. Stub отказ на kernel-first этапе
не считается исполнением этих late guards.

## Исполняемые мутации

Обе временные поломки скомпилировались и исполнили ровно один exact gate без skips:

1. `validate_base` проверяет только первый диск: gate
   `sequential::base_sketch::native_base_refuses_one_bad_aspect_and_distant_walls`
   падает на `wall guard` для дальнего инструмента.
2. Writer записывает старые storage bytes вместо новой базы: gate
   `sequential::base_sketch::native_base_bounds_refs_sql_cache_and_mesh`
   падает на `writer lost the base coordinate change`.

Исходники восстановлены в finally побайтово (SHA256 записаны в `mutations.json`),
обе положительные проверки повторены. Compile failures при разработке новых
тестов и первоначальные устаревшие ожидания доступности не считаются мутациями.

## Исполнение и ресурсы

Все сборки последовательны: `CARGO_BUILD_JOBS=1 CMAKE_BUILD_PARALLEL_LEVEL=1`.
Использованы только прежние `/private/tmp/ferrite-24b-native-target` и
`/private/tmp/ferrite-25j-stub-target` (stub только debug). Pinned OCCT/PlaneGCS
не пересобирались с новыми входами. Логи и извлечённые argv находятся в
`/private/tmp/ferrite-26f`.

Native env: `OpenCASCADE_DIR=$PWD/vendor/install/lib/cmake/opencascade`,
`CMAKE_PREFIX_PATH=$PWD/vendor/install`, `FCAD_PLANEGCS_DIR=$PWD/vendor/planegcs`.
Build-tree tests используют соответствующие DYLD library dirs; staged запуск
проверяется с обеими DYLD-переменными unset. Loader failure probes не включались.
В начале: 71 GiB свободно, normal pressure (1), swap 0; во время сборок 72 GiB,
memory free 66%. Причина прежнего §26E memory peak не переинтерпретируется.

Фактические локальные результаты (повторы exact gates не прибавлены к broad run):

| Конфигурация / проверка | Исполнено успешно | N/A / ignored | Лог |
| --- | ---: | --- | --- |
| Native: document, topology, eval, jobs, eval/planegcs | 507 | 2 no-solver N/A; 1 прежний ручной benchmark ignored | `core.log` |
| Native CLI: circular_cut, edit_circular_cut, edit_sketch, json_v1, print_topology | 55 | 0 / 0 | `cli-final.log` (22), `cli-remainder.log` (33) |
| Native app: весь `sketch::` | 30 | 0 / 0 | `sketch-app.log` |
| Точный прежний redraw regression | 1 | 0 / 0 | `redraw.log` |
| Новый native packed block из YAML, буквальный argv | 6 | 0 / 0 | `packed-native.log` |
| Mixed OCCT/no-solver: весь существующий packed block с новым gate | 8 | 0 / 0 | `packed-mixed.log` |
| Настоящий debug stub: новые exact discovery/writer gates | 2 | 0 / 0 | `packed-stub.log` |
| Настоящий debug stub: document `cut_edit::` | 22 | 0 / 0 | `stub-document.log` |

Итого broad native: **593 исполненных**, отдельно 2 N/A и 1 ignored. Harness
называет 509 core cases passed, но два no-solver случая вернули `skipped:` и в
507 исполненных не входят. Все exact/no-skip блоки действительно выполнили свои
имена. Первоначальный CLI run остановился на старом ожидании недоступности базы;
после узкого исправления ожидания весь соответствующий suite повторён успешно.
Эта неудачная попытка и повторные успешные gates не добавлены в итоговый счёт.

Stub проверен **до исполнения** свежих CLI/test binaries: реальный CMake cache
содержит `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`; `otool -L` показывает
только libiconv/libSystem. Неподходящий прежний release target не использован.
В mixed test binary есть OCCT, нет PlaneGCS (`mixed-imports.txt`). Отдельный
stub CLI запрос к допустимой 16-Cut базе даёт kernel-first exit 2, не создаёт
destination и не меняет source; это не late-guard proof.

`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo fmt --all -- --check`, actionlint обоих workflows, shellcheck reader
script, export-boundary check, MIT header check и `git diff --check` прошли.
Header script проверил 359 tracked файлов; новый Rust test также имеет MIT header.
После финальной правки iterator в test helper measurement gate повторён успешно
(`final-bounds.log`).

Pinned ufbx 0.23.0, commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`, загружен
с проверкой checksum и собран во временной папке. Буквально извлечённый новый
reader loop прочитал три малых FBX: каждый `checks=6 failures=0`. Markdown-рецепт
извлечён по маркеру без prose parsing и исполнен staged CLI: девять успешных
правок на 2/4/16 Cut, три wall refusals, SQL/source/независимый STL и ещё три
ufbx reads; `FCAD_26F_RECIPE_OK /private/tmp/ferrite-cut-base-7pkcr3kh`.
Первая попытка рецепта build-tree CLI не загрузила PlaneGCS из-за DYLD окружения,
оставшегося от mixed run; она сохранена отдельно и не засчитана. Рецепт полностью
повторён успешно через fresh staged bundle без DYLD; loader failure probes
не включались.

## Реальное окно и GUI/CLI parity

Native CUA доступен, Mac разблокирован. Fresh staged
`/private/tmp/ferrite-26f/stage/FerriteCAD.app` прошёл closure inspection обоих
executables, `codesign --verify --deep --strict` и `--solver-info` с обеими DYLD
переменными unset. В bundle 55 файлов / 95,431,699 bytes. Запущен один собственный
viewer под `tools/watch-viewer-memory.py --limit-mib 1536 --seconds 1200`.

В полном окне 988×768 открыта настоящая 16-Cut история `gui-source.fcad`.
С прокруткой доступна команда базы
`01a0b898-7355-7531-8e20-bd1424435b44`; canvas, числовые поля, Undo/Redo и Save
помещаются в окно. Реально выполнены:

1. Оба правых X изменены 80 → 86. Undo оставил промежуточный непрямоугольный
   draft с отказом, Redo вернул допустимый прямоугольник. Другие вершины и tools
   не двигались скрыто.
2. Save dialog отменён; оба X=86 остались. Повторный Save опубликовал
   `/private/tmp/ferrite-26f/gui-copy.fcad`; async Open обновил title, scene и
   catalogue. Повторно открытая форма читает новый путь и сохранённые X=86.
3. На новой копии оба правых X изменены на 69.5: видимый отказ показывает
   `Cut 01a0b898-7f3c-76f0-aa9f-2d45159f9503` (девятый Cut), зазор 0 mm.
   Недопустимые значения остаются после Undo/Redo; публикация недоступна.
4. Draft отменён, собственный viewer закрыт. Guard: **exit 0**, aborted=false,
   416.28 s, peak physical footprint **218.454 MiB** при лимите 1536 MiB;
   pressure всё время 1 (normal), swap 0, минимум свободного диска 71.345 GiB.

Свежий staged CLI от **того же source** и discovery version создал
`gui-peer.fcad`. Все SQL ячейки GUI/CLI совпали; source SHA256 остался
`8fa88a4d9f7518512394e0ddb902fdd851b65fc6a6f33ddc7f3a661beaa3ae04`.
История содержит те же 16 абсолютных tools. Независимый parser измерил обе
копии: bbox `[0,0,0]..[86,50,12]`, mesh volume 49786.87467312844 mm³, корректные
bore/floor/through, winding и closure. STL и FBX побайтово одинаковы:

| Export | Bytes | SHA256 |
| --- | ---: | --- |
| STL | 405484 | `5be6b382ea4fd027ba0e7edceecaeeebbebfe8a895041d51a17c8e0b76c25dbb` |
| FBX | 505197 | `035d70b2e506d14468db334373875626357eb0578628a9f9605100049af157e8` |

GUI FBX дополнительно прочитан pinned ufbx: `checks=6 failures=0`. Машинное
свидетельство: `/private/tmp/ferrite-26f/gui-evidence.json`; guard log —
`gui-memory.jsonl`. Неуспешный async Open и устаревшие ответы проверены app
state-machine gate, а не искусственно вызваны в реальном окне.

## Границы доказательств

Remote CI этого diff и Windows/Linux GUI не запускались: изменения не опубликованы.
Все 154 прежних required names сохранены, добавлены семь: теперь **161**. Все 58
прежних reader calls сохранены, три малых FBX добавлены в прежний loop: **61 на ОС**
в будущем runtime CI. Это состав workflow, не заявление, что весь runtime campaign
повторён локально. Новый большой STEP campaign не выполнялся ради новых случаев.

Автоматическая проверка запретила попытку снимка постороннего Safari из-за
неотносящегося к задаче содержимого. Снимок не выполнен; последующие GUI-действия
ограничиваются собственным staged FerriteCAD.

## Исходный diff, переданный на ревью

`git diff --stat`: **22 tracked файла, +693/−124**. Дополнительно три
untracked файла, +1197 строк. Полный review scope: **25 файлов,
+1890/−124**. Ни один файл не staged; HEAD не изменён.

| Статус | + | − | Файл |
| --- | ---: | ---: | --- |
| M | 15 | 0 | `.github/workflows/ci.yml` |
| M | 42 | 0 | `.github/workflows/runtime-layout.yml` |
| M | 6 | 0 | `README.md` |
| M | 256 | 11 | `crates/ferritecad-app/src/sketch.rs` |
| M | 1 | 0 | `crates/ferritecad-app/src/sketch/drag_tests.rs` |
| M | 15 | 0 | `crates/ferritecad-cli/src/json.rs` |
| M | 8 | 3 | `crates/ferritecad-cli/tests/circular_cut.rs` |
| M | 6 | 3 | `crates/ferritecad-cli/tests/edit_circular_cut.rs` |
| M | 69 | 51 | `crates/ferritecad-cli/tests/support/edit_sequential_cuts.rs` |
| M | 27 | 7 | `crates/ferritecad-cli/tests/support/sequential_circular_cuts.rs` |
| M | 38 | 10 | `crates/ferritecad-document/src/cut_edit.rs` |
| M | 30 | 6 | `crates/ferritecad-document/src/document.rs` |
| M | 2 | 2 | `crates/ferritecad-document/src/edit.rs` |
| M | 3 | 1 | `crates/ferritecad-document/src/lib.rs` |
| M | 122 | 20 | `crates/ferritecad-document/src/sketch_edit.rs` |
| M | 3 | 9 | `crates/ferritecad-jobs/src/edit.rs` |
| M | 3 | 0 | `docs/circular-cut-history.md` |
| M | 7 | 0 | `docs/cli-capabilities.md` |
| M | 8 | 1 | `docs/cli-json-v1.md` |
| M | 6 | 0 | `docs/edit-sketch-copy.md` |
| M | 14 | 0 | `docs/implementation-plan.md` |
| M | 12 | 0 | `tools/check-fbx-complex.sh` |
| ?? | 698 | 0 | `crates/ferritecad-cli/tests/support/edit_cut_base_sketch.rs` |
| ?? | 251 | 0 | `docs/edit-cut-base-sketch-verification.md` |
| ?? | 248 | 0 | `docs/edit-cut-base-sketch.md` |


## Независимое ревью §26F

Reviewer reproduced and fixed a typed-error regression in coordinate preparation:
`coordinate_choice` flattened `CadError` into a discovery refusal string, then
`replace_sketch_coordinates` wrapped it as `Unsupported`. A missing dependency
SQL table consequently lost its `Io` kind and SQLite cause; ordinary unsupported
messages also acquired a duplicate prefix. Preparation now returns the original
`Result<SketchChoice>`; only catalogue presentation converts the error to text.

The new exact gate
`sequential::base_sketch::coordinate_refusals_preserve_their_error_kind_and_cause`
compiled and failed against the original diff (`Unsupported` versus `Io`). It
passes with the fix in native and genuine stub builds and also checks the SQLite
cause and the single-prefix ordinary refusal. It is required by the existing
three-platform CI step, beside the two kernel-free discovery/writer gates.

Independent native review first ran document/topology/eval/jobs: 507 executed,
two explicit no-solver N/A cases and one existing ignored benchmark; CLI 64,
app Sketch 30 and the idle-redraw regression 1 executed without skips. After the
fix, document 187 (one ignored benchmark), affected CLI suites 41 and app Sketch
30 passed without skips; workspace all-target/all-feature clippy passed again.
These overlapping reruns are not added into a misleading distinct-test total.
Stub rerun: three actual base-Sketch tests (including the new regression), four
explicit geometry skips, and 22 document Cut tests. Its debug CMake cache reports
`OpenCASCADE_DIR-NOTFOUND`; the rebuilt CLI imports neither OCCT nor PlaneGCS.

All eight packed OCCT/no-solver gates executed without skips; the native CLI
was restored afterwards. The recipe was freshly extracted from Markdown and
completed with `FCAD_26F_RECIPE_OK`. Fmt, all-feature workspace clippy, actionlint,
shellcheck, export boundary, 359 licence headers and diff whitespace passed.

A fresh review bundle passed runtime closure, strict signature verification and
solver-info with DYLD unset. In its own real macOS window, a private copy of the
16-Cut source was widened 80→88 mm. Reviewer exercised the scrolling action list,
Undo/Redo, invalid intermediate contour, Save Cancel, publication and accepted
async Open. Reopened fields showed both right X=88. Moving them to 69.5 produced
a visible refusal naming the ninth Cut UUID
`01a0b898-7f3c-76f0-aa9f-2d45159f9503`; cancelling retained the accepted model.
The viewer exited normally after 126.56 s: peak footprint **213.095 MiB**, pressure
normal, swap 0, watchdog limit unchanged at 1536 MiB. This does not establish the
cause of the earlier memory spike.

The review GUI copy and an independent CLI copy have identical SQL cells and
byte-identical exports. Source SHA256 remains
`8fa88a4d9f7518512394e0ddb902fdd851b65fc6a6f33ddc7f3a661beaa3ae04`.
The independent STL parser measured 88×50×12 mm, volume 50986.8746731284 mm³,
closed oriented mesh and all 16 absolute tools. STL: 405484 bytes,
SHA256 `4485ebad20bc25b7f6768f82cb299baa5b0a2f198d593009f89a19bea09b77e4`;
FBX: 505197 bytes,
SHA256 `f29eae6509d149db6e409bc0fe805b2779e0e2479a192732258fa28b96b1ce2e`.
Pinned ufbx read the GUI export with 6 checks and 0 failures.

Review logs, before/after failure proof and `gui-evidence.json` are under
`/private/tmp/ferrite-26f-review/`. The GUI filename is `gui-copy.fcad.fcad`:
the system Save field retained its extension when the automation entered another
one; the actual published path was observed and used for all comparisons.
A clipboard paste timeout was followed by fresh observation and keyboard input.
One review command named a nonexistent document test target; it was not counted
and was corrected to the actual library filter before the recorded stub pass.
Remote exact-head and merge checks are audited after publication in the PR;
local evidence above is not a claim of remote CI success.
