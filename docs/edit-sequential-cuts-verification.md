# §26D — локальные доказательства

2026-09-18, macOS arm64. Ветка `edit-sequential-cuts`, база
`f5160df2808b2776f362d28b234e4c006770eebd`. Изменения оставлены unstaged и
uncommitted. Push/PR/merge не выполнялись; следующий шаг — независимое ревью.
Новый удалённый CI не запускался. [Контракт и рецепт](edit-sequential-cuts.md).

## Проверенная база

Fresh `git fetch origin`, затем `git rev-parse HEAD main origin/main` дали
одинаковый точный SHA выше; дерево было чистым. `gh pr view 46 --json
state,mergeCommit` подтвердил MERGED и тот же merge SHA. `gh run list --commit
f5160df2808b2776f362d28b234e4c006770eebd` показал шесть success workflows:
35341425892, 35341425783, 35341425781, 35341425814, 35341425833, 35341425829.
Все 27 check-runs SHA успешны.

Реальные logs runtime run 35341425892 скачаны `gh run view ... --log` и
сверены с exact required names, извлечёнными из workflow базы. На **каждой**
из macOS/Windows/Linux: 141/141 исполненных обязательных имён, 48 pinned ufbx
чтений, 0 failures и 0 skip markers этих gates. Это доказательство базы.
Новый workflow сохраняет все 141 имени, добавляет 6 (итого 147); reader loop
добавляет четыре небольших FBX к 48 прежним (52 на ОС при будущей CI сборке).
Локальное исполнение новых macOS argv не подменяет удалённый CI diff.

## Реализация и границы

Политика refs записана в контракт до изменения Rust. Один `saved_history`
проверяет все шесть/восемь объектов, полную predecessor chain, точный набор
рёбер, оба сохранённых инструмента и значения refs. Общие validation и
reference construction используются add/discovery/edit. Владелец Body
по-прежнему выводится из history, вторых ownership edges нет.

`previous_feature_id` теперь берётся из отдельного immediate predecessor,
а не из plate base. UI/CLI — прежние thin adapters, worker и copier.
При появлении первого дна добавляются две ссылки в разных producer namespaces;
при появлении второго — одна. Pocket→through отказывает при подготовке со
всеми защищаемыми UUID. Старые refs не переписываются. Writer re-derivation
и SQL allowlist сохранены; проверка duplicate new IDs добавлена в транзакцию.
Все используемые capabilities уже требуются поддержанным источником, их rows
в проверенной матрице остаются побайтово прежними. Архив v3, map, resolver,
predecessor-qualified cache key и геометрическое ядро не менялись.

## Геометрия и SQL

32 случая: оба выбранных звена × четыре исходные пары глубин
12/12, 12/7, 4/12, 4/7 × center-only/radius-only/depth-only/combined.
Плита 80×50×12; инструменты `(20,20),r5` и `(55,30),r7`. Сдвиг выбранного
центра `(+2.125,-1.25)`, радиус `+0.625`, новая глубина первого 3.25,
второго 5.5. Центры расположены наклонно; радиусы и pocket depths различны.
Физические SQL rowids отрицательные, ordinal обращены, все display names
равны `same`. Соответствия выбираются только по UUID/связям.

Аналитический B-Rep объём сверяется с
`80*50*12 − π*r1²*d1 − π*r2²*d2` с абсолютным допуском 1e-6 mm³, независимо
от тесселяции. `measure` сначала разрешает конкретную сохранённую ссылку,
затем измеряет полученный handle: surface type, радиус, координаты стенок,
z-span, положение каждого дна/наружных cap и нормали. Проверены исторические
producer и все final faces, одно уникальное значение на face в final producer.

На каждой правке: cold, старый настоящий sidecar (Hit/Miss/Miss для первого,
Hit/Hit/Miss для второго), повторный Hit/Hit/Hit, отдельный свежий sidecar
Miss/Miss/Miss, затем Hit/Hit/Hit. Восстановление origin names не возвращает
старые floor/shape. Повторный допустимый edit не добавляет refs.

Каждая SQL-ячейка всех таблиц сравнивается с allowlist: два выбранных
payload/hash, meta.modified_at, ровно 0/1/2 добавленных refs. Проверяются
старые ref rows вместе с rowid, object/curve UUID, document identity, deps,
Body tip, previous/profile, optional capability row 900, прочие rowids и
посторонняя BLOB-таблица. Источник после каждого вызова byte-for-byte прежний.

