# Карта предметных возможностей: UI ↔ CLI ↔ общий API

Срез §24A, обновлён срезами §24B–§24J и §25A–§25H. Это описание **текущих** команд и владельцев, а не проект нового протокола.

Документ нужен агенту и человеку, которым дали задачу и публичный CLI: что уже можно получить тем же предметным результатом, что в UI, что умеет только один клиент, и где библиотечный метод ещё не является пользовательской операцией. Архитектурное правило — [§4.5 плана](implementation-plan.md): UI и CLI — два клиента общих операций; равенство считается по сохранённой модели и экспортируемым артефактам, а не по hover, жестам мыши или промежуточным кадрам.

Сборки и чертежи в beta не входят. Импортированная STEP-сборка сохраняет структуру и может быть экспортирована в FBX; это не редактор сборок.

§26G: [высота базы circular Cut history](edit-cut-base-height.md) правится
существующими `Edit extrusion` и `edit-extrude --distance-mm` через общий copy
job. Глубины tools абсолютны; новые дна получают собственные/descendant refs,
потеря сохранённого дна и выход глубины за толщину отказываются с UUID. Discovery
`features[].base_height_edit` содержит typed контекст того же snapshot.

§26H: [явный ThroughAll](circular-cut-through-all.md) у circular Cut. UI —
`End: Blind depth / Through all` в прежних формах `Cut circle into…`/`Edit cut…`;
CLI — request v2 тех же `cut-circular-copy`/`edit-circular-cut` с
`extent:{"kind":"through_all"}`. Намерение хранится как `EndCondition::ThroughAll`
(payload v3, `feature.through-all.v1`); длину evaluator берёт из текущей высоты
базы, поэтому Cut остаётся сквозным после `edit-extrude`, reopen, cold rebuild и
cache hit. Blind↔ThroughAll без потери дна сохраняет имена; ThroughAll→карман
добавляет дно; карман с сохранённым дном→ThroughAll и request v1 на ThroughAll
отказываются с UUID. Discovery: прежние v1-блоки не меняют типов и для истории
с ThroughAll честно недоступны; новые `cut_edit_v2`, `circular_cut_edit_v2`,
`base_height_edit_v2`, `cut_history_v2` несут `extent` и `request_versions`.

§26I: та же история Cut [в простом Line-полигоне](polygon-cut-history.md) —
3..256 сторон без constraints, обе ориентации, наклонные стены и вогнутые
вершины (L-профиль). Инструмент должен отстоять от реальных конечных отрезков
контура, а не от его bounding box; правка базы проверяет все инструменты и
называет UUID нарушающего Cut. Команды и request прежние. Discovery: для
прямоугольника прежние блоки побайтно те же; для иного полигона v1/`_v2`
недоступны, новые `cut_edit_v3`, `circular_cut_edit_v3`, `base_height_edit_v3`,
`cut_history_v3` несут `bounds_mm` и типизированный `boundary`.

§27A: `create-sketch-revolve <request.json> -o <new.fcad> [--json]` —
[полный оборот](full-turn-revolve.md) простого Line-полигона 3..256 вершин без
constraints вокруг локальной оси Y эскиза. Все точки строго при X > 0;
касание/пересечение оси, частичный угол, другая ось и несколько контуров
отказываются (exit 2). Request v1 строгий: `request_version`, `points_mm`,
`axis:"sketch_y"`, `angle:"full_turn"`. Хранится намерение `feature.revolve`
с новой capability `feature.revolve.v1`; каждая Line даёт одну грань под своим
UUID (`RevolveFace`). Сборки до §27A открывают такой документ только для
чтения. Discovery: новый массив `revolves`; `features` его не содержит, прежние
редакторы отказываются. Без ядра создание отказывается и ничего не публикует.

§27B: [координаты профиля такого Revolve](edit-revolve-profile.md) меняет
прежний `edit-sketch-copy` (request v1 без изменений) в новую копию. Все UUID
линий, объектов и имён граней сохраняются; ось, полный оборот и NewBody
остаются. Касание/пересечение оси, самопересечение, смена порядка/числа линий
или направления обхода отказываются (exit 2). Discovery добавляет
`sketches[].profile_feature` с `kind` `blind_extrude` или `full_turn_revolve`.

§27C: тот же `create-sketch-revolve` и тот же request v1 принимают и сплошной
профиль: X ≥ 0 и ровно одна целая Line точно на X = 0 (−0 равно 0), прочие
вершины при X > 1e-6. Одиночное касание, пересечение, несколько отрезков на
оси и вершина вплотную к оси отказываются (exit 2). Такой Revolve хранится
payload v2 с `axis_segment` и требует capability
`feature.revolve.axis-closed.v1`; сборка §27B открывает его только для чтения
и не правит. Line на оси грани не даёт и имени не получает. `edit-sketch-copy`
правит его координаты, оставляя ту же Line на оси; переход между сплошной
деталью и деталью с отверстием отказывается (exit 2). Discovery:
`profile_feature.kind` `full_turn_revolve_axis_closed`.
[Контракт §27C и исполняемый рецепт](axis-closed-revolve.md).

§27D: тот же `create-sketch-revolve` принимает строгий request v2 с
`extent:{"kind":"angle","degrees":137.5}` либо `extent:{"kind":"full_turn"}`.
Допустимый угол — 0.01°–359.99° включительно, вокруг +Y; 360° нужно явно
задавать как полный оборот. Обе прежние формы профиля поддержаны. У сектора
два собственных `RevolveCap` и capability `feature.revolve.partial.v1`;
`revolves[].extent` становится `partial_turn`, новый `angle_deg` — число
(null у полного оборота). Правка координат сохранённого сектора в §27D
отказывалась; §27F её добавляет (см. ниже). Request v1, response и exit
0/2/7 сохраняются.
[Контракт §27D и исполняемый рецепт](partial-angle-revolve.md).

§27E: угол сохранённого сектора меняется в новой копии:
`edit-revolve-angle source.fcad --feature UUID --expect-version HASH
--request request.json -o copy.fcad [--json]` с request v1
`{"request_version":1,"angle_deg":220}`. В окне этому соответствует
`Edit Revolve angle <имя> — <UUID>…`: поле Angle °, Apply, Undo/Redo и
`Save edited Revolve copy…`. UI и CLI идут через один job,
`prepare_revolve_angle` → `write_revolve_angle` → прежний `edit_object_copy`:
- тот же snapshot;
- baseline cold rebuild;
- пере-вывод строки в транзакции записи;
- обязательное разрешение всех прежних имён;
- повторная проверка версии источника;
- атомарная no-clobber публикация.

Меняются только payload/payload_hash строки Revolve и `meta.modified_at`.
Все UUID (объекты, Lines, оба `RevolveCap`), refs, capabilities, версии
payload и прочие SQL-ячейки сохраняются, источник побайтово цел. Тот же угол
принимается и публикует копию с побайтово тем же Revolve.

Отказывают (exit 2):
- полный оборот (`unsupported`);
- UUID, который не Revolve;
- неизвестный UUID (`input`);
- угол вне 0.01°–359.99° (`input`, решает `RevolveAngle`);
- любой другой состав документа.

Discovery — аддитивный `revolves[].angle_edit`. Смена оси/направления и
full↔partial не входят.
[Контракт §27E и исполняемый рецепт](edit-revolve-angle.md).

§27F: координаты сохранённого сектора (v3 с отверстием, v4 замкнутый на оси)
правятся тем же `Edit Sketch` (перетаскивание или точные X/Y, Undo/Redo) и
тем же `edit-sketch-copy` с прежним request v1. Угол не входит в запрос и не
меняется.

Как и у полного оборота, сохраняются:
- класс детали (с отверстием или сплошная);
- Line на оси;
- число и порядок Lines, все UUID, оба `RevolveCap`.

Меняются только payload/payload_hash строки Sketch; `meta.modified_at` при
правке координат не штампуется. Discovery аддитивна: строка Sketch сектора
становится `editable:true` с новым видом `profile_feature.kind`
`partial_turn_revolve` / `partial_turn_revolve_axis_closed` и полем
`angle_deg`.

Request этого маршрута стал строже: дубли ключей (в том числе через JSON
escape) и массивы на месте объектов отказываются как `input`.
[Контракт §27F и исполняемый рецепт](edit-partial-revolve-profile.md).

