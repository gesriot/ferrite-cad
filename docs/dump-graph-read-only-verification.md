# §24J-1 — read-only dump-graph: проверка и передача

2026-09-27, локально на Mac Apple Silicon. Изменения **незакоммичены и не staged**.
Commit, push, PR, merge, tag и remote workflow не выполнялись. Окон, viewer, GPU,
Unity и geometry не запускались. Graphviz не вызывался.

## База

После `git fetch origin` чистые `main` и `origin/main` остались на
`5ea113bbe5df382d738bc747499318b53f03dc43`. Локальная ветка
`dump-graph-read-only` создана от этого коммита и не имеет upstream.
Конечный HEAD тот же: коммита нет.

[PR #66](https://github.com/gesriot/ferrite-cad/pull/66) на момент проверки
открыт и не смержен (`mergeCommit` пуст, head `06848b36b8f36901f5f2f72266172f73323174e4`).
`gh pr checks 66` показал 15 checks, все `pass`. Это CI среза §28D-1, не CI
этого diff. Ветка PR не использовалась как база.

**CI незакоммиченного diff не запускался.**

## Контракт

`dump-graph` открывает документ существующим `Document::open_read_only` и печатает
прежние `render::graph_text` / `graph_dot`. Второго открытия, мигратора, kernel,
content version и нового jobs-слоя нет. `clear-cache` по-прежнему вызывает
`Document::open`.

| Случай | Результат |
| --- | --- |
| Текущая схема | Прежний text/DOT stdout, exit 0. Default text равен `--format text`. Файл и соседние sidecars не меняются. |
| Schema v2, WAL header, stale WAL/SHM при DELETE, minimum reader | Прежний отказ общего reader, exit 2, пустой stdout, без миграции. |
| Missing/broken путь | Exit 2, пустой stdout, файл/sidecar/каталог не создаются. |
| Нет права записи, каталог не принимает новый файл, файл читается | Dump успешен. |
| Файл нельзя прочитать | Прежний operational `error [io]`, exit 2. |
| `--format json`, usage | Прежний clap, exit 2, пустой stdout. Help называет read-only и только `text`/`dot`. |

## До правки и failing-first

Peer CLI собран из неизменённого `Document::open`:

```sh
source /private/tmp/ferrite-26g/env.sh
cargo build -p ferritecad-cli --release
```

`Finished release` за 19.54s. Бинарь
`/private/tmp/ferrite-24b-native-target/release/ferritecad`, SHA-256
`2c5a385ac0c015fa14f78669d4bbb0a2cc84344b6b8d809878599de26a682f10`
(копия: `/private/tmp/ferrite-grok47-trial-2/ferritecad-before`).
Сборка перекомпилировала Rust-крейты CLI; OCCT, PlaneGCS и Boost заново не собирались.

Один `create --sample` дал документ schema v3, 86016 байт, SHA-256
`8acbb8f754f58de6cb41385209f44349910e52bc3579bd7b36a6fd1380f1f11f`
(`/private/tmp/ferrite-grok47-trial-2/plate-pristine.fcad`). На копиях этих байт
default, `--format text` и `--format dot` дали exit 0, пустой stderr и те же
bytes/mtime/`user_version` 3. Default stdout совпал с text (364 байта). DOT —
661 байт. UUID в этом файле дальше не менялись.

До перехода на `open_read_only` process-тест schema v2 реально исполнился и упал
на неожиданном успехе и записи, не на компиляции:

```sh
cargo test -p ferritecad-cli --release --test dump_graph -- --list
cargo test -p ferritecad-cli --release --test dump_graph legacy_schema_wal_and_reader_floor_are_refused -- --exact --test-threads=1 --nocapture
```

`--list`: 5 тестов. Исполнен 1, 4 отфильтрованы. Паника:

```text
dump-graph format=None exit=Some(0) stdout_len=364 storage_changed=true user_version=3 stderr=
```

`test result: FAILED. 0 passed; 1 failed`. Exit cargo 101.
Лог: `/private/tmp/ferrite-grok47-trial-2/failing-first.log`.

## После правки

Тот же pristine-файл, уже новый release CLI:

default, text и dot — exit 0, stdout и stderr побайтово совпали с захватом до
правки, SHA-256 и mtime копии не изменились, `user_version` остался 3.
Логи `before-*.stdout` / `after-*.stdout` в том же каталоге.

```sh
cargo test -p ferritecad-cli --release --test dump_graph -- --test-threads=1 --nocapture
```

5 passed; 0 failed; 0 ignored; 2.40s. Покрытие: текущий sample с sentinel
`-cache`/`-other`; schema v2 (таблицы import сняты, `user_version=2`) для
default/text/dot; WAL header; stale `-wal`/`-shm` при восстановленном DELETE;
`minimum_reader_version=999`; missing file; отсутствующий каталог; broken file;
clap `--format json`, `--help`, usage; file mode без записи и каталог `0o500`,
где создание файла получило отказ; mode `0o000` после фактического отказа чтения.
Сравнение — bytes и mtime файлов, не mtime каталога. Пропуск тестов не
использовался. Сессия uid 501; запись в read-only файл получила `EACCES`.

```sh
cargo fmt --all -- --check
cargo test -p ferritecad-cli --release --test validate -- --list
cargo test -p ferritecad-cli --release --test validate -- --test-threads=1
cargo test -p ferritecad-cli --release --bin ferritecad -- --list
cargo test -p ferritecad-cli --release --bin ferritecad -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`fmt` exit 0. `validate`: 4 listed, 4 passed, 0 ignored, 3.74s.
Unit tests бинаря `ferritecad`: 9 listed, 9 passed, 0 ignored, 0.00s.
Clippy: exit 0, `Finished dev profile` за 3.53s, warnings-as-errors.
Все команды шли в одном Bash после `source /private/tmp/ferrite-26g/env.sh`
(`CARGO_TARGET_DIR=/private/tmp/ferrite-24b-native-target`, `CARGO_BUILD_JOBS=1`).
Новый target directory не создавался.

## Ограничения

Маршрут dump-graph — `open_read_only` и прежняя печать графа; geometry, solver и
OCCT API этот путь не вызывает. Готовый CLI по-прежнему слинкован с уже собранным
OCCT: это свойство бинаря, не вызов ядра командой. Hot journal и symlink target
WAL в этот suite не добавлялись; общий reader не менялся. `clear-cache` может
мигрировать. Windows и Linux локально не исполнялись. Удалённый CI этого diff
не запускался.

## Независимое ревью, 2026-09-28

Предыдущие разделы описывают исходную передачу. Ревью не нашло функционального
дефекта; production-код и tests приняты без исправлений. Уточнены только ссылки
и статус протокола в актуальной карте CLI/плане, добавлена эта запись.

Свежий release CLI повторно исполнил 5 `dump_graph`, 4 `validate` и 9 unit tests:
**18 passed, 0 failed, 0 ignored, без skips**. Проверены фактические отказы записи
и чтения под обычным пользователем. Fmt и workspace release clippy со всеми
targets/features и `-D warnings` прошли. Использован прежний native target;
библиотеки, новый stub target и оконное приложение не собирались/не запускались.

Независимый process comparison старого CLI и нового на одном свежем sample:
default/text — 364 байта, DOT — 661 байт; stdout/stderr/exit совпали побайтово,
bytes/mtime документа не изменились. Отдельные настоящие v2-копии с новым CLI
дали text/DOT exit 2, пустой stdout и `needs migration`; bytes/mtime сохранены.
Логи и сравнение: `/private/tmp/ferrite-grok47-review-2/`.

```sh
source /private/tmp/ferrite-26g/env.sh
cargo test --offline --release -p ferritecad-cli --test dump_graph --test validate --bin ferritecad -- --test-threads=1 --nocapture
cargo fmt --all -- --check
cargo clippy --offline --release --workspace --all-targets --all-features -- -D warnings
```

Перед публикацией этого среза PR #66 подтверждён 15/15 успешными checks на точном
head `06848b36` и слит как `4e0e5cad`. Его tree совпадает с проверенным head.
Этот результат относится к предыдущему UI-срезу. CI настоящего среза запускается
на опубликованной ветке и оценивается отдельно, локальные проверки его не заменяют.