Независимый binary STL parser проверяет обе полости: inward winding,
z-span, наличие/отсутствие дна, парные directed edges, bbox и объём.
Граница mesh volume — между аналитическим значением и значением с радиусами
`r−0.05`, плюс 1e-3 mm³ на float rounding; deflection 0.05 mm, angular 0.1 rad.
Это отдельная оценка тесселяции, не B-Rep volume.

Измеренные B-Rep объёмы (mm³); в скобках число добавленных refs:

| Исходные d1/d2 | Правка | Выбран первый | Выбран второй |
|---|---|---:|---:|
| 12/12 | центр | 45210.265723612 (+0) | 45210.265723612 (+0) |
| 12/12 | радиус | 44959.920059029 (+0) | 44865.672279422 (+0) |
| 12/12 | глубина | 45897.489116585 (+2) | 46210.862983781 (+1) |
| 12/12 | всё | 45829.687165760 (+2) | 46052.924321860 (+1) |
| 12/7 | центр | 45979.955923742 (+0) | 45979.955923742 (+0) |
| 12/7 | радиус | 45729.610259159 (+0) | 45778.943081297 (+0) |
| 12/7 | глубина | 46667.179316715 (+2) | 46210.862983781 (+0) |
| 12/7 | всё | 46599.377365890 (+2) | 46052.924321860 (+0) |
| 4/12 | центр | 45838.584254330 (+0) | 45838.584254330 (+0) |
| 4/12 | радиус | 45755.135699469 (+0) | 45493.990810140 (+0) |
| 4/12 | глубина | 45897.489116585 (+0) | 46839.181514499 (+1) |
| 4/12 | всё | 45829.687165760 (+0) | 46681.242852578 (+1) |
| 4/7 | центр | 46608.274454460 (+0) | 46608.274454460 (+0) |
| 4/7 | радиус | 46524.825899599 (+0) | 46407.261612015 (+0) |
| 4/7 | глубина | 46667.179316715 (+0) | 46839.181514499 (+0) |
| 4/7 | всё | 46599.377365890 (+0) | 46681.242852578 (+0) |

## Отказы и GUI worker

Исполняемые отказы: protected floors, совпадение/пересечение/вложенность,
касание и gap 0.5e-7, выход за стенку, неверные feature/curve UUID, stale
version, занятый output, source/hardlink/Unix symlink. Kernel-free discovery
отказывает constrained base и обоим constrained tools, лишнему ребру,
reversed/Add/ThroughAll в истории. Старые проверки третьего Cut сохранены.

Для обоих выбранных звеньев cancellation достигает 0.1/0.4/0.95; source race
реально вносится при progress≥0.95 после двух rebuild. Нет публикации,
scratch удалён, kernel live_shape_count=0. Отдельный закрытый stdout/stderr
даёт exit 7 после публикации, сохранённая копия холодно измеряется.
Stub kernel-first отказ отдельно и не выдаётся за поздний version guard.

Headless egui widgets выбирают оба Cut, проверяют точные исходные десятичные
числа в первом Undo, один Apply/один шаг, Redo, видимый overlap refusal,
Save Cancel, настоящий stale worker failure, чужой completion и отказ async
Open с сохранённым draft. Успешные app worker и CLI одного request дают
побайтово одинаковые STL и FBX. Peer CLI пересобирается до app comparisons.

## Направленные временные поломки

Локальный `/private/tmp/ferrite-26d/mutations.py` (не отдельный framework)
поочерёдно внёс две компилируемые поломки:

1. Для второго Cut заменён входящий target key константой, поэтому правка
   первого не инвалидирует final результат. Exact geometry gate упал на
   `dependent chain must invalidate exactly`: Hit/Miss/Hit вместо Hit/Miss/Miss.
2. Protected-floor transition заменён на разрешённый Kept. Exact document
   gate упал на `expect_err("protected floor")` при подготовке.

Каждый результат: exit 101, 0 passed, **1 failed**, исполненный assertion,
не compile failure/нулевой запуск. Файлы восстановлены byte-for-byte,
после каждой мутации положительный exact gate прошёл. SHA-256 при восстановлении:

