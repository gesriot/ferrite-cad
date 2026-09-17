# §26A — локальные доказательства и ограничения

Контракт среза: [первый Cut в существующем Body](circular-cut-copy.md).
Решение об истории: [ADR 4](decisions/0004-feature-predecessor.md).

База: `main = origin/main = 0780dc0cb79f685fc1eecd32c6ca4590f5c5e7c3` (merge
PR #43, head `925cbadcdc038467a03c7b3c54d75b18c28dee92`, base
`dd63718ff393ba4b7f886e717d793662b6b9a1fd`), дерево
`59f451446a36490439d28356366e6d960f8b6064`. На этом merge 27/27 check-runs —
success (проверено `gh api .../check-runs` в этой сессии). Ветка
`circular-cut-copy`, 0 коммитов впереди; изменения оставлены
unstaged/uncommitted. **CI нового diff не запускался и пройденным не
объявляется.**

Окружение: macOS (Darwin 24.6.0), aarch64-apple-darwin, Open CASCADE 8.0.1 и
planegcs (FreeCAD 1.0.1) из уже собранных закреплённых деревьев в `vendor/`,
`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `--test-threads=1`, сборки
последовательно. Пересобран **только** собственный C++ bridge — его входы
изменились (`fc_occt_cut`, `fc_occt_cut_carried`, `fc_occt_sub_shape_count`);
OCCT и planegcs не пересобирались, их входы не менялись.

**Про потерю логов.** В середине среза `/private/tmp` был очищен извне: оба
переиспользуемых target (`ferrite-24b-native-target`, `ferrite-25j-stub-target`)
и рабочий каталог с логами исчезли, свободное место скакнуло с 13 до 59 ГиБ.
Репозиторий и `vendor/` не пострадали. Оба target пересобраны, и **каждое** число
ниже взято из повторного прогона после пересборки: полный обход suites, обе
направленные поломки, рецепт, pinned ufbx, stub, смешанная сборка и все три
упакованные CI-команды. Ничего из доочистного прогона в отчёт не переносится.

Диск: 79 ГиБ свободно после пересборки (native target 7.2 ГиБ, stub 2.9 ГиБ);
`vm.swapusage` после очистки сообщает 0 — своп был сброшен вместе с ней, поэтому
сравнение с прежними 2709 МиБ здесь смысла не имеет и не приводится. Чужие
процессы и кэши не трогались; чужой worktree
`/Users/drt/.codex/worktrees/a200/ferrite-cad` не открывался; третий большой
target не заводился.

Логи и артефакты этого среза — в `$S =
/private/tmp/claude-501/-Users-drt-Desktop-github-ferrite-cad/ad55f8d7-a19f-4478-8151-ac8d79aa4162/scratchpad`:
`$S/ci/*.log` (исполненные упакованные CI-команды, suite-сводки, stub),
`$S/recipe.py` (рецепт, извлечённый из Markdown по маркеру),
`$S/read_production` (собственный pinned ufbx reader),
`$S/runner/circular-cut/*.fbx` (два артефакта, прочитанные этим reader),
`$S/pristine/SHA256` (контрольные суммы двух файлов, которые ломались и
восстановлены — `shasum -a 256 -c` проходит).

## Что решено про историю Body

Полностью — в [ADR 4](decisions/0004-feature-predecessor.md); здесь только то,
что проверено исполняемо.

`Extrude.target_body` вместе с `Body.tip_feature` даёт настоящий цикл
`cut → body → cut`, который `evaluation_order` не может упорядочить. Это не
отказ, который снимается: порядка, в котором оба объекта вычисляются после
другого, не существует. Поэтому фича называет **фичу** (`Extrude.previous`,
ребро `DependencyRole::Predecessor`), Body называет свой tip прежним ребром, а
владение не хранится вовсе — Body владеет тем, до чего дотягивается от tip по
`previous`.

Исполняемые подтверждения (`cargo test -p ferritecad-document --lib cut_edit`):

* `a_feature_names_the_result_it_modifies_and_a_body_names_its_tip` — фича несёт
  `previous` и не несёт `target_body`, оба ребра на месте, старое ребро tip
  удалено, документ валиден; записанная фича — payload **v2** с
  `feature.predecessor.v1`, прежняя — v1, и `readable_schema_versions` для
  `Extrude` это `[2, 1]`.
* `naming_a_body_instead_of_a_predecessor_cannot_be_ordered_at_all` — документ
  в старой форме написан руками и **не валидируется**: отчёт содержит слово
  `cycle`. Это и есть причина, по которой появилось новое поле.
* `one_history_per_body_and_one_body_per_tip` — вторая фича над тем же
  результатом даёт `feature.forked-history`.
* `the_writer_refuses_a_prepared_cut_the_document_would_not_have_made` — четыре
  подделки публично изменяемого prepared payload (больший инструмент, сдвинутый
  tip, чужой producer у ссылки, потерянный предшественник) отказываются до
  изменения; честный план после каждой попытки пишется, и документ между
  попытками не меняется.
* `the_supported_class_is_stated_and_anything_wider_says_why` — не-прямоугольник
  и constrained-профиль отказываются со своими причинами.
* `the_tool_must_stay_inside_the_part_by_more_than_the_kernels_own_tolerance` —
  точное касание и всё ближе отказываются; зазор больше допуска принимается;
  глубина, равная высоте, разрешена, больше — нет.

## Ядро и boolean

`GeometryKernel::cut(request, track, context)` возвращает `CutResult` с
`History` по **названным** sub-shape обоих входов, `CarriedOutcome`
(`Kept`/`Modified`/`Deleted`) на каждую и измеренный удалённый объём.
`CutResult::validate` отказывает истории про чужую shape, выходу из другой
shape, «удалено и при этом есть геометрия», «сохранено и геометрии нет» и
неположительному удалённому объёму.

`fc_occt_cut` строит `BRepAlgoAPI_Cut`, проверяет отмену до и после, `IsDone`,
ровно один solid, `BRepCheck_Analyzer` и разницу объёмов. Историю он снимает,
**пока алгоритм жив** — Open CASCADE отвечает про вход только это время, —
и кладёт её в запись результата; `fc_occt_cut_carried` читает её потом. Rust-side
`cut` дополнительно отказывает tracked sub-shape, не принадлежащей ни одному
входу, и индексу, который сессия не выдавала; при любой ошибке сборки результата
созданная shape освобождается.

Измерено на настоящей геометрии (`cargo test -p ferritecad-occt --test cut_occt`,
4 теста, без skips в native сборке):

| Что | Измерено |
| --- | --- |
| сквозной инструмент r5 в плите 60×40×10 | removed = π·25·10, 7 граней, объём 24000−π·25·10 |
| стенка отверстия | `Cylinder { radius: 5.0 }` под **собственной** гранью инструмента |
| грани плиты при сквозном отверстии | 4 `Kept`, 2 `Modified`, 0 `Deleted` |
| крышки инструмента при сквозном отверстии | обе `Deleted` |
| карман r5 глубиной 4 | removed = π·25·4, 8 граней; у инструмента выживают стенка и **одна** крышка (дно), удалена одна |
| промах, уничтожение тела, чужая tracked sub-shape, отмена | отказы `Unsupported`/`Input`/`Cancellation` |
| shape с самой собой и две сессии | отказ `CutRequest::new` до вызова ядра |

## Имена после boolean

`TopologyMap::record_cut` раскладывает результат на три группы, каждая из
настоящей истории:

* грани **инструмента** — под ролями, которые дала им его собственная экструзия
  (`ExtrudeSide` на сегмент, `ExtrudeCap` на конец);
* грани **прежней фичи** — под новыми `CarriedCap`/`CarriedSide`, потому что это
  другая геометрия: `ExtrudeCap{End}` под Cut это дно кармана, а
  `CarriedCap{End}` — верх детали;
* удалённое — как удалённое (`carried_deleted`), и resolver отвечает на такую
  ссылку «эта фича её убрала», что отличается от «такого имени не было».

Ничто не нумеруется по обходу и не выбирается ближайшим. Архив фичи расширен
тремя тегами (9/10/11) и списком удалённых имён; формат поднят до **v2**, потому
что вырос не только словарь, но и раскладка записи — запись v1 отказывается
целиком и пересчитывается, что для кэша и есть правильная цена.

Строгая проверка refs не ослаблена, а усилена: общий `edit_object_copy` теперь
требует, чтобы разрешились и ссылки, которые операция **добавила**, иначе можно
было бы опубликовать геометрию, на которую никто не может указать.

## Измеренная геометрия и identity

Источник: `create-sketch-extrude` плита 60×40×10 из начала координат.
Инструмент r5 в (20, 15).

| Глубина | Граней | Объём (аналитический B-Rep) | Ссылок |
| --- | --- | --- | --- |
| 10 (насквозь) | 7 | 24000 − π·25·10 = 23214.601837 | 10 |
| 4 (карман) | 8 | 24000 − π·25·4 = 23685.840735 | 11 |

В обоих случаях: 6 объектов (плоскость, два Sketch, две фичи, один Body), Body
сохранил свой UUID, его tip — новая фича, прежняя фича по-прежнему `NewBody` без
предшественника и по-прежнему **v1**, новая — `Cut` с предшественником и **v2**;
исходный профиль не тронут; каждая ссылка источника на месте; все ссылки
разрешаются после reopen и cold rebuild; `rebuild --cold` печатает `tip Cut`.
Каждая SQL-ячейка сверена с явным allowlist: две новые строки `objects`,
payload/hash/schema_version выбранной строки Body, новые строки
`deps`/`topology_refs`/`capabilities`, заменённое ребро прежнего tip и
`meta.modified_at`. Источник побайтово цел после каждого прогона.

Независимый разбор binary STL при `--linear-deflection 0.05 --angular-deflection
0.1`: замкнутость (каждое направленное ребро ровно раз, у каждого есть
обратное), стенка отверстия по фактическим хордам вокруг заданного центра,
**ориентация полости внутрь** (иначе это был бы выступ, а не отверстие), дно
кармана на z = 4 и закрытая дальняя сторона против открытой у сквозного,
габариты детали и объём в границах, которые вписанная призма может дать. Рецепт
печатает `FCAD_26A_RECIPE_OK 23214.927292 23685.970917` — оба меш-объёма чуть
больше точных, потому что вписанное отверстие удаляет чуть меньше цилиндра; за
точный B-Rep меш не выдаётся.

FBX обоих частей прочитан **собственным** pinned ufbx (`checks=6 failures=0`
каждый) и через прежний reader loop `tools/check-fbx-complex.sh`, который
напечатал новый маркер `FCAD_CUT_UFBX_EXECUTED` рядом со всеми прежними.
`export-fbx` сообщает `geometries: 1` — экспортируется один конечный Body, не
инструмент и не два промежуточных solid.

## Кэш

Ключ Cut — `cut_cache_key(kernel, target_key, tool_key, tolerance)` с
собственной `ALGORITHM_VERSION`; `target_key` — ключ фичи, чей результат
режется. `Extrude::cache_key` кормит `previous`.

`native_the_cut_is_keyed_by_what_it_cut` — один документ, один sidecar,
изменение **на месте**: Miss/Miss → Hit/Hit → после поднятия высоты детали с 10
до 16 снова Miss/Miss, и объём равен 60·40·16 − π·25·4. Это единственная
расстановка, в которой устаревшая запись действительно могла бы быть отдана.
Отдельно проверено, что cold и cache Miss/Hit дают одинаковые грани, объём и
набор разрешённых ролей, и что Body после Hit — это Cut, а не Extrude.

## Клиенты

CLI `cut-circular-copy`: строгий request v1 из четырёх полей,
`deny_unknown_fields`, bounded чтение 65536 байт, UTF-8 preflight,
`--expect-version`, прежний envelope, коды 0/2/7. Discovery — аддитивное
`cut_edit` у каждой строки `bodies`, включая плоскость, фичу-предшественника,
высоту, габариты, направление и требуемый зазор; работает **без ядра** и без
окна.

UI: действие только у поддерживаемого Body, причина в подсказке у остальных;
форма называет базовую XY-плоскость и +Z, показывает размеры детали и фичу,
которую правка изменит; четыре поля и один подтверждённый `Apply cut` — один шаг
bounded Undo/Redo на все четыре; изменение числа после Apply снова закрывает
Save; Undo/Redo не обращается ни к одному job; Cancel не оставляет ничего.
`native_cut_worker_and_cli_publish_the_same_part` — один проход формы →
настоящий async worker → тот же запрос отдельным процессом CLI: один
document_id, шесть объектов, каждый объект кроме двух новорождённых совпадает
целиком, Body отличается ровно одним идентификатором (своим новым tip), а STL и
FBX побайтово равны.

Окно не запускалось.

## Отказы без публикации

Каждый проверен с остальными корректными аргументами; ни один не оставил файла,
и источник после каждого побайтово цел.

Структурные и числовые (исполняются **без ядра**): нулевой и отрицательный
радиус, нулевая и отрицательная глубина, глубина больше высоты детали,
инструмент, свисающий за край, инструмент, **точно касающийся** стенки,
инструмент мимо детали, радиус за опубликованным пределом, чужой Body,
устаревшая версия, `request_version: 2`, отсутствующее поле, лишнее поле
`plane`, трёхкомпонентный центр, `1e999`, и exit 7 при закрытом stdout на
отказанной операции.

Через ядро: источник как собственное назначение, занятый выход, hard link на
источник, и инструмент, уничтожающий всё тело.

## Сборки

**Native (OCCT + planegcs).** Полный проход по всем крейтам после пересборки:
**1936 исполненных тестов, 0 падений, 0 skips**. По крейтам: types 34, kernel
110, topology 82, exchange 28, document 171, eval 185, jobs 53, occt 133, scene
175, export 60, ui 102, viewport 142, solver-lab 14, viewport-gpu 134, cli 159,
app 320, sketch-solver 34. Новое в этих числах: occt `cut_occt` 4, document
`cut_edit` 6, cli `circular_cut` 6, app `cuts` 2.

**Настоящий stub (нет ни ядра, ни solver).** Доказан:
`OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` в `CMakeCache.txt`, и в imports
тест-бинаря (`otool -L`) **ноль** `libTK*` и **ноль** `planegcs`. После очистки
`/private/tmp` этот target пришлось собрать заново, и здесь всплыла деталь,
которой в прежних протоколах не было: `CMAKE_IGNORE_PREFIX_PATH` — переменная
cmake, а не окружения, поэтому «просто не давать `OpenCASCADE_DIR`» больше не
даёт stub — cmake находит homebrew-сборку OCCT. Каталог bridge-build
сконфигурирован один раз вручную с `-DCMAKE_IGNORE_PREFIX_PATH`, и build script
переиспользует этот кэш; чужая установка homebrew при этом не тронута, и
заведомо сломанные бинарники не запускались. В такой сборке: `circular_cut` — 1
исполненный (discovery и весь протокол отказов), 4 честно пропущенных, 1
пропущен как предназначенный смешанной сборке; app `cuts` — 2 честно
пропущенных; core suites (document/topology/kernel/eval) — 548 исполненных. Заведомо сломанные loader probes не запускались;
`FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался.

**Смешанная (OCCT есть, solver нет), в том же прежнем target.**
`occt_without_solver_cuts_an_unconstrained_part` исполнен по точному имени, без
skips, рядом с двумя прежними mixed-gate: Cut без ограниченного эскиза
действительно не требует solver и даёт ту же деталь.

## Упакованные CI-команды исполнены

Не `bash -n`, а настоящий argv, извлечённый из YAML и запущенный:

* новый шаг «Cut a circular tool into a saved body through CLI and worker» —
  5 CLI gate, 4 kernel gate, 6 history gate и 2 UI gate, каждый по точному имени
  и с проверкой на `skipped:`; шаг дописал `FCAD_CUT_FBX_DIR` в `$GITHUB_ENV`;
* шаг «Build circles with OCCT and refuse constraints without the solver» — три
  mixed-gate, включая новый;
* шаг «Publish complete and partial JSON FBX and read them with pinned ufbx» —
  `tools/check-fbx-complex.sh` целиком: все прежние маркеры и новый
  `FCAD_CUT_UFBX_EXECUTED`.

Первый прогон нового шага **нашёл настоящий дефект упаковки**: гейты
`ferritecad-occt` и `ferritecad-document` строятся без feature `planegcs`, а
шаг выставляет `FERRITECAD_REQUIRE_PLANEGCS=1`, и build script solver'а падал.
Исправлено явным `FERRITECAD_REQUIRE_PLANEGCS=0` **на этих двух командах**, а не
на шаге, чтобы гейты, которым solver нужен, его требовали; после исправления шаг
проходит целиком. Это ровно тот класс, который ревью §25M/§25N ловило на
`bash -n`.

## Две направленные поломки

Обе компилировались и провалились на исполненных assertions; исходники
восстановлены побайтово (`shasum -a 256 -c`), положительные гейты повторены.

### 1. Body показывает деталь до выреза

`eval/cold.rs`: фича записывает shape, которую ей дали, а не ту, которую сделала.

```
native_a_through_hole_and_a_pocket_are_what_the_numbers_say
  the finished part's faces
  left: 6   right: 7
```

### 2. Cut перестаёт зависеть от того, что режет

`eval/cache.rs`: `cut_archive_key` игнорирует `target_key`.

```
native_the_cut_is_keyed_by_what_it_cut
  the cache did not do what it was expected to
  left:  [("plate", Miss), ("cut", Hit)]
  right: [("plate", Miss), ("cut", Miss)]
```

## Ограничения

* **Оконный GUI smoke не исполнен.** В этой сессии нет инструмента управления
  нативным интерфейсом macOS: доступны Bash, файловые инструменты, браузерный
  Chrome-скилл и терминальный `agterm`. Браузерный инструмент нативным
  UI-доступом не является и так не называется. Bundle не собирался, viewer и
  watchdog не запускались. Headless widgets/worker проверкой окна не считаются.
* Второй Cut в ту же деталь, face attachment, произвольные плоскости,
  Add/Intersect, Fillet, ThroughAll, live preview, правка созданного Cut и
  in-place Save в срез не входят и отказываются.
* Настоящий solver conflict к этому срезу отношения не имеет: деталь
  неограниченная, и Cut solver не спрашивает.
* Большой STEP и GPU/pixel локально не повторялись.
* CI нового diff не запускался; CI базы учитывается отдельно.
* Причина прежнего OOM не установлена и исправленной не объявляется.

## Независимое ревью Codex — 2026-09-17

Этот раздел относится к исправленному diff; предыдущие разделы сохраняют
протокол первоначальной реализации. Артефакты ревью:
`/private/tmp/ferrite-pr44-review/`. До правок сохранены исходный diff и SHA-256
всех 80 файлов. Чужой worktree не изменялся.

### Найденные и исправленные дефекты

1. **Cut менял исходный B-Rep.** Исполненная проверка `encode_shape` до/после
   boolean упала на исходной детали. Включён `SetNonDestructive(true)`;
   теперь оба входа сохраняются побайтово. История стенки инструмента честно
   стала `Modified` вместо `Kept`: OCCT копирует изменяемую грань. Проверки
   цилиндрической поверхности, объёма, 4 Kept + 2 Modified граней плиты и
   удаления крышек сохранены.
2. **Два Body могли владеть общей историей при разных tip.** Новый обход
   predecessor-цепочек даёт `body.shared-history`; пройденные суффиксы не
   обходятся повторно. Старые `body.shared-tip`, `feature.forked-history` и
   проверка циклов остаются. Failing-first: валидатор возвращал пустой отчёт.
3. **Перезапись tip могла потерять неизвестные поля Body.** Каталог и writer
   теперь требуют lossless payload Body. Исполненный тест вводит неизвестное
   CBOR-поле с правильным hash и проверяет отказ и сохранность источника.
4. **Ordinal переполнялся при добавлении двух объектов.** Используется
   `checked_add(2)` до создания UUID; проверены границы i64::MAX−2/−1/MAX.
   Failing-first в release получал отрицательный ordinal вместо отказа.
5. **Общий edit-extrude предлагал править Cut.** Общая проверка ограничена
   исходным NewBody без predecessor/target_body. Исходную Blind-высоту можно
   менять с пересчётом Cut; сам Cut получает явный отказ. Каталог и прямой API
   проверены вместе.
6. **После публикации отказ async Open терял Cut draft.** Cut переведён на
   прежнее `draft_published`/`draft_load_finished`. Native worker gate проверяет
   точные typed/applied/history, чужой отказ, свой отказ, успешное принятие и
   отсутствие последующего воскрешения принятого черновика. Failing-first
   действительно терял draft.
7. **Mixed gate запускался в полной native-сборке и падал.** Он ограничен
   `cfg(not(feature = "planegcs"))`; отдельный mixed CI шаг сохраняется.
   До исправления exact native запуск выполнил один тест и упал на REQUIRE.

Все семь дефектов воспроизведены до исправления исполняемыми assertions.
Первый пробный запуск отдельного OCCT suite с REQUIRE_PLANEGCS=1 остановился
в build script и не считается воспроизведением; затем с корректным окружением
получено именно runtime-падение B-Rep проверки. После исправлений targeted
наборы: document cut_edit 10, OCCT cut 4, native CLI cut 5, app cut 2 — все
прошли, без skips. Четыре новых document gate включены по exact-name в прежний
runtime workflow.

Отдельно JSON inspect использует уже прочитанный `source.cut_bodies`: повторная
классификация того же снимка удалена, wire не менялся.

### Реальный GUI smoke

Свежие release CLI/viewer собраны после исправлений и staged штатными
runtime-closure/stage-runtime-layout scripts. Bundle
`/private/tmp/ferrite-pr44-review/gui/layout/FerriteCAD.app` проверен
`codesign --verify --deep --strict`; `--solver-info` работает без DYLD.
Старый bundle не использовался, loader-failure probes не включались.

Реальный macOS CUA: открыть временную плиту 60×40×10 → Circular cut →
радиус −5 (видимый отказ, сохранённые поля) → радиус 5, центр (20,15), глубина
10 → Apply → Undo (все четыре поля пусты) → Redo (точный запрос возвращён) →
Save Cancel (draft сохранён) → публикация `GUI through.fcad` → async Open,
новый title, сквозное отверстие видно сверху. Через системный Open повторно
открыта исходная плита; опубликован `GUI pocket.fcad` глубиной 4: верхняя
сторона закрыта, со стороны основания видны полость и дно.

Независимый разбор обоих GUI-файлов и парных CLI-копий:

| Копия | refs после cold rebuild | STL bytes | Объём STL, мм³ | FBX bytes |
| --- | ---: | ---: | ---: | ---: |
| Through, h10 | 10/10 | 26084 | 23214.927292 | 54263 |
| Pocket, h4 | 11/11 | 25884 | 23685.970917 | 54264 |

GUI/CLI STL и FBX побайтово равны. Собственный STL parser проверил замкнутость,
ориентацию стенки, центр/радиус/глубину, наличие дна и границы объёма; pinned
ufbx прочитал все четыре FBX (по 6 checks, 0 failures). Это объём тесселяции,
не аналитический объём B-Rep. SQL-проверка сохранила все исходные object cells
кроме payload/hash Body, прежние refs и meta кроме modified_at; проверены
две добавленные object rows и замена BodyTip. Hash исходника записан **до** GUI
и остался прежним после обеих публикаций и peer CLI.

Watchdog: PID 47820, limit 1536 MiB, peak footprint **196.4231 MiB**,
pressure=1 на всех samples, swap без роста, минимум свободного диска 78.64 GiB,
штатный exit 0, guard не сработал. Первый запуск guard внутри sandbox был
отклонён системой при чтении memory pressure **до** создания viewer; повтор с
доступом к измерению выполнен. После штатного закрытия CUA-инспектор повторно
открыл пустое окно: оно сразу закрыто и не включено в измерение/GUI-доказательство;
инвентаризация подтвердила, что FerriteCAD больше не запущен.

Публичный Markdown-рецепт заново извлечён и исполнен исправленным staged CLI:
`FCAD_26A_RECIPE_OK 23214.927292 23685.970917`.

GUI-проверка относится к macOS. Windows/Linux GUI не проверялись; причина
прежнего OOM не установлена. Ни успешный smoke, ни footprint этого опыта
не объявляются доказательством устранения OOM.

Расширенный прогон дополнительно нашёл некорректную прежнюю фикстуру
`a_later_failure_releases_what_the_cache_restored`: она превращала вторую
независимую feature в Add первой, сохраняя два Body с перекрывающейся историей.
Новый валидатор правильно отказал раньше проверяемого освобождения cache.
Фикстура теперь освобождает прежний Body tip и явно проверяет валидность перед
rebuild; проверка Unsupported и освобождения восстановленной shape сохранена.
Повторён весь `warm_rebuild`: 7/7 pass. Первое падение не засчитано проходом.

### Прогоны исправленного diff

- Native workspace без полных app/viewport-gpu suites: 1492 harness passes,
  из них 4 явных пропуска (два прежних Metal/pixel gate без адаптера в sandbox
  и два теста только для отсутствующего solver). **1488 исполненных**;
  два прежних ignored — ручной timing benchmark и регенерация fixture manifest.
  В этом числе первый неудачный warm_rebuild заменён целиком положительным
  повтором 7/7; остальные прошедшие наборы не перезапускались ради счётчика.
- App headless: sketch 12, constraints 19, edits 8, STL exports 5 и cuts 2 —
  **46 исполненных**, без skips. Peer CLI пересобран до workers.
- Новый маршрут отдельно: document 10, OCCT 4, CLI 5, app 2, без skips;
  эти тесты уже входят в числа выше и повторно не суммируются.
- Stub: CMakeCache NOTFOUND и `otool -L` без libTK/planegcs; document 10 и
  CLI discovery/protocol 1 исполнены. Четыре native Cut и один mixed gate
  явно пропущены (CLI harness сообщает 6, но исполнен один).
- Fmt, workspace clippy all-targets/all-features `-D warnings`, export/solver
  boundaries, actionlint, shellcheck, native inventory (58 checks), licence
  headers и `git diff --check` прошли. Пять новых Rust headers проверены также
  до staging, поскольку основной header script читает tracked files.

Полный viewport-gpu suite и остальные app pixel tests локально не запускались.
Два pixel tests, случайно входящих в широкий CLI suite, попытались открыть
адаптер и явно пропустились; это не GPU-доказательство. Большой STEP/FBX
process corpus в расширенном CLI suite исполнен, включая 46 definitions /
140 Models / 34 Geometry / 986873 triangles; удалённый строгий reader campaign
проверяется отдельно на точном публикуемом SHA.

Mixed step заново извлечён из текущего YAML и исполнен с тем же package /
feature / gate argv (добавлен только `--offline`): **3/3 exact gates** для
circle, annulus и circular Cut, без skips. `otool -L` показывает OCCT и не
показывает planegcs. После этого peer CLI возвращён к полной native-сборке.
После staging основной licence gate проверил **354/354** исходника.
Удалённый CI и merge проверяются по точным SHA после публикации; локальные
результаты выше не подменяют эту проверку.
