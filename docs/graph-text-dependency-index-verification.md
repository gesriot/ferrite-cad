# §24J-2 — индекс зависимостей для текстового dump-graph

2026-09-28, локально на Mac Apple Silicon. При передаче реализации на ревью
изменения были **незакоммичены и не staged**. До передачи commit, push, PR,
merge, tag, GitHub-комментарии и remote workflow не выполнялись.
Окон, viewer, GPU, Unity, geometry и Graphviz не запускались. OCCT, PlaneGCS и
Boost заново не собирались. `FCAD_ALLOW_LOADER_FAILURE_PROBES` не включался.

## База

После `git fetch origin` чистые `HEAD`, `main` и `origin/main` совпали:

`bcd0d37f0d147bfb13ebb382fe686067fafac190`

Дерево этого merge и дерево head PR #67 `0ace514d45049906a04ac5f6b7b192a4d97e8ad8`
одно: `9145dad2a1ff60c673500463bc437496a4609407`.

[PR #67](https://github.com/gesriot/ferrite-cad/pull/67) на старте был `MERGED`
(`mergedAt` 2026-09-28T10:34:57Z). [PR #66](https://github.com/gesriot/ferrite-cad/pull/66)
тоже `MERGED`, merge `4e0e5cad2490578fe22b935c80aa9d913f44d5c4`. Ветки этих PR не
восстанавливались. Worktree `a200` не открывался.

Check-runs опубликованного merge на момент старта: 23 completed, 23 success,
0 failed, 0 incomplete. Legacy combined status был `pending` с пустым списком
statuses. Это CI уже слитого §24J-1, не этого diff.

Локальная ветка `graph-text-dependency-index` создана от `origin/main`.
`git switch -c` поставил upstream на `origin/main`; он снят
`git branch --unset-upstream`. Upstream нет. Конечный HEAD тот же: коммита нет.

**CI незакоммиченного diff не запускался.**

## Что изменилось

`render::graph_text` по-прежнему читает `objects` и `dependencies`, затем вызывает
`Document::evaluation_order()`. Отказ `evaluation_order` и отказ разбора роли
происходят до первой текстовой строки.

Уже прочитанный вектор группируется так:

```rust
let mut needs_by_dependent: HashMap<_, Vec<&Dependency>> = HashMap::new();
for dep in &deps {
    needs_by_dependent
        .entry(dep.dependent)
        .or_default()
        .push(dep);
}
```

В карте лежат ссылки на строки вектора, не копии. Объект без записей печатает
только свою строку. `graph_dot` не переписывался.

Сложность относится только к заменённому сопоставлению. Раньше каждый из V
объектов просматривал все E строк: Θ(V·E) сравнений `dependent`. Теперь один
проход строит `HashMap` (ожидаемо O(E) вставок) и каждый объект делает один
ожидаемо O(1) поиск. Дополнительная память — E ссылок и по одной записи на
различный `dependent`. Хеш-таблица живёт только внутри `graph_text`.

Это не обещание, что весь `dump-graph` читает dependencies один раз.
`Document::evaluation_order()` по-прежнему сам загружает objects и dependencies.
Эти чтения не менялись. Поиск — ожидаемый, не худший случай `HashMap`.

## Побайтовое сравнение до и после

Один Bash после `source /private/tmp/ferrite-26g/env.sh`
(`CARGO_TARGET_DIR=/private/tmp/ferrite-24b-native-target`, `CARGO_BUILD_JOBS=1`).

До правки:

```sh
cargo build -p ferritecad-cli --release --offline
```

`Finished release` за 13.35s. Перекомпилирован только `ferritecad-cli`.
Бинарь скопирован в `/private/tmp/ferrite-grok47-trial-3/ferritecad-before`,
SHA-256 `c909c7d278eb3c998619d33b59ac9615a5e40b3161a7059075281dd38a482ef3`.

Лёгкий граф на копии `create --sample`: шесть RFC UUIDv7, узел без зависимостей,
ещё один готовый узел, ветвление, цепочка, две роли одной пары, имена с пробелом
и с `α` / `板`, один объект без имени. Пустой документ — отдельный `create`.
Цикл, отсутствующий endpoint и неизвестная роль — отдельные копии в том же
каталоге. Неконсистентная ссылка вставлена только там, с `foreign_keys=OFF`.

Финальный release CLI, SHA-256
`838422be158ac0da3ff86bd30b0344329c45502d977d879b68867e92ebb41cb8`,
на тех же файлах дал те же stdout, stderr и exit. SHA-256 и mtime файлов не
изменились.

| Файл | Формы | Exit | stdout |
| --- | --- | --- | --- |
| `graph-light.fcad` | default, text | 0 | 692 байта, одинаковые |
| `graph-light.fcad` | dot | 0 | 1135 байт |
| `empty.fcad` | default, text | 0 | 0 байт |
| `empty.fcad` | dot | 0 | 78 байт |
| `cycle.fcad` | default/text | 2 | пусто; stderr 147 байт, цикл |
| `missing.fcad` | default/text | 2 | пусто; stderr 149 байт, endpoint не в графе |
| `unknown-role.fcad` | default/text | 2 | пусто; stderr 67 байт, `not_a_role` |

Пустой text stdout у пустого документа — прежний результат. DOT цикла по-прежнему
exit 0: `graph_dot` не вызывает `evaluation_order`. Неизвестная роль в DOT
по-прежнему печатает заголовок и узлы, затем тот же `error [input]`; text до
этого отказа ничего не печатает. Эти байты сняты со старого бинаря и повторены
новым.

Порядок text на лёгком графе, сверху вниз: `…0001 plane α`, `…0002 datum.plane`,
`…0003 sketch line` → plane, `…0004 side wall` → profile, `…0005 boss 板` →
plane, затем predecessor и profile на одну пару, `…0006 tip body` → predecessor.

## Тесты и направленная поломка

В `dump_graph.rs` три process-теста: полный text и DOT лёгкого графа, пустой
документ, text-отказ цикла / отсутствующего endpoint / неизвестной роли без
stdout и без изменения файла. Ожидание записано строками, не счётчиком.

После восстановления исходника:

```sh
cargo test -p ferritecad-cli --release --offline --test dump_graph -- \
  --test-threads=1 --nocapture \
  text_keeps_evaluation_order_and_every_stored_need \
  empty_document_text_is_empty_and_dot_is_only_the_header \
  cycle_missing_endpoint_and_unknown_role_print_nothing
```

3 passed; 0 failed; 0 ignored; 5 filtered out; 1.45s.

До этого `.entry(dep.dependent)` был временно заменён на `.entry(dep.dependency)`.
Тот же тест порядка скомпилировался и упал на байтах stdout: строки `needs`
оказались у объекта-зависимости. `0 passed; 1 failed`, exit cargo 101.
Лог: `/private/tmp/ferrite-grok47-trial-3/mutation-fail.log`.
`render.rs` возвращён копией сохранённых байт, не через git restore/reset.
`cmp` совпал, SHA-256 `da729f2600e4a11ce48fb91c5cd9ff9656eed817002c42c4b394af8820a2242d`.

Повтор всего прежнего набора после явной пересборки peer CLI:

```sh
cargo build -p ferritecad-cli --release --offline
cargo test -p ferritecad-cli --release --offline \
  --test dump_graph --test validate --bin ferritecad -- \
  --test-threads=1 --nocapture
cargo fmt --all -- --check
cargo clippy --offline --release --workspace --all-targets --all-features -- -D warnings
```

`dump_graph`: 8 passed, 0 failed, 0 ignored, 0 filtered, 3.55s.
`validate`: 4 passed, 0 failed, 0 ignored, 0 filtered, 3.63s.
Unit tests бинаря `ferritecad`: 9 passed, 0 failed, 0 ignored, 0 filtered, 0.00s.
Fmt exit 0. Clippy exit 0, `Finished release` за 4.61s, `-D warnings`.
Пропусков нет.

## Замер

Одинаковые файлы `scale-{100,1000,4000,10000}.fcad`: цепочка, две разные роли
между соседями, V/E = 100/198, 1000/1998, 4000/7998, 10000/19998.
RFC UUIDv7. Release CLI, stdout в `/dev/null`, exit 0, 1 прогрев + 5 запусков,
медиана `time.monotonic` всего процесса.
Числа ниже — финальный бинарь против бинаря до правки. Это не время одного
поиска. На малых V загрузчик и SQL закрывают разницу. Утверждения по времени в
тесты не добавлялись.

| V | E | Медиана до, ms | Медиана после, ms |
| --- | --- | --- | --- |
| 100 | 198 | 54.00 | 53.04 |
| 1000 | 1998 | 59.06 | 57.84 |
| 4000 | 7998 | 83.36 | 73.71 |
| 10000 | 19998 | 163.55 | 103.91 |

Выборки до, ms: 100 — 54.36, 53.75, 54.05, 53.80, 54.00; 1000 — 59.69, 60.13,
58.55, 59.06, 58.59; 4000 — 83.36, 83.41, 82.70, 83.29, 83.57; 10000 — 163.23,
164.09, 163.55, 163.06, 165.79.

Выборки после, ms: 100 — 53.04, 52.65, 53.28, 53.81, 52.82; 1000 — 57.57, 56.73,
58.83, 57.84, 58.28; 4000 — 73.52, 73.80, 73.96, 73.71, 72.34; 10000 — 103.24,
104.04, 103.63, 105.26, 103.91.

Ориентир Codex (55.5 / 61.3 / 87.8 / 173.8 ms) снят другим прогоном и сюда не
подставлен. JSON: `timing-before.json`, `timing-after-final.json` в каталоге
логов.

## Память, диск, процессы

16 GiB RAM. Во время передачи `memory_pressure` показывал 0 swapins и 0 swapouts.
Свободных страниц к концу было мало, purgeable оставались. Диск: около 141 GiB
свободно и до, и после (`/private/tmp` на том же томе Data). Свои `cargo` /
`rustc` к концу не остались. Чужие процессы не останавливались. Чужой target и
кэши не чистились. Логи и фикстуры: `/private/tmp/ferrite-grok47-trial-3/`.

## Ограничения

Индекс не является новым обходом DAG и не кэшируется между запусками. Windows и
Linux локально не исполнялись. Удалённый CI этого diff не запускался.

## Независимое ревью, 2026-09-28

Production-код принят без исправлений. Индекс ищется по `dependent`,
но не перечисляется для вывода; строки каждой группы сохраняют порядок
исходного вектора, включая несколько ролей одной пары. Проверка порядка графа
остаётся до первой строки text. Прежние read-only process tests не ослаблены.

Ревью усилило фикстуру порядка: первоначально порядок объектов, UUID и
топологический порядок совпадали. Подмена обхода на `objects.iter()` с
сохранённым вызовом валидации скомпилировалась и прошла первоначальный тест.
Теперь объектный порядок `5,2,3,4,1,6`, а ожидаемый топологический —
`2,5,3,1,4,6`: ни простой проход объектов, ни сортировка UUID ему не равны.
Тот же мутант с новой фикстурой падает на исполненном сравнении stdout
(1 failed, cargo exit 101). Production-файл восстановлен побайтово;
положительный suite повторён. Дополнительного тестового framework нет.

Повторены свежая сборка CLI, все 8 dump_graph + 4 validate + 9 unit tests,
fmt и workspace release clippy со всеми targets/features и `-D warnings`.
Итого 21 исполненный тест, без skips и ignored.

Сохранённый CLI до правки и свежесобранный CLI сравнивались независимо на пяти
фикстурах (обычный граф, пустой документ, цикл, отсутствующий endpoint,
неизвестная роль), в трёх формах каждой: default/text/DOT. Во всех 15 случаях
stdout, stderr и exit совпали побайтово; SHA-256 и mtime файлов сохранились.

Отдельный чередующийся замер (один прогрев, пять запусков каждой версии,
stdout в `/dev/null`, медиана всего процесса) дал:

| V | E | До, ms | После, ms |
| --- | --- | --- | --- |
| 100 | 198 | 59.18 | 59.34 |
| 1000 | 1998 | 66.12 | 65.33 |
| 4000 | 7998 | 91.31 | 82.55 |
| 10000 | 19998 | 178.56 | 118.93 |

Это отдельный прогон, не замена исходным измерениям выше. На малых документах
разница несущественна; CI не содержит timing assertion. Артефакты ревью:
`/private/tmp/ferrite-grok47-review-3/`. Удалённый CI опубликованного commit
учитывается отдельно; локальные результаты не объявляются проверкой трёх ОС.
