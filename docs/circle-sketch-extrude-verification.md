# §25J — реализация и локальная проверка

2026-09-15. База `99fdbadb41a348d125684059a7943e390720c8b5`, PR #37 MERGED
(2026-09-15T04:48:53Z, ветка `line-pair-orientation`). После
`git fetch --all --prune` HEAD/main/origin/main совпадали, дерево было чистым.
Ветка `circle-sketch-extrude` (без префикса `codex/`). CI точного merge SHA
перепроверен с полной пагинацией: **27/27 check runs success**, 6/6 workflows
(CI, combined runtime layout, planegcs pin, product sbom, rust sbom,
rust notices). Это **CI базы**; CI нового незакоммиченного diff не запускался и
базе не приписывается. Чужой detached worktree
`/Users/drt/.codex/worktrees/a200/ferrite-cad`, fixtures и чужие изменения не
затрагивались; ничего не staged, не commit, не push.

Этот абзац фиксирует передачу автора до ревью. Дополнения независимого ревью
и его локальные результаты приведены ниже; CI опубликованных head/merge
фиксируется отдельно по точным SHA в PR.

## Предметный результат

[Контракт и рецепт](circle-sketch-extrude.md).

Пользователь задаёт центр XY, радиус и высоту и получает новый сохранённый
`.fcad` с настоящей окружностью Sketch и параметрическим цилиндром Extrude —
одной предметной операцией из UI и CLI, одним общим jobs-маршрутом.

**Что осталось аналитическим.** Document хранит `SketchGeometry::Circle
{ center, radius }` с собственным `StableEntityId` — не тесселированный payload
и не вторая форма persistent circle; схема БД не менялась.

Kernel получил третью форму кривой `SegmentGeometry::Circle` — **замкнутую**.
`start()`/`end()` для неё отказывают («a closed curve has no endpoints»), так
что придумать вершину нельзя даже случайно. `ProfileLoop` стал двумя явными
формами вместо ослабленной проверки длины:

* `ProfileLoop::new` — прежняя цепь, ≥ 2 сегментов, столько же углов; она теперь
  дополнительно отказывает замкнутой кривой внутри цепи;
* `ProfileLoop::closed_curve` — один замкнутый curve, `joints()` пуст,
  `is_closed_curve()` истинно; открытый сегмент здесь отказывается.

`ProfileJoint(id, id)`, фиктивных curve UUID и паник нет. Cache key различает и
вид кривой (`circle` рядом с `line`/`arc`), и вид loop (`closed-loop` рядом с
`loop`); байты для прежних Line/Arc цепей не изменились, поэтому старые ключи не
сдвинулись. Mock kernel рисует каждую кривую хордой и потому **явно отказывает**
замкнутой, вместо того чтобы выдумать углы.

Native bridge: добавлено значение `FC_OCCT_SEGMENT_CIRCLE = 2` в прежний enum,
**в прежнем `FcOcctSegment`** — ни одно поле не добавлено и не переставлено,
layout и ABI не изменились. После ревью размер 80, alignment 8 и offset каждого
поля проверяются с обеих сторон: C++ `static_assert` и Rust `offset_of!`.
Профиль — либо цепь ≥ 2 сегментов, либо ровно одна окружность; окружность в цепи
отказывается. Для окружности не строится ни одной собственной вершины: OCCT
получает `BRepBuilderAPI_MakeEdge(gp_Circ)`, и число углов записи равно нулю, а
не числу сегментов — `fc_occt_extrude_sweep_edges` и
`fc_occt_extrude_cap_vertices` отказывают на любом joint index такой формы. Шов
OCCT наружу как вершина Sketch не сообщается. Добавлен один аддитивный
`fc_occt_face_surface` (`plane`/`cylinder`/`other` + радиус), читающий
`BRepAdaptor_Surface` — это и есть способ отличить аналитический цилиндр от
веера плоскостей. `FaceSurface` живёт в `ferritecad-kernel`, поэтому stub-сборка
компилируется и отказывает, а не теряет тип. Исходники и pin OCCT не менялись;
изменённый bridge собран существующим pinned OCCT 8.0.1. **Pin-workflow — ручной
выбор версии; он здесь не запускался и не считается обязательным из-за C++ diff.**

История привязана к настоящему Circle UUID: `ExtrudeSide { profile_segment }` +
`AllDerivedFrom { ancestor }` и два `ExtrudeCap { side }`. Ни индекса грани, ни
порядка тесселяции.

