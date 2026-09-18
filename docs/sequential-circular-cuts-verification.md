# §26C — фактическая локальная проверка

Дата: 2026-09-18, macOS arm64. Срез реализован в ветке
`sequential-circular-cuts`. Изменения **unstaged/uncommitted**. CI нового diff
не запускался; push, PR и merge не выполнялись. Следующий шаг — независимое
ревью, следующий функциональный срез не начат.

## База и область изменения

В начале выполнен fresh `git fetch origin`. `HEAD`, `main` и `origin/main`
совпали с `5ba38445fd1706d08a52c8c17698dcf492e144db`; рабочее дерево было чистым.
Через GitHub прочитаны PR #45 (MERGED, тот же merge SHA) и шесть успешных
workflow runs именно этого SHA: `35317443340`, `35317443418`, `35317443345`,
`35317443322`, `35317443376`, `35317443452`. Это проверка базы, не нового diff.
Чужой worktree `a200` не изменялся. Индекс не использовался для подготовки diff.

Перед кодом расширен [ADR 4](decisions/0004-feature-predecessor.md).
Происхождение грани — `(origin feature UUID, исходная cap/segment role)`;
перенос выполняется по actual boolean history для всех уровней. Старые refs
продолжают адресовать исторический producer. На втором результате появились
явные `OriginCap`/`OriginSide`, отдельная required capability и archive v3.
Legacy carried роли сохраняют смысл собственных граней непосредственного
предшественника. Archive key дополнительно учитывает identity predecessor.
Новые Body ownership edges, schema migrations и изменения OCCT/FFI отсутствуют.

[Контракт](sequential-circular-cuts.md) перечисляет точный SQL allowlist,
JSON additions, положительный зазор `> 1e-7 mm`, отказ третьему Cut и отказ
редактированию обоих Cut восьмиобъектного результата. UI и CLI используют
существующий shared copy job, без второго copier.

## Среда и ресурсы

Все сборки последовательны, `CARGO_BUILD_JOBS=1`,
`CMAKE_BUILD_PARALLEL_LEVEL=1`. Повторно использованы проверенные targets:

- native release/debug: `/private/tmp/ferrite-24b-native-target`;
- настоящий stub debug: `/private/tmp/ferrite-25j-stub-target`;
- pinned OCCT: `vendor/install/lib/cmake/opencascade`, библиотеки
  `vendor/install/lib`; planegcs: `vendor/planegcs`.

До нагрузки и после основных прогонов свободно 74 GiB, `memory_pressure -Q`
показал 69–68% free; `vm.swapusage` — 0. Пиковая память компилятора не измерялась.
Чужие процессы/кеши не очищались. Loader-failure probes не выполнялись,
`FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался. Причина прежнего OOM неизвестна.

Артефакты и полные локальные логи: `/private/tmp/ferrite-26c` (временные файлы,
могут быть очищены ОС). `env.sh` задаёт native target, pinned paths,
`FERRITECAD_REQUIRE_OCCT=1`; при native planegcs-прогонах дополнительно
`FERRITECAD_REQUIRE_PLANEGCS=1`. В packed core gates без feature он равен 0.

## Исполненные проверки

Финальные команды после восстановления мутаций и усиления SQL/normal assertions:

```sh
source /private/tmp/ferrite-26c/env.sh
export FERRITECAD_REQUIRE_PLANEGCS=1
cargo fmt --all -- --check
cargo test --release -p ferritecad-document -p ferritecad-topology \
  -p ferritecad-eval -p ferritecad-jobs --features ferritecad-eval/planegcs \
  -- --test-threads=1
cargo test --release -p ferritecad-cli --test circular_cut \
  --test edit_circular_cut --test print_topology --features planegcs \
  -- --nocapture --test-threads=1
cargo test --release -p ferritecad-app --features planegcs cuts::tests:: \
  -- --nocapture --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release --features planegcs -p ferritecad-cli -p ferritecad-app