- eval/cold.rs: `a19e45b5dc94fb5235877e7785954dbe928bed4f401e6601bd176b9fa3068509`;
- document/cut_edit.rs: `c129b935c67c372b312681b468624115e15ffd0162946236da0ea22c3a8f75b3`.

Позднее в cut_edit.rs изменены только поясняющие комментарии о числе имён.
В eval/cold.rs финального diff нет. Логи и snapshots восстановления сохранены
в scratch; baseline failing-first всего набора не заявляется.


## Команды и учёт прогонов

Все команды из корня репозитория. `/private/tmp/ferrite-26d/env.sh` создан
заново после проверки vendor paths и задаёт:

```sh
export CARGO_BUILD_JOBS=1 CMAKE_BUILD_PARALLEL_LEVEL=1
export CARGO_TARGET_DIR=/private/tmp/ferrite-24b-native-target
export OpenCASCADE_DIR="$PWD/vendor/install/lib/cmake/opencascade"
export CMAKE_PREFIX_PATH="$PWD/vendor/install"
export FCAD_PLANEGCS_DIR="$PWD/vendor/planegcs"
export DYLD_LIBRARY_PATH="$PWD/vendor/install/lib:$PWD/vendor/planegcs"
export FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1
```

Исполненный core:

```sh
cargo test --release -p ferritecad-document -p ferritecad-topology \
  -p ferritecad-eval -p ferritecad-jobs --features ferritecad-eval/planegcs \
  -- --test-threads=1
```

507 harness-passed, 0 failures, один прежний ignored manual timing
`edit::tests::measure_extrude_catalog_read`. Независимый повтор с `--nocapture`
выявил внутри этого числа два stub-only теста, которые в native-сборке явно
пропускаются: `a_build_with_no_solver_publishes_no_facts` и
`a_build_with_no_solver_refuses_before_the_kernel`. Поэтому реально исполнены
505 core tests; эти два отказа без solver не считаются проверенной геометрией.
Headless `cargo test --release -p ferritecad-app --features planegcs
cuts::tests:: -- --nocapture --test-threads=1`: 7 passed, без skips.
После добавления подсказки соседнего инструмента новый UI exact gate повторён
через извлечённый workflow и прошёл. Финальный полный app cuts также повторён:
7/7, 0 skips (`ui-final.log`), без прибавления повторов к общему счёту. Сопутствующий `solver_info` binary с нулём
совпавших tests не засчитывается как исполненный gate.

Из фактического YAML извлечены без переписывания argv run-блоки (заменено только
matrix.name на macos):

- `packed-native.sh`: пять новых gates, exact-name success и no-skip;
- `packed-mixed.sh`: весь существующий mixed step с новым gate;
- `packed-stub.sh`: два новых структурных/writer gates.

`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1` сохранялись; core packages
без solver feature получали `FERRITECAD_REQUIRE_PLANEGCS=0`. Перед app worker
сравнениями в блоке выполнен настоящий `cargo build --release --features
planegcs -p ferritecad-cli`. Все shell scripts вызывают реальный cargo с
исходными argv. Полные логи в `/private/tmp/ferrite-26d/`; повторные packed
прогоны не суммируются с основной матрицей как новые тесты.

При разработке были два compile-only отказа тестового кода: ContentHash вместо
DocumentVersion и неверное имя метода writer (`put_dependency`). Они исправлены;
это не негативные геометрические доказательства. Мутационные падения выше —
отдельные успешно скомпилированные исполнившиеся assertions.


Полный CLI прогон:

```sh
cargo test --release -p ferritecad-cli --test circular_cut \
  --test edit_circular_cut --test print_topology --features planegcs \
  -- --nocapture --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

CLI: 13 + 7 + 8 = **28 passed**, без failures/skips. Вместе с core и app cuts
это 542 harness-passed, из них 540 исполненных tests и два stub-only пропуска;
packed/mutation повторы отдельно.
Clippy прошёл после замены индексного test loop на iterator/enumerate;
первый lint отказ не выдан за успешный прогон.

`actionlint .github/workflows/ci.yml .github/workflows/runtime-layout.yml`,
`shellcheck tools/check-fbx-complex.sh`, `bash tools/check-export-boundary.sh`,
`bash tools/check-licence-headers.sh` прошли. Последний читает tracked files
(357); SPDX нового untracked Rust module проверен отдельно. Нет новых native
ABI inputs, dependencies или STEP fixtures; большой STEP corpus не повторялся.


### Настоящий stub и mixed

Извлечённый mixed step: **6 exact gates passed**, 0 skips. Новый
`sequential::edits::occt_without_solver_edits_both_history_links` реально строит
и измеряет обе двухзвенные копии. `otool -L` фактически исполненного
`circular_cut-a2ac09e525e5b280` подтверждает OCCT и отсутствие planegcs.

Stub использовал `/private/tmp/ferrite-25j-stub-target`, без OCCT/solver paths
и DYLD, `FERRITECAD_REQUIRE_OCCT=0`, `FERRITECAD_REQUIRE_PLANEGCS=0`.
Его CMakeCache содержит `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` и
`CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local;<repo>/vendor/install`.
`otool -L` запущенного `debug/deps/circular_cut-3cb2f3330ca53ea4` показывает
только libiconv/libSystem, без native imports. Два packed gates passed;
отдельно все **20 document cut tests passed**, без skips. Поэтому отсутствие
ядра доказано cache/imports, не одним unset переменной окружения.

После mixed/stub восстановлена combined release сборка:
`cargo build --release --features planegcs -p ferritecad-cli -p ferritecad-app`.
Третьего target нет. Native и stub target проверены перед использованием,
OCCT config сообщает 8.0.1, planegcs PROVENANCE — pinned FreeCAD 1.0.1.

## Рецепт и pinned reader

Проверен pinned ufbx commit `fcc5d6ba444cfd3eb80677dba5e37e493941abe5`
(0.23.0) через штатный `tools/unity-fbx-smoke/scripts/fetch_ufbx.sh` с
обоими SHA-256. Reader заново собран после Rust-прогонов последовательными
`cc -std=c11 -O2` командами из `tools/check-fbx-complex.sh`:
сначала `read_production.c` с `-Wall -Wextra -Werror`, отдельно ufbx.c,
затем link. Из того же существующего shell script извлечён и исполнен новый
reader block: `edit-0-12`, `edit-1-12`, `edit-0-7`, `edit-1-7`, каждый 6/0;
получен `FCAD_EDIT_SEQUENTIAL_UFBX_EXECUTED`. Второй большой STEP corpus не создан.

Markdown-блок `FCAD_26D_AGENT_RECIPE` извлечён regex и реально исполнен:

```sh
export FERRITECAD=/private/tmp/ferrite-26d/stage/FerriteCAD.app/Contents/MacOS/ferritecad
export FCAD_UFBX_READER=/private/tmp/ferrite-26d/read_production
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 /private/tmp/ferrite-26d/recipe.py
```

Получен `FCAD_26D_RECIPE_OK`; STL volumes по восьми копиям:
45830.586450, 46053.731206, 46599.957737, 46053.731206,
45830.586450, 46681.789368, 46599.957737, 46681.789368 mm³.
Все восемь FBX рецепта отдельно приняты тем же reader (6/0 каждый).
Модели: `/var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-edit-sequential-ondtr_px`.

## GUI: точный отложенный smoke

Native CUA доступен. Первое обращение к Finder заняло 806.8 s; позднейший
screenshot показал разблокированный desktop. Непосредственно перед запуском
viewer новая CUA проверка ответила: **“The Mac is locked and automatic unlock
could not unlock it.”** Viewer не запускался, watchdog не запускался;
оконных результатов и memory footprint окна для §26D нет. Headless egui
проверки выше не объявлены GUI smoke. Запрашивать разблокировку и ждать её
вместо продолжения headless работы не потребовалось.

До этой проверки создан свежий штатно staged bundle
`/private/tmp/ferrite-26d/stage/FerriteCAD.app`. Для обоих release executables
исполнен `tools/runtime-closure.sh` с pinned OCCT/planegcs search paths:
0 unexpected libraries, 50 OCCT + 1 planegcs. `tools/stage-runtime-layout.sh`
использовал productVersion из inventory; `codesign --verify --deep --strict`
прошёл. `--solver-info` при unset DYLD сообщил available и pinned FreeCAD 1.0.1
provenance. Полный stage command script: `/private/tmp/ferrite-26d/stage.sh`.

После ручной разблокировки повторить native screenshot; проверить отсутствие
другого viewer. Затем ровно один собственный viewer:

```sh
unset DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
python3 tools/watch-viewer-memory.py \
  --log /private/tmp/ferrite-26d/gui-memory.jsonl --limit-mib 1536 --seconds 1800 \
  -- /private/tmp/ferrite-26d/stage/FerriteCAD.app/Contents/MacOS/ferritecad-viewer \
  /var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-edit-sequential-ondtr_px/two-12-12.fcad