Jobs: одна классификация `NewDocument::needs_kernel()` — kernel-free
`create_document` отказывает обоим нарисованным профилям («drawn profile
creation requires a checked kernel route»), checked route строит и проверяет
тело **до** публикации (`check_profile`), empty/sample по-прежнему создаются без
ядра. Переиспользованы прежние preflight → build → cold rebuild → refs → close →
atomic publish, no-clobber, cancellation и late-cancel policy; нового writer,
lifecycle, emitter или схемы нет.

CLI: `create-circle-extrude request.json -o new.fcad [--json]`, request v1
`{"schema_version":1,"center_mm":[x,y],"radius_mm":N,"height_mm":N}` — точное
написание, bounded чтение 65536 bytes, `deny_unknown_fields`, UTF-8 paths,
прежний единственный fallible emitter, `operation:"create-circle-extrude"`,
один объект + LF, коды 0/2/7. Text-режим вызывает тот же job.

UI: в том же окне `Sketch + Extrude — new document` появился выбор
`Profile: Line polygon | Circle`, численные Center X/Y, Radius, Blind height и
`Save circle extrusion…`. Тот же `pending` канал и тот же async Open после
подтверждённой публикации — второго документного lifecycle нет. Line-черновик
переживает переключение режима и возвращается целым.

Прежние Line-редакторы окружность **не** получили: `inspect` показывает такой
Sketch как `editable:false` и `constraint_edit.available:false` с прежним
сообщением «sketch edit requires 3..256 unconstrained Line segments», поэтому
coordinate drag, Snap и constraint-редактор её по-прежнему отказывают.

## Native проверки текущего diff