```

Финальная матрица: core document/topology/eval/jobs **506 passed**, 0 failed,
один прежний ignored manual timing `edit::tests::measure_extrude_catalog_read`;
CLI **25 passed** (10 circular, 7 edit-cut, 8 print-topology); app cuts **6 passed**.
Native skip markers отсутствуют. Итого 537 исполненных тестов в этой матрице,
без прибавления повторных packed gates. Fmt, workspace clippy all-targets/
all-features `-D warnings` и финальная release сборка прошли.
Логи: `final-core.log`, `final-cli.log`, `final-ui.log`, `final-clippy.log`,
`final-build.log`.

Отдельно исполнены `actionlint .github/workflows/ci.yml
.github/workflows/runtime-layout.yml`, `shellcheck tools/check-fbx-complex.sh`,
`bash tools/check-export-boundary.sh`, `bash tools/check-licence-headers.sh`.
Новый untracked Rust module проверен на SPDX отдельно: tracked-only script
его пока не перечисляет. Native/FFI/bridge не менялись, поэтому новые ABI/pin
inputs не заявляются; тяжёлый STEP corpus повторно не запускался.

### Упакованные workflow команды

Run-блоки извлечены из фактического YAML, заменено только значение matrix OS
на `macos`. Реально исполнены bash scripts с исходными argv, exact names,
`--nocapture`, `--test-threads=1`, проверками наличия `test NAME ... ok` и
отсутствия `skipped:`:

- `packed-native.sh`: 9 обязательных gates — пять CLI sequential, document
  writer, topology codec, eval cache identity, app widgets/worker/CLI;
- `packed-mixed.sh`: 5 gates, четыре прежних circle/annulus/cut/edit-cut и новый
  `sequential::occt_without_solver_builds_two_unconstrained_cuts`;
- `packed-stub.sh`: 2 gates — sequential discovery/publication refusal и writer.

Все эти gates действительно исполнились, без skip. Сопутствующий app
`solver_info` test binary с нулём совпавших тестов не засчитывается как gate.
Аудит имён runtime workflow: 131 прежнее, 141 теперь, удалённых нет, добавлено 10.
Новый stub блок добавлен в обычный CI. Локальная проверка macOS не означает
исполнение новых argv на Windows/Linux.

### Реальный stub и mixed

Stub запускался без `OpenCASCADE_DIR`, `CMAKE_PREFIX_PATH`,
`FCAD_PLANEGCS_DIR`, `DYLD_*` и require flags. Его реальный CMakeCache содержит
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`, а
`CMAKE_IGNORE_PREFIX_PATH` исключает `/opt/homebrew`, `/usr/local` и pinned
vendor install. `otool -L` фактически запущенного
`debug/deps/circular_cut-3cb2f3330ca53ea4` показывает только `libiconv.2.dylib`
и `libSystem.B.dylib`, без OCCT/solver.

Помимо двух packed gates выполнены:

```sh
cargo test -p ferritecad-cli --test circular_cut --test edit_circular_cut \
  -- --nocapture --test-threads=1
cargo test -p ferritecad-document --lib cut_edit::tests:: \
  -- --nocapture --test-threads=1
```

Первый сообщает 12 + 8 passed, но **фактически 3 структурных теста и 17
явных kernel/mixed skips**. Это не 20 геометрических доказательств. Document:
19 passed, 0 skips. Stub sequential gate проверяет настоящий отказ публикации
с корректным документом/request; файлы не публикуются.

Mixed использует OCCT из vendor, `--no-default-features`, без planegcs feature;
пять gates passed, 0 skips. Второй unconstrained Cut строится и его поверхности
измеряются. Поздний version guard доказывается отдельным native gate после
обоих rebuild, а не stub kernel-first отказом.

## Геометрия, идентичность и сохранность

Восемь fixtures: 80×50×12; основной набор `(20,20), r5` и `(55,30), r7`;
альтернативный `(58,17), r3.5` и `(21,33), r6`. Для каждого набора глубины
`12/12`, `12/7`, `4/12`, `4/7`. Object names одинаковые, ordinal перевёрнуты.
Выбор истории/ссылок выполняется по UUID и связям.

| Глубины | Faces | Native volume, основной набор (mm³) | Native volume, альтернативный (mm³) | STL volume из рецепта (mm³) |
|---|---:|---:|---:|---:|
| 12 / 12 | 8 | 45210.265723612 | 46181.017853572 | 45211.421667 |
| 12 / 7 | 9 | 45979.955923742 | 46746.504531218 | 45980.792954 |
| 4 / 12 | 9 | 45838.584254330 | 46488.893933623 | 45839.479829 |
| 4 / 7 | 10 | 46608.274454460 | 47054.380611269 | 46608.851116 |