```

1. Edit первого `01a0b51e-3c6a-77c3-96c8-d24ac5a0e590` (оба display names
   `same`, выбирать UUID). Ввести `(22.125,18.75), r5.625, d3.25`; Apply →
   Undo (точно `(20,20),5,12`) → Redo → Save Cancel → снова Save в
   `gui-first.fcad` рядом с источником. Проверить async Open/title и оба Cut.
2. Edit второго `01a0b51e-3ce1-7d92-971e-840f74a18a8a` в открытой копии.
   Ввести центр `(22.125,18.75), r7.625, d5.5`, Apply: видимый overlap refusal,
   draft остаётся. Исправить центр на `(57.125,28.75)`, Apply → Save
   `gui-second.fcad`, проверить async Open. Первый pocket должен сохраниться.
3. Следующим блоком создать CLI peer второй правки из **того же** gui-first
   и сравнить оба GUI результата с CLI. Block не исполнен: GUI файлов нет.

```sh
export FERRITECAD=/private/tmp/ferrite-26d/stage/FerriteCAD.app/Contents/MacOS/ferritecad
export FCAD_GUI_MODELS=/var/folders/yd/qbk3h_lj5xzckgq2dqf74tcw0000gn/T/ferrite-edit-sequential-ondtr_px
python3 - <<'PY'
import os, pathlib, json, subprocess, hashlib
cli=os.environ['FERRITECAD']; root=pathlib.Path(os.environ['FCAD_GUI_MODELS'])
def run(args):
    p=subprocess.run([cli,*map(str,args)],capture_output=True,text=True)
    assert p.returncode==0,(p.stdout,p.stderr)
    return json.loads(p.stdout)
source=root/'gui-first.fcad'
catalog=run(['inspect',source,'--json'])['result']
second=next(f['circular_cut_edit']['saved'] for f in catalog['features']
    if f['feature_id']=='01a0b51e-3ce1-7d92-971e-840f74a18a8a')
request=root/'gui-second-peer.json'
request.write_text(json.dumps(dict(request_version=1,tool_curve_id=second['tool_curve_id'],
    center_mm=[57.125,28.75],radius_mm=7.625,depth_mm=5.5)))
run(['edit-circular-cut',source,'--feature',second['feature_id'],'--expect-version',
    catalog['content_version'],'--request',request,'-o',root/'cli-second.fcad','--json'])
for gui,peer in [('gui-first','edit-12-12-0'),('gui-second','cli-second')]:
    for fmt in ['stl','fbx']:
        paths=[]
        for name in [gui,peer]:
            dest=root/f'{name}-comparison.{fmt}'
            run([f'export-{fmt}',root/f'{name}.fcad','-o',dest,'--json'])
            paths.append(dest)
        assert paths[0].read_bytes()==paths[1].read_bytes()
        print(gui,fmt,hashlib.sha256(paths[0].read_bytes()).hexdigest())
        if fmt=='fbx':
            subprocess.run(['/private/tmp/ferrite-26d/read_production','--identity',str(paths[0])],check=True)