§25A добавляет [собственный Line-полигон → Blind Extrude](sketch-extrude-create.md):
UI `Create sketch + Extrude…` и CLI `create-sketch-extrude request.json -o new.fcad [--json]`
используют `PolygonExtrusion` + `CreateDocumentRequest` / `create_document_with_kernel`.
§25J добавляет [аналитическую окружность → Blind Extrude](circle-sketch-extrude.md):
тот же редактор в режиме `Profile: Circle` и CLI
`create-circle-extrude request.json -o new.fcad [--json]` используют
`CircleExtrusion` и тот же `create_document_with_kernel`.
§25K добавляет [правку центра и радиуса сохранённой окружности](edit-circle-copy.md)
в новой копии через ту же shared copy operation.
§25L добавляет [круговой профиль с одним концентрическим отверстием](annular-sketch-extrude.md):
тот же редактор в режиме `Profile: Circle with hole` и CLI
`create-annular-extrude request.json -o new.fcad [--json]` используют
`AnnularExtrusion` и тот же `create_document_with_kernel`.
§25M добавляет [правку общего центра и обоих радиусов сохранённой пары](edit-annular-copy.md)
в новой копии через ту же shared copy operation.
§25N добавляет [сохранённый радиус и закреплённый центр окружности](circle-radius-constraints.md):
та же команда `edit-sketch-constraints-copy` и тот же solver-маршрут получают
две новые формы запроса, а `ferritecad-sketch-solver` — настоящую аналитическую
окружность с тремя неизвестными.
§26A добавляет [первый Cut в существующем Body](circular-cut-copy.md):
`cut-circular-copy source.fcad --body UUID --expect-version HASH --request
request.json -o copy.fcad [--json]` вырезает круглый инструмент на базовой
XY-плоскости детали на конечную глубину вдоль +Z. Body сохраняет свой UUID, а
его tip становится новой фичей; история выражена явно
([ADR 4](decisions/0004-feature-predecessor.md)).
§26C расширяет ту же команду на [второй отдельный circular Cut](sequential-circular-cuts.md):
все четыре сочетания through/pocket, зазор между дисками строго больше 1e-7 mm,
сохранение исторических refs и явные origin names на final Body. Добавление
третьего Cut и редактирование обоих Cut восьмиобъектной копии пока недоступны.
§25O распространяет тот же маршрут на [кольцо](annular-circle-constraints.md):
радиус каждой из двух окружностей, явная концентричность и один закреплённый
общий центр. Solver, C ABI, persisted layout и capability при этом не менялись —
концентричность это прежний `Coincident` двух центров.
§25B добавляет координатную правку сохранённого Line Sketch в новой копии;
Произвольные constraints и persistent history остаются будущей работой.
§25E/F дают отдельную сохраняемую H/V/Line length правку ограниченного Line Sketch.
§25G добавляет в ту же операцию один Fixed endpoint профиля в заданных X/Y mm.
§25H добавляет в ту же операцию равенство длин двух названных Lines профиля.
§25D добавляет opt-in Snap Off/0.1/1/5/10 mm в том же canvas; это жест UI,
не новая CLI-команда и не поле JSON.
JSON v1 доступен только явно перечисленным командам.

## Как читать таблицу

- **Предметный результат** — что должно оказаться в `.fcad` или в выданном файле.
- **UI сейчас** — что пользователь может сделать в `ferritecad-viewer` без чтения исходников.
- **Публичный CLI сейчас** — подкоманда `ferritecad` из `--help`. Отсутствие строки в `--help` значит, что команды нет.
- **Общий API / владелец** — библиотечный вход, которым реально пользуется клиент. Ссылки ведут в исходники, чтобы назвать владельца, а не чтобы агент читал их ради поддерживаемой задачи.
- **Фактический пробел** — чего нет у пользователя, даже если в Rust есть тип или метод.

**Библиотечная возможность ≠ пользовательская операция.** `rebuild_cached`, `Document::write`, `ObjectPayload::Parameter` и методы `OcctKernel` вне контракта `GeometryKernel` существуют в коде и тестах. Это не команды CLI и не действия UI, пока их нет в публичном `--help` или на экране.

Помечено **измерено**, если поведение снято с настоящего CLI в этом срезе. Помечено **из кода / тестов**, если оно известно из исходников, `--help` или существующих тестов и здесь заново не воспроизводилось.

## Что этот срез сознательно не делает

§24D/§24D-1/§24F/§24G/§24I/§24J задают [JSON v1](cli-json-v1.md) только для opt-in `inspect --json`, `edit-extrude --json`, `create --json`, `export-stl --json`, `export-fbx --json`, `import-step --json` и `validate --json`. Общего JSON всех команд, DSL, RPC/MCP server, универсального dispatcher и библиотеки «всех операций» нет. Stdin, отмена CLI и пакетный ввод остаются будущими требованиями §4.5.

§24B добавил ровно одну общую операцию — создание документа — и её второго клиента (`New…` во вьюере). Новых команд и флагов у `ferritecad` нет. Остальные возможности сохраняют прежних клиентов.

## Карта текущих операций

