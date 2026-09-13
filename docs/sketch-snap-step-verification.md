# §25D — проверка шага привязки координат

Дата: 2026-09-13. База/HEAD `f04a6333ae9d37dfbc714dc8e44512d7612fe402`,
ветка `sketch-snap-step`. Первичная передача была unstaged/uncommitted.
Ниже отдельно записаны её результаты и последующее независимое ревью.

## База и границы

В начале выполнен fetch, `main = origin/main`, дерево чистое. PR #29 — MERGED;
head `712109a76d19dfb7f3544ac14280a4b2a49cacb1` и merge имеют одинаковое дерево
`de9f3b7c6eb09f61f3f5691666ad709b21391219`. Проверены 27/27 success checks,
6/6 success workflow.
[Runtime базы](https://github.com/gesriot/ferrite-cad/actions/runs/34778570255),
[CI базы](https://github.com/gesriot/ferrite-cad/actions/runs/34778570234).
Это CI базы; **CI незакоммиченного diff не выполнен**.

AGENTS.md в репозитории не найден. Чужой a200 (`5b207191`, detached) и
исходные fixtures не изменялись. Jobs/document/CLI/API/schema/C++/FFI/dependencies
не менялись; inventories не требуют генерации. Большой STEP/partial FBX, полный
GPU suite и OCCT из исходников локально не запускались.

## Реализованное поведение

[Протокол](sketch-snap-step.md): в том же canvas нового и сохранённого Line Sketch
компактный выбор `Snap: Off / 0.1 mm / 1 mm / 5 mm / 10 mm`, по умолчанию Off.
Настройка редактора, не модель. Свободный click и drag округляют документные
координаты относительно начала XY; половина шага — от нуля; строки без
`0.30000000000000004`. Drag сохраняет grab offset, привязывает только оси со
смещением, фиксирует шаг на press. Поля X/Y и Snap Off без привязки.
Переключение Snap не двигает вершины и не пишет Undo. Сетка по-прежнему
ограничена по числу линий и не задаёт шаг.

Три регрессии независимого ревью §25C сохранены: порядок press/release в одном
кадре, несколько жестов A→B→A, отмена до release. Общая PolygonExtrusion /
SketchChoice policy не дублировалась.

Native gate `native_dragged_sketch_worker_and_cli_preserve_identity_and_geometry`
расширен: Snap 1 mm, Fit, слегка неточные пиксели обеих правых вершин L 60→80
дают ровно `"80"` без правки полей. Имя теста и workflow не добавлялись.

## Локальные проверки diff

Логи в `/private/tmp/ferrite-25d/`. Native env: `/private/tmp/ferrite-25c/native-env.sh`,
target `/private/tmp/ferrite-24b-native-target`. `CARGO_BUILD_JOBS=1`,
`--test-threads=1`.

| Проверка | Исполненный результат | Лог |
|---|---|---|
| fmt, workspace clippy all targets/features `-D warnings` | pass, повторены после restore | stdout clippy/fmt |
| Sketch pointer + native workers | 17/17 native, 0 skip | cargo test `sketch::` |
| 8 edit workers | 8/8 | `edits.log` |
| creates + dialogs | 17 + 3 | `creates.log`, `dialogs.log` |
| 4 native STL workers | 4/4 | `stl.log` |
| CLI create / edit-extrude / edit-sketch / json_v1 / sketch_extrude | 9+2+5+13+4 | `cli.log` |
| document + jobs | 38+1 ignored timing, 32+29+28+9 document libs, 50 jobs | `document-jobs.log` |
| Watchdog unit tests | 5/5 | `watchdog-tests.log` |
| Pinned ufbx 0.23.0 `--identity` на `drag.fbx` | 6 checks, 0 failures | `ufbx-drag.log` |

Native artifacts: `/private/tmp/ferrite-25d/native-artifacts/{drag.fcad,drag.stl,drag.fbx}`.
STL 1084 bytes. Peer CLI — уже собранный native `ferritecad` того же target
(исходники CLI не менялись).

Отдельная stub-сборка: `/private/tmp/ferrite-24j-stub-target`, `stub-env.sh`,
`CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew`. Warning «Open CASCADE was not usable»;
otool viewer без TKernel (`stub-imports.log`). Sketch harness 17 ok =
**14 исполненных + 3 явных skip-return** native workers
(`skipped: no OCCT …`). Snap pointer test на stub исполнен. Skips не выдаются
за геометрию.

Временная поломка: в `Canvas::preview` отключён `Snap::apply` (снова `to_string`).
Исполненный тест
`snap_step_rounds_pointer_input_without_changing_fields_or_off_precision`
упал: `left: ["80.26315789473685", "0.37"]`, `right: ["80", "0.37"]`
(`mutant-snap-off.log`, cargo test exit 101). Это assertion failure, не
compile failure и не ноль тестов. Источник восстановлен; 13 drag tests включая
native gate снова pass (`restored-snap-native.log`).

## GUI и память

CUA/getApp/AX/screenshot в этой сессии недоступны. Интерактивное окно
(Snap Off/On, drag до точных 80, Undo/Redo, публикация) **не проверялось**.
Свежий staged bundle и watchdog-прогон живого viewer из-за этого не запускались.
Headless тесты не выдаются за проверку окна. Причина прежнего пользовательского
OOM по-прежнему не установлена. Намеренные dyld missing-library probes не
запускались.

Пять unit-тестов `tools/test-viewer-memory-guard.py` — про рассинхрон журнала
и лимит, не про Snap в окне.

## Файлы и передача

| Файл | Статус |
|---|---|
| `crates/ferritecad-app/src/sketch.rs` | tracked, modified |
| `crates/ferritecad-app/src/sketch/drag_tests.rs` | tracked, modified |
| `docs/sketch-snap-step.md` | new / untracked |
| `docs/sketch-snap-step-verification.md` | new / untracked |
| `docs/sketch-vertex-drag.md` | tracked, modified |
| `docs/implementation-plan.md` | tracked, modified |
| `docs/sketch-extrude-create.md` | tracked, modified |
| `docs/edit-sketch-copy.md` | tracked, modified |
| `docs/cli-capabilities.md` | tracked, modified |
| `README.md` | tracked, modified |

Новый native test / workflow / DTO / dependency не добавлялись. Полный
трёхплатформенный runtime (72 native / 21 validation / 5 watchdog) этого
незакоммиченного diff в CI не гонялся; локально сохранены прежние имена
gates и отсутствие новых skip. Следующий срез не начат.

## Независимое ревью §25D

Ревью воспроизвело и исправило четыре дефекта:

1. Число непосредственно ниже половины шага 0.1 mm округлялось вверх из-за
   прибавления 0.5. Используется `f64::round`, без ручного сдвига порога.
2. Cast NaN в integer превращал недопустимую координату в ноль и позволял общей
   polygon policy принять изменённый контур. Non-finite остаётся недопустимым.
3. Большое конечное смещение насыщало integer cast и переполняло последующее
   умножение. Integer-промежуточного результата больше нет; отказ проверяет
   существующая общая policy, без отдельного UI-лимита.
4. Click Snap 10 mm после drag в том же кадре ретроактивно менял его шаг:
   вместо 66 mm получалось 70. Принятая кнопкой смена Snap теперь применяется
   при соответствующем событии в общей последовательности; шаг жеста всё так же
   фиксируется на press.

Все четыре сначала дали исполняемый assertion/panic failure, затем прошли.
Старые три проверки порядка жестов §25C сохранены. Настройка не переносится
в document/jobs/CLI. Также актуализирована таблица CLI capabilities, где оставались
устаревшие ограничения до появления координатного редактирования.

Финальные native проверки: fmt, workspace clippy all targets/features с
`-D warnings`; document/eval/jobs/UI — **466 passed**, один прежний timing ignored;
CLI — **81 passed** (8 unit + 73 process); Sketch — **21**, creates — **17**,
edit workers — **8**, STL workers — **4**, без native skips. CLI/app пересобраны
до worker gates. В свежем stub Sketch harness 21 passed означает **18 исполненных
+ 3 явных native skips**; импорты обоих бинарников не содержат OCCT/PlaneGCS.
Пять watchdog tests реально исполнены. На headless `drag.fbx` и GUI FBX pinned
ufbx 0.23.0 дал по **6 checks, 0 failures**.

Интерактивный smoke выполнен на свежем staged bundle (UUID
`14CE612B-4E1C-31C0-AB61-C0505C191E93`, deep strict codesign pass), PID 28917:

- Snap Off: 60 → 80.07456568667763 mm при неточном drag; Undo вернул 60.
- Snap 1 mm: те же неточные движения обеих правых вершин дали ровно 80 без
  исправления числовых полей. Undo отменил только второй жест, Redo вернул его.
- Системный Save Cancel сохранил обе координаты и шаг. Повторный Save опубликовал
  `gui-edited.fcad`; async Open показал модель и обновил заголовок.
- Повторное открытие редактора этой копии сбросило Snap в Off. Черновик отменён,
  viewer закрыт штатно. После закрытия CUA к нему больше не обращался.

GUI и CLI копии совпали по всем SQL-ячейкам, включая attached source claims;
исходник не изменился. Cold rebuild/validate прошли. STL и FBX совпали побайтово.
Независимый binary STL разбор: 20 triangles, 1084 bytes, bounds 80×40×10 mm,
volume 20000 mm³. Живой viewer под watchdog: **188.02 MiB peak footprint**, cap
1536 MiB, pressure normal (1), swap без роста, exit 0, PID отсутствует.
Финальная последовательная native кампания под guard: peak 258.25 MiB, exit 0.
Причина прежнего пользовательского OOM не установлена; намеренные missing-library
probes и тяжёлые локальные STEP/full GPU кампании не запускались.

Первое подключение CUA было отклонено автоматической проверкой как предполагаемый
запуск неизвестного приложения. После read-only подтверждения уже работающего PID,
пути и UUID подключение к этому процессу было разрешено; новый viewer не запускался.

Логи и исполняемые проверки ревью: `/private/tmp/ferrite-pr30-review/`, в том числе
`final-*.log`, `snap-*-before.log`, `snap-failing-first.log`, `final-verification.json`,
`final-memory.json`, `gui-watch.jsonl`, `guard-tests.log` и `gui-ufbx.log`.
Проверки опубликованного head/merge на трёх ОС учитываются отдельно в PR; приведённые
локальные результаты не выдаются за Linux/Windows GUI или CI незакоммиченного diff.
