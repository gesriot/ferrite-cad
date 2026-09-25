# CLI JSON v1: opt-in контракт

§24D задаёт opt-in контракт inspect и edit-extrude. §24D-1 добавляет третью
существующую команду, `create`, через тот же конверт v1. §24F добавляет Body
discovery и `export-stl --json`; §24G добавляет `export-fbx --json`,
§24I — `import-step --json`, §24J — `validate --json` совместимо,
без изменения `schema_version`:

```text
ferritecad edit-circular-cut <source.fcad> --feature <uuid> --expect-version <token> --request <cut.json> -o <new.fcad> --json
ferritecad cut-circular-copy <source.fcad> --body <uuid> --expect-version <token> --request <cut.json> -o <new.fcad> --json
ferritecad edit-circle <source.fcad> --sketch <uuid> --expect-version <token> --request <circle.json> -o <new.fcad> --json
ferritecad edit-annular <source.fcad> --sketch <uuid> --expect-version <token> --request <annulus.json> -o <new.fcad> --json
ferritecad create-circle-extrude <request.json> -o <new.fcad> --json
ferritecad create-annular-extrude <request.json> -o <new.fcad> --json
ferritecad edit-sketch-constraints-copy <source.fcad> --sketch <uuid> --expect-version <token> --request <constraints.json> -o <new.fcad> --json
ferritecad edit-sketch-copy <source.fcad> --sketch <uuid> --expect-version <token> --request <coordinates.json> -o <new.fcad> --json
ferritecad validate <source.fcad> --json
ferritecad import-step <source.step> -o <new.fcad> --json [--name <name>] [--force]
ferritecad export-fbx <source.fcad> -o <out.fbx> --json [--force]
ferritecad inspect <source.fcad> --json
ferritecad edit-extrude <source.fcad> --json --feature <uuid> --distance-mm <n> -o <new.fcad> [--expect-version <token>]
ferritecad create-sketch-extrude <request.json> -o <new.fcad> --json
ferritecad create <path> --json [--sample] [--size W D H] [--length-unit <u>] [--angle-unit <u>]
ferritecad export-stl <source.fcad> -o <out.stl> --json [--solid <name-or-id>] [--linear-deflection <mm>] [--angular-deflection <rad>] [--force]
```

Обычный текстовый режим сохраняется. Общие document/jobs операции остаются
владельцами чтения, создания, допустимости правки и публикации.
Нет JSON остальных команд, stdin/batch, RPC, DSL, сервера,
правки формул/Parameter или in-place Save. Возможности остальных
команд описаны в [карте CLI](cli-capabilities.md).

## Конверт и совместимость

При доставленном отчёте stdout содержит ровно один UTF-8 JSON-объект и один LF
после него. Переводы строк внутри строк экранируются сериализатором. Обычных
stdout-логов рядом нет. Диагностика для человека идёт на stderr.

| Поле | Тип и правило |
| --- | --- |
| `schema_version` | integer, сейчас ровно `1` |
| `operation` | string: `inspect`, `edit-extrude`, `create`, `export-stl`, `export-fbx`, `import-step`, `validate`, `create-sketch-extrude`, `create-circle-extrude`, `create-annular-extrude`, `edit-sketch-copy`, `edit-circle`, `edit-annular`, `edit-sketch-constraints-copy`, `cut-circular-copy` или `edit-circular-cut` |
| `ok` | boolean |
| `result` | объект соответствующей операции, присутствует только при `ok: true` |
| `error` | объект ошибки, присутствует только при `ok: false` |

Все перечисленные далее поля результата обязательны. Optional значения явно
равны `null`, не исчезают. Пустая коллекция — `[]`. `result` и `error` взаимно
исключаются и не заменяются на `null`. Порядок ключей и конкретное экранирование
Unicode не значимы. Числа конечны; клиент должен читать JSON стандартным парсером.

Клиент проверяет версию и имя операции, игнорирует неизвестные поля внутри
знакомых объектов и не предполагает фиксированную длину массивов. В v1 допустимо
добавлять поля без изменения смысла существующих. Удаление, смена типа или смысла
требует новой `schema_version`. Неизвестная версия требует обновления клиента.
Имена и сообщения не служат машинными идентификаторами.

UUID — канонические строки RFC 4122 UUIDv7 с дефисами, как в существующем CLI. `document_id` именует модель; копия правки
сохраняет этот ID и UUID объектов. Независимый `create` получает новые UUID.
`content_version` — **полный непрозрачный токен** содержимого: передавайте его
без преобразований в `--expect-version`. Текущая реализация выдаёт 64 строчных
hex-символа. Не вычисляйте токен самостоятельно и не используйте mtime вместо него.
Версия JSON-контракта, версия схемы `.fcad` и версия алгоритма content hash —
разные понятия. После обновления алгоритма получите токен новым inspect.

## Правка сохранённого Sketch (§25B, §26F)

`sketches[].cut_history` — null для самостоятельного/неподдерживаемого Sketch;
для базы допустимой 1–16 Cut history — объект `{body_id, base_feature_id, tools,
wall_clearance_mm}`. `tools` имеет прежние поля tool DTO и порядок history;
координаты абсолютные, глубина/радиус остаются прежними. Контекст задаёт проверку
новых стен, не новый запрос. [Точный контракт](edit-cut-base-sketch.md).


Девятая opt-in команда `edit-sketch-copy`: result `destination:string`,
`document_id:UUIDv7`, `sketch_id:UUIDv7`, все обязательны. Exit 0/2/7 и прежний
emitter. Inspect совместимо добавляет `sketches` с UUID/name, ordered vertices
по curve UUID/start_mm, editable, local refusal и document_refusal.
Все точные поля, null policy, eligibility, versioned request и исполняемый рецепт
зафиксированы в [контракте правки Sketch](edit-sketch-copy.md).
Это координатная правка в новой копии с сохранением IDs, не полная sketch editing API.

## Создание собственного контура (§25A)

`ferritecad create-sketch-extrude <request.json> -o <new.fcad> [--json]` — восьмая
opt-in команда. `operation:"create-sketch-extrude"`, result с обязательными
`destination:string` и `document_id:UUIDv7` после cold-check и publish, exit 0;
обычный operational error/exit 2; потеря отчёта/exit 7. Семь прежних команд
не меняются. Точные request v1, ограничения, правила UI/API и запускаемый рецепт:
[Sketch → Extrude → FCAD](sketch-extrude-create.md). Это не JSON произвольной модели.

## Создание аналитической окружности (§25J)

`ferritecad create-circle-extrude <request.json> -o <new.fcad> [--json]` —
отдельная opt-in команда с тем же envelope. `operation:"create-circle-extrude"`,
result с обязательными `destination:string` и `document_id:UUIDv7` после
cold-check и publish, exit 0; operational error/exit 2; потеря отчёта/exit 7.
Прежние команды и их поля не меняются.

Request v1 этой команды называет свою версию `schema_version` (не
`request_version` соседнего polygon-запроса) и требует ровно четыре поля:

```json
{"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}
```

`center_mm` — ровно две конечные координаты в mm; `radius_mm` и `height_mm` —
конечные строго положительные mm. Лишние поля (включая `request_version`,
`points_mm`, `holes`), неизвестная версия, неправильные типы, null, NaN и
Infinity отвергаются до создания kernel, без публикации и без scratch.
Окружность остаётся аналитической: `inspect` показывает её Sketch как
неподдерживаемый Line-редакторами (`editable:false`, `constraint_edit.available:false`),
а тело имеет одну цилиндрическую поверхность. Точные ограничения, границы
аналитического B-Rep и тесселяции и запускаемый рецепт:
[окружность → Extrude → FCAD](circle-sketch-extrude.md).

## Создание полой детали (§25L)

`ferritecad create-annular-extrude <request.json> -o <new.fcad> [--json]` —
отдельная opt-in команда с тем же envelope. `operation:"create-annular-extrude"`,
exit 0 после cold-check и publish; operational error/exit 2; потеря отчёта после
состоявшейся публикации/exit 7. Прежние команды и их поля не меняются.

Request v1 называет свою версию `schema_version`, как и соседний
`create-circle-extrude`, который он расширяет, и требует ровно пять полей:

```json
{"schema_version":1,"center_mm":[12.0,-7.0],"outer_radius_mm":10.0,
 "inner_radius_mm":4.0,"height_mm":15.0}
```

`center_mm` — ровно две конечные координаты в mm, **общий** центр обеих
окружностей; `outer_radius_mm`, `inner_radius_mm` и `height_mm` — конечные
строго положительные mm, причём `inner < outer` и разность не меньше 0.001 mm.
Отдельного центра у отверстия нет: класс этой команды концентрический. Лишние
поля (включая `request_version`, `radius_mm`, `holes`, `inner_center_mm`),
неизвестная версия, неправильные типы, null, NaN и Infinity отвергаются до
создания kernel, без публикации и без scratch.

`result` называет каждый созданный объект, чтобы читателю не приходилось
угадывать по имени или порядку — и, в частности, называет **обе** окружности:

| Поле | Значение |
| --- | --- |
| `destination` | string, опубликованный путь |
| `document_id` | UUIDv7 нового документа |
| `sketch_id` | UUIDv7 созданного Sketch |
| `extrude_id` | UUIDv7 созданного Extrude |
| `body_id` | UUIDv7 созданного Body |
| `outer_curve_id` | UUIDv7 `Circle`, ограничивающей деталь |
| `inner_curve_id` | UUIDv7 `Circle`, образующей отверстие |

Оба UUID окружностей обязательны и различны; ни один не выводится из другого, и
оба берутся из самого создания, а не из повторного чтения файла. Обе окружности
остаются аналитическими: `inspect` показывает их Sketch неподдерживаемым и
Line-редакторами (`editable:false`), и редактором окружности §25K
(`circle_edit.available:false`, `circle_edit.circle:null`), а тело имеет две
цилиндрические поверхности радиусов R и r. Точные ограничения, числовая policy,
границы аналитического B-Rep и тесселяции и запускаемый рецепт:
[окружность с отверстием → Extrude → FCAD](annular-sketch-extrude.md).

## Правка сохранённого circular Cut (§26B/D/E)

`edit-circular-cut source.fcad --feature UUID --expect-version HASH
--request request.json -o copy.fcad --json` использует прежний envelope v1,
`operation:"edit-circular-cut"` и коды 0/2/7. Request строго содержит
`request_version:1`, `tool_curve_id`, `center_mm:[x,y]`, `radius_mm`, `depth_mm`.
После публикации result обязательно содержит `destination`, `document_id`,
`body_id`, `feature_id`, `sketch_id`, `tool_curve_id`, `previous_feature_id` и
boolean `leaves_a_floor`. UUID существующих объектов сохраняются; exit 7 не
отменяет состоявшуюся публикацию.