| Предметный результат | UI сейчас | Публичный CLI сейчас | Общий API / владелец | Фактический пробел |
| --- | --- | --- | --- | --- |
| Пустой нативный `.fcad` | `New…` → «Empty document» → системный Save-диалог → создание → обычный Open результата. | `ferritecad create <path> [--json]` | Один маршрут: [`ferritecad_jobs::create_document`](../crates/ferritecad-jobs/src/create.rs). Внутри — `Document::create_with` в scratch, транзакция [`Document::write`](../crates/ferritecad-document/src/document.rs), закрытие соединения и атомарная публикация через [`Temporary`](../crates/ferritecad-jobs/src/publish.rs). CLI: [`create`](../crates/ferritecad-cli/src/main.rs). UI: [`creates`](../crates/ferritecad-app/src/creates.rs). JSON v1 сообщает опубликованные `destination` и `document_id`. | Ни у `create`, ни у UI нет замены существующего файла: занятое назначение — отказ. Пустой документ и пустое окно без документа — разные состояния. |
| Sample plate с шириной, глубиной, высотой | `New…` → «Sample plate» → W/D/H в мм → Save-диалог → создание → обычный Open. Размеры подписаны единицей; значения по умолчанию те же, что у CLI. | `ferritecad create <path> --sample --size W D H [--json]` | Тот же `create_document` с `NewDocument::SamplePlate(PlateSize)`. Построение плиты — приватная транзакция в [`create.rs`](../crates/ferritecad-jobs/src/create.rs); в CLI-крейте её больше нет. Размеры — координаты эскиза и `Expression::constant` высоты, не объекты `Parameter`. | `--size` задаёт размеры только при создании. Постоянную Blind-высоту уже созданной плиты можно изменить через edit-extrude (§24C); ширину и глубину можно менять координатами Sketch через `edit-sketch-copy` (§25B). Случайные `ObjectId`/`DocumentId` при каждом создании не совпадают — это контракт, не баг. |
| Прочитать документ (метаданные, объекты, ссылки) | `Open…` читает `.fcad` read-only через `snapshot_of` → `prepare::load`, с холодным перестроением. | `ferritecad inspect <path> [--json]` | [`Document::open_read_only`](../crates/ferritecad-document/src/document.rs). Текст — `render::inspect`; JSON — общие `ExtrudeEditSource` и `stl_bodies(&Document)` на том же закреплённом снимке, без kernel/rebuild, миграции и записи. | JSON v1 даёт `features` для правки Extrude, `sketches` для координатной правки и `bodies` для экспорта native Body, а не всю БД/геометрию. UI не показывает сырой граф и topology refs. |
| Собственный полигон → новый Sketch/Extrude/Body | `Create sketch + Extrude…`, мышь/точные координаты, Close contour, draft undo/redo, Save в новый файл | `ferritecad create-sketch-extrude request.json -o new.fcad [--json]` | `PolygonExtrusion` → `CreateDocumentRequest` → `create_document_with_kernel`, общий writer/evaluator/Keep publish | Один XY Line polygon/mm/Blind/NewBody; [точные ограничения и рецепт](sketch-extrude-create.md). Правка сохранённого Sketch — отдельный §25B ниже; H/V редактируются отдельно в §25E; persistent undo отсутствует. |
| H/V/длина сохранённого Line Sketch → новая копия | `Edit constraints …`, segment list, Add H/V/length (mm), `Replace length` для сохранённого Distance, Remove exact UUID, bounded Undo/Redo, Save copy | `edit-sketch-constraints-copy source --sketch UUID --expect-version TOKEN --request constraints.json -o copy [--json]` | document managed H/V/Line length/closure policy + narrow writer → jobs `edit_sketch_constraints_copy`, тот же snapshot/cold/Keep lifecycle; UI/CLI только adapters | [Контракт и рецепт](sketch-constraints-copy.md). Stored coordinates не меняются, solves временны; H/V + Start/End Distance одного Line + adjacent Coincident, без arbitrary point-to-point/formula/drag/in-place Save. [Рецепт длины](line-length-constraints.md). `Replace length` — UI-адаптер над тем же remove/add, не новая CLI-команда. |
| Один закреплённый endpoint Line-профиля → новая копия | `Edit constraints …`, segment list, `Pin Start`/`Pin End`, поля Fixed X/Y (mm), `Add Fixed point`, Remove exact UUID сохранённого Fixed, та же bounded Undo/Redo и Save copy | та же `edit-sketch-constraints-copy`, request v1 `{"rule":"fixed","curve_id":UUID,"at":"start"\|"end","x_mm":N,"y_mm":N}` | тот же document managed класс (`SketchConstraintRule::Fixed` + существующий перевод в solver), тот же narrow writer и jobs `edit_sketch_constraints_copy`; новой команды и нового writer нет | [Контракт и рецепт](fixed-sketch-vertex.md). Не более одного Fixed на профиль; ноль и отрицательные mm допустимы, NaN/Infinity — отказ. Stored координаты остаются inputs solver и не перезаписываются решением; замена — remove exact UUID + add в одном request. Coordinate drag/Snap constrained Sketch по-прежнему запрещены. |
| Равенство длин двух Lines профиля → новая копия | `Edit constraints …`, segment list, `Pair Line A`/`Pair Line B`, показ выбранной пары, `Add Equal length`, Remove exact UUID сохранённого равенства, та же bounded Undo/Redo и Save copy | та же `edit-sketch-constraints-copy`, request v1 `{"rule":"equal_length","a_curve_id":UUID_A,"b_curve_id":UUID_B}` | тот же document managed класс (`SketchConstraintRule::EqualLength` + существующий перевод в solver), тот же narrow writer и jobs `edit_sketch_constraints_copy`; новой команды, writer и схемы БД нет | [Контракт и рецепт](equal-line-lengths.md). Пара — две разные целые Lines одного Sketch; `(A,B)` и `(B,A)` — один слот, сохранённый reversed `SegmentRef` читается как та же Line. Self-pair/чужая Line/duplicate — структурный отказ до solver; избыточность и конфликт устанавливает настоящий solver. Числа у связи нет: изменение ведущей длины меняет обе стороны и сохраняет UUID равенства. |
| Относительная ориентация двух Lines профиля → новая копия | `Edit constraints …`, тот же segment list и тот же выбор пары `Pair Line A`/`Pair Line B`, действия `Add Parallel` и `Add Perpendicular`, Remove exact UUID сохранённой связи, та же bounded Undo/Redo и Save copy | та же `edit-sketch-constraints-copy`, request v1 `{"rule":"parallel","a_curve_id":UUID_A,"b_curve_id":UUID_B}` и `{"rule":"perpendicular",…}` | тот же document managed класс (`SketchConstraintRule::Parallel`/`Perpendicular` + существующий перевод в solver), тот же narrow writer и jobs `edit_sketch_constraints_copy`; новой команды, writer, схемы БД и FFI нет | [Контракт и рецепт](line-pair-orientation.md). Пара — две разные целые Lines одного Sketch; `(A,B)` и `(B,A)` — один слот, и **один ответ на пару**: Parallel и Perpendicular делят его, а EqualLength той же пары — независимое свойство. Self-pair/чужая Line/duplicate/другой ответ/reversed duplicate — структурный отказ до solver; избыточность и конфликт устанавливает настоящий solver. Связь не Horizontal/Vertical: она держится на наклонном профиле, который остаётся свободным вращаться. |
| Аналитическая окружность → новый Sketch/Extrude/Body | `Create sketch + Extrude…` → `Profile: Circle`, численные Center X/Y, Radius, Blind height, `Save circle extrusion…`; Line-черновик рядом сохраняется | `ferritecad create-circle-extrude request.json -o new.fcad [--json]` | `CircleExtrusion` → `CreateDocumentRequest` → `create_document_with_kernel`; один `NewDocument::needs_kernel()` классифицирует оба нарисованных профиля, общий writer/evaluator/Keep publish | Один XY `SketchGeometry::Circle`/mm/Blind/NewBody; [контракт и рецепт](circle-sketch-extrude.md). Окружность аналитическая и в документе, и в B-Rep: одна цилиндрическая поверхность и два плоских cap, история привязана к UUID окружности. STL/FBX — тесселяция, не аналитика. Circle constraints, смешанные профили и рисование мышью не входят; правка радиуса доступна отдельной операцией §25K, концентрическое отверстие — §25L/§25M. Прежние Line-редакторы окружность по-прежнему отказывают. |
| Полая деталь: окружность с одним концентрическим отверстием | `Create sketch + Extrude…` → `Profile: Circle with hole`, численные Center X/Y, Outer radius, Inner radius, Blind height, `Save annular extrusion…`; Line- и Circle-черновики рядом сохраняются | `ferritecad create-annular-extrude request.json -o new.fcad [--json]` | `AnnularExtrusion` → `CreateDocumentRequest` → `create_document_with_kernel`; тот же `NewDocument::needs_kernel()`, writer, evaluator и Keep publish, что у одной окружности | Две концентрические XY `SketchGeometry::Circle`/mm/Blind/NewBody, `0 < r < R`, стенка не тоньше 0.001 mm; [контракт и рецепт](annular-sketch-extrude.md). Отверстие — второй контур того же профиля, а не второе тело и не boolean: в B-Rep две цилиндрические поверхности радиусов R и r и два плоских cap, по одной side-ссылке на каждую окружность по её собственному UUID. Внешний и внутренний контуры определяются геометрией, а не порядком записи. STL/FBX — тесселяция, не аналитика. Произвольные вложенные контуры, несколько отверстий, неконцентрическое отверстие, смешанные профили, circle constraints и boolean Cut не входят; правка радиусов доступна отдельной операцией §25M. Line- и Circle-редакторы этот Sketch по-прежнему отказывают. |
| Правка сохранённой кольцевой пары: общий центр и оба радиуса | `Edit annulus <имя> — <UUID>…` — оба UUID окружностей, их сохранённые центры и радиусы, неизменяемая высота, точные Center X/Y, Outer radius, Inner radius, один `Apply annulus change` на все три числа, `Save edited annulus copy…` | `ferritecad edit-annular source.fcad --sketch UUID --expect-version HASH --request request.json -o copy.fcad [--json]` | `AnnulusEdit` → `replace_annulus_geometry` → прежний `edit_object_copy`: тот же snapshot, baseline cold rebuild, strict-проверка refs, повторная проверка версии, закрытие SQLite и атомарная no-clobber публикация; `write_annulus_geometry` идёт через общий с окружностью guard `write_prepared_sketch_geometry` | Ровно класс §25L внутри frame §25K; роли outer/inner — по сохранённым радиусам, не по порядку кривых; [контракт и рецепт](edit-annular-copy.md). Меняются только центр и радиус двух названных кривых, их payload/payload_hash и `meta.modified_at`; оба UUID, их порядок, высота, refs и все прочие SQL-ячейки сохраняются, источник побайтово цел. Высота остаётся у `edit-extrude`. Discovery — аддитивный `annulus_edit` в `inspect --json`, работает и без ядра. Произвольные отверстия, boolean Cut, circle constraints, in-place Save, рисование мышью, новая схема БД и новые FFI не входят. |
| Параметрическая окружность: сохранённый радиус и закреплённый центр | `Edit constraints <имя> — <UUID>…` → строка `Circle · <UUID> · centre (x, y) mm · radius r mm`, поле `Radius mm`, прежние `Fixed X/Y (mm)`, кнопки `Add radius` и `Add Fixed centre`; один Apply — один Undo, 128 шагов | `ferritecad edit-sketch-constraints-copy source.fcad --sketch UUID --expect-version HASH --request request.json -o copy.fcad [--json]` с формами `{"rule":"radius",…}` и `{"rule":"fixed",…,"at":"center",…}` | `AddSketchConstraint::Circle` → `prepare_sketch_constraints` → прежний `edit_object_copy`; solver получил `CircleId`, `Circle{center: PointId, radius}` и `Constraint::Radius`, C ABI — отдельный `circles[2]`, `FcGcsCircle` и `fc_gcs_session_radii` | Одна неконструкционная XY `Circle` в прежнем frame §25K; [контракт и рецепт](circle-radius-constraints.md). Геометрия берётся из решения PlaneGCS; сохранённый Sketch остаётся исходным приближением, как у Line-ограничений. Измеренные DOF: свободная окружность 3, с радиусом 2, с закреплённым центром 1, с обоими 0. Sketch с ограничением про окружность хранится как payload v3 и объявляет `sketch.constraints.circle.v1`; прежние v1/v2 не меняются, SQL-схема не поднята. Diameter, Tangent, EqualRadius, Arc, смешанные профили, отверстия, mouse drag, live solve и выражения вместо literal radius не входят; §25L/M и прежние редакторы контрактов не теряют. |
| Первый Cut в существующем Body: круглый инструмент на конечную глубину | `Cut circle into <имя> — <UUID>…` у поддерживаемого Body; форма называет базовую XY-плоскость и направление +Z, показывает размеры детали, её высоту и фичу, которую правка изменит; поля Centre X/Y, Radius, Depth, один подтверждённый `Apply cut` на все четыре, bounded Undo/Redo, `Save cut copy…` | `ferritecad cut-circular-copy source.fcad --body UUID --expect-version HASH --request request.json -o copy.fcad [--json]`, request v1 `{"request_version":1,"center_mm":[x,y],"radius_mm":N,"depth_mm":N}` | `CircularCut` → `prepare_circular_cut` → узкий `write_circular_cut` → прежний `edit_object_copy`; `GeometryKernel::cut` + `fc_occt_cut` (`BRepAlgoAPI_Cut`) с настоящей history по названным граням обоих входов | Прежний frame §25K плюс осепараллельный прямоугольный неограниченный профиль; [контракт и рецепт](circular-cut-copy.md). Фича называет фичу (`Extrude.previous`, ребро `Predecessor`), Body называет tip; владение не хранится дважды, граф ацикличен по построению, payload v2 + `feature.predecessor.v1`. Имена — из OCCT history: инструмент под `ExtrudeSide`/`ExtrudeCap`, прежние грани под `CarriedCap`/`CarriedSide`, удалённое записано как удалённое. Ключ Cut зависит от обоих входов; cold и cache Miss/Hit совпадают. Касание внешней стенки, промах, уничтожение тела и расщепление отказывают. Face attachment, произвольные плоскости, Add/Intersect, Fillet, ThroughAll, live preview и in-place Save не входят; после Cut прежние четырёхобъектные редакторы честно отказывают. |
| До 16 отдельных circular Cut в том же Body | Та же форма Add cut, с ограниченной по высоте прокруткой инструментов и требуемым зазором | Та же `cut-circular-copy` и request v1 | Прежний shared copy job; qualified origin names через OCCT history, archive v3 | До 16 попарно разделённых дисков; все прежние Cut/refs/SQL сохраняются. На 16 добавить нельзя, править можно любой. [Контракт и рецепт](circular-cut-history.md). |
| Правка сохранённого circular Cut | `Edit cut <имя> — <UUID>…`: сохранённые числа, Apply, Undo/Redo, Save Cancel и сохранность draft при отказах | `ferritecad edit-circular-cut source.fcad --feature UUID --expect-version HASH --request request.json -o copy.fcad [--json]` | `CircularCutEdit` → `prepare_cut_parameters` → узкий `write_cut_parameters` → прежний `edit_object_copy`; каталог из одного снимка | Любой Cut линейной истории до 16 над прямоугольной плитой (§26E). Прежние UUID/история/refs/SQL сохранены. Через→карман добавляет собственную ссылку на дно; по одному OriginCap на каждом последующем producer. Карман→через отказывает со всеми защищаемыми UUID. Discovery: `features[].circular_cut_edit`; прежний `editable` у Cut остаётся false. [Контракт и рецепт](circular-cut-history.md). |
| Параметрическое кольцо: два радиуса, концентричность и закреплённый общий центр | `Edit constraints <имя> — <UUID>…` → строки `Boundary circle · <UUID> · centre (x, y) mm · radius R mm` и `Bore circle · …`, поле `Radius mm`, прежние `Fixed X/Y (mm)`, `Pair Circle A`/`Pair Circle B` и `Add Concentric`; persisted строки `Concentric`/`Radius`/`Fixed centre` с собственными Remove; один Apply — один Undo, 128 шагов | та же `ferritecad edit-sketch-constraints-copy source.fcad --sketch UUID --expect-version HASH --request request.json -o copy.fcad [--json]`, новая форма `{"rule":"concentric","a_curve_id":UUID_A,"b_curve_id":UUID_B}` рядом с прежними `radius`/`fixed at center` | `AddSketchConstraint::Concentric` → `prepare_sketch_constraints` → прежний `edit_object_copy`; **solver и C ABI не менялись** — концентричность это прежний `Constraint::Coincident` двух центров, а центр окружности стал адресуемой точкой ещё в §25N | Ровно две неконструкционные XY `Circle` в прежнем frame §25K; [контракт и рецепт](annular-circle-constraints.md). Роли boundary/bore — по сохранённым радиусам через ту же функцию, что у §25M, и должны сохраниться **после** решения: пара, поменявшаяся размерами, отказывается. Измеренные DOF: две свободные окружности 6, с концентричностью 4, с двумя радиусами 2, с `Fixed` одного центра 0; сдвиг единственного pin двигает оба центра. Persisted layout и capability не поднимались: `Coincident` двух центров уже требует payload v3 и `sketch.constraints.circle.v1`. Writer различает снимаемую концентричность и обязательное Line-замыкание по тому, что говорит правило. Снятие концентричности допустимо, только если результат остаётся кольцом. Discovery — аддитивное поле `role` у `constraint_edit.circles`. Diameter, Tangent, EqualRadius, Arc, несколько отверстий, неконцентрический результат, mouse drag, live solve, boolean Cut и in-place Save не входят. |
| Центр и радиус сохранённой окружности → новая копия | `Edit circle <имя> — <UUID>…`, численные Center X/Y и Radius, показ сохранённых центра/радиуса/высоты и обоих UUID, bounded Undo/Redo черновика, `Save edited circle copy…` | `ferritecad edit-circle source.fcad --sketch UUID --expect-version HASH --request request.json -o copy.fcad [--json]` | document `CircleChoice`/`replace_circle_geometry` + узкий writer → jobs `edit_circle_copy` поверх прежнего `edit_object_copy`: тот же snapshot, baseline rebuild, проверка refs, повторная проверка версии и атомарная публикация | [Контракт и рецепт](edit-circle-copy.md). Ровно класс §25J: одна неконструкционная неограниченная `Circle`. Меняются только центр и радиус названной окружности; UUID окружности, высота Extrude, refs и все прочие SQL-ячейки сохраняются. `inspect --json` даёт отдельное `circle_edit`, не меняя смысла `editable`/`vertices`/`constraint_edit`. Высоту по-прежнему меняет `edit-extrude`. |
| Координаты сохранённого Line Sketch → новая копия | `Edit Sketch <name> — <UUID>…`, прежний canvas/draft и async edit worker | `edit-sketch-copy source.fcad --sketch UUID --expect-version TOKEN --request coordinates.json -o copy.fcad [--json]` | document `SketchChoice`/polygon policy → jobs `edit_sketch_copy` → общий snapshot/cold/Keep путь | Тот же набор curve IDs в прежнем порядке и winding; [контракт](edit-sketch-copy.md). |
| Проверить документ без ядра | Нет отдельной команды. Неоткрываемый файл даёт `Open failed`; это отказ загрузки, не отчёт `validate`. | `ferritecad validate <path> [--json]` | Общий [`validate_document`](../crates/ferritecad-jobs/src/validate.rs): один `Document::open_read_only`, UUID и `Document::validate` из закреплённого снимка, закрытие SQLite до owned результата. Правила и stable codes принадлежат document. | Без writes/миграции/kernel. JSON ok:true и valid:true/false дают 0/1; operational error — 2, delivery failure — 7. Warnings сохраняются. Старые schema/WAL/minimum reader теперь честно отказывают также в text validate. Это не гарантия геометрии/STEP/FBX complete; UI validate/repair отсутствуют. [Протокол](read-only-validation.md). |
| Cold rebuild нативного графа | Не команда. Open/Export сами делают холодное перестроение как часть чтения. | `ferritecad rebuild --cold <path>` | [`rebuild_cold`](../crates/ferritecad-eval/src/cold.rs) в `ferritecad-eval` против [`OcctKernel`](../crates/ferritecad-occt/src/kernel.rs) / [`GeometryKernel`](../crates/ferritecad-kernel/src/kernel.rs). Документ — `open_read_only`. | Без `--cold` команда отказывает (exit 2, **измерено**). [`rebuild_cached`](../crates/ferritecad-eval/src/cold.rs) есть в библиотеке и тестах и **не** предлагается CLI/UI. Публичного cached-rebuild нет. |
| Граф зависимостей | Нет. Список определений во вьюере — не dump графа. | `ferritecad dump-graph <path> [--format <text\|dot>]` | [`Document::evaluation_order`](../crates/ferritecad-document/src/document.rs), [`evaluation_order`](../crates/ferritecad-document/src/graph.rs), печать — [`render::graph_text` / `graph_dot`](../crates/ferritecad-cli/src/render.rs). | `dot` — Graphviz, не JSON-контракт всех команд. `--format json` нет (**измерено**, clap exit 2). |
| Topology references: что документ назвал и держится ли это | Клик по именованной грани/ребру/углу нативного тела показывает переносимое имя в инспекторе. Это просмотр, не отчёт по всем ссылкам. | `ferritecad print-topology <path>` | Хранение — [`Document::topology_refs`](../crates/ferritecad-document/src/document.rs). Разрешение — `RebuildResult::resolve` после `rebuild_cold`. Отчёт и коды — [`topology::print_topology`](../crates/ferritecad-cli/src/topology.rs). *Из кода:* lost → exit 3, invalid → 1, unsupported/прочее → 2. | Нет команды «добавить/изменить ссылку». Роль сегмента эскиза как самостоятельной ссылки в плане помечена как граница, не упущение CLI. Импортированная топология не именуется durably. |
| STEP → новый `.fcad` (байты источника внутри) | Нет. Open принимает `.fcad`, не STEP. Отдельное продолжение после §23 это признаёт. | `ferritecad import-step <file.step> -o <out.fcad> [--name <name>] [--force] [--json]` | Общий [`import_step_document`](../crates/ferritecad-jobs/src/import.rs): одно чтение STEP, kernel factory/import callback, владение handles, `Document::store_step_import`, закрытие SQLite и атомарный publish через `Temporary`. Text/JSON CLI готовят один request и отображают owned outcome; JSON 0/4 — publication, 5 — typed reader rejection, 2 — operational error, 7 — delivery failure. [Протокол request/outcome/отмены](shared-step-import.md). | UI не импортирует STEP; CLI cancellation flags отсутствуют. Импорт не делает сборку редактируемой. Нет STEP-экспорта (команды `export-step` нет). |
| Binary STL одного тела | `Export STL…` → явный UUID при нескольких Body → параметры → `Save STL…`; Cancel и подтверждение Replace. | `ferritecad export-stl <path> -o <file.stl> [--solid <name-or-id>] [--linear-deflection <mm>] [--angular-deflection <rad>] [--force] [--json]` | Общий [`export_document_as_stl`](../crates/ferritecad-jobs/src/stl.rs): выбор Body, одно read-only чтение, cold rebuild, `GeometryKernel::tessellate`, `binary_stl`, `Temporary`. CLI — адаптер, UI — owned worker. | Только один native Body; imported-only, сборки и несколько тел одним STL не поддерживаются. JSON сообщает опубликованные destination/Body/triangles/bytes (§24F). [§24E: протокол и границы наблюдения](stl-export-verification.md). |
| FBX 7.4 ASCII всей модели | `Export FBX…` при принятой сцене. Замена существующего файла — вопрос окна, не `--force`. Отмена есть. | `ferritecad export-fbx <path> -o <file.fbx> [--force] [--json]` | Один маршрут: [`ferritecad_jobs::export_document_as_fbx`](../crates/ferritecad-jobs/src/fbx.rs) ← [`export_scene`](../crates/ferritecad-scene/src/export.rs) ← `prepare::load`. CLI: [`export_fbx::export_fbx`](../crates/ferritecad-cli/src/export_fbx.rs). UI: [`exports::export_into`](../crates/ferritecad-app/src/exports.rs). | Это два клиента одной операции. JSON v1 сообщает опубликованный полный/частичный результат, счётчики и typed omissions; partial — ok:true и exit 6 (§24G). CLI не выставляет отмену; UI — выставляет. |
| Диагностика sketch solver (что слинковано) | Не панель. Команда бинаря вьюера. | Нет у `ferritecad`. Есть `ferritecad-viewer --solver-info` (окно не открывается). | [`ferritecad_sketch_solver::provenance`](../crates/ferritecad-sketch-solver/src/lib.rs), маршрутизация — [`solver_info`](../crates/ferritecad-app/src/main.rs). Что rebuild нашёл по constrained sketch — `SketchSolveReport` в `ferritecad-eval`; во вьюере панель `Sketch solves` после Open. | CLI документа эту диагностику не печатает. Нет команды «решить эскиз отдельно от rebuild». Сборка без planegcs отвечает unavailable и exit 3 (*из README/кода*). В этом срезе solver **available**, exit 0 (**измерено**). |
| Удалить регенерируемый `.fcad-cache` | Нет. Viewer/CLI cold-путь sidecar не пишут. | `ferritecad clear-cache <path>` | [`CacheStore::discard`](../crates/ferritecad-document/src/cache.rs). | Пользовательского warm rebuild нет, поэтому после `rebuild --cold` sidecar обычно отсутствует (**измерено**: «no cache sidecar»). Библиотечный `rebuild_cached` кэш писать умеет — это не пользовательская операция. |

