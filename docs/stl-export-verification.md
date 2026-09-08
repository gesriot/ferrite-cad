# §24E: общий STL export — протокол передачи

База: `main = origin/main = 1d1df7b6721da7d9f07240c0c9cfa0c4a727fb4a`,
merge [PR #19](https://github.com/gesriot/ferrite-cad/pull/19), head
`d006484b06c08aceb4365bb5635ebbc1d330bd3e`. Перед началом проверены свежий fetch,
чистое дерево, merge и success всех 23 jobs пяти workflow точного merge SHA,
включая [combined runtime](https://github.com/gesriot/ferrite-cad/actions/runs/34225247796).
Это доказательство базы, не нового diff. Ветка `shared-stl-export`; изменения
оставлены незакоммиченными для независимого ревью. Чужой worktree a200 и исходные
fixtures не менялись. Следующий срез не начат.

## Контракт и границы

`ferritecad_jobs::export_document_as_stl(StlExportRequest, kernel factory,
OperationContext)` возвращает `StlExport` только после публикации: destination,
Body UUID/optional name, triangles, bytes. Типизированный запрос содержит путь,
назначение, `BodySelection`, `TessellationParams`, `Existing`. Factory создаёт
ядро в вызывающем worker после read-only open и выбора Body: ошибки чтения/выбора
сохраняются и в сборке без OCCT. Выбор и cold rebuild используют один снимок.
Нет миграции, cache sidecars, промежуточной `.fcad` и повторного открытия источника.

Общий выбор: единственный Body допустим без указания; несколько требуют выбора;
UUID имеет приоритет перед совпадающим текстом имени; неоднозначное имя и
отсутствующий UUID отказывают. ImportedStep не становится Body. CLI готовит
`NameOrId`/`Only`, UI передаёт `Id`, не ordinal, имя или GPU pick id. Инструкции
`--solid`/`--force` остаются словами CLI, а форма использует выбор и Replace.
Параметры сохраняют defaults 0.01 mm / 0.5 rad; binary STL хранит миллиметры.
Обычные CLI flags, stdout и exit codes сохранены; новый JSON-флаг не добавлен.

Форма использует уже принятые Body facts LiveScene. Путь, UUID и параметры
захватываются до Save и не подменяются последующим Open. Источник — сохранённый
файл на этом пути в момент запуска worker: изменения после Open учитываются,
пропавший UUID не заменяется другим. Экспортируется всё выбранное тело независимо
от камеры/видимости. Пустое окно, пустой документ и imported-only не предлагают STL.

До работы и непосредственно перед publish проверяется cancellation. Scratch
записывается, синхронизируется и закрывается до атомарного publish; no-clobber
повторяется в `Temporary::publish`, source alias проверяется также перед publish,
в том числе при Replace. Отказ/отмена до publish сохраняют прежние файлы и очищают
scratch. Все shapes освобождаются, включая ошибки rebuild/tessellation/serialization.
После publish cancellation не проверяется как отказ: готовый файл остаётся при
поздней отмене или потерянном UI-ответе.

UI переиспользует lifecycle Exports: owned workers, generation guard,
cancel/join на shutdown. Cancel Save не запускает worker; Cancel Replace не пишет;
Open/новая заявка оставляют старый ответ без права менять новый статус. STL status
отдельно показывает Body/triangles/bytes, не FBX omissions. FBX writer, identity,
wire contract, C++/FFI и GPU renderer не менялись. Runtime dependency edges и
Cargo.lock не менялись: регенерация inventories/notices/SBOM не требовалась.

## Воспроизводимый публичный CLI-рецепт

В PATH должен быть свежий native `ferritecad` с доступными OCCT/PlaneGCS libraries;
либо задайте `FERRITECAD` полным путём. Создаются только временные файлы.

```sh
set -eu
stl_cli="${FERRITECAD:-ferritecad}"
stl_dir="$(mktemp -d)"
"$stl_cli" create "$stl_dir/plate.fcad" --sample --size 60 40 10
"$stl_cli" validate "$stl_dir/plate.fcad"
"$stl_cli" rebuild "$stl_dir/plate.fcad" --cold
"$stl_cli" export-stl "$stl_dir/plate.fcad" -o "$stl_dir/plate.stl"
python3 - "$stl_dir" <<'PY'
import pathlib, struct, sys
root = pathlib.Path(sys.argv[1])
data = (root / 'plate.stl').read_bytes()
count, = struct.unpack_from('<I', data, 80)
assert count == 12 and len(data) == 84 + 50 * count == 684
vertices = [struct.unpack_from('<3f', data, 84 + 50*t + 12 + 12*v)
            for t in range(count) for v in range(3)]
assert [max(p[a] for p in vertices) - min(p[a] for p in vertices)
        for a in range(3)] == [60, 40, 10]
assert not list(root.glob('*.fcad-*'))
print('12 triangles, 684 bytes, 60 x 40 x 10 mm; no sidecars')
print(root)
PY
```

В нескольких Body используйте `--solid <Body UUID>` или уникальное имя;
CLI без выбора перечислит кандидатов и откажет. UUID Body — не UUID Extrude из
JSON inspect. Срез inspect JSON для Extrude здесь не расширялся до Body API.
Для повторной записи STL существует `--force`, исходная `.fcad` и её aliases
остаются запрещённым назначением.

UI-рецепт: Open созданной плиты → Export STL… → проверить выбранный Body и defaults
→ Save STL… → новый файл. Для нескольких Body сначала выбрать нужный UUID.
Повторить с Cancel в системном Save; затем с занятым назначением и Cancel/Replace
в вопросе приложения. Сравнить файл с CLI при одинаковых Body и параметрах.
Длинную операцию можно отменить через Cancel export; уже опубликованный файл
при позднем ответе сохраняется.

## До/после переноса

До правок свежим CLI точной базы экспортированы приватная копия committed plate
и новая плита 91×53×17. После правок оба экспорта повторены в те же пути с
`--force`: точные stdout/stderr, STL SHA-256 и SHA-256 исходников совпали.
У обоих STL 12 triangles / 684 bytes; stderr пустой, exit 0.

| Размеры, mm | SHA-256 STL |
| --- | --- |
| 60×40×10 | `95fa8589e0a699bb0e29c7eec88e598e94de4a21e77dcafd7693b214f27cf736` |
| 91×53×17 | `cd66ebe36fd71138760d3db73d646d8cfe705ed620916bf669d34a4414bd62fc` |

Точный stdout двух запусков (каждая последняя строка заканчивается LF):

```text
wrote /private/tmp/ferrite-24e/baseline/plate.stl (12 triangles, 684 bytes) from Plate (019fe39c-0c09-78a1-80d3-907d146bc611)
  deflection: 0.01 mm linear, 0.5 rad angular
```

```text
wrote /private/tmp/ferrite-24e/baseline/other.stl (12 triangles, 684 bytes) from Plate (01a0810f-6fe9-7863-8743-ccf2212b16ac)
  deflection: 0.01 mm linear, 0.5 rad angular
```

## Проверки нового diff, macOS arm64

Логи этой локальной сессии: `/private/tmp/ferrite-24e/`; пути временные и не
являются частью поставки. Native target `/private/tmp/ferrite-24b-native-target`
пересобран на новом diff; OCCT 8.0.1 и planegcs FreeCAD 1.0.1 переиспользованы.
`FERRITECAD_REQUIRE_OCCT=1`, `FERRITECAD_REQUIRE_PLANEGCS=1` включены.

| Проверка | Реальный результат |
| --- | --- |
| `cargo fmt --all -- --check`; workspace clippy all targets/all features `-D warnings` | pass |
| Свежая native CLI/viewer сборка, viewer `--solver-info` | pass; solver available, exit 0 |
| Полные all-features tests document/jobs/ui/cli/app | 629 harness passed, 0 failed, 1 existing ignored timing test; **21 GPU checks явно skipped**: sandbox не дал Metal adapter |
| Новый `exports::stl::tests::native_` | **4/4 выполнены**, без skip; повторены после добавления unnamed Body |
| Старые UI/CLI edit gates | **8/8 выполнены**, без skip |
| JSON `native_json_inspect_edit_contract`, включая create | выполнен; JSON suite 11/11 |
| Существующий `tests/export_stl.rs` | 9/9; файл тестов не менялся |
| Существующие app FBX tests | 26/26; дополнительно входят в полный прогон |
| Общие jobs STL guards/resource tests | 4/4; входят в jobs 41/41 |
| Отдельный свежий no-native target | CLI/viewer build pass; CMake `OpenCASCADE_DIR-NOTFOUND`, solver unavailable exit 3 |
| No-native create/edit/STL/JSON suites | 31 harness passed, включая **8 явных native skips**, которые не считаются геометрией |
| No-native app STL suite | один настоящий refusal/process gate; четыре native gate явно skipped |
| Реальный macOS GUI smoke свежим viewer через CUA | Open, явный UUID, Save, Cancel Save, Cancel Replace, Replace; два результата побайтово совпали с CLI |
| Export/solver/notice ownership, SPDX, actionlint | pass |

Полный прогон: `cargo test -p ferritecad-document -p ferritecad-jobs -p ferritecad-ui
-p ferritecad-cli -p ferritecad-app --all-features -- --nocapture`. Для проверки
четырёх новых gate после свежей сборки CLI используйте:

```sh
FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1 \
cargo test --all-features -p ferritecad-app --bin ferritecad-viewer \
  exports::stl::tests::native_ -- --nocapture
```

Native gates сравнивают **peer CLI process** с настоящим UI form/request/owned
worker, который вызывает общий job: две плиты; два Twin с высотами 10/23; UUID
priority перед похожим именем; смена ordinal/имени; unnamed Body. Проверяются
байты и размеры геометрии, source bytes, занятое назначение, ноль/несколько Body,
imported-only, отсутствующий UUID, ambiguous name, bad tessellation, WAL refusal,
hardlink и Unix symlink, Replace alias, новое содержимое сохранённого файла,
исчезнувший UUID, Save/Replace cancel, stale answers и shutdown join. MockKernel
дополняет реальные процессы отказами rebuild/tessellation/serialization и
измерением нулевого числа живых shapes при выходе.

Гонки синхронизированы на закрытом scratch (0.95) и завершённом publish (1.0):
появление назначения, cancel до publish, late cancel, ответ прежнего документа
или уже опубликованной операции при новом работающем worker. Проверяются реальные
файлы/результаты и отсутствие scratch, а не только mock call counts.

Четыре временные поломки дали **исполняемое падение нужного gate**, затем исходники
восстановлены и положительные проверки повторены:

| Поломка | Наблюдённое падение |
| --- | --- |
| Id selector берёт первое тело | UI/CLI parity: геометрия выбранного второго тела неверна |
| Keep при publish превращён в Replace | publication race неожиданно успешен, занятое назначение перезаписано |
| Cancel проверяется после publish | native worker сообщает Cancelled вместо успеха при готовом файле |
| Generation guard принимает старый ответ | stale answer принят после запуска новой операции |

Логи: `mutation-body-id.log`, `mutation-no-clobber.log`,
`mutation-late-cancel.log`, `mutation-stale-answer.log`. Compile errors и нулевые
тесты не засчитывались. Новая mutation-инфраструктура не добавлена.

Combined runtime workflow теперь исполняет четыре точных имени STL gate с
обязательными OCCT/PlaneGCS на Linux/macOS/Windows, отвергает `skipped:` и отсутствие
любого имени. Старые восемь edit gate и JSON gate сохранены. **Удалённый CI нового
незакоммиченного diff не выполнялся**; он запускается после ревью и публикации.

## Наблюдение GUI

**Выполнен на финальном свежем viewer через unified-computer-use.** Временный
`/private/tmp/ferrite-24e/FerriteCAD-STL.app` содержит пересобранный debug viewer;
для запуска через macOS добавлен только bundle rpath и ad-hoc подпись. Native
libraries скопированы из существующего локального bundle, исходные не менялись.
Первоначальный запуск зависал в dyld при доступе к libraries checkout, затем
окно некоторое время не давало кадр. После появления toolbar viewer перезапущен
с финальным бинарём и весь smoke выполнен на нём; старое окно не засчитывалось.

Наблюдалось в CUA:

1. Пустое окно: Export STL недоступен. Open приватного `gui-two-bodies.fcad`
   принял два Body с одинаковым именем Twin. В форме оба UUID различимы;
   автоматического выбора нет, Save неактивен до выбора. Defaults 0.01 mm / 0.5 rad.
2. Выбран второй `01a08125-f09e-7931-af0a-0bafe9f071f1`. После системного Save
   наблюдён `Exported STL`, UUID, 12 triangles, 684 bytes. Save на этом первом
   проходе был принят при вмешательстве пользователя, о котором сообщил CUA;
   итоговое окно и реальный файл перепроверены. `gui-two-bodies.stl` побайтово
   совпал с настоящим CLI `--solid` этого UUID, размеры 60×40×23 mm.
3. Выбран первый `01a08125-f09c-77d1-be0d-084ed1f02205`; в Save введено
   `gui-cancelled.stl`, нажат Cancel. Файла нет, форма/выбор сохранены.
4. С тем же первым UUID указан занятый `gui-two-bodies.stl`. После системного
   Replace появился собственный вопрос FerriteCAD. Нажат Cancel приложения:
   прежний STL второго тела побайтово цел.
5. Повторён выбор первого UUID и Save; подтверждены системный Replace и Replace
   приложения. Наблюдён опубликованный результат первого UUID; файл совпал с CLI
   первого тела, размеры 60×40×10 mm, и отличается от файла второго тела.

SHA-256 исходника до/после одинаков; `.fcad` sidecars не появились. Логи
`gui-before.json`, `gui-peer.log`, `gui-parity.log` связывают наблюдённые файлы с
настоящими CLI-запусками. Окно осталось с результатом экспорта для просмотра.
Отмена уже работающего долгого job/shutdown/stale answer проверены process/worker
gates, не ручным кликом на быстрой плите. Linux/Windows GUI не проверялись;
локальный GUI smoke не отменяет явные GPU skips полного sandbox-прогона.

## Независимое ревью

Ревью выполнено без запуска окна, GUI-автоматизации и GPU по новому указанию
пользователя. Наблюдение GUI выше относится к реализации и здесь не повторялось.
Безоконные тесты form/request/worker не открывают системные диалоги и не зависят
от состояния экрана.

Исправлен alias fixture: `mut` использовался только в `cfg(unix)` ветке, что
оставляло Windows `unused_mut` под обязательным `-D warnings`. Теперь набор путей
формируется без лишней изменяемости; hardlink остаётся на всех ОС, symlink на Unix.
Комментарий Cancel согласован с реальным контрактом поздней публикации.

Повторены jobs 41/41; CLI create/edit/STL/FBX/JSON 9 + 2 + 9 + 9 + 11; безоконные
exports 31/31 (включая четыре STL native gate) и edits 8/8. Native skips нет.
После исправления fixture четыре STL gate повторены. Fmt, workspace clippy
all targets/features `-D warnings`, export boundary, actionlint, shellcheck и
diff check прошли. Повторный экспорт обеих baseline-плит совпал по точным
stdout/stderr, STL и исходным SHA-256. Рецепт извлечён из этого Markdown и выполнен
до validate/cold rebuild/проверки 12 triangles, 684 bytes, bbox 60×40×10.
Первый запуск рецепта через дополнительный системный zsh потерял DYLD env и
остановился в loader; запуск в оболочке с native env прошёл. Это unbundled debug
CLI; поставку отдельно проверяет runtime workflow.

Отдельный пересобранный OCCT stub CLI/viewer подтвердил отсутствие native imports:
реальные отказы/создание/чтение выполнены, восемь native skips CLI suites явно
отделены от геометрии. Безоконный app STL refusal/process gate выполнен 1/1.
Удалённые результаты точного head и merge, итоговый diffstat — в PR этого среза.

## Файлы и diffstat реализации до ревью

Включая новые untracked файлы; staging/commit не выполнялись.

| Файл | + | − |
| --- | ---: | ---: |
| `.github/workflows/runtime-layout.yml` | 32 | 0 |
| `README.md` | 36 | 4 |
| `crates/ferritecad-app/src/exports.rs` | 101 | 19 |
| `crates/ferritecad-app/src/exports/stl.rs` | 189 | 0 |
| `crates/ferritecad-app/src/exports/stl/tests.rs` | 961 | 0 |
| `crates/ferritecad-app/src/main.rs` | 148 | 10 |
| `crates/ferritecad-cli/src/export.rs` | 28 | 212 |
| `crates/ferritecad-jobs/src/lib.rs` | 6 | 0 |
| `crates/ferritecad-jobs/src/stl.rs` | 260 | 0 |
| `crates/ferritecad-jobs/src/stl/tests.rs` | 275 | 0 |
| `crates/ferritecad-ui/src/lib.rs` | 3 | 0 |
| `crates/ferritecad-ui/src/panels.rs` | 35 | 0 |
| `crates/ferritecad-ui/src/stl.rs` | 87 | 0 |
| `docs/cli-capabilities.md` | 23 | 2 |
| `docs/implementation-plan.md` | 25 | 10 |
| `docs/stl-export-verification.md` | 245 | 0 |
| `tools/check-export-boundary.sh` | 9 | 1 |

Всего: 17 файлов, +2463 / −258 строк.