В `inspect --json` каждая строка `features` имеет required `circular_cut_edit`:
`available`, nullable `refusal`/`document_refusal` и nullable `saved`. В saved
указаны выбранные UUID, сохранённые числа, высота/габариты детали, направление,
зазор, `leaves_a_floor`, nullable `floor_reference_id` и `through_allowed`.
Прежний `editable` остаётся ответом про `edit-extrude`, поэтому у Cut он false.
Неподдержанный или выходящий за числовую policy сохранённый Cut имеет
`available:false`, причину и `saved:null`. Discovery не требует ядра;
сам CLI открывает ядро перед job, поэтому stub может отказать раньше проверки
версии или назначения внутри job.

Поддержана правка любого Cut линейной истории до 16 Cut. `previous_feature_id`
в result и discovery всегда непосредственный predecessor выбранной фичи.
В saved аддитивны `base_feature_id`, `tip_feature_id`, `disk_clearance_mm`,
`protected_floor_reference_ids` и nullable `neighboring_tool` (те же поля,
что у `existing_cut` ниже). Неизвестные поля следует игнорировать.
Через→карман добавляет собственный floor ref и OriginCap(selected, End)
на каждом последующем producer. Карман→через отказывает со всеми
защищаемыми UUID. [Контракт §26E](circular-cut-history.md).
[Полный контракт, все поля discovery и запускаемый рецепт](edit-circular-cut-copy.md).

## До 16 Cut в существующем Body (§26A/C/E)

`ferritecad cut-circular-copy <source.fcad> --body UUID --expect-version HASH
--request <request.json> -o <copy.fcad> [--json]` — отдельная opt-in команда с
тем же envelope. `operation:"cut-circular-copy"`, result с обязательными
`destination`, `document_id`, `body_id`, `feature_id`, `sketch_id`,
`tool_curve_id` и `previous_feature_id` после cold-check и publish, exit 0;
operational error/exit 2; потеря отчёта/exit 7 — публикация могла состояться,
копия цела, повторять операцию без проверки назначения нельзя. Прежние команды и
их поля не меняются.

Request v1 — строго четыре поля, `request_version` (единственное написание
версии этой команды):

```json
{"request_version":1,"center_mm":[20.0,15.0],"radius_mm":5.0,"depth_mm":10.0}
```

`center_mm` — ровно две конечные координаты инструмента в mm **на собственной
базовой XY-плоскости детали**; поля для другой плоскости или для грани здесь
нет, потому что этот срез ни к чему не привязывается. `radius_mm` — конечный
строго положительный радиус. `depth_mm` — конечная строго положительная глубина
вдоль +Z, не больше высоты детали; флага «насквозь» нет, сквозное отверстие
получается, когда числа так говорят. Инструмент обязан отстоять от каждой стороны
детали строго больше чем на линейный допуск ядра.

`inspect --json` получает у каждой строки `bodies` аддитивное `cut_edit` с
`available`/`refusal`/`document_refusal` и `target`, который называет Body,
плоскость, фичу-предшественника, Sketch профиля, высоту, габариты детали,
направление и требуемый зазор — или `null` для неподдержанного Body. Прежние
поля `bodies`, `features` и `sketches` не меняются. Discovery и структурные
отказы работают в сборке без ядра; применение правки без ядра отказывается.
Для §26C у поддержанного `target` обязательны аддитивные поля
`base_feature_id`, `disk_clearance_mm` и `existing_cut`. Первое называет исходный
NewBody, тогда как прежнее `tip_feature_id` — непосредственного предшественника
нового Cut. `existing_cut` равно `null` у исходной плиты; у плиты с первым Cut
это объект с `feature_id`, `tool_sketch_id`, `tool_curve_id`, `center_mm`,
`radius_mm`, `depth_mm`. `disk_clearance_mm` равно 1e-7 mm; фактический зазор
между дисками должен быть строго больше. Request, result и exit codes прежние.
После 16-го Cut `cut_edit.available` false; все 16 доступны через
`features[].circular_cut_edit`. Более длинная история открывается по общим
правилам, но этот редактор возвращает явный отказ без усечения каталога.
В §26E `target.tools` и `saved.tools` — массив всех инструментов в порядке
Feature.previous от первого Cut до tip, с теми же полями, что `existing_cut`.
Для N>1 `existing_cut:null`; для N≠2 `neighboring_tool:null`. Старые значения
на прежних документах сохраняются. Массив edit включает выбранный инструмент.
Конфликт с любым другим диском называет его feature UUID.
[Точный контракт §26E и исполняемый рецепт](circular-cut-history.md).
Точные ограничения, семантика истории, правила UI/writer и запускаемый рецепт:
[первый Cut](circular-cut-copy.md), [второй Cut и новые поля discovery](sequential-circular-cuts.md).

## Явный ThroughAll для circular Cut (§26H)

JSON v1 (envelope, `operation`, exit 0/2/7) не меняется. Для `cut-circular-copy`
и `edit-circular-cut` добавлен **request v2**; request v1 принимается как прежде
и всегда означает Blind:

```json
{"request_version":2,"center_mm":[20.0,15.0],"radius_mm":5.0,"extent":{"kind":"through_all"}}
{"request_version":2,"center_mm":[20.0,15.0],"radius_mm":5.0,"extent":{"kind":"blind","depth_mm":4.0}}
```

У `edit-circular-cut` в v2, как и в v1, обязателен `tool_curve_id`. Оба варианта
строгие (`deny_unknown_fields`, ≤65536 байт): `depth_mm` на верхнем уровне v2,
лишнее поле внутри `extent`, неизвестный `kind` и `request_version` кроме 1/2
отказываются с exit 2 до ядра. Request v1 правки, направленный на Cut с
сохранённым ThroughAll, отказывается при подготовке (exit 2, в сообщении UUID
Cut и `request_version 2`): v1 не может выразить это намерение и не должен
тихо превратить его в Blind.

Прежние блоки discovery не меняют ни полей, ни типов: `depth_mm` остаётся
обязательным числом. Блоки `bodies[].cut_edit`, `features[].circular_cut_edit`,
`features[].base_height_edit` и `sketches[].cut_history` не могут описать
историю, где есть ThroughAll Cut (у него нет Blind-глубины), и сообщают это так,
как им уже было позволено: `cut_edit.target`/`circular_cut_edit.saved` — `null`
при `available:false` и `refusal`, называющем блок `_v2`; `base_height_edit` и
`cut_history` — `null`. Вычисленная высота никогда не выдаётся за Blind-глубину.
На документах без ThroughAll прежние блоки побайтно совпадают с выводом §26G.

Аддитивные блоки `bodies[].cut_edit_v2`, `features[].circular_cut_edit_v2`,
`features[].base_height_edit_v2` и `sketches[].cut_history_v2` присутствуют
всегда и повторяют форму прежних, но у каждого инструмента (`tools`,
`existing_cut`, `neighboring_tool`) и у `saved` вместо `depth_mm` стоит `extent`:
`{"kind":"blind","depth_mm":d}` или `{"kind":"through_all"}`; `target`/`saved`
получают `request_versions` (`[1,2]` для добавления и Blind, `[2]` для
ThroughAll). `leaves_a_floor` у ThroughAll всегда false. Result обеих команд
получает аддитивное `extent`.
[Контракт §26H и исполняемый рецепт](circular-cut-through-all.md).

## Cut в простом Line-полигоне (§26I)

Envelope, `schema_version:1`, операции, request v1/v2 и exit 0/2/7 не меняются.
Меняется только класс базовой детали: любой простой замкнутый полигон из 3..256
Line без constraints. Прежние блоки `extents_mm` означали прямоугольник, и
bounding box иного контура им не является, поэтому:

* для детали-прямоугольника все блоки v1 и `_v2` побайтно те же;
* для иного полигона `cut_edit`/`cut_edit_v2`.`target` и
  `circular_cut_edit`/`circular_cut_edit_v2`.`saved` — `null` при
  `available:false` и `refusal`, называющем блок `_v3`; `base_height_edit`,
  `base_height_edit_v2`, `cut_history`, `cut_history_v2` — `null`.

Аддитивные блоки `bodies[].cut_edit_v3`, `features[].circular_cut_edit_v3`,
`features[].base_height_edit_v3` и `sketches[].cut_history_v3` присутствуют
всегда (и для прямоугольника) и повторяют `_v2`, но вместо `extents_mm` несут:

```json
"bounds_mm": [[0.0, 0.0], [60.0, 40.0]],
"boundary": {"kind": "line_polygon", "orientation": "counter_clockwise",
             "area_mm2": 1600.0,
             "segments": [{"curve_id": "…", "start_mm": [0.0, 0.0], "end_mm": [60.0, 0.0]}, …]}
```

`bounds_mm` — только диапазон, никогда не доказательство вложенности.
`segments` — сохранённые Line в порядке хранения с их UUID; `orientation`
(`counter_clockwise`/`clockwise`) — направление, в котором их нарисовали.
[Контракт §26I и исполняемый рецепт](polygon-cut-history.md).

## Revolve на полный оборот (§27A)

`ferritecad create-sketch-revolve <request.json> -o <new.fcad> [--json]`.
Request v1 (`deny_unknown_fields`, не больше 65536 байт):

```json
{"request_version":1,"points_mm":[[4,0],[10,0],[10,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}
```

Envelope общий: `operation:"create-sketch-revolve"`, result
`{destination, document_id}`; exit 0 — опубликовано, 2 — отказ (ничего не
записано), 7 — отчёт не доставлен после публикации.

`inspect --json` получает аддитивный массив верхнего уровня `revolves`:

```json
"revolves": [{"feature_id": "…", "name": "Revolve1", "body_id": "…",
  "profile_sketch_id": "…", "plane_id": "…",
  "axis": "sketch_y", "extent": "full_turn", "operation": "new_body",
  "profile": {"available": true, "refusal": null,
              "segments": [{"curve_id": "…", "start_mm": [4.0, 0.0], "end_mm": [10.0, 0.0]}, …]}}]
```

`features`, `bodies`, `sketches` и их типы не меняются. Revolve не попадает в
`features`, поэтому прежний потребитель не примет его за Extrude; блоки правки
тела и эскиза недоступны с отказом. Для документа без Revolve массив пуст.
[Контракт §27A и исполняемый рецепт](full-turn-revolve.md).

## Правка профиля Revolve и `profile_feature` (§27B)

