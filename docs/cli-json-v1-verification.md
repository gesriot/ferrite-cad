# §24D: протокол проверок JSON v1

Дата: 2026-09-08. База и HEAD: `1126543ce2ecc90eba918918e4efc5f2e94468d4`,
`main = origin/main` после fetch. PR #17 проверен как MERGED с этим merge SHA;
head PR — `b035f3d64055f0d3949dc21be7bc608d4b378b16`. На точной базе проверены
23 успешных jobs пяти workflow, включая [combined runtime layout](https://github.com/gesriot/ferrite-cad/actions/runs/34202314972).
Это свидетельство базы, **не CI нового diff**.

Ветка `cli-json-inspect-edit` создана от чистой базы. AGENTS.md в рабочем дереве
и его родительских каталогах не найден. Чужой detached worktree a200 не изменён.
Первичная реализация была передана незакоммиченной: commit/push/PR/merge
исполнитель не делал. Независимое ревью описано ниже; следующий срез не начат.

## Первичная сдача: реально выполнено на новом diff

Хост: macOS arm64. Native build использовал существующие OCCT 8.0.1 из
`vendor/install` и PlaneGCS из `vendor/planegcs`. Native C++/FFI не менялись;
OCCT из исходников заново не собирался. CLI и viewer пересобраны из текущих
Rust-исходников (`cargo build -p ferritecad-cli -p ferritecad-app --all-features`).
Локальный native target — `/private/tmp/ferrite-24b-native-target`; имя каталога
от прежнего среза, но сборка и все перечисленные тесты выполнены заново.

| Проверка | Результат |
| --- | --- |
| `cargo fmt --all --check` и `git diff --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| document/jobs `--all-features` | 169 passed, 0 failed; 1 ignored: старый ручной `measure_extrude_catalog_read`, не CI assertion |
| CLI `create`, `edit_extrude`, `json_v1`, `--all-features` | 9 + 2 + 7 passed, 0 failed |
| Viewer `edits::tests::`, `--all-features`, обязательные OCCT/PlaneGCS | 8/8 именованных gates выполнены, 0 skips |
| `native_json_inspect_edit_contract` в native CLI suite | выполнен и passed, 0 skips |
| Свежий отдельный no-native target, CLI/viewer `--no-default-features` | build pass; OCCT stub, solver unavailable (exit 3) |
| No-native CLI `create`, `edit_extrude`, `json_v1` | harness 9 + 2 + 7 passed; внутри JSON suite ровно один явный native-gate skip |
| Python-рецепт из `cli-json-v1.md` | извлечён и выполнен без правок; create → JSON inspect/edit/inspect → validate/cold rebuild/STL/FBX, source bytes неизменны |
| Три временные поломки production-свойств | 3 runtime failures, 0 survivors; исходник восстановлен побайтово, финальный набор повторён после восстановления |

В native suites два теста, предназначенных только для отказа stub-сборки
(`no_kernel_build_refuses_edit_without_touching_source_or_destination` и
`no_native_inspection_succeeds_and_edit_reports_unavailable_without_publication`),
завершаются как неприменимые. Их поведение проверено именно отдельной no-native
сборкой. Это не доказательства отказа kernel в native-сборке. И наоборот,
единственный явный skip no-native JSON gate не считается геометрическим успехом.

No-native target `/private/tmp/ferrite-24d-no-native-target` был новым. Для CMake
исключён системный `/opt/homebrew` и package registries; задан отсутствующий
OpenCASCADE_DIR. Cache подтверждает `OpenCASCADE_DIR:PATH=OpenCASCADE_DIR-NOTFOUND`.
`FCAD_PLANEGCS_DIR` указывает на отсутствующий каталог, feature planegcs отключён;
свежий viewer `--solver-info` подтвердил unavailable, exit 3. Read-only process
проверки проходят без OCCT/PlaneGCS и сравнивают байты источника и список файлов,
включая отсутствие новых WAL/SHM/cache/scratch sidecars.

Новый workflow-step добавлен в существующий combined runtime layout matrix
Linux/macOS/Windows: `--release --features planegcs`, обязательный OCCT, конкретное
имя `native_json_inspect_edit_contract`, `--exact --nocapture`, запрет `skipped:`
и проверка строки выполнения именно этого теста. Восемь прежних именованных
edit gates сохранены. Actionlint прошёл. **Новый diff на Linux/Windows и удалённом
macOS runner не исполнялся**, поскольку публикация пользователем запрещена.
GPU и ручной GUI не запускались: UI, renderer и их пути не изменены.

## Что доказывает process gate

[Тесты](../crates/ferritecad-cli/tests/json_v1.rs) запускают настоящий `ferritecad`,
разбирают stdout стандартным serde_json parser и отдельно проверяют exit/stderr.
Пустой и смешанный каталоги сверены с общими фактами document: UUID, полная версия,
порядок, null-имя, расстояния, Formula/Parameter/Symmetric/ThroughAll и общий
copy_access (stored trigger и неизвестная required capability). Unicode, пробелы,
кавычки, слеши и перевод строки проверяются после декодирования JSON; Unix также
имеет кавычки/перевод строки в filename. Непредставимые пути проверены до IO/правки
(Unix bytes; код Windows gate использует unpaired UTF-16 surrogate).

Native gate использует два размера плиты: 80×50×12 и 91×53×17, display units in,
новая высота 27 mm. Получает ID/версию из JSON, проверяет новую высоту повторным
inspect, validate, cold rebuild с 3/3 refs, STL bounds/volume независимым чтением
треугольников и равенство STL/FBX с общей jobs-операцией. Сравнивает всё SQL
содержимое копий одного источника, исключая только время модификации; UUID не
нормализуются. Исходные байты сохраняются. Текстовый CLI даёт тот же документ.
Отдельный двухфичевый каталог меняет явно выбранную вторую Extrude и сохраняет
первую; имя не принимается за уникальный адрес автоматически.

Проверены структурированные отказы для NaN/inf/нулевого/отрицательного расстояния,
несуществующего UUID, устаревшего токена (включая реальное изменение source после
inspect), занятого назначения, самого source и hard-link alias. Formula/Parameter
и ограничения режима дают unsupported; общий trigger/FK guard тоже отказывает.
Clap-ошибки, help/version, неизвестная команда и аргумент/filename `--json`
проверяются отдельно, до обещания JSON. Missing/corrupt/old schema/future reader/
WAL/недоступный rowid проверяются без миграции и создания файлов.

Закрытый stdout воспроизводится реальным OS pipe: write end перенаправлен в CLI,
единственный reader уже завершился. После edit процесс возвращает 7, не 0/101;
копия существует, полностью совпадает с jobs-результатом и читается с высотой 27.
Нет panic, повторной операции, потери source или удаления публикации.

## Отрицательные контроли

| Временная поломка | Наблюдаемый runtime отказ |
| --- | --- |
| Удалить учёт `source.refusal` из effective `editable` | mixed catalog получил true вместо false под copy guard |
| JSON inspect открыть через `Document::open` | вместо read-only unsupported началась миграция v2 и пришла io-ошибка миграции |
| Сменить delivery exit 7 на 0 | реальный закрытый stdout после publish дал 0 вместо требуемого 7 |

Каждый мутант собран и исполнил ровно выбранный тест. Compile failure, zero-test
или skip не засчитывались. Это три временные правки одного файла с восстановлением,
без новой mutation-инфраструктуры. Первая проба нативного теста до финального
набора обнаружила ошибку **фикстуры**: SQL UUID сравнивался со строкой вместо BLOB,
поэтому stale-source UPDATE менял 0 строк. Исправлены BLOB-параметр и assert одной
изменённой строки; после этого весь native gate прошёл.

## Состав зависимостей и повторение

Cargo.lock добавляет только два runtime ребра CLI: serde и serde_json, без смены
версий пакетов. Штатные `generate-rust-sbom.sh --all`,
`generate-native-inventory.sh`, `generate-product-sbom.sh --all` и
`generate-rust-notices.sh` обновили три target-набора. В runtime inventory теперь
входят serde_json/itoa/zmij, а на macOS/Windows также memchr (на Linux он уже был).
Это механическая актуализация существующих lock-пакетов, без исследования лицензий.

Checks: Rust SBOM 25, native inventory 58, product SBOM 40, Rust notices 68;
notice ownership 11; licence headers 298 файлов; export boundary, solver ownership,
PlaneGCS pins — pass. Генераторы и проверки не меняли native pins или ownership.

Основные команды повторения после настройки существующих native библиотек:

```sh
export FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_PLANEGCS=1
cargo build -p ferritecad-cli -p ferritecad-app --all-features
cargo test -p ferritecad-document -p ferritecad-jobs --all-features -- --nocapture
cargo test -p ferritecad-cli --all-features --test create --test edit_extrude --test json_v1 -- --nocapture
cargo test -p ferritecad-app --bin ferritecad-viewer --all-features edits::tests:: -- --nocapture
cargo test -p ferritecad-cli --all-features --test json_v1 native_json_inspect_edit_contract -- --exact --nocapture
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

Публичный рецепт без чтения исходников — [cli-json-v1.md](cli-json-v1.md).
Локальные сырые логи этой проверки лежат в `/private/tmp/ferrite-24d/`: `base-ci.json`,
`native-build.log`, `document-jobs.log`, `cli-native.log`, `ui-edit-gates.log`,
`clippy.log`, `no-native-build.log`, `cli-no-native.log`, `no-native-solver.log`,
`json-native-final.log`, `json-no-native-final.log`,
`recipe.log`, `mutation-*.log`, `generate-*.log`, `check-*.log`. Эти временные логи
не являются переносимым CI-артефактом и не заменяют независимое ревью.

## Независимое ревью, 2026-09-08

Найден и исправлен дефект доставки ошибки. JSON emitter сначала вызывал общий
report через `eprintln!`; закрытый stderr вызывал panic до записи JSON в исправный
stdout. Новый тест настоящего CLI собрался и упал на **exit 101 вместо 2**, stdout
был пуст. Общая запись диагностической цепочки теперь fallible, emitter продолжает
доставку структурированного ответа при отказе stderr. Проверены inspect и
edit-extrude: закрыт только stderr → полноценная JSON-ошибка, exit 2; закрыты оба
канала → exit 7 без panic. Отказ не оставляет output или sidecars. Формат обычной
диагностики и категории ошибок сохранены.

При расширении проверки обнаружена ошибка входных данных теста: UUID вида
`00000000-0000-0000-0000-000000000001` недопустим для ObjectId (требуется UUIDv7).
Он маскировал отказы числа и content token в исходных clap-кейсах. Теперь используется
настоящий ObjectId::new(), и каждый кейс требует сообщение именно своего аргумента.
Иллюстративные UUID в JSON-контракте заменены допустимыми UUIDv7.

После production fix повторены fmt, workspace clippy со всеми targets/features
и -D warnings, document/jobs (169 passed, 1 ignored — прежний ручной benchmark),
CLI create/edit/json (9 + 2 + 8 passed) и все 8 native UI/CLI edit gates. Новый
native JSON process gate прошёл с настоящим OCCT; геометрических skips нет.
В отдельном no-native target пересобраны CLI/viewer: OCCT stub, solver-info exit 3;
тот же набор CLI suites проходит, с одним явным пропуском native JSON gate.
Новый закрытый-stderr тест выполняется в обоих режимах. GUI/GPU и native C++
не менялись. Повторно выполнен рецепт из текущей документации настоящим native CLI.
Offline checks прошли: Rust SBOM 25, native inventory 58, product SBOM 40,
Rust notices 68; actionlint также прошёл. Результаты удалённого CI точного
head/merge фиксируются в сопровождающем PR.
Сырые логи независимого ревью — `/tmp/ferrite-pr18-review/`.

## Файлы и diffstat первичной сдачи (до независимого ревью)

Включая новые файлы: 21 файл, +2935/−188 строк.

| Файл | Добавлено | Удалено |
| --- | ---: | ---: |
| `.github/workflows/runtime-layout.yml` | 22 | 0 |
| `Cargo.lock` | 2 | 0 |
| `README.md` | 8 | 1 |
| `crates/ferritecad-cli/Cargo.toml` | 2 | 0 |
| `crates/ferritecad-cli/src/json.rs` | 200 | 0 |
| `crates/ferritecad-cli/src/main.rs` | 43 | 10 |
| `crates/ferritecad-cli/tests/json_v1.rs` | 1002 | 0 |
| `docs/cli-capabilities.md` | 29 | 14 |
| `docs/cli-json-v1-verification.md` | 167 | 0 |
| `docs/cli-json-v1.md` | 241 | 0 |
| `docs/implementation-plan.md` | 27 | 11 |
| `licences/rust/NOTICE-aarch64-apple-darwin.md` | 102 | 65 |
| `licences/rust/NOTICE-x86_64-pc-windows-msvc.md` | 108 | 71 |
| `licences/rust/NOTICE-x86_64-unknown-linux-gnu.md` | 7 | 1 |
| `sbom/native/native-assets-inventory.json` | 3 | 3 |
| `sbom/product/ferritecad-product-aarch64-apple-darwin.cdx.json` | 178 | 3 |
| `sbom/product/ferritecad-product-x86_64-pc-windows-msvc.cdx.json` | 178 | 3 |
| `sbom/product/ferritecad-product-x86_64-unknown-linux-gnu.cdx.json` | 133 | 3 |
| `sbom/rust/rust-fragment-aarch64-apple-darwin.cdx.json` | 176 | 1 |
| `sbom/rust/rust-fragment-x86_64-pc-windows-msvc.cdx.json` | 176 | 1 |
| `sbom/rust/rust-fragment-x86_64-unknown-linux-gnu.cdx.json` | 131 | 1 |