Последовательные сборки, `CARGO_BUILD_JOBS=1`, tests `--test-threads=1`,
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1`, GPU unset,
loader-failure probes unset. Env/target: `/private/tmp/ferrite-25g/native-env.sh`,
`/private/tmp/ferrite-24b-native-target` — оба существовали и указывают на
реальные pinned библиотеки (`vendor/install` OCCT 8.0.1, `vendor/planegcs`).
DYLD экспортирован в той же Bash-сессии. Peer CLI собран перед app worker
gates. Команды — `/private/tmp/ferrite-25j/native-tests.sh`.

| Проверка | Результат | Лог в `/private/tmp/ferrite-25j/` |
| --- | --- | --- |
| fmt, workspace clippy all-targets/all-features `-D warnings`, `git diff --check` | pass, 0 warnings | `fmt.log`, `clippy.log` |
| `ferritecad-kernel` + `ferritecad-topology` | **192 passed**, 0 skips | `kernel-tests.log` |
| `ferritecad-occt` (реальный bridge, FFI) | **125 passed**, 0 skips | `occt-tests.log` |
| document/jobs/eval `--all-features` | **377 исполнено**, 3 stub-only N/A, 1 прежний ignored benchmark (380 harness-passed) | `domain-tests.log` |
| CLI binary/circle/sketch/edit/JSON/validate/export | **59 passed**, 0 skips | `cli-tests.log` |
| App `sketch::` | **23 passed**, 0 skips | `app-sketch__.log` |
| App `creates::` / `edits::tests::` / `constraints::tests::` | **17 / 8 / 15 passed**, 0 skips | одноимённые логи |
| licence headers (336 файлов), export boundary, native inventory (58 checks), shellcheck, actionlint | pass | `native-inventory.log` |

Новые named gates, исполненные по exact name с `--nocapture` (чистая строка
`test … ok`):
`native_circle_extrude_process_analytic_geometry_and_delivery`,
`native_circle_height_copy_keeps_the_circle_and_every_identity`,
`ffi::tests::a_circular_profile_builds_an_analytic_cylinder_and_names_no_corner`,
`ffi::tests::the_bridge_refuses_a_circle_mixed_into_a_chain`,
`ffi::tests::the_segment_and_surface_numbers_match_the_c_header`,
`sketch::tests::native_circle_draft_and_cli_publish_equivalent_models`.
Ревью добавило в этот же шаг существующий widget gate
`sketch::tests::circle_widgets_submit_exact_numbers_and_keep_the_polygon_draft`:
итого семь circle gates на каждой ОС, без нового workflow.
Они добавлены в существующий `combined runtime layout` workflow одним шагом,
который идёт на всех трёх ОС; нового тяжёлого workflow нет. Прежние gates не
ослаблялись.

## Геометрия: аналитический B-Rep и тесселяция — врозь

Всё ниже прочитано из **фактически переоткрытого документа**, а не из
request/result DTO.

Аналитический B-Rep (реальный OCCT, после `rebuild_cold` переоткрытого файла):

| Документ | грани | боковая поверхность | cap | объём B-Rep mm³ |
| --- | ---: | --- | --- | ---: |
| центр (12, −7), r 10, h 15 | 3 | `Cylinder { radius: 10 }` | 2 × `Plane` | π·10²·15 = 4712.38898 |
| центр (−3.5, 4.25), r 6.75, h 2.5 | 3 | `Cylinder { radius: 6.75 }` | 2 × `Plane` | π·6.75²·2.5 = 357.84704 |
| та же модель после `edit-extrude` 15 → 25 | 3 | `Cylinder { radius: 10 }` | 2 × `Plane` | π·10²·25 = 7853.98163 |

Допуск объёма — относительный `1e-6`. Второй центр с **дробным** радиусом
существует именно затем, чтобы захардкоженный цилиндр не прошёл: направленная
поломка ниже подтвердила, что он этот случай и ловит. Все три `TopologyRef`
разрешаются после cold reopen, `shape_count() == 1`, и после теста
`live_shape_count() == 0` — handles освобождаются и на отказах.

Тесселированный экспорт читается **отдельно** и никогда не выдаётся за
аналитику. Первоначальный запуск с настройками по умолчанию дал 396
треугольников и 4709.28891 mm³ для r 10, h 15. Первоначальная оценка числа
сторон из общего числа треугольников была неверна: крышки тоже триангулируются,
а регулярный многоугольник не даёт нижнюю границу для произвольных углов.

Ревью заменило её независимым чтением фактических граничных вершин STL.
Тест и рецепт явно запрашивают linear deflection 0.05 mm и angular deflection
0.1 rad. Последовательные угловые промежутки проверяют ошибку хорды через
`r·(1−cos(gap/2))`; объём сравнивается с площадью реального вписанного
многоугольника `r²/2·Σsin(gap)` и с цилиндрами радиусов `max(0,r−0.05)` и `r`.
Учтено округление float32. Bound box проверяется с обеих сторон относительно
центра ± радиус, высоты и заданной linear deflection. Точное π по мешу не
обещается.

`edit-extrude` 15 → 25 через **прежний** copy job: Circle, её UUID, центр и
радиус, все `TopologyRef` (UUID и роли) и SQL-данные сохранены. Ревью заменило
проверку числа object rows сравнением каждой ячейки: меняются только payload
и hash выбранного Extrude; в `meta` допускается изменение modified-времени.
Исходный `.fcad` побайтово не изменился.

UI worker и peer CLI дали **одинаковую геометрию** (побайтово равный STL) при
**закономерно разных** UUID: `read_semantics` сравнивает семантику (имена,
позиции сегментов, роли ссылок) и требует равенства, а идентичности — и требует
различия. FBX двух независимо созданных документов **не** сравнивались: их
identity properties несут собственные UUID; вместо этого оба читаются pinned
ufbx. Ни прочие расхождения геометрии, ни метаданные не нормализовались.

## Процессы, отказы и cleanup

* строгий request: неизвестная версия (`unsupported`), `request_version` вместо
  `schema_version`, отсутствующие поля, лишние `holes`/`points_mm`, центр из
  одного или трёх чисел, строки/bool/null вместо чисел, нулевой, отрицательный
  и слишком большой радиус/высота, и текстовые `NaN`/`Infinity`/`1e999`, которые
  JSON не умеет назвать, — все `input`/`unsupported`, exit 2, **без публикации,
  без scratch и с неизменным каталогом**;
* no-clobber: занятый путь, сам request-файл, его hard link и symlink —
  отказ, содержимое цели не тронуто;
* usage/help остаются clap-текстом, `--json` с ним не смешивается;
* закрытый stdout после публикации — exit 7, **опубликованный документ цел и
  читается** (проверено переоткрытием);
* non-UTF-8 путь отказывается до чтения файла;
* cancellation до публикации, race с появившимся файлом, cleanup scratch и
  late-cancel — прежний `create_checked`, который тот же для обоих нарисованных
  профилей; его границы проходит polygon gate, а circle отдельно доказывает, что
  ядро действительно открывается и его вердикт решает;
* каталог после native gate содержит ровно ожидаемые файлы — ни scratch, ни
  `.fcad-cache`, ни SQLite sidecars.

## Настоящий stub

Отдельный target `/private/tmp/ferrite-25j-stub-target`, env
`/private/tmp/ferrite-25j/stub-env.sh` (`OpenCASCADE_DIR`/`FCAD_PLANEGCS_DIR` в
несуществующие каталоги, DYLD/LD сняты, `CMAKE_TOOLCHAIN_FILE` убирает
`/opt/homebrew` из `find_package`). Отсутствие env само по себе доказательством
не считалось: подтверждено, что `CMakeCache.txt` содержит
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`, а `otool -L` собранного
`ferritecad` не показывает **ни одного** TK/OCC/planegcs import.