`edit-sketch-copy`, его request v1, envelope и exit 0/2/7 не меняются. Для
самостоятельного документа §27A строка `sketches[]` теперь имеет
`editable:true` (если документ и структура позволяют) и заполненные
`vertices`; прежде она отказывала. На прежних моделях все прежние поля и
значения те же. Аддитивное поле каждой строки `sketches[]`:

```json
"profile_feature": null
"profile_feature": {"kind": "blind_extrude", "feature_id": "…", "height_mm": 10.0}
"profile_feature": {"kind": "full_turn_revolve", "feature_id": "…", "body_id": "…",
                    "axis": "sketch_y", "extent": "full_turn", "axis_clearance_mm": 1e-6}
```

`null` ровно тогда, когда `vertices` — null. `kind` называет политику, которую
применит задание: `PolygonExtrusion` или `FullTurnRevolution`. Revolve
по-прежнему не входит в `features`; `revolves[].profile.available` означает
только читаемость профиля, а разрешение правки — `editable` строки Sketch.
[Контракт §27B и исполняемый рецепт](edit-revolve-profile.md).

## Правка сохранённой кольцевой пары (§25M)

`ferritecad edit-annular <source.fcad> --sketch UUID --expect-version HASH
--request <request.json> -o <copy.fcad> [--json]` — отдельная opt-in команда с
тем же envelope. `operation:"edit-annular"`, result с обязательными
`destination`, `document_id`, `sketch_id`, `outer_curve_id` и `inner_curve_id`
после cold-check и publish, exit 0; operational error/exit 2; потеря отчёта/exit
7. Прежние команды и их поля не меняются.

Request v1 — строго шесть полей, `request_version` (единственное написание
версии этой команды):

```json
{"request_version":1,"outer_curve_id":"UUID","inner_curve_id":"UUID",
 "center_mm":[-3.5,4.25],"outer_radius_mm":6.75,"inner_radius_mm":2.125}
```

`outer_curve_id` и `inner_curve_id` — UUID **сохранённых** окружностей, каждый в
той роли, которую он уже занимает: перестановка ролей, один UUID дважды и чужой
UUID отвергаются. `center_mm` — ровно две конечные координаты, **общий** центр
обеих окружностей; отдельного центра у отверстия здесь нет, класс
концентрический. `outer_radius_mm` и `inner_radius_mm` — конечные строго
положительные mm, `inner < outer`, разность не меньше 0.001 mm. `height_mm`,
`radius_mm`, `inner_center_mm` и любые другие поля лишние и отвергаются: высоту
меняет прежний `edit-extrude`.

`inspect --json` получает у каждой строки `sketches` аддитивное `annulus_edit` с
`available`/`refusal`/`document_refusal` и `annulus`, который называет оба UUID в
их ролях, **оба** сохранённых центра, оба радиуса и высоту — или `null` для
неподдержанного Sketch. Прежние `editable`, `vertices`, `circle_edit`,
`constraint_edit` и приоритет документного отказа не меняются. Discovery и
структурные отказы работают в сборке без ядра; применение правки без ядра
отказывается. Точные ограничения, правила UI/writer и запускаемый рецепт:
[правка кольцевой пары в новой копии](edit-annular-copy.md).

## Правка сохранённой окружности (§25K)

`ferritecad edit-circle <source.fcad> --sketch UUID --expect-version HASH
--request <request.json> -o <copy.fcad> [--json]` — отдельная opt-in команда с
тем же envelope. `operation:"edit-circle"`, result с обязательными
`destination`, `document_id`, `sketch_id` и `curve_id` после cold-check и
publish, exit 0; operational error/exit 2; потеря отчёта/exit 7. Прежние команды
и их поля не меняются.

Request v1 — строго четыре поля, `request_version` (единственное написание
версии этой команды):

```json
{"request_version":1,"curve_id":"UUID","center_mm":[12.0,-7.0],"radius_mm":10.0}
```

`curve_id` — UUID **сохранённой** окружности; `center_mm` — ровно две конечные
координаты; `radius_mm` — конечное строго положительное число mm. `height_mm`,
`schema_version`, `vertices` и любые другие поля лишние и отвергаются: высоту
меняет прежний `edit-extrude`.

`inspect --json` получает у каждой строки `sketches` отдельное поле
`circle_edit` с обязательными `available`, `refusal`, `document_refusal` и
`circle`. Прежние `editable`, `vertices` и `constraint_edit` сохраняют смысл —
они отвечают про Line-редакторы. `circle` — объект
`{curve_id, center_mm, radius_mm, height_mm}` для поддержанного Sketch и `null`
иначе. Discovery работает и в сборке без ядра; применение правки без ядра
отказывается. Точные ограничения, правила UI/API и запускаемый рецепт:
[правка окружности в копии](edit-circle-copy.md).

## Результат validate (§24J)

`ferritecad validate <source.fcad> --json` проверяет внутреннюю согласованность
сохранённого документа. Text/JSON используют один `validate_result` и общий
`ferritecad_jobs::validate_document(&Path)`: один `Document::open_read_only`,
metadata и `Document::validate` на закреплённом снимке, закрытие SQLite до возврата.
Content version не вычисляется. Ядро, rebuild и sidecar cache не используются.

| Поле result | Тип и смысл |
| --- | --- |
| `document_id` | canonical UUIDv7 string: identity проверенного снимка |
| `valid` | boolean: в diagnostics нет severity `error` |
| `errors` | неотрицательный JSON integer, Rust u64: число diagnostics с severity `error` |
| `warnings` | неотрицательный JSON integer, Rust u64: число diagnostics с severity `warning` |
| `diagnostics` | ordered array, `[]` при отсутствии findings; порядок и повторы document validator сохраняются |

Каждый diagnostic содержит обязательные `code` (stable string), `severity`
(string: `error` или `warning`), `message` (точная строка, включая Unicode,
кавычки и LF) и `object_id` (canonical UUIDv7 string либо явный `null`). UUID
может указывать на отсутствующий объект: это часть обнаруженной проблемы.
Сообщение предназначено человеку; клиент сопоставляет code, не фрагменты prose.
Counts получены из этого же owned report, не дополнительным проходом по БД.
Все поля обязательны, `errors + warnings == len(diagnostics)` для известных
severity v1. Неизвестные поля игнорируются; неизвестный code сохраняется и
обрабатывается по severity. Неизвестная severity требует остановки/обновления
клиента, её нельзя считать warning, отсутствием ошибки или основанием продолжить.
Новые codes допустимы внутри v1; смысл существующих codes не меняется.

Проверка состоялась: `ok:true`; без errors — `valid:true`, exit **0**, в том числе
при warnings. С errors — **`ok:true`, `valid:false`, exit 1**. Для downstream
недостаточно проверить `ok`: нужно проверить exit, valid и принять решение по
warnings. Например, `object.unknown-type` — warning о сохраняемом объекте, который
эта сборка не интерпретирует; это не гарантия безопасного экспорта всей модели.

```json
{"schema_version":1,"operation":"validate","ok":true,"result":{"document_id":"019923ca-34da-7000-8000-000000000001","valid":true,"errors":0,"warnings":0,"diagnostics":[]}}
```

```json
{"schema_version":1,"operation":"validate","ok":true,"result":{"document_id":"019923ca-34da-7000-8000-000000000001","valid":false,"errors":1,"warnings":0,"diagnostics":[{"code":"reference.missing-edge","severity":"error","message":"A semantic reference has no dependency edge","object_id":"019923ca-34da-7000-8000-000000000002"}]}}
```

Тексты примеров иллюстративны, не стабильный интерфейс. Operational refusal
отличается от invalid report: `ok:false`, только прежний error.kind/message/causes,
exit **2**, без STEP reader rejection code/step_read. Ошибка открытия или
декодирования не заменяется пустым отчётом. Например, отсутствующая dependency
edge даёт `reference.missing-edge`, а неизвестная role, не поддающийся декодированию
CBOR или неподдерживаемый reader не дают завершённого ValidationReport.

```json
{"schema_version":1,"operation":"validate","ok":false,"error":{"kind":"unsupported","message":"This document needs migration; read-only validation refuses it","causes":[]}}
```

**Намеренное изменение text validate:** старая SQL schema, WAL и несовместимый
minimum reader теперь отказывают без миграции/нормализации. Общий read-only guard
также отказывает при наличии WAL/SHM sidecars, даже с DELETE-заголовком: SQLite
мог бы изменить SHM. Проверяются запрошенный путь и разрешённая цель symlink;
чужие sidecars не удаляются. Для поддерживаемых
документов прежние тексты, порядок findings и exit 0/1 сохраняются. Missing path
не создаётся; bytes/mtime/чужие sidecars и каталог остаются неизменными при
valid/warnings/errors, отказе и потере отчёта. Ни auto-repair, ни migrate-команды нет.
UTF-8 source проверяется до работы только в JSON; текстовые правила путей прежние.
`validate -- --json` — текстовая проверка файла `--json`,
`validate --json -- --json` — JSON-проверка того же имени. Help/usage остаются текстом.

Потеря stdout при любом исходе даёт **7** без повторной проверки и изменения
файлов. Закрытый stderr не мешает error JSON через исправный stdout; оба закрытых
канала не вызывают panic. JSON использует прежний fallible emitter.

Это не проверка геометрии: valid не доказывает solid после native rebuild,
корректный исходный STEP, успешное повторное чтение imported geometry или FBX
complete. Historical STEP diagnostics, validation findings и нынешние FBX omissions
— разные факты. Validate и последующий export открывают отдельные снимки;
общего snapshot/version guard между командами нет. [Протокол](read-only-validation.md).

## Результат import-step (§24I)

`ferritecad import-step <source.step> -o <new.fcad> --json [--name <name>] [--force]`
вызывает тот же общий STEP job, что текстовый import. DTO строится только из
завершённого outcome, без второго import или открытия готового `.fcad`.

| Исход | Envelope | Exit |
| --- | --- | --- |
| Документ опубликован без diagnostics | `ok:true`, `result`, diagnostics `[]` | 0 |
| Документ опубликован с diagnostics | `ok:true`, `result`, непустые diagnostics | 4 |
| Reader отверг STEP | `ok:false`, `error.code:"reader_rejected"`, `error.step_read` | 5 |
| Operational failure до публикации | Прежний `ok:false`, `error.kind/message/causes`; без code/step_read | 2 |
| Потеря сериализации/доставки любого из этих исходов | Ответ может отсутствовать/оборваться; исход операции не отзывается | 7 |

Все поля Published `result` обязательны:

| Поле | Тип и смысл |
| --- | --- |
| `destination` | UTF-8 string: реально опубликованный путь, как передан в request |
| `document_id` | UUIDv7 созданного документа |
| `object_id` | UUIDv7 сохранённого ImportedStep object, не native Body/Extrude |
| `source_id` | UUIDv7 embedded STEP source; локальный definition key без него не идентификатор |
| `name` | string: точное имя объекта, включая пустую строку/Unicode/quotes/LF |
| `source_name` | string или явный null: сохранённый basename, никогда не полный путь |
| `source_byte_len` | неотрицательное JSON integer, Rust u64, размер прочитанных STEP bytes |
| `source_hash` | 64 строчных hex: полный BLAKE3 исходных bytes; не document content_version |
| `importer` | объект с обязательными string `id`, `version`, `build`; пустой build — `""` |
| `step_schema` | string: декларация схемы STEP, без изменения; пустая — `""` |
| `source_unit` | string: декларация единицы STEP, без изменения; пустая — `""` |
| `definitions` | неотрицательное JSON integer, Rust u64: число сохранённых definitions |
| `placements` | неотрицательное JSON integer, Rust u64: число сохранённых placements |
| `diagnostics` | ordered array объектов ниже, `[]` при тишине reader |

Каждый diagnostic имеет обязательные `stage`, `severity`, `entity`, `message`.
Stage — `load`, `transfer`, `identity`, `validation` или `unknown`; severity —
`warning`, `fail` или `unknown`. Entity/message — точные строки, включая пустые,
Unicode/quotes/LF. Порядок и повторы сохраняются. Новые domain варианты отображаются
в `unknown`, пока wire mapping явно не расширен. Клиент не должен считать
незнакомый stage/severity безопасным или превращать его в отсутствие diagnostics.

Reader rejection совместимо расширяет только свой error двумя обязательными
полями: `code:"reader_rejected"` и `step_read`. `step_read` содержит ровно известные
этому v1 обязательные поля `source_byte_len`, `source_hash`, `importer`,
`diagnostics` с типами выше. Common `kind` = `input` (стабильная coarse категория
ErrorKind), `message` — изменяемое описание, `causes` = `[]`: diagnostics —
наблюдения reader, а не цепочка исключений. Ни result, ни publication destination,
ни document/object/source/occurrence UUID при rejection не выдаются. Обычные ошибки
всех шести команд сохраняют прежние kind/message/causes и **не выдают** code/step_read.
Неизвестные поля клиент игнорирует; неизвестный error code обрабатывает как отказ,
а не как разрешение продолжить. Отсутствие code у exit 2 — operational failure.

Тишина reader не доказывает корректность STEP. Exit 4 требует явного решения по
сохранённым diagnostics, даже если файл опубликован. Эти исторические diagnostics
не являются текущими FBX omissions и не определяют FBX complete. Source unit —
декларация STEP, не текущая единица координат FBX/STL. Полные vectors placements,
transforms/colours и discovery всей сборки здесь не выдаются; job сохраняет их
как прежде. Независимые text/JSON imports получают новые UUID: файлы и ID между
ними не обязаны совпадать. Между import/inspect/export нет общего снимка/version guard.

Примеры полного published result (exit 0) и reader rejection (exit 5).
Пути/UUID иллюстративные; строки kernel/reader отражают эту локальную сборку.

```json
{"schema_version":1,"operation":"import-step","ok":true,"result":{"destination":"clean.fcad","document_id":"01a08678-73a0-7600-9e35-0cc666783899","object_id":"01a08678-739d-7102-b3d6-4262de198511","source_id":"01a08678-73a1-77e2-8abf-2cbbd53c45ca","name":"clean","source_name":"clean.step","source_byte_len":15408,"source_hash":"7e1601d7ddbb078d1270bfce5115a6427215d3a995cfc8a2bf01b0224f4f646a","importer":{"id":"occt","version":"8.0.1","build":"bridge 0d13f6abb948c22016e26bebaf0e64464863a1cf0dc71e9798636c957d9b0313 aarch64-apple-darwin"},"diagnostics":[],"step_schema":"AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF {1 0 10303 442 1 1 4 }","source_unit":"millimetre","definitions":1,"placements":1}}
```

```json
{"schema_version":1,"operation":"import-step","ok":false,"error":{"kind":"input","message":"STEP reader rejected the source; nothing was published","causes":[],"code":"reader_rejected","step_read":{"source_byte_len":9375,"source_hash":"3f72cf95c90084f50506a45e998643e11757aebf262ff162f51c5ea5249bf4be","importer":{"id":"occt","version":"8.0.1","build":"bridge 0d13f6abb948c22016e26bebaf0e64464863a1cf0dc71e9798636c957d9b0313 aarch64-apple-darwin"},"diagnostics":[{"stage":"load","severity":"fail","entity":"","message":"DATA NOT AVAILABLE FOR CHECK"}]}}}
```

## Результат inspect

Это срез чтения для выбора Extrude и native Body, а не сериализация всей БД, геометрии,
результата validate или полного текстового inspect.

| Поле `result` | Тип и смысл |
| --- | --- |
| `document_id` | UUID модели |
| `content_version` | полный токен содержимого закреплённого снимка |
| `display_units` | объект `{length: string, angle: string}`; символы длины `mm`, `cm`, `m`, `in`, `ft`, угла `rad`, `deg` |
| `distance_unit` | строка `mm`; все `distance_mm` всегда миллиметры, независимо от display units |
| `edit_extrude` | объект общей доступности ниже |
| `features` | массив существующих нативных Extrude; другие типы объектов сюда не входят |
| `bodies` | массив сохранённых native Body; обязательное поле, добавлено в §24F |

`features` сохраняет порядок общего каталога: порядок объектов по parent UUID,
ordinal, UUID (корневые parent `null` идут первыми), отфильтрованный до Extrude.
Это порядок представления, не адресация фич. Выбирайте UUID явно; имена могут
повторяться или отсутствовать. При неоднозначности рецепт ниже отказывает.

`bodies` сохраняет тот же общий `objects()` порядок после фильтра до Body:
parent UUID (сначала null), ordinal, UUID. Это не сортировка по имени. Пустой и
imported-only документ возвращают `bodies: []` с `ok: true`. Каталог включает
сохранённые Body без tip feature; наличие записи не обещает геометрию, успешный
rebuild или наличие ядра. ImportedStep, Extrude и GPU pick ids в него не входят.

| Поле элемента `bodies` | Тип и смысл |
| --- | --- |
| `body_id` | канонический UUIDv7 конкретного native Body для `export-stl --solid` |
| `name` | string или явный `null`, точное сохранённое имя, включая Unicode/кавычки/LF |

Body и Extrude — разные объекты: `features[].feature_id` адресует правку
выдавливания, `bodies[].body_id` адресует экспорт тела. Extrude UUID в `--solid`
отказывает; команда не ищет его Body автоматически. Имя может отсутствовать или
совпадать с другими именами и UUID, поэтому агент выбирает конкретный Body UUID.
Прежние `features`, доступность правки и content version сохраняют свой смысл.

| Поле элемента `features` | Тип и смысл |
| --- | --- |
| `feature_id` | UUID конкретной Extrude для `--feature` |
| `name` | string или `null`, точное хранимое имя |
| `distance_mm` | number или `null`; значение Blind/Symmetric из общего каталога, в том числе сохранённое значение выражения; у ThroughAll `null` |
| `editable` | boolean, учитывает и локальную допустимость, и общий `copy_access` |
| `refusal` | string или `null`, только причина отказа этой фичи |
| `base_height_edit` | объект или `null`; контекст правки исходной высоты поддерживаемой 1–16 Cut истории (§26G) |

`base_height_edit` содержит `body_id`, `profile_sketch_id`, `extents_mm`,
`tools` (прежний DTO абсолютных инструментов в порядке истории) и
`protected_floors`: массив `{feature_id, reference_ids}` с собственными и
descendant floor UUID каждого Cut. Для standalone/неподдерживаемой фичи — null.
Это аддитивный контекст, не новый request. `editable` и command/response v1
продолжают отвечать про `edit-extrude`; [политика высоты](edit-cut-base-height.md)
требует поддерживаемого каталога и разрешимости всех прежних/новых refs после
публикации. Глубины tools не меняются. Exit 7 не разрешает слепой retry.

| Поле `edit_extrude` | Тип и смысл |
| --- | --- |
| `available` | boolean: документ допускает копию и есть хотя бы одна поддерживаемая фича |
| `refusal` | string или `null`: общий запрет, пустой каталог либо отсутствие поддерживаемой фичи |
| `document_refusal` | string или `null`: именно общий запрет `copy_access`, отдельно от локальных причин |

При общем запрете **все** `editable` равны false, даже если локальная `refusal`
равна null. Пустой или нередактируемый документ успешно читается: `ok: true`,
`available: false` и объяснение. Наличие расстояния не означает допустимость
правки. Поддерживается только Blind с числовым литералом без зависимости
Parameter. Formula, Parameter, Symmetric и ThroughAll отказывают.

Чтение использует один `Document::open_read_only`, `ExtrudeEditSource` и общий
`stl_bodies(&Document)` на том же закреплённом снимке, с одним вычислением content
hash. Metadata, version, features и bodies согласованы. Не открывает файл повторно
ради каталога, не мигрирует, не пишет sidecars, не вызывает kernel/rebuild. Старую
SQL-схему, WAL-состояние и несовместимость read-only open команда отказывает.
Успешный inspect **не обещает**, что геометрия перестроится или что OCCT доступен.

Пример успешного чтения (UUID и токен иллюстративные):

```json
{"schema_version":1,"operation":"inspect","ok":true,"result":{"document_id":"019ecc22-0841-7c0e-ad61-374b404f7219","content_version":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","display_units":{"length":"in","angle":"deg"},"distance_unit":"mm","edit_extrude":{"available":true,"refusal":null,"document_refusal":null},"features":[{"feature_id":"019ecc22-43cb-7c1d-8da9-f386566b9f19","name":"Extrude1","distance_mm":12.0,"editable":true,"refusal":null}],"bodies":[{"body_id":"019ecc22-84cf-71a0-95e2-2f475f395fff","name":"Plate"}]}}
```

## Результат edit-extrude

| Поле `result` | Тип и смысл |
| --- | --- |
| `destination` | string: путь реально опубликованной копии |
| `document_id` | сохранённый UUID модели |
| `feature_id` | UUID выбранной и изменённой Extrude |