## Ограничения моделирования (чтобы агент не обещал лишнего)

Создать sample plate **не значит** произвольно редактировать параметры, эскизы и фичи существующей модели. `create --sample --size` и `New… → Sample plate` задают прямоугольник и высоту **в момент создания**. §24C позволяет изменить существующее постоянное Blind-выдавливание в новой копии (см. ниже). §25A позволяет создать собственный Line-полигон с Extrude, §25B — изменить координаты сохранённого Sketch, включая ширину и глубину плиты. Число и порядок сегментов сохранённого Sketch менять нельзя; отдельных операций отверстия, fillet или создания параметра нет. В документе плиты нет объектов `Parameter`: ширина и глубина — координаты четырёх линий эскиза, высота — константа `Blind` у `Extrude` ([`create.rs`](../crates/ferritecad-jobs/src/create.rs)). Типы `Parameter` и `SketchConstraint` в `ferritecad-document` есть; пользовательской операции «задать параметр / поставить ограничение» нет.

Плита — **шаблон**, и окно называет её так. `New…` во вьюере не вводит несохранённый редактируемый документ: имя файла выбирается до создания, результат открывается обычным Open, `Save` / `Save As` для уже существующей модели по-прежнему нет.

Допустимость размеров решает документ, а не форма и не флаг. **Измерено:** ширина и глубина принимают ноль и отрицательные значения (это координаты эскиза), высота обязана быть положительной (`extrude distance must be positive`), любой не-конечный размер отказывается (`model values must be finite`). §24B этих правил не менял.