| Stub-проверка | Исполнено | Explicit skips |
| --- | --- | --- |
| `ferritecad-cli --test circle_extrude` | 3 | **2** native-only |
| `ferritecad-cli --test sketch_extrude` | 3 | **1** native-only |
| `ferritecad-jobs create::` | 16 passed | 0 |
| `ferritecad-kernel` | 110 passed | 0 |
| `ferritecad-eval` | 142 | **39** native-only |
| `ferritecad-app sketch::` | 19 | **4** native-only |

Skips перечислены поимённо в `/private/tmp/ferrite-25j/stub-*.log` через
`--nocapture` и относятся только к native-геометрии. Итого 293 реально
исполненных проверки и 46 явных skips; skips вычтены из harness-passed. Harness-passed со skip
доказательством геометрии не считается — геометрию доказывает native-таблица
выше. Ключевое: в stub-сборке
`circle_alias_and_stub_kernel_refusals_preserve_storage` **исполнился и прошёл**,
то есть сборка без ядра отказывает (`unsupported`, exit 2, exit 7 при закрытых
pipes) и **не публикует геометрически непроверенный circle document**.

## Чувствительность проверок

Две узкие направленные поломки **скомпилировались и дали исполнившиеся
провалы**; скрипты — `/private/tmp/ferrite-25j/mutate-*.py`:

1. **Потеря радиуса**: `populate_circle` записывает `radius: 10.0` вместо
   `circle.radius_mm()`. Request, wire, UUID и первая измеренная модель
   (радиус 10) остаются верными — и именно второй документ с дробным радиусом
   поймал подмену: `native_circle_extrude_process_analytic_geometry_and_delivery`
   провалился на `left: 10.0, right: 6.75` (**4 passed / 1 failed**,
   `mutation-1.log`).
2. **Обход обязательного kernel-check**: `create_document_with_kernel` считает
   `drawn = false`, то есть перестаёт открывать ядро для нарисованного профиля.
   `drawn_profiles_need_the_checked_route_and_stored_models_do_not` провалился с
   опубликованным `unchecked.fcad`, которого не должно было существовать
   (**15 passed / 1 failed**, `mutation-2.log`). Тот же обход в stub-сборке
   опубликовал бы непроверенный документ, что ловит stub-gate выше.

Источник восстановлен **побайтово** (sha256
`07d826ce95a8ee15c2b3234c7bd3c595ff84f0cf9f026c34aa31debc36a68d9c` для
`create.rs` до и после), и весь положительный campaign повторён целиком.
Compile failure, 0 tests и skip пойманной поломкой не считались. Новой общей
mutation/DTO/test-инфраструктуры не создавалось.

## Рецепт и pinned reader

Рецепт извлечён из Markdown скриптом
`/private/tmp/ferrite-25j/extract_recipe.py` по маркеру `FCAD_25J_AGENT_RECIPE`
и исполнен настоящим CLI без разбора человеческого stdout
(`agent-recipe.log`): create → inspect → validate → cold rebuild → STL/FBX →
правка высоты → повторные экспорты → отказы. Он печатает `analytic_mm3`
(π r² h, вычисленное им самим) и `mesh_mm3` (интеграл по STL), и проверяет
вписанную полосу, а не точное π: 4712.38898 против 4709.28891 для r 10 h 15 и
7853.98163 против 7848.81485 после 15 → 25.

