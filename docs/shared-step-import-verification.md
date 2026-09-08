# §24H — локальная проверка общего STEP import

Начальные разделы сохраняют протокол передачи до ревью. Дополнительный раздел
ниже фиксирует независимо повторённые локальные проверки.

Дата: 2026-09-08. Реализация Codex, полностью без окон. Изменения оставлены
**незакоммиченными**; следующий шаг — независимое ревью. Commit/push/PR/merge
не выполнялись. GUI/Unity/GPU tests не запускались и не требуются этим срезом.

## Точная база и scope

- Ветка: `shared-step-import`.
- `HEAD = main = origin/main = 577aa6eb9d7ee8abbfe4d787ca421ba437ceddac`.
- Свежий fetch перед созданием ветки подтвердил актуальность main и чистое дерево.
- [PR #22](https://github.com/gesriot/ferrite-cad/pull/22) merged; implementation
  head `273df100e61093e08669a05399917928a2eb1123` и merge имеют одно дерево
  `bcd93f1b43f7bb744c75bdef06f6776f056b65dc`.
- CI **базы** повторно прочитан: 23/23 checks, пять workflow success;
  [runtime точного merge](https://github.com/gesriot/ferrite-cad/actions/runs/34260654662)
  содержит 48 passed named gates без skips (16 на каждой ОС).
- Чужой detached worktree `a200` остаётся на
  `5b20719145222befc0bd25647f47ba3d67fee66d`, не изменялся. Исходные fixtures
  не изменялись; destructive cases использовали частные копии.
- **CI текущего незакоммиченного diff не выполнен.** Новый трёхплатформенный
  runtime step исполнится после ревью и публикации.

Реализация ограничена переносом STEP import в jobs и текстовым адаптером.
[Request/outcome/ownership/cancellation и публичный рецепт](shared-step-import.md).
`ImportStepRequest` + kernel factory/import callback + `OperationContext` →
`Published` с owned persisted facts либо `Rejected` только с facts чтения.
Ошибки отдельно через `Result`; handles не возвращаются. CLI сохраняет
stdout/stderr, порядок строк, --name/default basename, --force и коды 0/4/5/2.
Ни JSON import, ни UI import, ни CLI cancellation flags не добавлены.

## Что проверено на новом diff

Native окружение: `/private/tmp/ferrite-24h/native-env.sh`, target
`/private/tmp/ferrite-24b-native-target`. CMake ссылается на
`vendor/install/lib/cmake/opencascade`, OCCT 8.0.1; `otool -L` свежего CLI
подтверждает `libTK*.8.0.dylib` и `libplanegcs.dylib`. Использованы vendor OCCT
и `vendor/planegcs`. OCCT source, C++/headers/FFI не менялись и не пересобирались
из исходников. Peer CLI и app пересобраны до headless app tests.

| Проверка | Реальный результат |
| --- | --- |
| `cargo fmt --all --check` | pass |
| `cargo clippy --offline --workspace --all-targets --all-features -- -D warnings` | pass |
| document tests | 132 passed; один существующий timing benchmark ignored |
| jobs tests | 46 passed, включая 5 новых детерминированных import tests |
| export tests | 60 passed |
| CLI unit/create/edit/text FBX/text STL/import/JSON/shared-import suites | 64 passed (7 + 9 + 2 + 9 + 9 + 13 + 13 + 2), без native skips |
| `native_shared_step_import_owns_geometry_and_matches_cli` | exact test pass; parity=4, rejected=3, native_cleanup=4 |
| `edits::tests::` headless app | все 8 прежних named gates passed |
| `exports::stl::tests::native_` headless app | все 4 прежних named gates passed |
| Существующая FBX кампания, `source tools/check-fbx-complex.sh --offline --all-features` | все 3 точных process tests passed; complete/partial/delivery routes исполнены |
| Pinned ufbx 0.23.0 strict | 256 + 6 + 6 + 256 checks, 0 failures |
| `tools/check-export-boundary.sh` | pass |
| `git diff --check` | pass |

Native suites запускались с `FERRITECAD_REQUIRE_OCCT=1`,
`FERRITECAD_REQUIRE_PLANEGCS=1`, `FERRITECAD_REQUIRE_GPU` unset.
Проверены точные строки результатов **17/17** named native gates на macOS:
прежние 8 edit + 1 JSON + 3 FBX + 4 STL и новый import. В workflow добавлен
один обязательный exact native import gate на Linux/macOS/Windows с отказом при
`skipped:` или отсутствии passed имени; старые gates/ufbx сохранены. Это будущие
51 required named executions CI, а не утверждение, что они уже выполнены.

## Владение, сохранность и гонки

Прямой job и настоящий peer CLI читают одинаковые private source bytes:
nested assembly, Unicode names, inch units и damaged broken reference. После reopen
сравнены все persisted definitions/placements, parent/local transforms/colours,
source hash/length/basename/kernel identity и ordered diagnostics. Document/object/
source UUID независимы; occurrence UUID образуют явную биекцию по placements, все
остальные facts равны. Job result содержит те же occurrence IDs, что `.fcad`:
повторная проекция ради отчёта сломала бы проверку. Source bytes и mtime сохраняются.

Нативный observer считает принятые/released handles и live shapes **до** деструктора
OCCT: ноль живых shapes, каждый полученный handle освобождён ровно один раз в
исходном kernel/потоке. Это проверено при успехе, настоящем отказе SQLite trigger,
ошибке publication, отмене после native import и поздней отмене. Reader rejection
на truncated/missing-terminator/unnameable-part fixtures даёт Rejected/CLI 5,
не передаёт handles и не публикует файл.

Детерминированные jobs tests отдельно проверяют:

- Один read: источник меняется из progress callback после 0.1; importer и
  опубликованный `.fcad` всё равно получают исходные bytes/hash. Изменённый файл
  job не перезаписывает.
- Отмена на 0.0/0.1/0.6/0.7/0.8/0.9 при Keep и Replace сохраняет исходник,
  старый output, состав каталога; нет scratch/sidecars. На 0.9 scratch можно
  переименовать и открыть read-only — SQLite уже закрыт. Это также проверяется
  на Windows в CI, где открытый SQLite handle мешал бы rename.
- Отмена на 1.0 при Keep/Replace оставляет Published и открываемый целый документ.
- Ошибка сохраняемой scene, невозможность создать scratch, ошибка публикации
  в чужой каталог: handles освобождены; чужое содержимое не удаляется.
- Destination, появившийся перед publish: Keep отказывает с сохранением байтов,
  Replace публикует целый документ. Late hardlink на source и подмена самого
  source на alias destination отказываются перед publish даже с Replace.
- Два definitions с одним handle освобождают его один раз; repeated placements
  не добавляют владения. Stored strings с Unicode/quotes/LF сохраняются.

Старые 13 import process tests сохранены. Дополнительные процессы проверяют
missing source до ядра, busy output, source/hardlink/Unix symlink с/без force,
неподдерживаемый `--json`, help и буквальное имя `--json` после `--`.
Source/output bytes, directory inventory и отсутствие scratch проверяются.

Две направленные временные поломки компилировались и **исполнили по одному тесту**:

1. Убрано `kernel.release`: storage/publication cleanup test упал на 0 releases
   вместо 2, exit 101.
2. Убран пред-публикационный source/alias recheck: race test получил Published
   вместо обязательного отказа, exit 101.

Обе поломки восстановлены; source побайтово совпал с сохранённой положительной
копией. Повторены все 5 job tests, native import gate и сборка peer CLI/app.

## STEP → сохранённый документ → FBX

После удаления только private STEP-копий новый native parity gate экспортирует
чистые job/CLI документы через настоящий JSON FBX. Существующая §24G кампания
проверяет production complete native/STEP и настоящую complex partial assembly
из embedded bytes `.fcad`, также после удаления private STEP. Большая сборка
не импортируется отдельно для каждой ошибки/pipe case.

В partial сохранились 46 definitions, 140 nodes, 34 geometries, 986873 triangles;
одна omission, exit 6, ok:true/complete:false. Проверены сохранённая геометрия,
пустые placements/properties отказавшей definition и source-qualified report.
Полный маршрут проверил 8 delivery cases, partial — 4, включая force replacement
и закрытые stdout/stderr; сохранились FBX и exit 7. Strict ufbx завершил четыре
независимых чтения без failures. FBX writer/wire/identity не менялись.

Свежий **базовый** CLI был сохранён до правок. На семи import cases (clean,
Unicode explicit name, default assembly name, diagnostics, два Rejected и force)
старый и новый отчёты совпали после замены только нового object UUID. Четыре
FBX одного сохранённого документа совпали побайтово между базовым и новым CLI.
Help и missing-source stdout/stderr/exit совпали точно при одинаковом argv[0].
Имена executable в clap help иначе закономерно различаются.

## Отдельная stub-сборка

`/private/tmp/ferrite-24h/stub-env.sh`, новый target
`/private/tmp/ferrite-24h-stub-target`, CMake wrapper исключает `/opt/homebrew`.
CMake: `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`,
`CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew`. `otool -L` показывает только libiconv
и libSystem, без OCCT/PlaneGCS. Это проверка текущего бинаря, не старого target.

CLI suites вместе с FBX campaign harness: **67 harness-passed**, среди них
**30 явных native skips**, которые не засчитываются геометрией. Реально проверены
новые I/O/no-clobber/alias/unsupported process refusals, сохранность existing
output даже с --force и отсутствие нового файла/scratch. Missing STEP отказывает
до попытки открыть kernel. Прежние JSON structured refusals/delivery тоже прошли.
Обновлённый shared-import suite повторён отдельно; 5 deterministic jobs tests
также прошли в stub target без skips.

Non-UTF-8 Unix filename storage test предусмотрен для Unix кроме macOS:
локальная APFS отказала в создании имени с `0xff`, поэтому на macOS этот case
не компилируется и не объявляется выполненным. На Linux он ещё требует CI.
Unicode filenames и Unicode/quotes/LF **stored names** проверены нативно на Mac;
Windows filenames в новых тестах не содержат запрещённых кавычек/LF.

## Выполненный рецепт и передача

Shell block заново извлечён из [публичного протокола](shared-step-import.md) и
выполнен как написан с `FCAD_CLI` на свежий native binary. Clean import exit 0,
private STEP удалён, inspect JSON дал imported-only `bodies:[]`, FBX exit 0.
Независимый ASCII subset reader проверил counts/bytes, 12 треугольников и размеры
60 × 40 × 10 mm с прежним FBX преобразованием в метры `(x,z,-y)`. Hash исходной
fixture и `.fcad` не изменился; остались только `.fcad`/`.fbx`.

Локальные логи, base CI evidence, CMake/imports, recipe и mutation logs находятся
в `/private/tmp/ferrite-24h`. Эти временные артефакты не входят в git diff.

Runtime dependencies и их edges не менялись: Cargo.lock, inventories/notices,
Rust/product SBOM не требуют обновления. Document schema, Temporary, C++/FFI,
renderer и пять JSON команд не менялись. Отмена внутри блокирующего OCCT import
не появилась; UI/JSON import — будущие отдельные срезы. Linux/Windows нового diff
локально не проверены. Следующий шаг — независимое ревью незакоммиченных изменений.

## Независимое локальное ревью, 2026-09-08

Повторно проверены RAII ownership сразу после import, закрытие SQLite перед
cleanup/publish, отказ без publication identity, единственная сохраняемая V3
проекция и текстовый адаптер. Исправлений production-кода не потребовалось.
В плане уточнён смысл alias recheck и записан следующий срез §24I JSON import.

Из текущего checkout заново собраны CLI/app; fmt и workspace clippy со всеми
targets/features прошли. Native: document 132, jobs 46, export 60, CLI 64;
8 edit + 4 STL headless workers и 3 FBX process gates прошли. Вместе с JSON и
shared import это 17/17 named native gates, без skips. Один старый timing test
ignored. Strict ufbx: 256 + 6 + 6 + 256 checks, 0 failures.

Отдельно повторены 5 deterministic jobs tests и 64 CLI harness passes на stub;
28 явных geometry skips в этих CLI suites не засчитаны native-проверками.
Stub CMake показывает OpenCASCADE NOTFOUND, CLI imports не содержат OCCT/PlaneGCS.
Большая stub FBX кампания при ревью повторно не запускалась.

С сохранённым до изменений базовым CLI сверены семь текстовых исходов (только
новые object UUID нормализованы), четыре FBX одного сохранённого документа,
help и missing-source. Публичный рецепт заново извлечён из Markdown и исполнен
до независимого разбора FBX после удаления private STEP. Export/solver boundaries,
11 notice ownership checks, licence headers, actionlint и diff --check прошли.
Логи ревью: `/tmp/ferrite-pr23-review/`. Новые temporary mutations при ревью не
повторялись; положительные tests, заявленные их свидетелями, исполнены заново.
Окна, GUI/Unity/GPU tests и OCCT source rebuild не запускались.

Это локальные результаты. Checks точного опубликованного head и merge main
фиксируются в PR и отдельном итоговом отчёте после публикации.

## Файлы и diffstat передачи, включая untracked (до ревью)

| Файл | + | − | Состояние |
| --- | ---: | ---: | --- |
| `.github/workflows/runtime-layout.yml` | 23 | 0 | modified |
| `README.md` | 13 | 3 | modified |
| `crates/ferritecad-cli/src/import.rs` | 64 | 157 | modified |
| `crates/ferritecad-jobs/src/lib.rs` | 5 | 0 | modified |
| `docs/cli-capabilities.md` | 8 | 3 | modified |
| `docs/implementation-plan.md` | 29 | 12 | modified |
| `crates/ferritecad-cli/tests/shared_step_import.rs` | 576 | 0 | untracked |
| `crates/ferritecad-jobs/src/import.rs` | 199 | 0 | untracked |
| `crates/ferritecad-jobs/src/import/tests.rs` | 511 | 0 | untracked |
| `docs/shared-step-import-verification.md` | 193 | 0 | untracked |
| `docs/shared-step-import.md` | 187 | 0 | untracked |

Итого: **11 файлов, +1808 / −175**; пять файлов untracked включены в подсчёт.