Native volume сравнивается с `W*D*H - π*r1²*d1 - π*r2²*d2`, tolerance 1e-6.
Каждый результат читается в свежей OCCT session: cold, cache misses всех трёх
features, затем hits всех трёх (24 rebuild в основной матрице). Все прежние
refs разрешаются на историческом producer. Все final refs разрешаются в
различные faces и покрывают весь final Body. Измеряется уже разрешённый handle:
аналитический тип поверхности, координаты и нормали mesh range этой грани.
Наружные caps/walls, обе цилиндрические стенки и каждое существующее дно
проверяются независимо. Измерения не выбирают имя или соответствие.

Контролируемое изменение только второго tool даёт `Hit, Hit, Miss`, первого —
`Hit, Miss, Miss`; затем проверяются cold и warm результаты с новыми глубинами.
Это fixture mutation, не обещание UI editor для восьми объектов.

Независимый binary STL parser проверяет обе полости, радиальные inward normals,
z-span, каждое дно/открытый дальний конец, bbox и замкнутость через парные
ориентированные рёбра. Граница mesh volume выведена из вписанных цилиндров
с `r-deflection`, плюс округление; deflection 0.05 mm, angular 0.1 rad.

SQL snapshot сравнивает все таблицы, UUID, старые payload/metadata, refs с rowid,
non-tip deps с rowid, optional capabilities с нестандартными rowid и чужую
таблицу BLOB. Разрешены только документированные изменения; источник побайтово
неизменен. Writer отдельно отказывает forged overlap/origin, занятому UUID и
повторному новому reference UUID. Сохраняются прежние §26B проверки numeric
discovery, optional rows, first Undo и guarded Unix-only symlink.

Native отказы: совпадение/пересечение/касание/вложенность дисков, зазор меньше
tolerance, внешняя стенка, чрезмерная глубина, занятый output, source/alias,
stale version, третий Cut. Cancellation и изменение source на progress ≥0.95
после rebuild действительно достигнуты: публикации нет, scratch удалён.

## Мутации и совместимость

`mutations.py` поочерёдно внёс две компилируемые поломки и вызвал exact gate
`sequential::native_each_tool_invalidates_only_its_dependent_chain`:

1. В `record_cut` поменял местами qualified значения первого pocket floor
   и внешнего End cap. Геометрия и resolvability остались, но runtime assertion
   упал с `outer cap changed meaning`.
2. После cache restore второго Cut подставил shape непосредственного
   предшественника в final state. Runtime assertion: 8 faces вместо 10.

Каждый запуск: exit 101, 0 passed, **1 failed**, 0 skips; compile failures и
zero-test runs не засчитаны. После каждого файла проверено byte-for-byte
восстановление. SHA-256 восстановленных файлов:

- map: `70da73d6bbe0a5276224b31ef5a7eb9454bb7c7c98c120b2ed1aba8ab7726271`;
- cold: `114a7492ccf7a8445d363c3052651c97acec5044fe2168dacc098e6605f505dd`.

Это hashes непосредственно при восстановлении в harness. Последующий
`cargo fmt` только переформатировал вызов `cut_archive_key` в cold.rs (переносы
и trailing comma); дополнительно проверено точное равенство текущего файла
результату rustfmt сохранённого baseline. Мутационная ветка не осталась.
Финальная native-матрица исполнена после fmt.
Положительный exact gate после восстановления passed. Логи `origin-swap.log`,
`stale-cached-body.log`, `mutation-positive.log`, результаты `mutations.json`.
Отдельного failing-first прогона всего нового набора на исходной базе не было;
негативное доказательство здесь — эти две исполненные мутации.

Codec gate отвергает архив v2 и проверяет v3 roundtrip qualified имён; прежние
archive/resolve тесты остаются. До stub rebuild сохранён прежний reader
`prechange-stub-reader`, SHA-256
`5baabc41149a5512f5dcbb707aeb926c2e44e98cae5d4d6bfcc4c7d561bbf517`.
Его точный build SHA не установлен, он не выдаётся за сборку merge SHA.
Фактический `inspect --json` принимает прежний first-cut и сообщает
`document_refusal: it requires topology.origin-face.v1, which this build does not
implement` для нового документа; все edit catalog actions unavailable,
байты обоих файлов сохранены. Его `rebuild --cold` kernel-first отказывает
из-за отсутствия OCCT и **не засчитывается** как capability/rebuild gate.

## CLI рецепт и pinned FBX reader

Блок `FCAD_26C_AGENT_RECIPE` извлечён regex из Markdown контракта в `recipe.py`
и выполнен настоящим свежим staged CLI при unset `DYLD_*`:

```sh
export FERRITECAD=/private/tmp/ferrite-26c/stage/FerriteCAD.app/Contents/MacOS/ferritecad
export FCAD_UFBX_READER=/private/tmp/ferrite-26c/read_production
python3 /private/tmp/ferrite-26c/recipe.py
```

Результат `FCAD_26C_RECIPE_OK`, четыре объёма в таблице выше. Temporary model root:
`/var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-sequential-2tk9c8lb`.
Reader собран штатными fetch/build командами из pinned ufbx 0.23.0,
commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`, с проверкой pinned checksums.
Из `tools/check-fbx-complex.sh` отдельно извлечён и исполнен **новый reader
block**: четыре сохранённых FBX, каждый `checks=6 failures=0`, итоговый
`FCAD_SEQUENTIAL_CUT_UFBX_EXECUTED`. Весь старый complex STEP corpus здесь не
исполнялся. Рецепт отдельно прочитал четыре своих FBX тем же reader.

## Настоящий GUI smoke

Native CUA фактически доступен; Finder screenshot показал разблокированный
desktop, до старта viewer отсутствовал. Свежие CLI/viewer собраны с planegcs,
runtime closure получен для обоих, `tools/stage-runtime-layout.sh` создал
`/private/tmp/ferrite-26c/stage/FerriteCAD.app`; `codesign --verify --deep --strict`
прошёл. `--solver-info` при unset `DYLD_LIBRARY_PATH`/`DYLD_FALLBACK_LIBRARY_PATH`
сообщил available и pinned FreeCAD 1.0.1 provenance.

Запущен **один собственный** viewer через:

```sh
python3 tools/watch-viewer-memory.py \
  --log /private/tmp/ferrite-26c/gui-memory.jsonl --limit-mib 1536 --seconds 1800 \
  -- /private/tmp/ferrite-26c/stage/FerriteCAD.app/Contents/MacOS/ferritecad-viewer \
  /var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-sequential-2tk9c8lb/first-12.fcad