Успех сообщается после публикации, а не после запуска worker. JSON и текстовый
режим готовят один и тот же `EditExtrudeRequest` и вызывают `edit_extrude_copy`.
Сохраняются ID, остальные данные, no-clobber, отказ source aliases, защита
triggers/FK/rowid, контроль версии, cold rebuild и ранее разрешимых refs.
Назначение никогда не перезаписывается. `--force` отсутствует. Для нового
расстояния и версии вызовите inspect копии: операция правки не открывает output
повторно ради отчёта. `--expect-version` опционален для совместимости, но агент,
действующий по прежнему чтению, должен передать полученный токен.

```json
{"schema_version":1,"operation":"edit-extrude","ok":true,"result":{"destination":"Копия плиты.fcad","document_id":"019ecc22-0841-7c0e-ad61-374b404f7219","feature_id":"019ecc22-43cb-7c1d-8da9-f386566b9f19"}}
```

## Результат create

Это создание **новой** модели: DocumentId и UUID объектов каждый раз другие.
Это не правка копии и не замена существующего файла. `--force` нет.

| Поле `result` | Тип и смысл |
| --- | --- |
| `destination` | string: путь реально опубликованного документа |
| `document_id` | UUID новой модели |

JSON и текстовый `create` готовят один `CreateDocumentRequest` и вызывают
`create_document`. Пустой документ и `--sample`/`--size` те же, что у обычного
create и UI New. Размеры по-прежнему в миллиметрах; display units модель не
масштабируют. Допустимость размеров решает документ: ширина и глубина могут
быть нулевыми или отрицательными (координаты эскиза), высота должна быть
положительной, не-конечное значение отказывает. Ядро не требуется.

Успех сообщается после публикации. Команда не открывает готовый файл повторно
ради дополнительных полей: UUID фич и content version получите `inspect --json`.
`destination` сохраняет переданный путь без канонизации. Независимые create
одной и той же спецификации нельзя сравнивать на равенство случайных UUID.

```json
{"schema_version":1,"operation":"create","ok":true,"result":{"destination":"Исходная плита.fcad","document_id":"019ecc22-0841-7c0e-ad61-374b404f7219"}}
```

Предупреждение, что путь не оканчивается на `.fcad`, в JSON-режиме идёт только
на stderr и пишется fallibly: закрытый диагностический канал не вызывает panic
и не портит JSON на stdout.

## Результат export-stl

| Поле `result` | Тип и смысл |
| --- | --- |
| `destination` | string: путь реально опубликованного STL, без канонизации |
| `body_id` | канонический UUIDv7 выбранного native Body |
| `body_name` | string или явный `null`: точное имя Body из снимка экспорта |
| `triangles` | неотрицательное целое JSON number: число записанных треугольников |
| `bytes` | неотрицательное целое JSON number: полный размер опубликованного binary STL в байтах |
| `length_unit` | фиксированная string `mm`: единица координат STL |

JSON и текст готовят один `StlExportRequest` и вызывают существующий
`export_document_as_stl`: выбор, kernel factory, cold rebuild, тесселяция и
атомарная публикация общие. Result берётся из завершённого `StlExport`, output
не открывается повторно. Defaults: linear deflection `0.01` mm, angular `0.5` rad;
display units документа не масштабируют STL. Счётчики — числа, не строки.

Без `--solid` допустим ровно один Body. При нескольких нужен явный выбор;
канонический UUID имеет приоритет перед совпадающим именем другого Body.
Неоднозначное имя, отсутствующий UUID, пустой/imported-only документ отказывают.
Экспортируется одно полное native Body, не сборка, несколько тел или выбранные грани.
Без `--force` существующее назначение сохраняется, в том числе при гонке перед
publish. Source и его aliases запрещены даже с `--force`.

Inspect и export — **отдельные снимки**. Экспорт читает актуальный сохранённый файл,
снова выбирает заданный UUID и возвращает актуальное имя. Изменения после inspect
попадут в STL; исчезнувший UUID отказывает без подстановки другого Body.
`--expect-version` у export-stl нет; `content_version` из inspect не является
автоматическим guard экспорта. Успех означает завершённый publish.

```json
{"schema_version":1,"operation":"export-stl","ok":true,"result":{"destination":"Плита.stl","body_id":"019ecc22-84cf-71a0-95e2-2f475f395fff","body_name":"Plate","triangles":12,"bytes":684,"length_unit":"mm"}}
```

```json
{"schema_version":1,"operation":"export-stl","ok":false,"error":{"kind":"input","message":"invalid input: empty.fcad contains no bodies to export","causes":[]}}
```

## Результат export-fbx (§24G)

`ferritecad export-fbx <source.fcad> -o <out.fbx> --json [--force]`
экспортирует полную сохранённую сцену через тот же jobs route, что текстовый CLI.
`operation` равен `export-fbx`. Все поля ниже обязательны, счётчики — JSON integers
без знака (bytes: uint64, остальные: uint32), не строки или числа с дробью.

| Поле `result` | Тип и смысл |
| --- | --- |
| `destination` | string, переданный UTF-8 путь реально опубликованного FBX без канонизации |
| `bytes` | uint64, число записанных writer байт |
| `models` | uint32, число Model объектов / узлов сцены |
| `geometries` | uint32, число Geometry объектов, одна на definition с треугольниками |
| `materials` | uint32, число записанных Material объектов |
| `complete` | boolean, true тогда и только тогда, когда omissions пуст |
| `omissions` | массив записей ниже в исходном порядке FbxWriteReport; пустой массив `[]` для полной публикации |

Каждая omission содержит ровно одну запись writer report, без объединения по
имени, диагностике или локальному STEP key:

| Поле omission | Тип и смысл |
| --- | --- |
| `source` | tagged object: `{"kind":"body","body_id":"<UUIDv7>"}` либо `{"kind":"imported","source_id":"<UUIDv7>","definition_key":"<string>"}` |
| `finding` | объект с обязательными string полями `stage`, `severity`, `entity`, `message` |
| `refusal` | string, стабильное typed имя текущего отказа; сейчас `IncompleteFace` |
| `placements` | массив string `node/<n>`, все placements записи в исходном порядке |

Native Body UUID и imported source UUID — разные домены. `definition_key`
имеет смысл только вместе с `source_id`: два sources с одним key дают две
отдельные записи. `finding.stage` — `load`, `transfer`, `identity`, `validation`
(резерв `unknown`); `finding.severity` — `warning`, `fail` (резерв `unknown`).
Это сохранённая диагностика, отдельно от текущего typed refusal. `entity` может
быть пустой строкой; `message` и все хранимые строки передаются точно, включая
Unicode, кавычки и LF. Здесь нет optional/null полей. Неизвестные stage/severity/
refusal нужно сохранить и показать как неподдерживаемую классификацию, не как
полную геометрию. По тексту message решения не принимаются.

`node/<n>` — локальное значение `FerriteCADNodeKey` **в этом конкретном FBX**,
не durable occurrence UUID и не стабильная ссылка между экспортами. Локальный
ExportDefinitionId не выдаётся. Recorded occurrence identity для legacy
unrecorded не выдумывается: placements передают только существующие file keys.

Полная публикация: `ok:true`, `complete:true`, `omissions:[]`, exit **0**.
Частичная публикация: **`ok:true`, `complete:false`, непустые omissions, exit 6**.
`ok` здесь подтверждает состоявшийся publish; файл сохраняет иерархию и доступную
геометрию, но не описывает всю геометрию модели. Это result, не error envelope.
В JSON-режиме partial не дублируется прозой на stderr. Отказ до публикации —
error/exit 2. Ошибка доставки после любого publish — exit 7, включая force-замену;
полный или частичный файл остаётся, повторного экспорта/rollback нет.

FBX writer и identity wire contract не меняются: ASCII 7.4.0, метры
(`UnitScaleFactor=100`, преобразование FCAD `(x,z,-y)*0.001`), hierarchy, instances,
materials и FerriteCAD properties. UI display units эти факты не меняют.
Счётчики и omissions берутся только из опубликованного FbxExport/FbxWriteReport,
без повторного чтения документа, STEP или output и без пересчёта omissions.

Inspect и FBX export читают **отдельные снимки**. FBX экспортирует текущую
сохранённую сцену целиком; `--expect-version`, selection и новые параметры
геометрии не добавлены. Source/aliases запрещены даже с `--force`, no-clobber
проверяется до ядра и снова атомарно при publish. UTF-8 пути проверяются до
файловых операций. Недоступное ядро может отказать раньше чтения документа.

Примеры wire-объектов (пути, ID и счётчики иллюстративные):

```json
{"schema_version":1,"operation":"export-fbx","ok":true,"result":{"destination":"plate.fbx","bytes":4128,"models":1,"geometries":1,"materials":1,"complete":true,"omissions":[]}}
```

```json
{"schema_version":1,"operation":"export-fbx","ok":true,"result":{"destination":"partial.fbx","bytes":5000,"models":2,"geometries":1,"materials":1,"complete":false,"omissions":[{"source":{"kind":"imported","source_id":"019ecc22-84cf-71a0-95e2-2f475f395fff","definition_key":"step.product_definition#2583"},"finding":{"stage":"validation","severity":"fail","entity":"step.product_definition#2583","message":"The stored finding may change wording."},"refusal":"IncompleteFace","placements":["node/1"]}]}}
```

```json
{"schema_version":1,"operation":"export-fbx","ok":false,"error":{"kind":"input","message":"invalid input: plate.fbx already exists; pass --force to replace it","causes":[]}}
```

## Ошибки, clap и доставка

`error` всегда содержит `kind: string`, `message: string`, `causes: string[]`.
Категория берётся из `ErrorKind::as_str()`: `input`, `constraint`, `topology`,
`kernel`, `rendering`, `io`, `cancellation`, `unsupported`. Клиент обрабатывает
неизвестную категорию как общий отказ. `message`, цепочка причин (от ближайшей к
глубинной) и строки edit refusal предназначены человеку и могут меняться; по фразам
не классифицируйте ошибки. Пустая цепочка — `[]`. Диагностика причины сохраняется
и в JSON, и, по возможности, на stderr. Ошибка записи диагностики не мешает
доставить JSON через исправный stdout: сохраняется exit 2. Reader rejection
отдельно даёт exit 5 и typed step_read; его diagnostics не дублируются на stderr. Если не удалось
доставить сам JSON, действует exit 7, даже когда stderr тоже закрыт. Например, отсутствующий файл даёт `io` с причиной ОС;
недоступное ядро даёт `unsupported`, не геометрический успех.

```json
{"schema_version":1,"operation":"edit-extrude","ok":false,"error":{"kind":"input","message":"invalid input: extrude distance must be positive","causes":[]}}
```