Импортированная STEP assembly **не редактор сборок**. `import-step` сохраняет definitions, placements и исходные байты. Нет команд разместить компонент, сопрячь, подавить, переименовать дерево или пересобрать сборку как нативную. `rebuild --cold` по такому документу **не пересчитывает** хранимую геометрию (**измерено**: «0 objects evaluated, 0 shapes built» и отдельная строка про imported object).

Агент **не получит** по одному заданию:

- произвольные операции моделирования за пределами Line-полигона → Blind Extrude, поддержанной координатной правки и изменения Blind-высоты;
- редактируемую сборку или чертёж;
- STEP наружу, DXF, ЕСКД;
- JSON-контракт всех команд, stdin-пакет, отмену CLI.

Отказ должен быть отказом, а не «почти той же» моделью.

## Что во вьюере не является разрывом предметного CLI

Орбита, pan, zoom, именованные виды, перспектива/ортогональ, hover, клик, Hide/Isolate/Show all, Frame, сетка, `Undo visibility` меняют **картинку сессии**. Они не пишутся в `.fcad` и не входят в экспорт: FBX — холодное чтение сохранённого файла, не копия GPU-снимка (README и [`ferritecad-app` exports](../crates/ferritecad-app/src/exports.rs)). Выбор именует definition и хранимые native names; GPU pick id в документ не сохраняется.

Это не пробел автоматизации модели. Если позже понадобится снимок или чертёж с заданным ракурсом, ракурс станет **явным параметром новой операции**, которой сейчас нет.

## Контракт текущего CLI

Два бинаря: `ferritecad` (предметные команды) и `ferritecad-viewer` (окно и `--solver-info`). Справка вьюера как у clap нет: лишний аргумент и `--help` — usage, exit 2 (**измерено**).

### Единицы

- Внутри модели длины — миллиметры, углы — радианы ([`Unit`](../crates/ferritecad-types/src/units.rs)).
- `create --length-unit` / `--angle-unit` — **единицы отображения** (по умолчанию `mm` / `deg`). Значения всё равно хранятся в мм и радианах.
- `--size` задаётся **в миллиметрах**, независимо от `--length-unit` (текст `--help`).
- `export-stl --linear-deflection` — мм (по умолчанию 0.01); `--angular-deflection` — радианы (по умолчанию 0.5).

### Адресация объектов

- Нативные объекты — `ObjectId` (печатаемый UUID) и необязательное имя (`XY`, `Profile`, `Extrude1`, `Plate`).
- `export-stl --solid` — имя или идентификатор **тела** (`Body`). При одном теле флаг необязателен; при нескольких — обязателен, без угадывания первого.
- Topology refs — `StableEntityId`, семантическая роль, правило выборки. Не индексы граней, не session handles.
- Импортированная деталь — `ImportedSourceId` + ключ вида `step.product_definition#5` (локален одному источнику).
- Экранные координаты, GPU picking id и порядок обхода **не** являются адресами CLI.

При совпадающих именах `--solid` отказывает и предлагает UUID. Строка, разбираемая как UUID, адресует идентификатор, даже если такое же имя есть у другого тела (*из кода*).

Идентификаторы нового `create` каждый раз другие. Рецепт не должен требовать конкретного UUID.

### Потоки и форматы

| Поток | Что туда пишется |
| --- | --- |
| stdout | Отчёты команд, включая невалидный документ (`validate`, exit 1), потерянные ссылки (`print-topology`, exit 3), замечания/отказ STEP (exit 4/5) и итог опубликованного partial FBX (exit 6). Также `--help`, `--version`, `--solver-info`. Наличие stdout не означает exit 0. |
| stderr | `error [kind]: …` и цепочка `caused by:` для `CadError`; usage при ошибке аргументов clap; `note:` если путь `create` без `.fcad`; подробности partial FBX. |

Форматы вывода сейчас:

- проза / фиксированные текстовые отчёты;
- `dump-graph --format dot` — Graphviz DOT, только эта команда;
- STL — бинарный, 80-байтовый заголовок без даты (**измерено**: `FerriteCAD binary STL. Units are millimetres. No timestamp, by design.`);
- FBX — ASCII 7.4;
- `inspect --json`, `edit-extrude --json`, `create --json`, `export-stl --json`, `export-fbx --json`, `import-step --json` и `validate --json` — один объект JSON v1 + LF, [точный контракт](cli-json-v1.md).

**Нет:** общего JSON всех команд, stdin как входа документа, пакетного списка задач, флага отмены у `ferritecad`. `inspect -` открывает файл с именем `-`, а не стандартный ввод (**измерено**). Существующий DOT не означает JSON-контракта остальных команд.

### Коды исхода `ferritecad`