```

Через реальное окно: Open first-cut → Top → Add cut → `(20,20),r7,d12`
даёт видимую ошибку gap=-12 → исправление `(55,30),r7,d12` → Apply → Undo
(пустые исходные поля, Save исчез) → Redo → Save Cancel (параметры и готовность
сохранены) → Save `gui-two.fcad` → async Open (title изменился, форма закрылась,
видны два отдельных отверстия). Наблюдения и screenshots сохранены в tool trace.
Это настоящий GUI smoke, отдельно от headless widgets tests.

Обе .fcad копии (GUI и CLI того же запроса/источника) экспортированы staged CLI:

- STL: 51 484 bytes, SHA-256
  `afacf7a908acbcc4b20cfdce325f4b84aa83fd9399ae503c799264b878be2568`;
- FBX: 112 495 bytes, SHA-256
  `2bb02be0935f5c501f8e71568d37199be112bedc783aa454c9372dbb2228395b`.

Файлы побайтово равны, GUI FBX также прошёл pinned reader 6/0. Экспорт для этого
сравнения делался CLI из GUI-сохранённой модели, не через кнопки Export окна.
Первый ошибочный вызов FBX с STL-only flags был исправлен; parser refusal не
засчитан успешным экспортом. Ошибочные flags старого rebuild также не зачтены.

Закрытие штатным Cmd-Q, watchdog exit 0, `aborted=false`, PID 67511,
длительность 195.4 s. Пик physical footprint 204.001 MiB, RSS 127.641 MiB,
swap 0, pressure=1 на всех samples. Повторного `getApp` после закрытия не было.

## Состав diff и передача

Итоговый diffstat получен из `git diff --numstat` плюс физические строки
трёх untracked файлов (без `git add`).
Scratch logs не входят в repository diff. Индекс остаётся пустым;
все изменения unstaged/uncommitted, новый CI не запускался и публикации ветки/PR
не было. После независимого ревью можно принимать решение о публикации.

<!-- DIFFSTAT -->

| Файл | + | − | Статус |
|---|---:|---:|---|
| `.github/workflows/ci.yml` | 11 | 0 | modified |
| `.github/workflows/runtime-layout.yml` | 60 | 0 | modified |
| `crates/ferritecad-app/src/cuts.rs` | 49 | 9 | modified |
| `crates/ferritecad-app/src/main.rs` | 12 | 0 | modified |
| `crates/ferritecad-cli/src/json.rs` | 23 | 0 | modified |
| `crates/ferritecad-cli/src/topology.rs` | 12 | 0 | modified |
| `crates/ferritecad-cli/tests/circular_cut.rs` | 5 | 5 | modified |
| `crates/ferritecad-cli/tests/edit_circular_cut.rs` | 2 | 5 | modified |
| `crates/ferritecad-document/src/cut_edit.rs` | 145 | 4 | modified |
| `crates/ferritecad-document/src/document.rs` | 46 | 17 | modified |
| `crates/ferritecad-document/src/lib.rs` | 2 | 1 | modified |
| `crates/ferritecad-document/src/model.rs` | 35 | 0 | modified |
| `crates/ferritecad-document/src/schema.rs` | 1 | 0 | modified |
| `crates/ferritecad-eval/src/cache.rs` | 37 | 2 | modified |
| `crates/ferritecad-eval/src/cold.rs` | 10 | 2 | modified |
| `crates/ferritecad-topology/src/archive.rs` | 68 | 49 | modified |
| `crates/ferritecad-topology/src/codec.rs` | 129 | 3 | modified |
| `crates/ferritecad-topology/src/map.rs` | 101 | 118 | modified |
| `crates/ferritecad-topology/src/resolve.rs` | 60 | 0 | modified |
| `docs/circular-cut-copy.md` | 5 | 2 | modified |
| `docs/decisions/0004-feature-predecessor.md` | 34 | 0 | modified |
| `docs/edit-circular-cut-copy.md` | 4 | 0 | modified |
| `docs/implementation-plan.md` | 26 | 11 | modified |
| `tools/check-fbx-complex.sh` | 12 | 0 | modified |
| `crates/ferritecad-cli/tests/support/sequential_circular_cuts.rs` | 854 | 0 | untracked |
| `docs/sequential-circular-cuts.md` | 233 | 0 | untracked |
| `docs/sequential-circular-cuts-verification.md` | 319 | 0 | untracked |

**27 файлов, +2295/−228; 24 modified и 3 untracked.**

## Независимое ревью перед публикацией

2026-09-18. Повторный fetch подтвердил базу `5ba38445` и PR #45 MERGED;
все 27 check runs базы успешны. Исходный diff и untracked файлы сохранены
в `/private/tmp/ferrite-26c-review/original`. Проверены перенос всех уровней
origin names, legacy carried semantics, archive v3/key, узкий writer и его
re-derivation, UI history и новые workflow argv. Блокирующих дефектов не найдено.
В README, общей capability-таблице и JSON-контракте исправлено устаревшее
описание одного Cut; добавлены поля discovery и явная граница редактирования
двухзвенной истории. Эти документальные правки не меняют исполняемый код.

Независимо повторены 506 document/topology/eval/jobs, 25 CLI и 6 headless app
тестов: 537 passed, 0 failures, 0 native skips; один прежний timing benchmark
ignored. Свежие release CLI/app собраны до worker gates. Mixed OCCT/no-solver
exact gate второго Cut исполнен; затем восстановлена сборка с planegcs.
В отдельном настоящем stub исполнены discovery gate и 19 document cut tests,
без skip: CMakeCache NOTFOUND и `otool -L` подтверждают отсутствие OCCT/solver.
Fmt, workspace clippy all-targets/all-features `-D warnings`, boundary,
licence, actionlint, shellcheck и whitespace checks прошли.

Рецепт заново извлечён из Markdown и исполнен текущим CLI: четыре объёма
`45211.421667 / 45980.792954 / 45839.479829 / 46608.851116` mm³.
Четыре FBX из повторного native gate прочитаны pinned ufbx, каждый 6/0.
Сохранённые GUI/CLI STL и FBX автора независимо сравнены побайтово; hashes
совпали с приведёнными выше. Журнал watchdog подтверждает exit 0, без abort.
Повторный оконный smoke ревью не выполнялся: CUA сообщил, что Mac заблокирован.
Предыдущий оконный прогон автора и повторные headless проверки учитываются
раздельно. Новых viewer-процессов ревью не запускало.

Аудит runtime workflow: все 131 прежних required test names сохранены,
теперь 141; reader loop добавляет четыре FBX к прежним 44 чтениям на ОС.
Это локальная проверка diff. Удалённый CI будет проверен после публикации
на точном head PR, затем отдельно на merge SHA.