| Ситуация | stdout / stderr | Exit |
| --- | --- | --- |
| Успех; для validate — valid:true, warnings допустимы | JSON result / возможный note о расширении при create | 0 |
| Проверка состоялась с errors: ok:true, valid:false | JSON result с diagnostics / пусто | 1 |
| STEP опубликован с diagnostics: ok:true | JSON result с diagnostics / пусто | 4 |
| STEP reader rejection: ok:false, code=reader_rejected | JSON error со step_read / пусто | 5 |
| Частичный FBX опубликован: ok:true, complete:false | JSON result с omissions / пусто | 6 |
| Отказ выполнения корректно разобранной JSON-команды | JSON error / диагностика | 2 |
| Ошибка аргументов до успешного clap parse | пусто / обычная текстовая usage-ошибка | 2 |
| `inspect --json --help`, `edit-extrude --json --help`, `create --json --help`, `export-stl --json --help`, `export-fbx --json --help`, `import-step --json --help`, `validate --json --help` | текст help / пусто | 0 |
| Корневой `--version` | текст версии / пусто | 0 |
| Ошибка сериализации или доставки отчёта, включая BrokenPipe | JSON может отсутствовать или быть неполным / диагностика по возможности | 7 |

JSON не обещан до успешного разбора аргументов. Пропущенный путь, синтаксически
неверный UUID `--feature`, нечисловое расстояние/deflection и неверный формат
токена — ошибки clap. `export-stl --solid` — строка name-or-id: несуществующее имя
или UUID дают структурированный execution refusal, а не UUID-ошибку clap.
Нечисловой deflection — clap; `NaN`, `inf`, ноль и отрицательное число, если они
разобраны clap (например `--linear-deflection=-1`), — execution `input`.
У `edit-extrude --distance-mm` значения `NaN`, `inf`, ноль и отрицательное число
разбираются как число, но отказываются при выполнении. У `create --size` действуют
правила создания: нечисловые и бесконечные значения недопустимы; высота должна
быть положительной, а нулевая ширина/глубина допускаются сохранённым графом.
Успешное создание само по себе не обещает успешный rebuild геометрии.
При нескольких дефектах входа порядок отказов задаёт общий
маршрут; например, недоступное ядро может отказать раньше предметной проверки.
Неверная команда с `--json`, `validate --json` без пути, корневой `--json` и неподдерживаемый
подкомандой `--version` также остаются текстовыми usage-ошибками. Глобального
флага нет. `inspect -- --json` читает файл с именем `--json` в текстовом режиме;
для JSON-чтения такого имени используйте `inspect --json -- --json`. То же для
create: `create -- --json` создаёт файл `--json` текстом; JSON-режим —
`create --json -- --json`.
У export-fbx `export-fbx -o out.fbx -- --json` и `--output=--json` тоже
не включают JSON. Пропущенный output, неизвестные selection/геометрические
флаги или `--expect-version` дают обычную clap usage, exit 2.
У export-stl `export-stl -o out.stl -- --json` читает источник с именем `--json`
в текстовом режиме. Значение `--solid=--json` тоже не включает JSON-протокол.

У import-step `import-step -o out.fcad -- --json` читает файл `--json` текстом,
`--name=--json` и `--output=--json` тоже не включают протокол. Для JSON нужен
самостоятельный flag до `--`: `import-step --json -o out.fcad -- --json`.
Невалидный UTF-8 в string-значении `--name` отвергает clap как usage; проверка
OS paths source/output выполняется после parse через общий UTF-8 helper до job.
Reader rejection и operational refusal с потерянным stdout тоже возвращают 7;
без publication ничего не появляется и занятое назначение не меняется.

Exit 7 — **ошибка доставки отчёта**, не обещание отката операции. Если stdout
закрыт после публикации, созданный/импортированный документ, копия правки, STL или полный/частичный FBX остаются целыми;
процесс не паникует, не удаляет файл и не запускает операцию снова. Не повторяйте
create, import, edit или export автоматически: проверьте назначение отдельно. Для `.fcad`
используйте inspect и при необходимости validate/rebuild; для STL — независимый
binary STL parser, для FBX — независимый FBX reader. Подтверждённая `--force`
замена импортированного `.fcad`, STL или полного/частичного FBX также не откатывается ради
отчёта. Даже если клиент не получил ответ, завершённая
публикация остаётся завершённой.

## Строки и пути

Имена и пути с пробелами, Unicode, кавычками, обратными слешами и переводами строк
передаются обычными JSON-строками с корректным экранированием. Допустимость самого
имени файла определяется ОС. `destination` сохраняет переданный путь, без
канонизации: относительный путь относится к рабочему каталогу процесса.

v1 принимает только пути, представимые в UTF-8. На Unix невалидные байты, на
Windows непарные UTF-16 surrogates в source/output дают структурированный `input`
**до открытия источника, создания и запуска операции**. Это ограничение
JSON-режима; текстовый режим не меняется. Никакого `to_string_lossy` как
обратимого формата пути нет. JSON не является байтовым кодированием произвольного
пути ОС.

## Запускаемый рецепт агента

Нужен собранный `ferritecad` с OCCT в PATH (либо задайте полный путь в `FCAD_CLI`).
JSON `create` работает без ядра, но правка, cold rebuild и экспорт требуют
настоящего OCCT. Все вызовы ниже — опубликованные CLI-команды; UUID и токен
извлекаются стандартным JSON parser Python. Имена не считаются уникальными
автоматически. Рецепт не читает исходники и не подставляет заранее известный UUID.

```sh
python3 - <<'PY'
import json
import os
from pathlib import Path
import subprocess
import struct
import tempfile

cli = os.environ.get("FCAD_CLI", "ferritecad")
root = Path(tempfile.mkdtemp(prefix="ferrite-json-"))
source = root / "Исходная плита.fcad"
destination = root / "Копия плиты.fcad"

def run(*args):
    return subprocess.run([cli, *map(str, args)], capture_output=True)

def text_command(*args):
    p = run(*args)
    if p.returncode != 0:
        raise RuntimeError((p.returncode, p.stderr.decode("utf-8", errors="replace")))

def json_command(operation, *args):
    p = run(operation, *args, "--json")
    if p.returncode == 7:
        raise RuntimeError("Report delivery failed; inspect destination before any retry")
    value = json.loads(p.stdout)
    assert value["schema_version"] == 1 and value["operation"] == operation
    if not value["ok"]:
        raise RuntimeError(value["error"])
    assert p.returncode == 0
    return value["result"]

created = json_command("create", source, "--sample", "--size", 80, 50, 12, "--length-unit", "in")
assert created["destination"] == str(source)
before = source.read_bytes()
reading = json_command("inspect", source)
assert created["document_id"] == reading["document_id"]
assert reading["edit_extrude"]["available"]
choices = [f for f in reading["features"] if f["editable"] and f["name"] == "Extrude1"]
if len(choices) != 1:
    raise RuntimeError("Select one explicit feature UUID; name is absent or ambiguous")
feature_id = choices[0]["feature_id"]
assert reading["distance_unit"] == "mm" and choices[0]["distance_mm"] == 12
edited = json_command("edit-extrude", source, "--feature", feature_id,
                      "--distance-mm", 27, "--expect-version", reading["content_version"],
                      "-o", destination)
assert edited["destination"] == str(destination)
assert edited["document_id"] == reading["document_id"]
assert edited["feature_id"] == feature_id
verified = json_command("inspect", destination)
assert verified["document_id"] == reading["document_id"]
assert verified["content_version"] != reading["content_version"]
assert verified["display_units"] == reading["display_units"]
matching = [f for f in verified["features"] if f["feature_id"] == feature_id]
assert len(matching) == 1 and matching[0]["distance_mm"] == 27
text_command("validate", destination)
text_command("rebuild", destination, "--cold")
body_choices = [b for b in verified["bodies"] if b["name"] == "Plate"]
if len(body_choices) != 1:
    raise RuntimeError("Select one explicit Body UUID; name is absent or ambiguous")
body_id = body_choices[0]["body_id"]
assert body_id != feature_id
stl_path = root / "plate.stl"
exported = json_command("export-stl", destination, "--solid", body_id, "-o", stl_path)
for field, expected in {"destination": str(stl_path), "body_id": body_id,
                        "body_name": body_choices[0]["name"], "triangles": 12,
                        "bytes": 684, "length_unit": "mm"}.items():
    assert exported[field] == expected  # Unknown additive fields are ignored.
data = stl_path.read_bytes()
count, = struct.unpack_from("<I", data, 80)
assert count == exported["triangles"] == 12
assert len(data) == exported["bytes"] == 84 + 50 * count == 684
vertices = [struct.unpack_from("<3f", data, 84 + 50*t + 12 + 12*v)
            for t in range(count) for v in range(3)]
assert [max(p[a] for p in vertices) - min(p[a] for p in vertices)
        for a in range(3)] == [80, 50, 27]
text_command("export-fbx", destination, "-o", root / "plate.fbx")
assert source.read_bytes() == before
print("Verified JSON edit, Body discovery and STL publication:", root)
PY
```

## Рецепт FBX: полнота и отдельная проверка файла

Из корня checkout, с native CLI на PATH либо `FCAD_CLI=/absolute/path/ferritecad`.
Рецепт создаёт native plate и импортирует **собственные временные копии** двух
опубликованных fixtures: чистую деталь и настоящую partial assembly. Исходные
fixtures не меняются. После import удаляются только эти копии STEP; FBX строится
из сохранённых `.fcad`. Для своих STEP замените пути в `cases` и ожидаемый код
import (0 — чистый, 4 — с диагностикой). UUID не заданы заранее.

Python использует JSON CLI и отдельно читает опубликованный ASCII FBX: размер,
реальные Model/Geometry/Material, локальные keys/properties/connections и размеры
native mesh. Это проверка объявленного subset файла, не универсальный FBX reader;
обязательная native кампания дополнительно читает эти экспорты pinned ufbx 0.23.0
в strict mode (`tools/check-fbx-complex.sh`), без Unity или окна.