Свежий pinned ufbx (`fetch_ufbx.sh`, commit `fcc5d6ba…`, ufbx 0.23.0) собран
заново в `/private/tmp/ferrite-25j/read_production`. Четыре новых небольших
артефакта — `circle.fbx`, `circle-taller.fbx`, `circle-ui.fbx`, `circle-cli.fbx`
(≈72 KB каждый) — реально прочитаны им локально
(`FCAD_PRODUCTION_FBX_UFBX_EXECUTED checks=6 failures=0` на каждом) и добавлены
в **существующий** reader loop `tools/check-fbx-complex.sh` через
`FCAD_CIRCLE_FBX_DIR`, чтобы в CI они не просто сохранялись без чтения.

## Что не проверялось

* **Интерактивный GUI** не запускался: ни окна, ни event loop, ни bundle, ни
  диалогов, ни CUA/osascript, ни Unity, ни GPU/render tests. Headless egui
  widget tests за GUI не выдаются.
* **Удалённый CI этого diff** не выполнялся: изменения оставлены
  unstaged/uncommitted, push/PR не делались. Приведённые 27/27 и 6/6 — это CI
  **базы** `99fdbad`. Новый шаг `combined runtime layout` и дополненный reader
  loop проверены локально (actionlint, shellcheck, те же команды в debug), но на
  трёх ОС GitHub не исполнялись.
* **OCCT pin workflow** не запускался: это ручной выбор версии, изменения
  исходников или pin здесь не потребовалось, и C++ diff сам по себе его не
  требует.
* **Большой STEP/complex-FBX корпус** локально не дублировался — изменение не
  выявило для него конкретного риска; прежние remote gates сохраняются.
* Причина прежнего OOM неизвестна и здесь не исследовалась.

## Независимое ревью, 2026-09-15

Логи: `/private/tmp/ferrite-pr38-review/`. Воспроизведён реальный дефект:
нажатие Cancel draft во время circle save стирало черновик. Расширенный
существующий widget test с настоящими egui pointer events сначала исполнился
и упал (`circle-lock-failing-first.log`, exit 101), затем прошёл после
блокировки формы на время worker. Проверены также переключение режима и
повторный Save; ни потери черновика, ни второго запроса нет.

ABI теперь проверен по каждому полю в скомпилированном C++ и Rust. Правка
высоты сравнивает все SQL-ячейки, допуская только payload/hash выбранного
Extrude и modified timestamp. Дополнительно реально исполнены cache miss и
cache hit в разных kernel/SQLite сессиях с проверкой аналитических поверхностей,
объёма и persistent refs, освобождения handles и отсутствия sidecars.

Первый независимый native прогон: kernel/topology 192, OCCT 125,
document/jobs/eval 377 исполненных + 3 stub-only N/A + один старый ignored
benchmark, CLI 59, headless app 67. После исправлений повторены fmt, workspace
clippy all targets/features `-D warnings`, OCCT 125, свежая сборка CLI/app,
circle CLI 5, app sketch 23 / create 17 / edit 8 / STL 4 — без native skips.
В настоящем stub повторены все шесть групп таблицы: 293 исполненных, 46 skips;
CMakeCache NOTFOUND и отсутствие OCCT/PlaneGCS imports проверены независимо.

Свежий pinned ufbx прочитал все четыре circle FBX: по 6 checks, 0 failures.
Текущий рецепт извлечён из Markdown и выполнен; при linear 0.05 mm/angular
0.1 rad оба STL содержат 500 треугольников. Для h 15 измерено
4710.43619 mm³, для h 25 — 7850.72698 mm³; аналитические объёмы соответственно
4712.38898 и 7853.98163 mm³. Это результаты явных настроек, а не изменение
аналитической геометрии. Первая попытка reader-скрипта ошибочно ожидала старые
constraint FBX в новом каталоге и отказала File not found; она не засчитана.
Исправленный скрипт прочитал четыре новых артефакта и выполнил рецепт полностью.

Export boundary, native inventory (58 checks), licence headers, actionlint,
shellcheck и whitespace прошли. Два новых Rust файла имеют MIT headers.
В runtime сохранены прежние 108 named executions и добавлены семь на каждой
ОС: ожидается 129. Новый reader marker обязателен; ожидаются 25 strict ufbx
readings на ОС. Эти числа — требования CI, а результат удалённого исполнения
записывается отдельно в PR после публикации.

Окна и GPU не запускались. Сборки последовательные; повторное наблюдение памяти:
free 46%, swap 2757.19 MiB без роста, собственных viewer процессов нет.
Свободно около 20 GiB диска; чужие кеши и worktrees не очищались. Причина
исторического OOM по-прежнему не установлена.
