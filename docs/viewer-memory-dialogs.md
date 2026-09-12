# Viewer: память и отказы системных диалогов (§23D)

База: `49c9ea4fe852060695334190e92c277a5681ff64`, merge PR #25.
Ветка: `viewer-memory-dialogs`. Изменения оставлены без commit/staging.

## Адаптер и граница восстановления

`crates/ferritecad-app/src/dialogs.rs` — один UI-адаптер Open/New/Edit/Export
FBX/Export STL. Его внутренний результат — `Selected(PathBuf)`, `Cancelled` или
`Failed`. Только выбранный путь доходит до прежнего обработчика операции.
Отказ показывает отдельную красную строку под toolbar и не заменяет job status.
Следующая успешно открытая панель (выбор либо отмена) убирает эту строку.
Хранится одна ошибка, не история. Формы New/Edit/STL сохраняются при отмене панели.

Адаптер меняет свою ошибку и запрос redraw. Принятые document/scene/title,
selection/visibility, доступность экспорта и generation остаются у прежних
владельцев. Нет работы без выбранного пути, второго jobs-маршрута, файловых
обходов или нового CLI-поведения. Общие jobs и правила публикации не изменены.

Аудированы закреплённые `rfd 0.17.2`, `objc2-app-kit 0.3.2`, `objc2 0.6.4`:

- В `rfd/backend/macos/file_dialog/panel_ffi.rs` первым AppKit-вызовом
  `Panel::build_pick_file` / `build_save_file` является constructor. Его NULL
  вызывает Rust panic **до** настройки панели, `Panel::new`, focus/policy managers,
  attachment к parent и modal loop. Viewer вызывает это синхронно на главном
  потоке winit. Autorelease pool `objc2::rc` освобождается при Rust unwind.
- Только точный payload `unexpected NULL returned from +[NSOpenPanel openPanel]`
  для Open или `unexpected NULL returned from +[NSSavePanel savePanel]` для Save
  переводится в `Failed`. Поддерживаются Rust `&str` и `String`.
- Любой иной panic, включая получение URL после открытия панели, продолжает
  unwind. Нет `AssertUnwindSafe`, нового `unsafe`, перехвата вокруг приложения,
  подавления panic hook или изменений служб macOS. Hook может записать известный
  panic в stderr даже при восстановлении. `panic=abort` восстановиться не может.

Это обработка конкретного отказа создания панели, **не исправление причины сбоя
view-bridge** и не обещание восстановления после любых ошибок AppKit. При
обновлении зависимостей эту границу нужно проверить снова. Windows/Linux идут
через прежние `rfd` backend: их `Option` не даёт дополнительных сведений об
ошибках, которые сам backend скрывает как `None`. Защита от constructor panic
относится только к macOS.

## Измерение памяти, 2026-09-12

Независимый [`watch-viewer-memory.py`](../tools/watch-viewer-memory.py) создаёт
один дочерний процесс остановленным, записывает PID/birth identity, вооружает
наблюдение и только затем разрешает exec. Он не присоединяется к существующему
viewer и отказывается стартовать рядом с обнаруженным viewer/test process.
Останавливаются только собственный PID и обнаруженные потомки с той же birth
identity. Остановка эксперимента даёт **125, не pass**.

Независимое ревью обнаружило два дефекта первоначального watchdog: SIGTERM
завершал только наблюдатель, а ошибка записи `aborted` в журнал прерывала очистку.
Оба воспроизведены на коротком собственном `/bin/sleep`, без GUI и OOM; после
выхода guard процесс оставался жив. Исправление обрабатывает SIGTERM/SIGHUP/SIGINT,
защищает получение PID при запуске и выполняет остановку независимо от доступности
журнала. Сигнал передаётся отдельным исключением, которое не поглощается кодом,
повторяющим I/O после `InterruptedError`. Повторные сигналы не прерывают очистку.
NaN/Infinity не принимаются как пределы. SIGKILL наблюдателя и короткий пик между
samples остаются за пределами гарантий; это не системная квота памяти.

`python3 tools/test-viewer-memory-guard.py`: пять регрессионных tests без viewer/GPU
проверяют обычный выход, три сигнала, ошибку журнала, превышение 32 MiB небольшим
выделением 64 MiB и нечисловые пределы. Собственные процессы после остановки не
остаются, независимый контрольный процесс сохраняется. Проверка включена в
обычный CI macOS; она не выдаётся за проверку Darwin watchdog на других ОС.