| Код | Когда | Источник |
| --- | --- | --- |
| 0 | Команда выполнена; validate не нашёл errors, warnings допустимы | **измерено** на create/inspect/validate/dump-graph/rebuild --cold/print-topology (решённые ссылки)/import без диагностики/clear-cache/полном STL и FBX |
| 1 | `validate`: проверка состоялась, есть errors (JSON ok:true, valid:false); `print-topology`: contradictory stored ref | validate **измерено** в §24J; topology — *из кода* |
| 2 | Не запустилась или отказала: clap usage, `CadError` по умолчанию, нет `--cold`, нет тел для STL, файл уже есть | **измерено** |
| 3 | `print-topology`: документ перестроился, имя потеряно | *из кода* |
| 4 | `import-step`: документ записан целиком, чтение что-то сообщило | **измерено**, §24H native job/CLI parity |
| 5 | `import-step`: ничего не записано (отказ читать); отчёт находится на stdout, в JSON — error.code=reader_rejected и step_read | **измерено при ревью** на файле, не являющемся STEP |
| 6 | `export-fbx`: файл опубликован и неполон | *из кода / README* |
| 7 | Только JSON v1: отчёт не доставлен; документ, копия правки, STL или полный/частичный FBX уже могли быть опубликованы, автоматический повтор недопустим | **измерено** реальным закрытым stdout |

Скрипт не должен считать 4 или 6 нулём и не должен считать 6 ошибкой «файла нет».

`ferritecad-viewer --solver-info`: 0 — solver есть (**измерено**); 3 — нет (*из кода*); 2 — usage (**измерено**).

### Замена файлов

- `create` существующий путь не заменяет и **не имеет** `--force`. Текст: `already exists; creating would destroy it` (**измерено**, exit 2).
- `import-step`, `export-stl`, `export-fbx` без `--force` не заменяют назначение. Текст: `already exists; pass --force to replace it` (**измерено**, exit 2, байты назначения не меняются).
- Исходник не может быть назначением даже с `--force` (*из кода*: `refuse_source_as_destination`).
- `import-step` и оба экспорта публикуют через scratch рядом с назначением: ошибка выполнения оставляет назначение как было. UI для FBX вместо `--force` спрашивает `Replace existing file?`.
- `create` публикует через scratch рядом с назначением, как `import-step` и оба экспорта (§24B). Документ строится целиком, соединение SQLite закрывается, и только потом файл появляется по указанному пути одним атомарным no-clobber шагом. Файл, появившийся после первоначальной проверки, сохраняется, а не заменяется.
- UI на занятое назначение отвечает `already exists; choose a different file name` — своими словами и без флага, которого у окна нет. Общий `create` не заменяет файл. Системный Save-диалог macOS при выборе существующего имени может сначала показать Replace; даже после подтверждения приложение отказывает и сохраняет прежний файл.

### Чтение и возможная запись документа

`dump-graph` и `clear-cache` используют `Document::open`, который может мигрировать старую схему и настраивает SQLite connection. Эти команды не обещают неизменность файла во всех случаях. Измерение неизменных байтов/mtime текущего документа не доказывает read-only контракт старых файлов. `validate` (с §24J), `inspect` (с §24C), Viewer, `rebuild --cold`, `print-topology` и оба экспорта используют `open_read_only`: старую схему или WAL-состояние отказывают, а не мигрируют (*из кода*).

§24J: общий read-only reader также отказывает при существующих WAL/SHM sidecars
с DELETE-заголовком (включая resolved symlink target): иначе SQLite может менять
SHM даже при read-only connection. Ни sidecars, ни hot journal recovery не
являются разрешением на запись.

§24H: общий STEP job повторяет source/alias protection непосредственно перед publish.
Keep сохраняет появившееся назначение, Replace публикует только готовый закрытый scratch.
Фазовая отмена доступна библиотечному клиенту; блокирующий OCCT import прерывается
только на границе вызова, и поздняя отмена не отзывает Published.

### Ошибка

Ошибка выполнения экспорта или импорта (exit 2) не публикует полузаписанный STL/FBX/новый `.fcad` импорта (*из кода* и README). Это не относится к явным результатам 4 и 6: файл уже опубликован. `CadError` идёт на stderr с `ErrorKind`; отказ STEP (exit 5) идёт отдельным отчётом на stdout. Отказ Open во вьюере не подменяет картинку. Операции CLI, принимающие `OperationContext`, используют `default()` и не предоставляют отмену.

Отказ или отмена, замеченная до публикации, не оставляют нового `.fcad` и scratch (§24B, **измерено**). Поздняя отмена уже опубликованный файл не удаляет: UI сообщает Created, но не переключает принятую сцену.

**Исправленный дефект `create`, §24B.** До §24B `ferritecad create bad.fcad --sample --size NaN 50 12` возвращал exit 2 (`model values must be finite`) и оставлял по новому пути пустой `.fcad` с нулём объектов: файл создавался до наполнения, а откат `Document::write` уже созданный файл не отменял. Теперь это два разных барьера — транзакция внутри документа и атомарная публикация целого файла, — и оба обязательны. Failing-first проверки живут в [`crates/ferritecad-cli/tests/create.rs`](../crates/ferritecad-cli/tests/create.rs): они запускают настоящий процесс и падают именно на оставленном файле; проверяются оба вида отказа — до транзакции (`--size NaN 50 12`) и внутри неё (`--size 60 40 NaN`, высота строится уже после трёх записанных объектов).

## Рецепты на существующих публичных командах

Примеры ниже — shell для macOS/Linux (или Bash с подходящим окружением в Windows); это не PowerShell. Публичный `ferritecad` должен быть в `PATH`; для поставки можно добавить её каталог CLI, а для checkout-сборки настроить библиотеки по README. Каждый блок запускается в отдельном subshell, создаёт собственный временный каталог и оставляет результаты в нём. `set -eu` останавливает блок при неожиданной ошибке; ожидаемые отказы в рецепте 3 разобраны отдельно. UUID в выводе будут другими. Не нужно читать исходники. Сборка должна быть с Open CASCADE: ответ `unsupported` / «this build has no Open CASCADE» — не успех.

Исходные fixtures репозитория не трогать. Для STEP копировать файл во **свой** каталог и удалять только эту копию.

Обработка кодов: продолжать только при 0 (в этих рецептах 4 и 6 не ожидаются). 2 — остановиться и не считать файл обновлённым.

### 1. Sample plate заданного размера → проверка → cold rebuild → экспорт

```sh
(
    set -eu
    recipe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-plate.XXXXXX")
    cd "$recipe_dir"
    ferritecad create plate.fcad --sample --size 80 50 12
    ferritecad validate plate.fcad
    ferritecad rebuild --cold plate.fcad
    ferritecad export-stl plate.fcad --output plate.stl
    ferritecad export-fbx plate.fcad --output plate.fbx
    printf 'Results: %s\n' "$recipe_dir"
)
```

Ожидание при exit 0:

- `created plate.fcad (…)`
- `plate.fcad is valid (0 warnings)`
- rebuild: ядро `occt 8.0.1 …`, `4 objects evaluated, 1 shape built`, `3 of 3 stored references resolved`; у `Extrude1` в отчёте `solid, 6 named faces` (имена rebuild, не число **хранимых** ссылок — их три)
- STL: `12 triangles, 684 bytes` с плиты `Plate`, deflection `0.01 mm` / `0.5 rad`
- FBX: `complete: nothing this document holds was left out of the file`

Полезные, но необязательные проверки: `inspect`, `print-topology` (три `resolved`), `dump-graph`. `clear-cache` после этого cold-пути сообщает, что sidecar нет — это нормально.

### 2. Небольшой STEP → `.fcad` → экспорт после удаления своей копии STEP

Этот блок запускается из корня checkout и копирует маленький `fixtures/step/canonical/01-single-part.step`. Для своего STEP задайте `source_step` абсолютным путём к нему; размеры и текст отчёта тогда будут зависеть от файла.

```sh
(
    set -eu
    source_step="$PWD/fixtures/step/canonical/01-single-part.step"
    recipe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-step.XXXXXX")
    cp "$source_step" "$recipe_dir/part.step"
    cd "$recipe_dir"
    ferritecad import-step part.step --output part.fcad
    rm part.step
    ferritecad export-fbx part.fcad --output part.fbx
    printf 'Results: %s\n' "$recipe_dir"
)
```

Ожидание при exit 0:

- import: `15408 byte(s) stored whole`, `1 definition, 1 placement`, ключ `step.product_definition#5`, и честное «nothing was reported while reading it» / «that describes this reader, not the file»
- после `rm` исходный fixture на месте, своей копии нет
- FBX всё равно пишется: геометрия берётся из байтов внутри `.fcad`

Не использовать здесь `export-stl`: у импортированного документа нет `Body`, команда отвечает `contains no bodies to export` и exit 2. Это текущий контракт STL, не сбой импорта. `rebuild --cold` по такому файлу не «пересобирает солид из фич»; он сообщает, что imported geometry не пересчитывается.

### 3. Отказ перезаписать результат без `--force`

```sh
(
    set -eu
    recipe_dir=$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-replace.XXXXXX")
    cd "$recipe_dir"
    ferritecad create plate.fcad --sample --size 80 50 12
    ferritecad export-fbx plate.fcad --output plate.fbx
    cp plate.fcad before.fcad
    cp plate.fbx before.fbx
    if ferritecad create plate.fcad --sample --size 1 1 1; then
        printf 'Unexpected create success\n' >&2
        exit 1
    else
        code=$?
        [ "$code" -eq 2 ] || exit "$code"
    fi
    cmp before.fcad plate.fcad
    if ferritecad export-fbx plate.fcad --output plate.fbx; then
        printf 'Unexpected export success\n' >&2
        exit 1
    else
        code=$?
        [ "$code" -eq 2 ] || exit "$code"
    fi
    cmp before.fbx plate.fbx
    ferritecad export-fbx plate.fcad --output plate.fbx --force
    printf 'Results: %s\n' "$recipe_dir"
)
```

