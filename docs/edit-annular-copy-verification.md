# §25M — локальные доказательства и ограничения

Контракт среза: [правка сохранённого кольцевого эскиза в новой
копии](edit-annular-copy.md).

База: `main = origin/main = 2487b7c4956670312187bda2594d612aa38a5a2a` (merge
PR #40, head `eea9152f89ed00204b49b96a82e495a1d3f007cc`, base `0ae5e32`), дерево
`d945573e69b4e759c820a4170e523500818c3139`. На этом merge 27/27 check-runs и
6/6 workflows — success (`CI`, `combined runtime layout`, `planegcs pin`,
`product sbom`, `rust notices`, `rust sbom`). Ветка `edit-annular-copy`,
0 коммитов впереди; изменения оставлены unstaged/uncommitted. **CI нового diff
не запускался и пройденным не объявляется.**

Окружение: macOS 15 (Darwin 24.6.0), aarch64-apple-darwin, Open CASCADE 8.0.1 из
закреплённой сборки `vendor/install`, planegcs из `vendor/planegcs`,
`CARGO_BUILD_JOBS=1`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `--test-threads=1`, сборки
последовательно. OCCT и PlaneGCS не пересобирались: их inputs не менялись, и
собственный shim этот срез тоже не трогает. Переиспользованы существующие
targets `/private/tmp/ferrite-24b-native-target` (16 ГБ) и
`/private/tmp/ferrite-25j-stub-target` (5,8 ГБ); новых не заводилось. Перед
тяжёлыми прогонами: свободно 14–16 ГиБ на `/private/tmp`, swap 2733/4096 МиБ,
чужие процессы (ChatGPT/Codex, другие сессии) не трогались.

## Инварианты, зафиксированные до реализации

| | Инвариант |
| --- | --- |
| INV-1 | `inspect --json` получает аддитивный `annulus_edit` с `available`/`refusal`/`document_refusal` и явными Sketch/outer/inner UUID и числами; прежние поля и приоритет документного отказа не меняются; один закреплённый read-only снимок и один `content_version`; работает без OCCT |
| INV-2 | меняются только центр и радиус двух названных кривых, их payload/payload_hash и `meta.modified_at`; все прочие SQL-ячейки, оба UUID и их порядок целы; источник побайтово цел при любом исходе |
| INV-3 | writer отказывает публично изменённому prepared payload (подменённый UUID любой из кривых, переставленные радиусы, другой plane, construction, третья кривая, одна кривая, constraints, радиус вне политики, разъехавшиеся центры) и устаревшей подготовке, не двигая `content_version` |
| INV-4 | прежний общий copy lifecycle целиком: snapshot + version guard → scratch → узкая запись → cold rebuild настоящим ядром → все прежде разрешавшиеся refs разрешаются → закрытие SQLite → атомарная no-clobber публикация; aliases/hardlinks/symlinks, занятое назначение, отмена между фазами, поздняя отмена и race at publish; kernel-free сборка не публикует |
| INV-5 | после close/reopen + cold rebuild: один solid, четыре аналитические грани — `Cylinder{R}` под ref своей Circle, `Cylinder{r}` под своей, два `Plane`; объём `π(R−r)(R+r)h` от переданных f64 с точностью 1e-6 |
| INV-6 | независимый разбор binary STL: замкнутость и единая ориентация, внутренняя стенка на r, сквозное отверстие, extents по новым числам, знаковый объём равен площади собственных измеренных периметров; аналитического π от меша не требуется |
| INV-7 | настоящие Miss/Hit на одном CacheStore дают те же грани и объём; после изменения отверстия ключ другой и измеренная полость новая |
| INV-8 | `edit-extrude` по-прежнему меняет только высоту и сохраняет обе Circle и обе side-ссылки; §25J/§25K/§25L creation и circle/polygon/height правки не регрессируют; документ с переставленными stored Circle правится с теми же ролями UUID |

## Архитектура

Общая подготовка выделена **ровно там, где она действительно общая**, а не
переименованной копией `circle_edit.rs`:

* `circle_edit::analytic_circle` — чтение одной неконструкционной аналитической
  `Circle` из `SketchCurve`. Кольцевой редактор вызывает его дважды; сколько
  окружностей должен держать Sketch и чему затем обязаны удовлетворять числа —
  вопрос каждого редактора отдельно.
* `Document::write_prepared_sketch_geometry` — guard узкой записи: перечитать
  строку, отказать изменившемуся Sketch, **заново вывести** разрешённую правку
  из чисел самого payload против текущего документа и сравнить payload целиком,
  затем обновить ровно payload и payload_hash. Один экземпляр на оба
  аналитических редактора: два могли бы разойтись, и разошедшийся принял бы
  подделку. Что это действительно один экземпляр, доказано поломкой 2 ниже —
  она валит writer-гейты **обоих** редакторов.

Всё остальное переиспользуется как есть: `sketch_edit::frame` (структура
документа), `AnnularExtrusion` (числовая политика), `ExtrudeEditSource` (один
закреплённый снимок), `edit_object_copy` (весь lifecycle), `CopyWrite::Annulus`
в той же строгой ветке `requires_resolved_references`, прежние JSON envelope и
emitter, общий механизм `draft_published`/`draft_load_finished`.

Роли outer/inner вычисляются одной функцией `roles()` по радиусам — её
используют и каталог, и проверка запроса, и повторный вывод writer'а, поэтому
они не могут разойтись во мнении. `replace_annulus_geometry` находит каждую
кривую **по её UUID**, а не по позиции строки.

## Исполненные проверки

### Document: каталог, подготовка и writer (без ядра)

`cargo test -p ferritecad-document --lib annulus_edit`, 7 тестов, все исполнены:

* `a_saved_pair_is_discovered_by_radius_in_either_stored_order` — оба порядка
  хранения дают один и тот же `SavedAnnulus` с теми же UUID в тех же ролях;
  Line- и Circle-каталоги на том же документе отказывают и ничего не выдумывают.
* `only_the_two_named_circles_and_only_their_geometry_is_rewritten` — оба
  порядка: id/parent/ordinal/name строки, `plane`, constraints, порядок кривых и
  обе identity целы; каждая кривая получила **свой** радиус; подготовка читается
  обратно ровно как исходный запрос.
* `each_of_the_three_numbers_can_move_on_its_own` — center-only, outer-only,
  inner-only.
* `the_annulus_writer_refuses_forged_payloads_and_stale_preparations` — 10
  подделок (подменённый outer UUID, подменённый inner UUID, переставленные
  радиусы при неизменных UUID, другой plane, construction, радиус вне политики,
  третья кривая, одна кривая, добавленные constraints, разъехавшиеся центры)
  плюс устаревшая подготовка; после каждой `content_version` не сдвинулась.
* `a_request_naming_the_wrong_curves_or_an_unstorable_number_is_refused` — 15
  запросов, включая перестановку ролей, один UUID дважды и оба чужих UUID.
* `a_sketch_this_editor_does_not_own_is_refused_rather_than_rewritten` — уже
  сохранённая геометрия вне политики и неконцентрическая пара с собственными
  причинами; сдвиг центра вчетверо меньше допуска ядра остаётся редактируемым, и
  оба сохранённых центра сообщаются по отдельности.
* `a_sketch_with_the_wrong_number_or_kind_of_curve_is_refused` — одна, три,
  Line-кривая, конструкционная.

### CLI: discovery, протокол и настоящая правка

`cargo test -p ferritecad-cli --test edit_annular`, 6 тестов, все исполнены:

* `annulus_discovery_and_protocol_without_kernel` — фикстура собрана **через
  Document API**, а не командой создания, поэтому discovery и отказы исполняются
  в сборке без ядра. `annulus_edit` сообщает оба UUID, оба центра, оба радиуса и
  высоту; `editable:false`, `vertices:null`, `circle_edit.available:false`,
  `constraint_edit.available:false`, `edit_extrude.available:true` не изменились.
  Документ с отверстием, записанным первым, discovery отдаёт с теми же ролями.
  24 отвергаемых JSON (чужая версия и `schema_version`, каждое отсутствующее
  поле, `height_mm`/`radius_mm`/`inner_center_mm` как лишние, центр не из двух
  чисел, обе границы радиусов и минимальная стенка, выход за 1e6, перестановка
  ролей, один UUID дважды, оба чужих UUID, нечитаемый UUID) плюс 4 сырых текста
  с NaN/Infinity/1e999; устаревшая версия, чужой Sketch, занятое назначение,
  источник как назначение, отсутствующий файл запроса, usage/help и exit 7 при
  закрытом stdout на **отказанной** операции. Ни один не написал ни байта.
* `native_annulus_edit_moves_and_resizes_only_the_named_pair` — источник сделан
  настоящим `create-annular-extrude` (12,−7) R10 r4 h15; четыре публикации:
  (−3.5,4.25) R6.75 r2.125, затем center-only, outer-only, inner-only. Для
  каждой: `result` называет Sketch и **оба** UUID, document_id сохранён, обе
  identity и порядок кривых целы, refs побайтово те же, высота 15, оба центра
  выровнены на запрошенный, источник побайтово цел, SQL сравнён с явным
  allowlist (ровно две изменившиеся ячейки `objects` плюс `meta.modified_at`),
  B-Rep и меш измерены. Затем настоящие Miss/Hit и повторные Miss/Hit после
  изменения отверстия на том же CacheStore, FBX, и `edit-extrude` 15 → 25 с
  сохранением обеих окружностей и всех ссылок; редактор принимает и копию с
  изменённой высотой.
* `native_annulus_edit_ignores_the_order_the_circles_are_stored_in` — документ с
  отверстием, записанным первым, правится с теми же ролями UUID, порядок
  хранения не двигается, геометрия и меш те же.
* `native_annulus_edit_refusals_preserve_source_and_destinations` — hard link,
  symlink, не-документ, отсутствующий источник, документ с одной окружностью
  (его `annulus_edit.available:false` с причиной «exactly two unconstrained
  Circles», при этом `circle_edit.available:true` — его владелец не изменился), и
  exit 7 после **состоявшейся** публикации: копия цела и измерена.
* `native_annulus_edit_cancellation_races_and_cleanup_are_atomic` — 8 случаев
  (`before`, `snapshot`, `rebuilt`, `closed`, `late`, `occupied`, `changed`,
  `alias`): коды ошибок, ноль утёкших handles, источник и его mtime целы, ни
  scratch, ни SQLite-спутников.
* `annulus_edit_non_utf8_paths_refuse_before_reading`.

### Измеренная геометрия

Копия (−3.5, 4.25) R6.75 r2.125 h15, прочитанная собственным парсером из
фактических байт binary STL при `--linear-deflection 0.05 --angular-deflection
0.1`:

```
triangles 1008
directed edges used != once: 0      (замкнута и одинаково ориентирована)
missing opposite edges: 0
signed volume 1933.486848   pi(R-r)(R+r)h 1934.288414   ratio 0.999586
distinct radii (3dp): [2.125, 6.75]     distinct z: [0.0, 15.0]
triangles covering the axis: 0
```

B-Rep той же копии: 4 грани, `Cylinder{6.75}` под ссылкой внешней окружности,
`Cylinder{2.125}` под ссылкой отверстия, два `Plane`, объём в пределах 1e-6 от
`π(R−r)(R+r)h`. Эталон — устойчивая форма; разность квадратов радиусов теряет
точность в самом эталоне. Высота после `edit-extrude` 25 даёт 3222.478.

### UI, headless

`cargo test -p ferritecad-app`, 311 тестов бинаря и 3 интеграционных, все
проходят. Два новых идут через **настоящие** widgets и worker, а не сеттеры:

* `annulus_edit_widgets_change_only_the_three_numbers_and_keep_the_draft` —
  begin на чужом Sketch и повторный begin на живом черновике отказываются;
  форма открывается на сохранённом и печатает оба UUID, Sketch UUID и
  «mm (retained)» у высоты; четыре поля набираются через настоящие клики и
  события клавиатуры; Save недоступен до Apply и ничего не отправляет; один
  Apply — один шаг истории на все три величины; Undo/Redo возвращают и
  повторяют его и **не** обращаются ни к одному job; девять отказов
  (отверстие как граница и больше неё, стенка 1e-4, ноль, отрицательное,
  не-число, нулевая граница, выход за 1e6, нечисловой центр) показываются в
  форме и не предлагают Save; запрос несёт identity из принятого чтения, а не
  из полей; во время сохранения Cancel/Save/Undo не трогают черновик, его режим
  и историю; Cancel завершает и ничего не оставляет.
* `native_annulus_edit_worker_and_cli_publish_equivalent_copies` — занятое
  назначение ничего не публикует и сохраняет черновик; второй запрос при
  занятом worker отвергается; устаревший ответ ничего не меняет; после
  публикации `draft_load_finished` с чужим путём ничего не восстанавливает, с
  своим — восстанавливает черновик **и** историю, а принятая сцена завершает
  редактирование; peer-CLI с тем же запросом даёт документ с теми же objects,
  dependencies, refs и document_id, и **побайтово одинаковые** STL и FBX;
  источник и его mtime целы.

### FBX через pinned ufbx

Три малых публикации добавлены в прежний reader loop
(`tools/check-fbx-complex.sh`, маркер `FCAD_ANNULUS_EDIT_UFBX_EXECUTED`):
`annulus-edited-cli`, `annulus-edit-ui`, `annulus-edit-cli`. Каждая прочитана
pinned ufbx `fcc5d6ba444cfd3eb80677dba5e37e493941abe5` в strict mode:

```
annulus-edited-cli  checks=6  FCAD_IDENTITY_SUMMARY nodes=1 definitions=1 occurrences=1 meshes=1
annulus-edit-ui     checks=6  ...
annulus-edit-cli    checks=6  ...
```

### Сборка без ядра

Настоящий stub доказан: `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND` и
`CMAKE_IGNORE_PREFIX_PATH:STRING=/opt/homebrew` в `CMakeCache.txt`; в imports
бинаря (`otool -L`) **ноль** `libTK*` и **ноль** `planegcs`. В такой сборке
`edit_annular` даёт 2 исполненных и 4 честно пропущенных теста, а
`annulus_edit` document-тесты — 7 исполненных. Заведомо сломанный бинарь ради
пробы загрузчика не запускался; `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался
ни разу.

Отказы, решаемые **внутри** общего job (устаревшая версия, занятое назначение),
в stub-сборке приходят как `unsupported`, потому что команда открывает ядро
перед передачей запроса. Это записано в самом гейте (`job_refusal`) и в
контракте, а не спрятано: обе формы — exit 2 и ничего не опубликовано, а
конкретные причины проверяются native-гейтами. Отказы, решаемые **до** job —
весь протокол запроса, синтаксис версии, не-UTF-8 путь — одинаковы в обеих
сборках и проверены точно.

### Рецепт из Markdown

Рецепт извлечён из `docs/edit-annular-copy.md` регулярным выражением по маркеру
`FCAD_25M_AGENT_RECIPE` (без разбора прозы) и исполнен настоящим release-CLI и
настоящим pinned reader: JSON inspect → выбор явных UUID и version →
edit-annular → reopen/validate/cold rebuild/print-topology → STL/FBX →
height-copy → повторные измерения → отказы. Вывод:
`FCAD_25M_RECIPE_OK 1933.486848 3222.47808`.

## Две направленные поломки

Каждая компилировалась и провалила **реально исполненные** assertions.
Исходники восстановлены побайтово (`shasum -a 256 -c`), положительные гейты
повторены после восстановления.

### 1. Подмена identity внутреннего радиуса

`annulus_edit.rs`: `replace_annulus_geometry` пишет по позиции строки вместо
поиска по UUID. Для документа, созданного §25L (граница записана первой),
ничего не меняется; для документа с отверстием, записанным первым, радиусы
меняются местами. Провалили два уровня:

* `annulus_edit::tests::only_the_two_named_circles_and_only_their_geometry_is_rewritten`
  — `curve 01a0a8e1-… got 6.75`, `left: 6.75 right: 2.125`: отверстие получило
  радиус границы;
* `native_annulus_edit_ignores_the_order_the_circles_are_stored_in` — команда
  вернула 2 вместо 0: тот самый writer-guard поймал подмену ролей до записи.

### 2. Обход payload guard

`document.rs`: `write_prepared_sketch_geometry` перестаёт заново выводить
разрешённую правку и сравнивать payload. Провалили writer-гейты **обоих**
редакторов, что и доказывает, что guard действительно один:

```
annulus_edit::tests::the_annulus_writer_refuses_forged_payloads_and_stale_preparations
  writer accepted forged outer identity
circle_edit::tests::the_circle_writer_refuses_forged_payloads_and_stale_preparations
  writer accepted forged curve identity
```

## Проверки, прогнанные целиком

`cargo fmt --all -- --check` — чисто. `cargo clippy --workspace --all-targets
--features planegcs -- -D warnings` — чисто. `tools/check-licence-headers.sh` —
343 файла, все MIT. Тесты, `--test-threads=1`, по крейтам последовательно:

| Крейт | Результат |
| --- | --- |
| ferritecad-types | 1 suite ok |
| ferritecad-kernel | 2 suites ok |
| ferritecad-topology | 2 suites ok |
| ferritecad-document | 6 suites ok (53 + 9 + 32 + 29 + 28 + 9) |
| ferritecad-eval | 10 suites ok |
| ferritecad-jobs | 1 suite ok (53) |
| ferritecad-occt | 16 suites ok |
| ferritecad-scene | 6 suites ok |
| ferritecad-export | 6 suites ok |
| ferritecad-ui / viewport / exchange | ok |
| ferritecad-app | 311 + 3 |
| ferritecad-cli | edit_annular 6, edit_circle 5, edit_sketch 5, edit_extrude 2, edit_constraints 9, annular_extrude 8, circle_extrude 5, sketch_extrude 4, create 9, rebuild 9, print_topology 8, validate 4, export_stl 9, export_fbx 9, json_v1 13, json_import 3 |

Ни одного падения и ни одного пропуска в native-сборке.

## CI

Новый шаг `Edit one saved annular profile through the native CLI and worker`
добавлен в существующий `runtime-layout` workflow рядом с шагом §25L, с тем же
no-skip/exact-name правилом: 5 CLI-гейтов и 6 пакетных (4 document, 2 app) — 11
исполнений на ОС сверх прежних, ни один прежний не удалён и не ослаблен. Три
малых FBX добавлены в прежний reader loop тем же способом, маркер
`FCAD_ANNULUS_EDIT_UFBX_EXECUTED` проверяется рядом с прежними. YAML и shell
синтаксически проверены (`ruby -ryaml`, `bash -n`). Большой STEP/complex-FBX/
pixel-корпус локально не дублировался.

## N/A и неисполненное — отдельный учёт

| Что | Статус | Почему |
| --- | --- | --- |
| Интерактивный GUI smoke | **отложен, не пройден** | по указанию пользователя: полностью headless. Форма, её отказы, история и восстановление черновика проверены через настоящие widgets и worker |
| `complex_step_pixels`, `imported_step_pixels`, `export_scene_complex`, `occurrence_identity_complex`, `export_fbx_complex`, `export_fbx_identity`, `import_step`, `shared_step_import`, `fillet_shell_corpus` | **локально не прогонялись** | большой STEP/complex-FBX/pixel-корпус; изменённый путь в STEP-импорт не входит, а `ferritecad-scene`, `ferritecad-export` и `ferritecad-topology` прогнаны целиком. В CI идут как прежде |
| Job-уровневые отказы в stub-сборке | **приходят как `unsupported`** | команда открывает ядро до передачи запроса; зафиксировано в гейте и контракте, обе формы — exit 2 без публикации |
| Пересборка OCCT/PlaneGCS | **N/A** | их inputs не менялись; собственный shim этот срез не трогает |
| `FCAD_ALLOW_LOADER_FAILURE_PROBES` | **не включался** | ни разу, ни в одном прогоне |
| Windows и Linux | **N/A локально** | измерения на macOS; новые гейты добавлены в прежний трёхплатформенный runtime workflow |
| CI нового diff | **не запускался** | изменения не закоммичены; пройденным не объявляется |

## Ограничения, остающиеся после среза

* Правится только пара концентрических окружностей класса §25L: произвольные
  отверстия, несколько отверстий, неконцентрическая пара и смешанные профили не
  входят.
* Высота остаётся у `edit-extrude`; собственного изменения высоты здесь нет.
* Принятая правка ставит обе окружности в один центр — документ, хранивший два
  близких, но разных центра, после правки хранит один.
* Circle constraints, рисование и drag мышью, boolean Cut, in-place Save, новая
  схема БД и новые FFI в срез не входят; контракты Line- и Circle-редакторов не
  ослаблены.
* Прежний OOM не объявляется устранённым: сборки шли последовательно, один build
  job, один test thread, память и диск проверялись перед прогонами.


## Независимое ревью Codex — 2026-09-16

Проверены роли по сохранённым радиусам, поиск по UUID, общий guard обоих
аналитических редакторов, строгие refs/copy lifecycle, JSON discovery и
восстановление annulus-черновика после неудачного Open. В production-коде
дополнительных исправлений не потребовалось.

Исправлена исполняемая ошибка нового CI-шагa: Bash `read` схлопывал два пробела
в строке `ferritecad-document  annulus_edit::tests::…`. Имя теста оказывалось в
`features`, а `gate` оставался пустым. Настоящий Cargo воспроизвёл exit 101
(`package ... does not contain this feature`) до запуска теста. Пустой столбец
заменён явным `-`; исправленный пакетный цикл извлечён из workflow и исполнен
локально (debug/offline вместо release, те же package/features/exact test names
и no-skip проверки): 4 document + 2 app gates прошли. Синтаксическая проверка
YAML/shell сама по себе эту ошибку не находила.

Независимый положительный прогон текущего diff:

- fmt; workspace clippy all targets/all features `-D warnings`; свежие CLI/app;
- domain/kernel/topology/export: 684 harness passes, из них 682 исполненных
  и 2 явно неприменимых stub-only проверки в native; 1 прежний timing ignored;
- CLI: 88 исполненных, без skips, включая 6 edit-annular и прежние
  annular/circle/edit/JSON/STL/FBX suites;
- app: 74 headless проверки (sketch/create/edit/constraints/STL), UI: 102,
  без skips; viewer/event loop и GPU не запускались;
- отдельный настоящий stub: CMakeCache NOTFOUND, CLI/app imports без OCCT и
  PlaneGCS; 15 исполненных (8 CLI, 7 document), 13 явных native skips
  (11 CLI, 2 app), не засчитанных геометрией;
- три новых FBX: pinned ufbx, по 6 checks, 0 failures;
- рецепт из Markdown: `FCAD_25M_RECIPE_OK 1933.486848 3222.47808`;
- actionlint, shellcheck, export/solver boundaries, notice ownership,
  license headers (включая три новых Rust-файла), `git diff --check`.

Логи: `/private/tmp/ferrite-pr41-review/`. Native использует существующий
`ferrite-24b-native-target`, stub — `ferrite-25j-stub-target`, сборки и тестовые
потоки ограничены одним. Во время проверки свободно 15 GiB, memory free 53–54%,
swap 2733.19 MiB не вырос; процессов viewer нет. Большой STEP/complex-FBX корпус
локально не дублировался. GUI остаётся отложенным, причина прежнего OOM неизвестна.
CI публикуемого head и merge проверяется отдельно по точным SHA; приведённые
выше результаты — локальный diff, а исходные 27/27 checks относятся к базе.