На этом 16 GiB Mac предел — **1536 MiB physical footprint**, normal pressure = 1,
минимум 4 GiB свободного диска. Есть конечное время и файл `<log>.stop`; отказ
обязательного измерения останавливает эксперимент. `proc_pid_rusage` даёт RSS и
footprint примерно каждые 0,5 s; `top -pid … -stats pid,mem,cmprs` — реальный CMPRS
примерно раз в 10 s. `sysctl` даёт pressure/swap, `statvfs` — свободный диск.
Footprint включает приписанную процессу unified/GPU memory: RSS, footprint и
compression **не складываются**. При наличии собственных дочерних процессов
суммируется только одинаковая footprint-метрика. Память WindowServer и отдельных
служб не объявляется памятью viewer. Короткий пик между samples может быть
пропущен; guard не гарантирует предел в каждый момент.

Обычные bundle получены через `runtime-closure.sh` → `stage-runtime-layout.sh`,
с `Info.plist`, install names/RPATH и ad-hoc подписью. Пройдены
`check-staged-layout.sh` и `check-native-inventory.sh`. Vendor-пути скрывались
**только от процесса package probe** локальным sandbox-профилем; общие каталоги
не переименовывались. Проверены cold rebuild, provenance и отказы при скрытии
библиотек внутри собственной раскладки. Старые review-копии не использовались.

Сценарий каждого обычного запуска: empty idle → начальный Open/Cancel → пять
загрузок `A, B, A, B, A` → суммарно десять Open/Cancel, три New/Save/Cancel с
отменой формы и три Export FBX/Cancel → Iso → idle → штатный Close. A — плита
60×40×10 mm, B — 30×25×17 mm, созданы настоящим CLI в приватном каталоге с
пробелами/Unicode. CUA наблюдал основное окно и панели; отметки после каждого
цикла связывают одно основное окно с sample того же PID. Системная панель —
дополнительный sheet, при вводе пути также есть Go To. Это не автоматический
census всех системных окон. Длительность жестов/idle в двух запусках различается.
Промежуточные CUA captures иногда показывали только UI/сетку; после нового redraw
модель была видна. Причина этой особенности захвата/отрисовки здесь не установлена;
потеря принятого document/snapshot не обнаружена.

| Метрика | База | Изменённый bundle |
|---|---:|---:|
| PID / длительность | 53697 / 422,9 s | 64840 / 384,8 s |
| Samples / максимум процессов | 779 / 1 | 710 / 1 |
| Пик footprint, MiB | 281,7 | 213,2 |
| Пик RSS, MiB | 239,1 | 142,5 |
| Empty idle footprint, MiB | 178,2 | 204,0 |
| После трёх циклов, MiB | 185,5 → 185,3 → 186,3 | 203,9 → 204,5 → 204,6 |
| Поздняя точка footprint, MiB | 187,2 | 206,0 |
| Поздняя compression (`top`) | около 8048K | около 27M |
| Системный swap до → после, GiB | 4,099 → 4,060 | 4,052 → 4,044 |
| Минимум свободного диска, GiB | 31,18 | 30,84 |
| Pressure / exit | всегда 1 / 0 | всегда 1 / 0 |

Отдельная проба восстановления после двух constructor faults: один PID 74886,
335,9 s, пик footprint 339,4 MiB, pressure 1, штатный Close/exit 0. Metal-тест
20 кадров: PID 74305, один выполненный test, exit 0, измеренный пик 159,5 MiB
(всего два samples у короткого процесса), обязательные GPU/OCCT/PlaneGCS.
Watchdog не остановил ни один из этих четырёх экспериментов.

**OOM пользователя не воспроизведён и не объявляется исправленным.** Малые
циклы дали близкие поздние контрольные точки с небольшим приростом, но не
доказывают ограниченность памяти на произвольном числе повторов или большой
сцене. Разницу абсолютных значений запусков нельзя приписывать адаптеру. Оснований
менять владение геометрией, renderer или параллелизм продукта не найдено. Большой
STEP и полный тяжёлый GPU corpus не запускались. Причина исходного пика, его
величина и связь с view-bridge неизвестны.

Ранняя попытка встретила заблокированный Mac: собственный viewer остановлен
через watchdog (125), GUI не засчитан. После ответа пользователя исполнены новые
измерения выше. Прежние review-копии без LC_RPATH — отдельные ошибки подготовки
harness, не доказательство OOM продукта.

## Проверки и рецепт