```sh
python3 - <<'PY'
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import quote

cli = os.environ.get("FCAD_CLI", "ferritecad")
root = Path(tempfile.mkdtemp(prefix="ferrite-fbx-json-"))

def run(*args):
    return subprocess.run([cli, *map(str, args)], capture_output=True)

def envelope(p, operation):
    if p.returncode == 7:
        raise RuntimeError("Report delivery failed; keep and inspect destination; DO NOT retry automatically")
    if not p.stdout:
        raise RuntimeError(("No JSON (possibly clap usage)", p.returncode, p.stderr.decode(errors="replace")))
    value = json.loads(p.stdout)
    assert value["schema_version"] == 1 and value["operation"] == operation
    if p.returncode == 2:
        assert value["ok"] is False and "result" not in value
        raise RuntimeError(value["error"])  # typed kind + human cause, no prose parsing
    assert p.returncode in (0, 6) and value["ok"] is True and "error" not in value
    return value["result"]

def inspect_fbx(path, result, native=False):
    # Read actual object records, not the writer's Definitions count table.
    counts = {"Model": 0, "Geometry": 0, "Material": 0}
    models, geometries, connections = {}, set(), []
    current = None
    header = False
    vertices = False
    low, high = [math.inf]*3, [-math.inf]*3
    def unescape(s):
        return s.replace("&quot;", '"').replace("&cr;", "\r").replace("&lf;", "\n")
    with path.open(encoding="utf-8") as f:
        for line in f:
            if line.strip() == "FBXVersion: 7400":
                header = True
            match = re.match(r'\t(Model|Geometry|Material): (\d+),', line)
            if match:
                kind, ident = match[1], int(match[2])
                counts[kind] += 1
                current = {"id": ident, "properties": {}, "mesh": '"Mesh" {' in line} if kind == "Model" else None
                if current is not None:
                    models[ident] = current
                if kind == "Geometry":
                    geometries.add(ident)
            if line.startswith('\t\t\tP: ') and current is not None:
                fields = next(csv.reader([line.strip()[3:]], skipinitialspace=True))
                if fields[0].startswith("FerriteCAD"):
                    current["properties"][fields[0]] = unescape(fields[4])
            if line == "\t}\n":
                current = None
            match = re.match(r'\tC: "OO", (\d+), (\d+)', line)
            if match:
                connections.append((int(match[1]), int(match[2])))
            if native and line.startswith("\t\tVertices:"):
                vertices = True
            elif vertices and line == "\t\t}\n":
                vertices = False
            elif vertices:
                values = [float(v) for v in line.strip().removeprefix("a:").strip().strip(",").split(",")]
                assert len(values) % 3 == 0 and all(map(math.isfinite, values))
                for i, v in enumerate(values):
                    a = i % 3
                    low[a], high[a] = min(low[a], v), max(high[a], v)
    assert header and path.stat().st_size == result["bytes"]
    assert [counts[k] for k in ("Model", "Geometry", "Material")] == [result[k] for k in ("models", "geometries", "materials")]
    assert geometries, "This recipe expects preserved geometry"
    if native:
        assert all(abs((high[a]-low[a])-size) < 1e-8 for a, size in enumerate((0.08, 0.012, 0.05)))
    by_key = {m["properties"]["FerriteCADNodeKey"]: m for m in models.values()}
    assert len(by_key) == len(models)
    reported = []
    for omission in result["omissions"]:
        source = omission["source"]
        if source["kind"] == "imported":
            definition = "fcad1:def:source:" + source["source_id"] + ":" + quote(source["definition_key"], safe="-._~")
        else:
            assert source["kind"] == "body"
            definition = "fcad1:def:object:" + source["body_id"]
        for key in omission["placements"]:
            reported.append(key)
            model = by_key[key]
            props = model["properties"]
            assert not model["mesh"] and not any(b == model["id"] and a in geometries for a, b in connections)
            assert props["FerriteCADDefinitionId"] == definition
            assert props["FerriteCADOmissionFinding"] == omission["finding"]["entity"]
            assert props["FerriteCADOmissionRefusal"] == omission["refusal"]
            assert props["FerriteCADComplete"] == "0"
    marked = [k for k, m in by_key.items() if "FerriteCADGeometryOmission" in m["properties"]]
    assert sorted(reported) == sorted(marked)

plate = root / "plate.fcad"
created = envelope(run("create", plate, "--sample", "--size", 80, 50, 12, "--json"), "create")
assert created["destination"] == str(plate)
cases = [(plate, True, True)]
for fixture, name, status in [
    ("fixtures/step/canonical/01-single-part.step", "clean", 0),
    ("fixtures/step/interoperability/c3d-ap203-complex-assembly.stp", "partial", 4),
]:
    private_step, document = root / (name + ".step"), root / (name + ".fcad")
    shutil.copyfile(fixture, private_step)
    p = run("import-step", private_step, "-o", document)
    assert p.returncode == status, (p.returncode, p.stderr)
    private_step.unlink()  # Only the copy made immediately above.
    cases.append((document, status == 0, False))

for source, expect_complete, native in cases:
    before = hashlib.sha256(source.read_bytes()).digest()
    destination = source.with_suffix(".fbx")
    p = run("export-fbx", source, "-o", destination, "--json")
    result = envelope(p, "export-fbx")
    assert result["destination"] == str(destination)
    assert result["complete"] == (not result["omissions"]) == (p.returncode == 0)
    assert result["complete"] == expect_complete
    inspect_fbx(destination, result, native)
    assert hashlib.sha256(source.read_bytes()).digest() == before
    if p.returncode == 6:
        # Explicit partial branch: inspect/report omissions, never treat this as
        # a complete asset or silently continue a downstream geometry pipeline.
        print("PARTIAL publication kept; downstream complete-model use stopped:", destination)
        for omission in result["omissions"]:
            print(json.dumps(omission, ensure_ascii=False))
        continue
    print("COMPLETE publication independently checked:", destination)
print("Artifacts retained:", root)
PY
```

## Рецепт JSON STEP import → read-only validate → FBX

Из корня checkout; native `ferritecad` в PATH или абсолютный `FCAD_CLI`.
Рецепт использует чистую опубликованную fixture, делает private STEP-копию и
удаляет только её после подтверждённой публикации. Для другого STEP измените
`original` и ожидаемые геометрические размеры. Exit 4 явно останавливает рецепт:
человек/клиент должен принять решение по diagnostics перед downstream processing.
Reader rejection, operational failure, validation errors/warnings, partial FBX и lost report также останавливают
работу; ни один из этих исходов не превращается в молчаливый полный успех.

```sh
python3 - <<'PY'
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import uuid

cli = os.environ.get("FCAD_CLI", "ferritecad")
root = Path(tempfile.mkdtemp(prefix="ferrite-json-validate-"))
original = Path("fixtures/step/canonical/01-single-part.step")
original_hash = hashlib.sha256(original.read_bytes()).digest()
source = root / "private STEP.step"
shutil.copyfile(original, source)
document, fbx = root / "part.fcad", root / "part.fbx"
name = 'Imported "plate"\n零'

def call(operation, *args):
    p = subprocess.run([cli, operation, *map(str, args), "--json"], capture_output=True)
    if p.returncode == 7:
        raise SystemExit("Report lost: preserve and inspect destination; do not retry automatically")
    if p.returncode not in (0, 1, 2, 4, 5, 6):
        raise SystemExit(f"Unexpected process exit {p.returncode}; inspect destination before retry")
    # These arguments are valid. A usage error outside this recipe has no JSON.
    value = json.loads(p.stdout)
    assert value["schema_version"] == 1 and value["operation"] == operation
    if p.returncode == 5:
        assert operation == "import-step" and not value["ok"] and "result" not in value
        assert value["error"]["code"] == "reader_rejected"
        print(json.dumps(value["error"]["step_read"], ensure_ascii=False))
        raise SystemExit("Reader rejected STEP; no publication, processing stopped")
    if p.returncode == 2:
        assert not value["ok"] and "result" not in value
        print(json.dumps(value["error"], ensure_ascii=False))
        raise SystemExit("Operational refusal; processing stopped")
    assert value["ok"] and "error" not in value
    return p.returncode, value["result"]

code, imported = call("import-step", source, "-o", document, "--name", name)
assert code in (0, 4) and document.is_file()
assert imported["destination"] == str(document)
assert imported["name"] == name and imported["source_name"] == source.name
assert imported["source_byte_len"] == source.stat().st_size
assert re.fullmatch("[0-9a-f]{64}", imported["source_hash"])
# source_hash is the reported BLAKE3. hashlib SHA-256 below independently checks
# that this recipe's external source and saved file are unchanged by later work.
for field in ("document_id", "object_id", "source_id"):
    ident = uuid.UUID(imported[field])
    assert ident.version == 7 and str(ident) == imported[field]
assert (code == 0) == (imported["diagnostics"] == [])
if code == 4:
    print(json.dumps(imported["diagnostics"], ensure_ascii=False))
    raise SystemExit("Published with diagnostics; explicit review/acceptance required before continuing")
assert imported["definitions"] == 1 and imported["placements"] == 1
assert source.read_bytes() == original.read_bytes()
source.unlink()  # Only the private copy made above.
code, inspected = call("inspect", document)
assert code == 0 and inspected["document_id"] == imported["document_id"]
assert inspected["bodies"] == []  # ImportedStep is not a native Body.
before = hashlib.sha256(document.read_bytes()).digest()
mtime = document.stat().st_mtime_ns
code, checked = call("validate", document)
assert checked["document_id"] == imported["document_id"]
assert code in (0, 1) and checked["valid"] == (code == 0)
assert checked["errors"] == sum(d["severity"] == "error" for d in checked["diagnostics"])
assert checked["warnings"] == sum(d["severity"] == "warning" for d in checked["diagnostics"])
if not checked["valid"] or checked["diagnostics"]:
    print(json.dumps(checked["diagnostics"], ensure_ascii=False))
    raise SystemExit("Validation findings: stop; errors must be resolved, warnings require a decision")
assert document.stat().st_mtime_ns == mtime
assert hashlib.sha256(document.read_bytes()).digest() == before
# Export opens the current file separately; validate is not a version guard.
code, exported = call("export-fbx", document, "-o", fbx)
assert exported["destination"] == str(fbx)
assert exported["bytes"] == fbx.stat().st_size
if code == 6:
    assert not exported["complete"] and exported["omissions"]
    print(json.dumps(exported["omissions"], ensure_ascii=False))
    raise SystemExit("Partial FBX kept; complete-model processing stopped")
assert code == 0 and exported["complete"] and exported["omissions"] == []
# Independent ASCII FBX subset reader; no FerriteCAD library or prose parsing.
text = fbx.read_text(encoding="utf-8")
assert "FBXVersion: 7400" in text
for field, kind in [("models", "Model"), ("geometries", "Geometry"), ("materials", "Material")]:
    assert exported[field] == len(re.findall(r"^\s*" + kind + r": [0-9]+,", text, re.M))
assert exported["geometries"] == 1
vertices = re.search(r"Vertices: \*(\d+) \{\s*a: ([^}]+)\}", text)
coordinates = [float(x) for x in vertices[2].strip().split(",")]
assert len(coordinates) == int(vertices[1])
# Unchanged FBX convention: metres and (x, z, -y), so 60 x 40 x 10 mm:
assert [max(coordinates[i::3]) - min(coordinates[i::3]) for i in range(3)] == [0.06, 0.01, 0.04]
polygons = re.search(r"PolygonVertexIndex: \*(\d+) \{\s*a: ([^}]+)\}", text)
indices = [int(x) for x in polygons[2].strip().split(",")]
assert len(indices) == int(polygons[1]) == 36 and sum(i < 0 for i in indices) == 12
assert all(0 <= (i if i >= 0 else -i-1) < len(coordinates)//3 for i in indices)
assert hashlib.sha256(document.read_bytes()).digest() == before
assert hashlib.sha256(original.read_bytes()).digest() == original_hash
assert {p.name for p in root.iterdir()} == {"part.fcad", "part.fbx"}
print("Verified JSON STEP publication, read-only validation and independent FBX:", root)
PY
```