PY
```

4. Закрыть окно Cmd-Q, дождаться watchdog exit; проверить peak/abort/swap.
   Не обращаться к app после закрытия способом, который запускает его вновь.
   Причина прежнего viewer OOM не установлена и исправленной не объявляется.


## Ресурсы и итог

Один build/test процесс за раз, обе concurrency переменные равны 1.
Переиспользованы только прежние targets: native 9.4 GiB, stub 4.2 GiB.
Scratch со stage/logs/reader около 97 MiB; временные модели живут отдельно.
В начале и в конце свободно около 74 GiB; промежуточные `memory_pressure -Q`
показывали 66–68% free, финально 68%. Измеренные `vm.swapusage` до/после — 0.
Пиковая память компилятора не измерялась. Финальная проверка процессов не
обнаружила cargo/rustc/viewer. Чужие worktree/cache/processes не изменялись,
cleanup чужих каталогов не выполнялся. Loader failure probes не включались.

Финальные fmt, whitespace, actionlint, shellcheck и аудит CI-имён прошли.
Индекс пуст. Все изменения ниже unstaged/uncommitted; удалённый CI нового diff
не заявляется. Единственное отложенное проверочное действие — оконный smoke
из-за заблокированного Mac; все headless gates, рецепт и pinned reader исполнены.
Следующий шаг — независимое ревью.

## Независимое ревью §26D

Повтор на той же базе: 540 исполненных tests основной native-матрицы,
два явно учтённых stub-only пропуска и один прежний ignored benchmark.
Mixed OCCT/no-solver: все шесть exact gates прошли; imports исполняемого
test binary содержат 50 libTK и не содержат planegcs. Genuine stub:
20 document tests и два exact gates прошли; CMakeCache — NOTFOUND,
imports свежего CLI и test binary — без OCCT/planegcs.

Пять новых combined gates извлечены из изменённого workflow и исполнены
с его argv; peer CLI собран перед app worker. Все прежние 141 required
runtime name сохранены, новый список содержит 147. fmt, workspace clippy
all-targets/all-features, actionlint, shellcheck, export boundary, headers
и whitespace прошли. Публичный Python-рецепт извлечён из Markdown и повторён;
его восемь FBX и четыре файла новой кампании прочитаны свежесобранным pinned
ufbx reader, каждый с `checks=6 failures=0`.

Блокирующих дефектов реализации не найдено. Исправлены комментарий writer
о количестве новых имён и учёт двух stub-only пропусков в native-отчёте.
CUA повторно подтвердил блокировку Mac; viewer не запускался, оконный smoke
остаётся отложенным. Свободно 74 GiB, memory_pressure сообщает 67% free;
тяжёлые сборки и тесты выполнялись последовательно в прежних targets.
Логи независимого повтора: `/private/tmp/ferrite-26d-review/`.
CI будущего head и merge SHA ещё предстоит проверить после публикации.

<!-- DIFFSTAT -->

Ниже diff на передаче, до небольших уточнений независимого ревью выше.

| Файл | + | − | Статус |
|---|---:|---:|---|
| `.github/workflows/ci.yml` | 11 | 0 | modified |
| `.github/workflows/runtime-layout.yml` | 46 | 0 | modified |
| `README.md` | 2 | 3 | modified |
| `crates/ferritecad-app/src/cuts.rs` | 161 | 6 | modified |
| `crates/ferritecad-cli/src/json.rs` | 18 | 1 | modified |
| `crates/ferritecad-cli/tests/edit_circular_cut.rs` | 1 | 1 | modified |
| `crates/ferritecad-cli/tests/support/edit_sequential_cuts.rs` | 583 | 0 | untracked |
| `crates/ferritecad-cli/tests/support/sequential_circular_cuts.rs` | 6 | 1 | modified |
| `crates/ferritecad-document/src/cut_edit.rs` | 470 | 248 | modified |
| `crates/ferritecad-document/src/document.rs` | 5 | 1 | modified |
| `crates/ferritecad-document/src/lib.rs` | 3 | 2 | modified |
| `crates/ferritecad-jobs/src/edit.rs` | 4 | 3 | modified |
| `docs/cli-capabilities.md` | 2 | 2 | modified |
| `docs/cli-json-v1.md` | 11 | 5 | modified |
| `docs/decisions/0004-feature-predecessor.md` | 14 | 0 | modified |
| `docs/edit-circular-cut-copy.md` | 3 | 0 | modified |
| `docs/edit-sequential-cuts-verification.md` | 382 | 0 | untracked |
| `docs/edit-sequential-cuts.md` | 278 | 0 | untracked |
| `docs/implementation-plan.md` | 23 | 0 | modified |
| `docs/sequential-circular-cuts.md` | 3 | 0 | modified |
| `tools/check-fbx-complex.sh` | 12 | 0 | modified |

**21 файлов, +2038/−273; 18 modified, 3 untracked.**