- Failing-first: точный test
  `dialogs::tests::null_panel_constructor_is_a_failure_instead_of_unwinding_the_viewer`
  исполнился и упал с известным panic до catch. После исправления проходят он,
  тест остальных panic и тест трёх исходов/видимой ошибки.
- `tests::dialog_refusal_preserves_the_accepted_scene_and_starts_no_work`
  сохраняет выбранное/скрытое, camera/title/catalogue/generation, доступность
  экспорта и source sentinel; callbacks загрузки/публикации не вызваны.
- В приватной GUI-сборке временная инъекция один раз вызвала точный Rust panic на
  каждой из двух constructor boundaries. После Open fault видны ошибка и прежний
  выбранный Body; тот же PID выполняет настоящий Open/Cancel и Open B. После Save
  fault сохраняются B и форма New; настоящий Save/Cancel, FBX/Cancel,
  Edit/Save/Cancel и STL/Save/Cancel снова работают. В каталоге остались две
  модели с прежними sizes/mtime. Это управляемый fault, не принудительный сбой
  службы. Инъекция удалена, production source побайтово сверён с сохранённым до
  неё, обычные CLI/viewer пересобраны, положительные tests повторены.
- Fmt; workspace clippy `--all-targets --all-features -- -D warnings`; UI 102,
  jobs 47, 8 native edit + 4 native STL, три adapter tests и отдельный preservation
  test. Дополнительный фильтр `dialog_` — 7 tests. Всё перечисленное без
  skips/ignored. Новый peer CLI собран до workers.
- `tests::twenty_frames_of_one_document_read_nothing_and_upload_nothing`:
  настоящий Metal/native, `--exact --test-threads=1`, watchdog. Полного app/GPU
  suite нет. OCCT source, зависимости и inventories не менялись.
- Watchdog проверен на `/bin/sleep` и приватном выделении 64 MiB с пределом
  32 MiB: 0 и ожидаемый 125. Независимый контрольный процесс пережил остановку цели.
- GUI/GPU локально — macOS aarch64. Linux/Windows GUI и native runtime этого diff
  здесь не исполнялись. Попытки `cargo check --all-targets --all-features` для
  Linux/Windows остановились в зависимостях: нет `x86_64-linux-gnu-gcc` и
  `ml64.exe` соответственно; это **не** успешные кросс-сборки. Их логи —
  `cross-linux.log` / `cross-windows.log`. Исходная реализация не меняла workflow. Ревью добавило пять process tests
  watchdog в CI macOS; сохраняются 21 validation executions и 54 required native gates. CI незакоммиченного diff
  **не выполнен**; проверки базы не являются проверками этих изменений.

Обычное измерение после сборки (`CARGO_BUILD_JOBS=2`) и штатных package checks:

```bash
# Один viewer; никаких параллельных GPU tests.
python3 tools/watch-viewer-memory.py \
  --log /private/tmp/my-viewer-measurement.jsonl --seconds 1200 \
  -- /absolute/staging/FerriteCAD.app/Contents/MacOS/ferritecad-viewer
# Последовательность A/B выше, затем штатный Close.
# Для остановки ТОЛЬКО этого эксперимента:
# touch /private/tmp/my-viewer-measurement.stop
```

Нужен доступ к процессным метрикам macOS. Если sandbox этого не позволяет, не
запускать viewer без guard. Exit 125 требует разбора `aborted/refused`, а не
продолжения нагрузки. После аварии проверить PID/exit в журнале **до** обращения
CUA к bundle: инструмент может запустить завершившееся приложение заново.

Логи: `/private/tmp/ferrite-23d/`: `memory-summary.json`, `*.jsonl`,
`*.actions.jsonl`, `*.stderr`, `dialog-failing-first.log`, `restored-*.log`,
`ui-jobs.log`, `edit-final.log`, `stl-final.log`, `clippy-final.log`, три staging
каталога. Приватная инъекция: `dialogs-fault-probe.rs` / `build-fault-probe.sh`,
не в production diff. `/tmp` материалы могут исчезнуть; результаты и ограничения
зафиксированы здесь. База перепроверена: PR #25 MERGED, 23/23 checks, пять workflow,
21 validation executions, 54 native gates, strict ufbx 256+6+6+256 на каждой ОС.

Следующий шаг — независимое ревью незакоммиченного diff. После устойчивости
продуктовый приоритет — интерактивный эскиз и Extrude через UI и CLI; этот срез
их не начинает.