Ожидание:

- первый create и первый export — exit 0
- второй create — exit 2, `already exists; creating would destroy it`; `--force` у `create` нет
- второй export без `--force` — exit 2, `already exists; pass --force to replace it`; `plate.fbx` не меняется
- третий, с `--force` — exit 0, файл заменён целиком

То же для `export-stl` и `import-step` на существующее назначение.

## Историческая проверка §24A

- База: `main` @ `aac7f6c72dcb6a85b708946c51eab615c985a706` (merge PR #12). Ветка документации: `cli-capability-map`.
- CLI: `/Users/drt/Desktop/github/ferrite-cad/target/release/ferritecad`, `ferritecad 0.0.1`, SHA-256 `b6c5d543530d8d2a8109b6f2b085361d18556b5c7123075abee3da592f69b1ae`. Слинкован с `@rpath/libTKernel.8.0.dylib` (OCCT 8.0.1). У этого checkout-бинаря нет `LC_RPATH`: для запуска из `target/release` нужен `DYLD_LIBRARY_PATH` на `vendor/install/lib` (и `vendor/planegcs` для вьюера). Поставочный `FerriteCAD.app/Contents/MacOS/ferritecad` библиотеки ищет сам.
- Viewer: тот же `target/release/ferritecad-viewer`. `--solver-info` → exit 0, `sketch solver: available`, provenance planegcs FreeCAD 1.0.1.
- Рецепт 1: create `--size 80 50 12` → validate → `rebuild --cold` (occt 8.0.1, 1 shape, 3/3 refs) → STL 684 байта / 12 треугольников → FBX 4104 байта, complete. Все exit 0. Sidecar после cold rebuild не появился.
- Рецепт 2: копия `01-single-part.step` (15408 байт) → import exit 0 → удалена только копия → FBX 4103 байта, complete, exit 0. Оригинал fixture не менялся. `export-stl` по этому `.fcad` — exit 2, тел нет.
- Рецепт 3: повторный `create` и экспорт без `--force` — exit 2, хеши STL/FBX не изменились; `export-fbx --force` — exit 0.
- Дополнительно **измерено**: пустой `create`; FBX пустого документа 1218 байт, complete, 0 nodes; `create` без `.fcad` пишет файл и `note:` на stderr, exit 0; clap unknown command / missing args / `--format json` — exit 2; `inspect` текущего файла байты не переписывает.

**Независимое ревью, 2026-09-06:** три shell-блока извлечены из этого документа и выполнены как написаны в отдельных временных каталогах (CLI и native libraries подготовлены в окружении). Подтверждены 684-байтовый STL, FBX 4104/4103 байта, cold rebuild 3/3 refs, отсутствие sidecar и отказ create/FBX без изменения назначения; отдельно — `inspect` с сохранением байтов/mtime, `print-topology`, DOT, `clear-cache`, обязательность `--cold`, отказ STL для imported-only, no-clobber STL/импорта и `--solver-info` (0). Все локальные ссылки существуют; shell-блоки проходят shellcheck. На входе, не являющемся STEP, измерен exit 5 с отчётом на stdout и без нового `.fcad`. Воспроизведён дефект `create` с `NaN`, описанный выше. Rust-код этот документальный срез не меняет.

Не измерялись в §24A и его ревью (на тот момент — *из кода/тестов/README*): exit 1/3/4/6, partial FBX, миграция старой схемы при `inspect`, побайтовое равенство UI- и CLI-FBX (зафиксировано ревью §23, здесь не повторялось), Windows/Linux CLI.

Локальные измерения §24J, включая JSON validate и исправление сохранности WAL/SHM,
записаны в [новом протоколе](read-only-validation-verification.md).

## Найденные пробелы

| Пробел | Статус |
| --- | --- |
| UI создаёт пустой документ или sample plate и меняет постоянную Blind-высоту в новой копии; нет in-place Save и STEP Open | факт |
| Нет произвольной правки параметров/эскиза/фич | Есть только изменение постоянной Blind-высоты в новой копии (§24C) |
| Нет UI/CLI STEP-экспорта | факт |
| `export-stl` только для `Body`; imported-only документ не экспортируется в STL | факт, **измерено** |
| inspect, validate, graph, topology, rebuild, import, cache — один клиент (CLI), не два | STL с §24E имеет два клиента общей jobs-операции |
| `rebuild_cached` есть в библиотеке, пользовательского warm rebuild нет | факт |
| Нет JSON всех команд, stdin/batch/отмены CLI | JSON v1 ограничен inspect/edit-extrude/create/export-stl/export-fbx/import-step/validate/create-sketch-extrude/create-circle-extrude/create-annular-extrude/edit-circle/edit-annular/edit-sketch-copy/edit-sketch-constraints-copy/cut-circular-copy/edit-circular-cut/create-sketch-revolve/edit-revolve-angle (§24D–J, §25A, §25E–J, §25K–O, §26A–E, §27A–E) |
| `create` без `--force` и с другим текстом отказа, чем publish-команды | факт |
| Случайные UUID независимых `create` не равны | обещание §4.5, не баг |
| Временная видимость и камера не в CLI | не предметный разрыв; см. выше |

Независимое ревью воспроизвело оставленный пустой файл после отказа `create --sample --size NaN 50 12` (см. «Ошибка»). Он исправлен в §24B общей операцией создания; failing-first запускает реальный CLI на старом маршруте. `export-stl` на imported STEP ведёт себя согласно своему правилу выбора тел.

## §24B: общий create и New

Реализован `ferritecad_jobs::create_document(CreateDocumentRequest, OperationContext)` →
`CreatedDocument` (путь и document ID). Типизированный `NewDocument` задаёт Empty или
SamplePlate(PlateSize); размеры хранятся в мм независимо от display units.
Построение графа больше не принадлежит бинарному CLI-крейту. Используются существующие
Document::write и Temporary: транзакция модели, затем закрытие SQLite и атомарный
no-clobber. Последняя проверка CancelToken стоит перед публикацией; поздняя отмена
не отменяет уже опубликованный файл.

New → форма → системный Save-диалог → worker → обычный асинхронный Open.
New недоступен во время Open/Export или подтверждения экспорта; Open/Export
недоступны при открытой форме New и до результата create. Cancel creation ждёт
ответ worker: успех после поздней отмены сообщается как созданный файл, но не
переключает сцену. Ошибка создания/загрузки и отмена сохраняют принятую сцену и
export source. Пустой сохранённый документ отличается от окна без документа.

Проверки запускают оба клиента: настоящий процесс `ferritecad create` и командный
маршрут UI (`ask_new` / `start_new` / `finish_create`) без диалоговых кликов.
Сравниваются полные payloads, parent/dependency links, роли и связь topology refs
с сегментами по согласованному сопоставлению UUID; самостоятельные создания
получают разные identity. Native gate делает cold rebuild с 3/3 refs, сравнивает
STL и FBX после сопоставления identity, а CLI/UI FBX одного документа — побайтово.
Отдельно проверены отказ внутри транзакции, гонка назначения перед publish,
отмена после закрытия SQLite, уборка scratch/sidecars, поздняя отмена и устаревший Open.
Обычный CI запускает CLI/app/UI/jobs/document gates; OCCT pin включает native gate.

§24 целиком не завершён. Ограниченная правка существующей модели добавлена §24C,
JSON семи команд — §24D/§24D-1/§24F/§24G/§24I/§24J. Stdin/batch, произвольный редактор и STEP из UI остаются недоступны.


## §24C: edit-extrude и Edit extrusion

| Операция | UI | CLI | Владелец |
| --- | --- | --- | --- |
| Изменить расстояние существующего constant Blind Extrude в новой копии той же модели | `Edit extrusion…` → явный UUID/имя, текущие mm → новое значение → `Save new file…` → обычный Open | `edit-extrude <source.fcad> --feature <uuid> --distance-mm <n> -o <new.fcad> [--expect-version <hash>] [--json]` | `ferritecad-jobs::edit_extrude_copy`; SQLite snapshot/version — document; геометрия — общий evaluator |

В JSON v1 UUID берётся из `result.features[].feature_id`, версия — `result.content_version`.
Текстовый `inspect` также печатает UUID в строке `feature.extrude` и `content version`.
При нескольких фичах UUID выбирается явно. Имена, позиции в списке и GPU IDs не адресуют
правку. Ввод всегда mm, независимо от `--length-unit` source. Сохраняются DocumentId,
ObjectId, IDs сегментов, refs и остальные данные; это версия той же модели, в отличие
от независимой модели `create`/New. Новый filename не означает новые UUID.

Source не перезаписывается и не мигрируется, output никогда не заменяется; `--force`
и in-place Save отсутствуют. Файл публикуется после одной транзакции, валидации,
cold rebuild и проверки ранее разрешимых refs. Stale source отказывает с предложением
reopen; при работе от результата прежнего inspect используйте `--expect-version`.
Версия учитывает также implicit rowid; после смены алгоритма content version
получите hash новым inspect. Если дополнительная таблица затеняет все три rowid
alias, полное чтение версии отказывает с её именем.
`inspect` теперь также read-only: старую схему/WAL отказывает. Старое описание записи
через `Document::open` выше по-прежнему относится к dump-graph/clear-cache.

Текстовый CLI: успех — exit 0 и `saved …`; отказ — exit 2 с `error [kind]` на stderr.
JSON v1: успех/отказ выполнения — структурированный stdout, диагностика — stderr.
Clap usage/help остаются текстом. Потеря отчёта после публикации — exit 7 с сохранённой копией.
Неверный UUID/значение/stale/busy — input; другая фича, Symmetric/ThroughAll,
формула/Parameter dependency/future capabilities/stored SQL trigger/нестандартное mutating FK action — unsupported; kernel/topology
refusal остаётся типизированным. Отказ/отмена до publish не оставляет файла и scratch;
после publish поздняя отмена не удаляет файл и не выдаёт публикацию за отказ.
Сборка без OCCT собирается и честно отказывает операции, требующей ядра.

UI доступен по принятой сцене; форма и worker блокируют Open/New/Export. Empty/imported-only
объясняют отсутствие native extrusion. После publish Saved показывает output отдельно
от результата Open; при ошибке Open старая сцена/export source/title сохраняются.
Полный повторяемый [CLI-рецепт, контракт и проверки §24C](edit-extrude-copy-verification.md).

## §24D: JSON v1 для inspect и edit-extrude

[Схема, ошибки, ограничения путей и запускаемый рецепт агента](cli-json-v1.md)
позволяют получить UUID и непрозрачную версию из JSON, выбрать ровно нужную
поддерживаемую Extrude, изменить её высоту и проверить копию повторным inspect.
Каталог различает общий copy_access и локальные отказы; успешное чтение не обещает
геометрический успех. Правка использует ту же jobs-операцию, что текстовый CLI/UI.
Нативный process gate включён в combined runtime layout на трёх ОС вместе с восемью
существующими edit gates. Первичная сдача и независимое ревью — в [протоколе](cli-json-v1-verification.md).

## §24D-1: create --json

Тот же конверт v1 для `ferritecad create <path> --json`. Результат — опубликованные
`destination` и `document_id` новой модели. Текстовый create и UI New не меняются;
общая операция, no-clobber и правила размеров те же. Рецепт агента начинается с
JSON create, затем JSON inspect/edit, без заранее известного UUID. §24F ниже
добавляет JSON STL, §24G — JSON FBX, §24I — JSON STEP import; JSON остальных команд по-прежнему нет.

## §24E: общий STL export и Export STL…

UI и существующий `export-stl` вызывают одну jobs-операцию. Форма берёт Body из
принятых фактов LiveScene без повторного открытия БД; захватывает путь принятой
сцены, UUID и параметры до Save. При нескольких Body автоматического выбора нет.
Имена показываются вместе с UUID; отсутствующее имя не мешает выбору. Defaults —
0.01 mm linear / 0.5 rad angular, файл — binary STL в mm. Камера и скрытие тела
не влияют на результат. ImportedStep не считается native Body.

Worker заново читает сохранённый файл одним read-only снимком и cold rebuild;
изменения с момента Open попадают в экспорт, исчезнувший UUID вызывает отказ.
Здесь нет `--expect-version`. Cancel до publish сохраняет исходник и назначение,
поздняя отмена оставляет готовый файл. Source aliases запрещены даже с Replace;
no-clobber повторяется при атомарной публикации. Окно присоединяет свои workers
при закрытии и игнорирует устаревшие ответы после смены документа/заявки.
STL показывает путь/Body/triangles/bytes, без специфичного для FBX списка omissions.

[Протокол §24E](stl-export-verification.md) содержит рецепт, локальные native и
no-native результаты, направленные поломки и отдельный статус наблюдения GUI.
JSON STL добавлен в §24F ниже; сборки, imported-only и несколько тел одним файлом остаются недоступны.


## §24F: Body discovery и export-stl --json

Обязательный `inspect --json` → `result.bodies` содержит `body_id` (UUIDv7 native
Body) и точное `name` (string/null), в общем порядке objects после фильтра. Пустой
и imported-only каталог — `[]`, успешное чтение. Body без геометрии тоже виден;
наличие записи не обещает доступное ядро. Features/metadata/content version
принадлежат тому же закреплённому read-only снимку. Общий `stl_bodies(&Document)`
используется STL selector и JSON discovery; kernel/rebuild и повторного открытия нет.

`export-stl --json` сохраняет выбор/deflections/force и вызывает прежний общий job.
Успешный result после publish: destination, body_id, body_name (string/null),
triangles и bytes (целые JSON numbers), length_unit="mm". Ошибки выполнения —
error envelope/exit 2; clap usage/help — текст. UTF-8 paths проверяются до файловых
операций. Потеря stdout — 7, опубликованный STL или подтверждённая замена остаются
целыми даже при закрытом stderr; автоматического повтора нет.

Body ID не Extrude ID: `--solid` не ищет тело по переданному UUID фичи. Между
inspect и export нет snapshot/version guard, `--expect-version` не добавлен:
экспорт читает текущее содержимое и отказывает исчезнувший UUID без подстановки.
[Контракт и безоконный рецепт агента](cli-json-v1.md) обходятся публичным CLI и
стандартным JSON/STL parser, без заранее известного UUID.
[Матрица и протокол §24F](body-stl-json-verification.md) отделяют native от stub,
реальные процессные отказы/pipe cases от skips. UI/renderer и runtime edges не менялись.


## §24G: export-fbx --json

Один `export_fbx_result` готовит запрос и вызывает прежний FBX job для text/JSON.
Explicit CLI DTO содержит destination, bytes, models, geometries, materials,
complete и omissions из опубликованного FbxExport/FbxWriteReport. Source identity
передаётся tagged object (Body UUID либо imported source UUID + definition_key),
finding сохраняет stage/severity/entity/message, refusal — typed stable name,
placements — локальные `FerriteCADNodeKey` этого файла, не durable occurrence ID.

Полная публикация — ok:true/exit 0; частичная — ok:true/complete:false/exit 6;
отказ — error/exit 2; потеря доставки — 7 без rollback полного/partial файла или
force-замены. В JSON-режиме partial stderr пуст, execution diagnostics fallible.
UTF-8 пути проверяются до файловой работы. Старый текст, ASCII 7400 writer,
метры/оси, hierarchy/instances/materials и identity wire contract сохранены.

[Точный контракт и Python-рецепт](cli-json-v1.md) не парсят человеческий отчёт,
явно обрабатывают partial и независимо читают FBX. Inspect и export — отдельные
снимки актуального сохранённого файла; FBX не имеет --expect-version/selection.
[Безоконный протокол §24G](fbx-json-publication-verification.md) отделяет
native/stub, skips и базовый CI от ещё не опубликованного diff.


## §24I: import-step --json

Один request adapter text/JSON вызывает общий STEP job. Explicit DTO сообщает
опубликованные identities и сохранённые import facts (0/4), reader rejection
без публикации (5, error.code=reader_rejected + step_read) либо operational error
(2). Fallible emitter возвращает 7 при потере отчёта, без повтора или отката.
UTF-8 source/output проверяются до job; stdout содержит один JSON object + LF,
clap usage/help остаются текстом. Диагностика reader сохраняет stage/severity/
entity/message и порядок; тишина не доказывает корректность STEP. Import diagnostics
не определяют нынешнюю полноту FBX. [Точный JSON v1 и рецепт](cli-json-v1.md),
[проверки и ограничения](cli-json-step-import-verification.md).

## §25B: правка сохранённого Line Sketch

`edit-sketch-copy` text/JSON и `Edit Sketch <name> — <UUID>…` используют один jobs
`edit_sketch_copy` и общий document каталог `ExtrudeEditSource.sketches`.
Inspect JSON v1 обнаруживает Sketch/curve UUID и координаты на том же снимке.
Все vertices передаются по сохранённым IDs/порядку; height, metadata и identities
сохраняются. Новая копия проходит cold refs/rebuild и atomic Keep. UI использует
прежний draft canvas, Save adapter и один edit worker с generation guards.
[Точный протокол и рецепт](edit-sketch-copy.md). Без constraints, изменения числа
сегментов/winding, STEP, multi-Body, in-place Save или persistent undo.

§26E расширяет Add cut до 16 отдельных инструментов. На границе добавление
недоступно, правка всех 16 доступна. Add/Edit читают единый bounded-каталог
с `tools` в порядке history; общий валидатор проверяет все диски.
[Discovery, nullable singular поля и UI/CLI эквивалентность](circular-cut-history.md).

§26F расширяет прежний `edit-sketch-copy`/Edit Sketch на исходный прямоугольник
допустимой 1–16 Cut history. Высота и инструменты остаются прежними; координаты
границ можно расширять, сужать и смещать при строгом зазоре всех дисков.
Discovery даёт typed `cut_history`, polygon draft и writer используют одну
проверку. [Контракт/рецепт](edit-cut-base-sketch.md). Остальные профильные editors
по-прежнему требуют отдельный четырёхобъектный документ.