Это независимое чтение фиксированного ASCII subset, не универсальный FBX reader.
Существующая обязательная native кампания `source tools/check-fbx-complex.sh`
дополнительно читает complete и complex partial после JSON STEP import и удаления
private STEP через pinned ufbx 0.23.0 strict. На macOS запускайте `source` в Bash
с уже заданными DYLD paths. Import, inspect, validate и export — отдельные операции над
сохранёнными состояниями; FBX не получил `--expect-version`.

## §25E: сохраняемые Horizontal/Vertical

`inspect.result.sketches[]` совместимо получает required `constraint_edit` с
available/refusal/document_refusal и ordered stored curves/constraints.
Это отдельная доступность: coordinate editing constrained Sketch остаётся запрещено.
Точные поля, null/[]/unknown policy, request remove-before-add, closure retention,
result и typed `error.constraint_conflict` описаны в
[Line constraints contract](sketch-constraints-copy.md#json-v1). `operation` —
`edit-sketch-constraints-copy`; только 0 (published), 2 (refused), 7 (delivery).
Старые девять операций сохраняют wire и коды. [Запускаемый рецепт](sketch-constraints-copy.md#запускаемый-рецепт-агента).


§25N снова сохраняет operation/schema/request v1 и все exit-коды. В `add`
аддитивно доступны две формы для Sketch, который **есть одна аналитическая
окружность**, и `at` прежнего `fixed` получает третье значение:

```json
{"rule":"radius","curve_id":"UUID_FROM_DISCOVERY","radius_mm":6.75}
{"rule":"fixed","curve_id":"UUID_FROM_DISCOVERY","at":"center","x_mm":-3.5,"y_mm":4.25}
```

`radius` — своё правило, а не `distance`: расстояние между двумя точками и
радиус окружности разные величины. `at:"center"` закрепляет центр окружности;
`start`/`end` по-прежнему закрепляют конец Line, и ни одно не подставляется за
другое. Слоты: один радиус на окружность, один pin на профиль; замена — remove
exact UUID + add. Line-правило на круговом профиле и круговое правило на
Line-профиле отвергаются.

`constraint_edit` в `inspect --json` получает **отдельный** список `circles`
(`curve_id`, `center_mm`, `radius_mm` — сохранённое приближение, не решённые
значения); `curves` остаётся списком Lines и для кругового профиля пуст.
`constraints` получает `{"kind":"radius","curve_id":…,"radius":N}`, а `fixed` —
`point.at` равный `"center"`. Прежние поля, `circle_edit` и `annulus_edit`
смысла не меняют; `circle_edit.available` становится false, пока окружность
несёт ограничения, и снова true после их удаления.

`result.solve` становится **nullable**: `null`, когда правка сняла последнее
ограничение и решать стало нечего. Для Line-профиля это недостижимо — closure
links остаются, — поэтому прежние ответы не меняются. Ноль степеней свободы и
отсутствие системы — разные факты, и null говорит второй.
[Полный контракт, архитектура solver и рецепт](circle-radius-constraints.md).

§25O распространяет этот же request v1 на две окружности кольцевого профиля:
`radius` на собственном UUID каждой окружности, один `fixed` с `at:"center"`
на профиль и новая строгая форма пары:

```json
{"rule":"concentric","a_curve_id":"BOUNDARY_UUID","b_curve_id":"BORE_UUID"}
```

В discovery каждая запись `constraint_edit.circles` получает `role`:
`"boundary"`, `"bore"` или `null` у одиночной окружности. Роли читаются из
сохранённых радиусов. Сохранённая концентричность возвращается прежним
`kind:"coincident"` с двумя точками `at:"center"`; новое слово применяется
только к запросу. Удаление — по точному constraint UUID, замена — атомарный
remove/add. Если решение меняет роли стенки и отверстия или нарушает политику
кольца, публикации нет. Operation, schema и коды 0/2/7 прежние.
[Полный контракт и исполняемый рецепт](annular-circle-constraints.md).

§25F сохраняет operation/schema/request v1. В `add` команды
`edit-sketch-constraints-copy` доступны `{curve_id,rule:"horizontal"}`,
`{curve_id,rule:"vertical"}` и `{curve_id,rule:"distance",distance_mm:N}`.
N конечный и положительный, в mm, между Start/End **одного** Line. Лишний
`distance_mm` у H/V, неизвестные/отсутствующие поля и неправильные типы запрещены.
Discovery/result Distance возвращают прежнее поле **`distance`**, exact endpoints
и constraint UUID. Ориентация H/V и одна длина могут сосуществовать; две длины
одного Line запрещены. Замена — remove exact UUID + add с новым UUID, остальные
IDs сохраняются. [Полный контракт и UI semantics](sketch-constraints-copy.md),
[рецепт с независимым измерением геометрии](line-length-constraints.md).

§25G снова сохраняет operation/schema/request v1 и все exit-коды. В `add`
аддитивно доступна четвёртая форма — ровно так и пишется:

```json
{"rule":"fixed","curve_id":"UUID_FROM_DISCOVERY","at":"start","x_mm":10,"y_mm":-5}
```

`at` — строка `start` либо `end` (lowercase, никакого `at`, `middle`, `Start`
или числа). `x_mm`/`y_mm` — JSON numbers, после декодирования в f64 конечные
миллиметры; ноль и отрицательные допустимы, NaN/Infinity/overflow, строки, bool,
null и отсутствие поля отвергаются. `distance_mm` у `fixed` и `at` у
H/V/`distance` — лишние поля и тоже отвергаются. Один Fixed на профиль: второй
add, второй сохранённый Fixed и ссылка на противоположный endpoint того же
adjacent Coincident joint отвергаются до публикации.

Discovery и `added_constraints` возвращают прежний DTO `{"kind":"fixed",
"point":{"curve_id","at"},"x","y"}` — **`point`/`x`/`y`, а не `x_mm`/`y_mm`**,
ровно как Distance сохраняет поле `distance`. Замена — remove exact UUID + add
в одном request; удаление Fixed не трогает Coincident closure, H/V и размеры.
[Полный контракт и рецепт](fixed-sketch-vertex.md).

§25H снова сохраняет operation/schema/request v1 и все exit-коды. В `add`
аддитивно доступна пятая форма — ровно так и пишется:

```json
{"rule":"equal_length","a_curve_id":"UUID_A","b_curve_id":"UUID_B"}
```

Оба поля обязательны и называют **разные** целые Lines выбранного Sketch;
ведущей стороны нет и числа у связи нет. `curve_id`, `distance_mm`, `at` и любые
другие поля здесь лишние и отвергаются, как и `a_curve_id`/`b_curve_id` у
прочих rules; не-UUID строка, число, bool и null отвергаются. Для занятости
слота `(A,B)` и `(B,A)` — одна пара, и сохранённый сегмент, записанный
End→Start, называет ту же Line. Self-pair, чужая Line, duplicate и reversed
duplicate отвергаются до публикации как `input`; избыточность попадает в
`solve.redundant_constraint_ids` опубликованного результата, а противоречие —
в typed `error.constraint_conflict` с `kind:"equal_length"` среди фактов.

Discovery и `added_constraints` возвращают прежний DTO `{"kind":"equal_length",
"a":{"from","to"},"b":{"from","to"}}` — **сегменты `a`/`b` с `{curve_id,at}`, а
не `a_curve_id`/`b_curve_id`**, и без поля `distance`. Замена — remove exact
UUID + add в одном request; удаление равенства не трогает Coincident closure,
H/V, длины и Fixed, а замена ведущей длины сохраняет UUID самого равенства.
[Полный контракт и рецепт](equal-line-lengths.md).

§25I снова сохраняет operation/schema/request v1 и все exit-коды. В `add`
аддитивно доступны шестая и седьмая формы — ровно так и пишутся:

```json
{"rule":"parallel","a_curve_id":"UUID_A","b_curve_id":"UUID_B"}
{"rule":"perpendicular","a_curve_id":"UUID_A","b_curve_id":"UUID_B"}
```

Оба поля обязательны и называют **разные** целые Lines выбранного Sketch;
ведущей стороны нет, а числа и угла в градусах у связи нет. `curve_id`,
`distance_mm`, `at` и любые другие поля здесь лишние и отвергаются, как и
`a_curve_id`/`b_curve_id` у прочих rules; не-UUID строка, число, bool, null и
`Parallel`/`perpendicularity` вместо точного имени отвергаются. Для занятости
слота `(A,B)` и `(B,A)` — одна пара, и **один ответ на пару**: `parallel` и
`perpendicular` занимают один слот, а `equal_length` на той же паре — отдельный.
Self-pair, чужая Line, duplicate, другой ответ и reversed duplicate отвергаются
до публикации как `input`; избыточность попадает в
`solve.redundant_constraint_ids` опубликованного результата, а противоречие —
в typed `error.constraint_conflict` с `kind:"parallel"`/`"perpendicular"` среди
фактов.

Discovery и `added_constraints` возвращают прежние DTO
`{"kind":"parallel","a":{"from","to"},"b":{"from","to"}}` и
`{"kind":"perpendicular",…}` — **сегменты `a`/`b` с `{curve_id,at}`, а не
`a_curve_id`/`b_curve_id`**, и без поля `distance`. Замена — remove exact UUID +
add в одном request, в том числе на другой ответ той же пары; удаление связи не
трогает Coincident closure, H/V, длины, Fixed и остальные связи, а замена
ведущей длины сохраняет их UUID.
[Полный контракт и рецепт](line-pair-orientation.md).
