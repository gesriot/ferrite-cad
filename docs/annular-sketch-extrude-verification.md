# §25L — локальные доказательства и ограничения

Контракт среза: [аналитический круговой профиль с одним концентрическим
отверстием](annular-sketch-extrude.md).

База: `main = origin/main = 0ae5e32872bca0268aad73b4431cab74b31c8a35` (merge
PR #39), дерево `16abb26fb4c9207d55489b6b03e972b306f6c558`, все 27 check-runs
этого SHA — success. Ветка `annular-sketch-extrude`; изменения оставлены
unstaged/uncommitted. **CI нового diff не запускался и пройденным не
объявляется.**

Окружение измерений: macOS 15 (Darwin 24.6.0), aarch64-apple-darwin, Open
CASCADE 8.0.1 из закреплённой сборки `vendor/install`, planegcs из
`vendor/planegcs`, `CARGO_BUILD_JOBS=1`, `--test-threads=1`. OCCT из исходников
не пересобиралась: pin и исходники OCCT не менялись. Собственный shim
пересобран — `bridge.cpp` и заголовок изменены, и это же меняет
`FERRITECAD_BRIDGE_BUILD`.

## Инварианты, зафиксированные до реализации

| | Инвариант |
| --- | --- |
| INV-1 | 4 объекта (XY DatumPlane, Sketch, Extrude, Body), 3 dependencies, 4 topology refs (2 cap, 2 side по UUID своей `Circle`); Sketch хранит ровно две неконструкционные `Circle` с разными `StableEntityId`, без constraints; схема БД не меняется |
| INV-2 | после reopen и cold rebuild — один solid, 4 аналитические грани: `Cylinder{R}`, `Cylinder{r}`, два `Plane`; объём `π·(R²−r²)·h` с относительной погрешностью < 1e-6 |
| INV-3 | side-ссылка с UUID внешней окружности разрешается в `Cylinder{R}`, с UUID внутренней — в `Cylinder{r}`; не меняется при перестановке двух `Circle` в хранимом Sketch |
| INV-4 | binary STL при явном deflection — замкнутая, одинаково ориентированная сетка; знаковый объём = кольцо в пределах бюджета тесселяции; хорды существуют и на `r`, и на `R`; ни один треугольник не накрывает ось |
| INV-5 | настоящие Miss→Hit в одном CacheStore дают одни и те же грани и объём; изменённый внутренний радиус даёт другой ключ и измеримо другую полость |
| INV-6 | `0 < r`, `R − r ≥ MIN_WALL_MM`, концентричность в пределах `Tolerance::DEFAULT_LINEAR`, всё ≤ 1e6 mm; любой другой запрос отказывается до публикации, не оставляя scratch и SQLite-спутников |
| INV-7 | discovery §25B и §25K честно отказывают двухокружностному Sketch; `edit-extrude` меняет только высоту и сохраняет обе `Circle`, обе side-ссылки и все прочие SQL-ячейки |

## Числовая policy: как выбрана толщина стенки

Независимое ревью повторило измерение на OCCT 8.0.1: `R=10`, `h=1`,
`r=10−wall` в `f64`. Эталон вычислялся устойчиво как `π(R−r)(R+r)h` по тем же
переданным ядру радиусам. Первоначальное сравнение с `π(R²−r²)h` включало
потерю точности самого эталона; прежние числа не являлись чистой ошибкой ядра.

| номинальная стенка, mm | грани | относительное расхождение B-Rep |
| --- | --- | --- |
| 1e-1 | 4 | 2.27e-15 |
| 1e-2 | 4 | 1.52e-13 |
| **1e-3** | 4 | **3.22e-13** |
| 1e-4 | 4 | 1.24e-12 |
| 1e-5 | 4 | 1.16e-11 |
| 1e-6 | 4 | 8.17e-11 |
| 1e-7 | 4 | 5.89e-9 |
| 1e-8 | 4 | 9.36e-9 |

Все строки построились. Это измерение одной конфигурации, не гарантия на весь
диапазон размеров. `MIN_WALL_MM=1e-3` оставлен как консервативная политика.
Сравнение границы исправлено на `R >= r + MIN_WALL_MM`; 10/9.999 принимается,
10/9.99900001 отвергается. Концентричность теперь использует евклидово
расстояние: смещение 0.9 допуска по каждой оси уже вне допуска.
Лог повторного замера: `/private/tmp/ferrite-pr40-review/stable-volume-probe.log`.
Временный измерительный код удалён после прогона.

## Исполненные проверки

Ниже сохранён протокол исполнителя. Независимые проверки и исправления ревью перечислены отдельно в конце документа.

### Настоящий native путь

`ferritecad-occt` unit (`ffi::tests`), 14 тестов:

* `an_annular_profile_builds_two_analytic_cylinders_and_names_no_corner` —
  две `FC_OCCT_SEGMENT_CIRCLE` в двух контурах через настоящий bridge:
  `faces = 4`, объём `3958.4067435231386` против точного
  `3958.4067435231395` (`π·(10²−4²)·15`) — расхождение 2.3e-16 относительных,
  при том что полный цилиндр дал бы `4712.38898038469`; `side_faces(0)` →
  `Cylinder{10}`, `side_faces(1)` → `Cylinder{4}`; оба cap — `Plane`;
  `sweep_edges(0)` и `cap_vertices(0,0)` отказывают (углов нет);
  `side_faces(2)` отказывает; тесселяция даёт 4 грани; archive round-trip
  восстанавливает обе стенки с теми же радиусами.
* `the_bridge_refuses_a_hole_that_is_not_strictly_inside_the_boundary` —
  обратная вложенность, совпадение и непересечение отказываются с
  `ErrorKind::Input`; эксцентричное, но правильно вложенное отверстие строится
  (это policy evaluator'а, а не bridge).
* `loop_lengths_must_partition_the_profile_segments` — `[]`, `[1]`, `[1,1,1]`,
  `[3]`, `[usize::MAX,1]`, `[usize::MAX,usize::MAX]` отказываются на стороне
  Rust; ревью дополнительно вызывает C ABI напрямую с теми же неправильными
  разбиениями и `[0,2]`, проверяя input error, нулевой output и отсутствие handles.
  `[2]` доходит до bridge и отказывается там («whole loop»); `[4]` для
  прямоугольника строится как раньше.
* `the_segment_and_surface_numbers_match_the_c_header` — порядок одиннадцати
  параметров `fc_occt_extrude` читается из самого заголовка, включая новые
  `const size_t *loop_segment_counts` и `size_t loop_count`; правило разбиения
  проверяется прямыми вызовами ABI, без поиска строки в `bridge.cpp`;
  `FcOcctSegment` по-прежнему сверяется `static_assert`
  в C++ и `offset_of!` в Rust.

`ferritecad-occt` integration (`occt_kernel`), 22 теста, в том числе два новых:

* `a_rectangular_hole_becomes_a_cavity_in_the_prism` — отверстие **из линий**, а
  не из окружностей: 10 граней, объём `(20·20 − 5·7)·2` с точностью 1e-9,
  `is_valid`, каждый из восьми сегментов поднял ровно одну грань под своим
  UUID, 8 `sweep_edges` и по 8 cap-вершин. Механика контуров общая, а не
  частный случай двух окружностей.
* `a_hole_that_encloses_nothing_is_refused` — прорезь нулевой площади: OCCT
  строит с ней валидную грань неизменной площади, поэтому adapter отказывает
  сам (`ErrorKind::Input`), а не возвращает плиту с двумя лишними гранями.

### Документ, B-Rep и identities

`ferritecad-cli --test annular_extrude`, 8 тестов, все исполнены (native):

* `native_annular_extrude_process_analytic_geometry_and_delivery` — неосевой
  центр `(12, −7)`, `R = 10`, `r = 4`, `h = 15`, пути с кириллицей и пробелом.
  4 объекта, 4 refs, две `Circle` с разными UUID; `result` называет
  `document_id`, `sketch_id`, `extrude_id`, `body_id` и **оба**
  `outer_curve_id`/`inner_curve_id`, каждый сверен с фактически сохранённым.
  Два `rebuild --cold` подряд не меняют ни одной identity. INV-2 и INV-3
  выполнены. Второй документ с дробными радиусами и другим центром/высотой —
  `(−3.5, 4.25)`, `R = 6.75`, `r = 2.125`, `h = 2.5` — ловит округление и
  потерю координат: hardcoded труба прошла бы первое измерение и провалила
  это. Каталог после прогона содержит ровно ожидаемые файлы: ни scratch, ни
  cache, ни SQLite-спутников.
* `native_annulus_ignores_the_order_the_two_circles_are_stored_in` — INV-3 при
  переставленных в хранимом Sketch кривых: те же UUID, те же refs, тот же
  solid, те же стенки под своими именами, тот же измеренный меш.
* `native_annulus_cache_serves_the_hole_the_document_actually_has` — INV-5.
* `native_annulus_height_copy_keeps_both_circles_and_every_identity` — INV-7:
  `edit-extrude` 15 → 25, обе `Circle` и обе side-ссылки целы, каждая
  SQL-ячейка сравнена с коротким явным списком разрешённых изменений (payload и
  payload_hash одной строки `objects` плюс `meta.modified_at`), источник
  побайтово цел.
* `annulus_discovery_refuses_both_earlier_editors_and_says_so` — INV-7:
  `editable:false` и `vertices:null` с причиной про Line; `circle_edit`
  `available:false`, `circle:null`, причина «exactly one unconstrained Circle»;
  `edit_extrude.available:true` и `distance_mm = 15`; фактический вызов
  `edit-circle` с корректным во всём остальном запросом отказан с exit 2 и
  `kind:"unsupported"`, источник цел, ничего не опубликовано.
* `annulus_request_refusals_and_usage_preserve_files` — 28 отвергаемых JSON
  (чужая версия, отсутствующие и лишние поля, в том числе `radius_mm`
  одноокружностного запроса и `inner_center_mm`, неправильные типы, null,
  отверстие размером с границу и больше неё, стенка 1e-4, выход за 1e6) плюс
  7 сырых текстов с NaN/Infinity/1e999; занятое назначение; usage/help; exit 7
  при закрытом stdout.
* `annulus_alias_and_stub_kernel_refusals_preserve_storage`,
  `annulus_json_non_utf8_path_refuses_before_reading` — hard link, symlink,
  не-UTF-8 путь.

Отмена между фазами и поздняя отмена покрыты прежними общими гейтами маршрута
создания (`ferritecad-jobs`, 53 теста): этот срез не добавил ни фазы, ни
собственной отмены — он добавил содержимое `NewDocument`.

### Меш, независимо от result DTO

Из фактических байт binary STL при `--linear-deflection 0.05
--angular-deflection 0.1`, без чтения отчёта экспорта иначе как для сверки
числа треугольников:

```
triangles 1008
directed edges used != once: 0       (замкнута и одинаково ориентирована)
undirected edges used != twice: 0
signed volume 3956.766410   annulus 3958.406744   full cylinder 4712.388980
  ratio to annulus 0.999586     ratio to full 0.839652
distinct radii (3dp): [4.0, 10.0]     distinct z: [0.0, 15.0]
triangles entirely on r~4 (bore wall): 252
triangles entirely on r~10 (outer wall): 252
bore normals pointing inward (toward axis): 252 of 252
outer normals pointing outward: 252 of 252
cap triangles: 504   covering the axis: 0
```

Полость измерена, а не выведена из числа треугольников, bbox или result DTO.
Радиусы читаются по фактическим хордам с допуском тесселяции; точного π от меша
никто не требует — знаковый объём сверяется с площадью, посчитанной по
**измеренным** периметрам обеих окружностей. Вписанная призма даёт объём чуть
меньше аналитического, и он на 16 % меньше сплошного цилиндра.

### FBX через pinned ufbx

Четыре малых публикации добавлены в прежний reader loop
(`tools/check-fbx-complex.sh`, маркер `FCAD_ANNULUS_UFBX_EXECUTED`): `annulus`,
`annulus-taller`, `annulus-ui`, `annulus-cli`. Каждая прочитана pinned ufbx
`fcc5d6ba444cfd3eb80677dba5e37e493941abe5` в strict mode:

```
annulus          checks=6  FCAD_IDENTITY_SUMMARY nodes=1 definitions=1 occurrences=1 meshes=1
annulus-taller   checks=6  ...
annulus-ui       checks=6  ...
annulus-cli      checks=6  ...
```

Отдельно фактическая геометрия прочитана тем же pinned ufbx и обратным
применением задокументированного отображения `C(x,y,z) = (x, z, −y)·0.001` —
не сверкой одного DTO с другим:

```
annulus         vertices=656 faces=652  bbox x[2..22] y[−17..3] z[0..15]
                radius about (12,−7): min 3.999999 max 10.000001
                signed volume 3956.556003   bore 126 tri   outer 200 tri
annulus-taller  ... z[0..25]   signed volume 6594.260006
```

### UI, headless

`ferritecad-app`, 309 тестов бинаря, все проходят. Два новых:

* `annulus_widgets_submit_exact_numbers_and_keep_the_other_drafts` — через
  настоящие виджеты: выбор `Circle with hole`, пять точных полей, отказы
  (отверстие размером с границу, больше неё, стенка 1e-4, ноль, отрицательное,
  не-число, пустое) показываются в форме и **не** предлагают Save; один Save —
  один request с точными числами; во время сохранения `Cancel draft`,
  переключение профиля и повторный Save не трогают черновик, его режим и
  соседние Line/Circle-черновики; Cancel завершает все три.
* `native_annulus_draft_and_cli_publish_equivalent_models` — Save Cancel и
  занятое назначение сохраняют черновик и не пишут ничего; headless worker
  публикует; устаревший ответ не открывает документ; тот же запрос через
  peer-CLI даёт эквивалентную модель с **новыми** UUID (сопоставлено явно:
  структура равна, identities различны) и побайтово одинаковый STL.

### Сборка без ядра

Настоящий stub доказан: `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` в
`CMakeCache.txt`, `CMAKE_IGNORE_PREFIX_PATH:STRING=/opt/homebrew`, и в imports
бинаря `otool -L` — ноль `libTK*`. В такой сборке
`create-annular-extrude` отказывает `kind:"unsupported"` с exit 2, не оставляя
файлов; `annular_extrude` даёт 3 исполненных и 5 честно пропущенных тестов.

### Рецепт из Markdown

Рецепт извлечён из `docs/annular-sketch-extrude.md` регулярным выражением по
маркеру `FCAD_25L_AGENT_RECIPE` (без разбора прозы) и исполнен настоящим
release-CLI и настоящим pinned reader: JSON create → inspect → validate → cold
rebuild → print-topology → STL/FBX → height-copy → повторные измерения →
отказы. Вывод: `FCAD_25L_RECIPE_OK 3956.76641 6594.610684`.

## Две направленные поломки

Каждая компилировалась, и каждая провалила **реально исполненную** смысловую
проверку. Исходники восстановлены побайтово (`shasum -a 256 -c`), положительные
гейты повторены после восстановления.

### 1. Потеря inner wire — получился бы полный цилиндр

`bridge.cpp`: отверстие не доходит до грани (`continue` перед
`face_builder.Add(hole)`). Провалили три уровня:

* `ffi::tests::an_annular_profile_builds_two_analytic_cylinders_and_names_no_corner`
  и `occt_kernel::a_rectangular_hole_becomes_a_cavity_in_the_prism` — вызов
  extrude падает с `Open CASCADE raised TopoDS_Builder::Add`;
* продуктовый гейт
  `native_annular_extrude_process_analytic_geometry_and_delivery` — `create`
  возвращает 2 вместо 0 с тем же сообщением, ничего не публикуя.

Побочный, но существенный вывод: потерять внутренний контур **тихо** нельзя.
Внутренняя окружность названа в документе, её история запрашивается у того же
sweep, и потеря контура ломает запрос истории раньше, чем успевает стать
неверным объёмом. Что измерение объёма всё же несущее, показывает вторая
поломка.

### 2. Неверная привязка inner/outer identity

`convert.rs`: геометрия правильная, но каждая стенка несёт UUID **другой**
окружности (метки контуров переставлены, радиусы нет). Деталь строится и
публикуется — и именно измерение ловит подмену: четыре гейта падают на
фактической поверхности под сохранённым именем,

```
left:  Some([Cylinder { radius: 4.0 }])
right: Some([Cylinder { radius: 10.0 }])
```

`native_annular_extrude_process_analytic_geometry_and_delivery`,
`native_annulus_ignores_the_order_the_two_circles_are_stored_in`,
`native_annulus_cache_serves_the_hole_the_document_actually_has`,
`native_annulus_height_copy_keeps_both_circles_and_every_identity`.

Попутно записано: более грубый вариант — переставить `ancestor` относительно
`output_role` в `populate_annulus` — до публикации не доходит вовсе, его
отвергает уже существующий инвариант документа («names the side raised from
segment X but selects everything derived from Y»). А перестановка **обоих**
полей сразу не является поломкой: получается тот же набор ссылок, записанный в
другом порядке, и все гейты честно проходят.

## Проверки, прогнанные целиком

`cargo fmt --all -- --check` — чисто. `cargo clippy --workspace --all-targets
--features planegcs -- -D warnings` — чисто. Тесты, `--test-threads=1`, по
крейтам последовательно:

| Крейт | Результат |
| --- | --- |
| ferritecad-types | 34 |
| ferritecad-kernel | 92 + 18 |
| ferritecad-topology | 44 + 38 |
| ferritecad-document | 46 + 9 + 32 + 29 + 28 + 9 |
| ferritecad-eval | 86 + 15 + 9 + 6 + 3 + 12 + 18 + 20 + 9 + 7 |
| ferritecad-occt | 14 + 11 + 3 + 1 + 1 + 6 + 3 + 22 + 5 + 10 + 9 + 2 + 13 + 17 + 11 |
| ferritecad-jobs | 53 |
| ferritecad-export | 40 + 15 + 5 |
| ferritecad-scene | 75 + 46 + 18 + 12 + 13 + 11 |
| ferritecad-exchange | 28 |
| ferritecad-sketch-solver | 3 + 10 + 14 |
| ferritecad-solver-lab | 3 + 14 |
| ferritecad-ui | 102 |
| ferritecad-viewport | 12 + 130 |
| ferritecad-viewport-gpu | 17 + 15 + 99 |
| ferritecad-app | 309 + 3 |
| ferritecad-cli | annular_extrude 8, circle_extrude 5, create 9, edit_extrude 2, sketch_extrude 4, rebuild 9, print_topology 8, validate 4, export_stl 9, edit_circle 5, edit_sketch 5, edit_constraints 9, json_v1 13, export_fbx 9, json_import 3, import_step 13, export_fbx_identity 1, shared_step_import 2 |

Ни одного падения и ни одного пропуска в native-сборке.

## N/A и пропуски — отдельный учёт

| Что | Статус | Почему |
| --- | --- | --- |
| Интерактивный GUI smoke | **отложен, не пройден** | по указанию пользователя: никаких локальных оконных тестов. Форма и её отказы проверены headless через настоящие виджеты |
| `complex_step_pixels`, `imported_step_pixels`, `export_scene_complex`, `occurrence_identity_complex`, `fillet_shell_corpus` | **не прогонялись локально** | большой STEP-корпус и pixel-гейты; изменённый путь (`record_extrude`, `fc_occt_extrude`) в STEP-импорт не входит, а крейт `ferritecad-topology` и `ferritecad-scene` прогнаны целиком. В CI они идут как прежде |
| Discovery двухокружностного Sketch в сборке **без** ядра | **исполнено при ревью** | тест записывает структурный документ через Document API, проверяет inspect и отказы обоих редакторов; производственный kernel-free create не добавлен |
| `FCAD_ALLOW_LOADER_FAILURE_PROBES` | **не включался** | ни разу, ни в одном прогоне |
| Пересборка OCCT из исходников | **N/A** | pin и исходники OCCT не менялись; пересобран только собственный shim |
| CI нового diff | **не запускался** | изменения не закоммичены; пройденным не объявляется |
| Windows и Linux | **N/A локально** | измерения сделаны на macOS; новые гейты добавлены в прежний трёхплатформенный runtime workflow |

## Ограничения, остающиеся после среза

* Ровно одно отверстие, ровно концентрическое, ровно круговое. Произвольные
  вложенные контуры, несколько отверстий, неконцентрическое и некруговое
  отверстие и смешанные Line/Arc-профили не входят — хотя механика контуров
  общая и проверена прямоугольным отверстием на уровне adapter.
* Правки радиусов этот срез не добавляет: ни редактор Line, ни редактор
  окружности §25K двухокружностный Sketch не принимают. Менять можно только
  высоту, прежним `edit-extrude`.
* Circle constraints, рисование и drag мышью, несколько тел, boolean Cut,
  in-place Save и новая схема БД в срез не входят.
* Прежний OOM не объявляется устранённым: сборки шли последовательно, один
  build job, один test thread, память проверялась перед прогонами.

## Независимое ревью §25L

Ревью выполнено без окон и GPU. База заново проверена: PR #39 merged,
`0ae5e32872bca0268aad73b4431cab74b31c8a35`, **27/27 checks, 6/6 workflows**.
Это проверка базы, не CI нового diff. Артефакты ревью:
`/private/tmp/ferrite-pr40-review/`.

Три исполняемых failing-first проверки обнаружили и подтвердили исправления:

* `R=10, r=9.999` ошибочно отвергался из-за округления разности; сравнение
  перенесено на масштаб радиуса. Заведомо меньшая стенка по-прежнему запрещена.
* Смещение центров на `0.9 * tolerance` по обеим осям принималось, хотя длина
  вектора больше допуска. Теперь используется `hypot`.
* После успешного create worker и отказа async Open пропадал черновик.
  Создание теперь использует общий `draft_published/draft_load_finished`:
  несвязанный отказ не восстанавливает его, относящийся к публикации —
  восстанавливает точные поля без повторного запроса, принятие сцены освобождает
  резервную копию. Прежняя правка Circle использует тот же механизм.

Проверка partition теперь вызывает также настоящий C ABI с неверными длинами,
нулевым контуром и переполнением: input error, нулевой handle, отсутствие утечки.
Поиск строки условия в C++ заменён этой исполняемой проверкой.

Discovery проверен в настоящей stub-сборке: тест создаёт структурную фикстуру
через Document API, без обхода production create. `inspect` сохраняет причины
недоступности обоих редакторов и доступную высоту. Попытка `edit-circle`
сохраняет прежний приоритет ошибки: без OCCT CLI сообщает отсутствие ядра;
в native — неподдерживаемый двухокружностный Sketch. Источник и каталог целы.

Итог локальных проверок ревью:

| Проверка | Исполнено |
| --- | --- |
| types/kernel/topology/document/eval/jobs/export | 675; ещё 2 stub-only N/A и 1 ignored timing benchmark отдельно |
| FFI + occt_kernel + 10 прежних малых OCCT integration suites | 103, без skips |
| CLI unit + 10 process suites | 82, без skips; annular suite повторён после уточнения stub fixture |
| Headless app sketch/creates/edits/constraints/STL | 72, без skips |
| ferritecad-ui | 102, без skips |
| Настоящий stub: annular/edit-circle CLI + annulus widget | 7 исполнено, 7 geometry skips отдельно |
| Четыре annular FBX, pinned ufbx | 4 × 6 checks, 0 failures |
| Рецепт из Markdown | `FCAD_25L_RECIPE_OK 3956.76641 6594.610684` |
| fmt, workspace clippy all targets/features, actionlint, shellcheck, diff whitespace | pass |
| native inventory / export boundary / licence headers | pass; 58 inventory checks, новые Rust headers проверены при staging |

Stub доказан `OpenCASCADE_DIR-NOTFOUND` и отсутствием libTK/PlaneGCS в imports
обоих свежих binaries; приложение не запускалось. Память проверялась,
viewer-процессов нет, swap не вырос в наблюдаемых замерах. Использованы прежние
target-каталоги, один build job и один test thread. Большой STEP-корпус,
интерактивный GUI, GPU/render и OCCT source rebuild локально не выполнялись.
Удалённый CI проверяется отдельно после публикации точного head; локальный
протокол не выдаётся за его результат.
