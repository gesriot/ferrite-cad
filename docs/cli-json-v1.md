# CLI JSON v1: inspect, edit-extrude и create

§24D задаёт opt-in контракт inspect и edit-extrude. §24D-1 добавляет третью
существующую команду, `create`, через тот же конверт v1:

```text
ferritecad inspect <source.fcad> --json
ferritecad edit-extrude <source.fcad> --json --feature <uuid> --distance-mm <n> -o <new.fcad> [--expect-version <token>]
ferritecad create <path> --json [--sample] [--size W D H] [--length-unit <u>] [--angle-unit <u>]
```

Обычный текстовый режим сохраняется. Общие document/jobs операции остаются
владельцами чтения, создания, допустимости правки и публикации. GUI не изменён.
Нет JSON остальных команд, stdin/batch, RPC, DSL, сервера, новых геометрических
операций, правки формул/Parameter или in-place Save. Возможности остальных
команд описаны в [карте CLI](cli-capabilities.md).

## Конверт и совместимость

При доставленном отчёте stdout содержит ровно один UTF-8 JSON-объект и один LF
после него. Переводы строк внутри строк экранируются сериализатором. Обычных
stdout-логов рядом нет. Диагностика для человека идёт на stderr.

| Поле | Тип и правило |
| --- | --- |
| `schema_version` | integer, сейчас ровно `1` |
| `operation` | string: `inspect`, `edit-extrude` или `create` |
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

## Результат inspect

Это срез чтения для выбора Extrude, а не сериализация всей БД, геометрии,
результата validate или полного текстового inspect.

| Поле `result` | Тип и смысл |
| --- | --- |
| `document_id` | UUID модели |
| `content_version` | полный токен содержимого закреплённого снимка |
| `display_units` | объект `{length: string, angle: string}`; символы длины `mm`, `cm`, `m`, `in`, `ft`, угла `rad`, `deg` |
| `distance_unit` | строка `mm`; все `distance_mm` всегда миллиметры, независимо от display units |
| `edit_extrude` | объект общей доступности ниже |
| `features` | массив существующих нативных Extrude; другие типы объектов сюда не входят |

`features` сохраняет порядок общего каталога: порядок объектов по parent UUID,
ordinal, UUID (корневые parent `null` идут первыми), отфильтрованный до Extrude.
Это порядок представления, не адресация фич. Выбирайте UUID явно; имена могут
повторяться или отсутствовать. При неоднозначности рецепт ниже отказывает.

| Поле элемента `features` | Тип и смысл |
| --- | --- |
| `feature_id` | UUID конкретной Extrude для `--feature` |
| `name` | string или `null`, точное хранимое имя |
| `distance_mm` | number или `null`; значение Blind/Symmetric из общего каталога, в том числе сохранённое значение выражения; у ThroughAll `null` |
| `editable` | boolean, учитывает и локальную допустимость, и общий `copy_access` |
| `refusal` | string или `null`, только причина отказа этой фичи |

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

Чтение использует один `Document::open_read_only` и `ExtrudeEditSource` на том же
закреплённом снимке, с одним вычислением content hash. Не открывает файл повторно
ради каталога, не мигрирует, не пишет sidecars, не вызывает kernel/rebuild. Старую
SQL-схему, WAL-состояние и несовместимость read-only open команда отказывает.
Успешный inspect **не обещает**, что геометрия перестроится или что OCCT доступен.

Пример успешного чтения (UUID и токен иллюстративные):

```json
{"schema_version":1,"operation":"inspect","ok":true,"result":{"document_id":"019ecc22-0841-7c0e-ad61-374b404f7219","content_version":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","display_units":{"length":"in","angle":"deg"},"distance_unit":"mm","edit_extrude":{"available":true,"refusal":null,"document_refusal":null},"features":[{"feature_id":"019ecc22-43cb-7c1d-8da9-f386566b9f19","name":"Extrude1","distance_mm":12.0,"editable":true,"refusal":null}]}}
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

## Ошибки, clap и доставка

`error` всегда содержит `kind: string`, `message: string`, `causes: string[]`.
Категория берётся из `ErrorKind::as_str()`: `input`, `constraint`, `topology`,
`kernel`, `rendering`, `io`, `cancellation`, `unsupported`. Клиент обрабатывает
неизвестную категорию как общий отказ. `message`, цепочка причин (от ближайшей к
глубинной) и строки refusal предназначены человеку и могут меняться; по фразам
не классифицируйте ошибки. Пустая цепочка — `[]`. Диагностика причины сохраняется
и в JSON, и, по возможности, на stderr. Ошибка записи диагностики не мешает
доставить JSON через исправный stdout: сохраняется exit 2. Если не удалось
доставить сам JSON, действует exit 7, даже когда stderr тоже закрыт. Например, отсутствующий файл даёт `io` с причиной ОС;
недоступное ядро даёт `unsupported`, не геометрический успех.

```json
{"schema_version":1,"operation":"edit-extrude","ok":false,"error":{"kind":"input","message":"invalid input: extrude distance must be positive","causes":[]}}
```

| Ситуация | stdout / stderr | Exit |
| --- | --- | --- |
| Успех корректно разобранной JSON-команды | JSON result / возможный note о расширении при create | 0 |
| Отказ выполнения корректно разобранной JSON-команды | JSON error / диагностика | 2 |
| Ошибка аргументов до успешного clap parse | пусто / обычная текстовая usage-ошибка | 2 |
| `inspect --json --help`, `edit-extrude --json --help`, `create --json --help` | текст help / пусто | 0 |
| Корневой `--version` | текст версии / пусто | 0 |
| Ошибка сериализации или доставки отчёта, включая BrokenPipe | JSON может отсутствовать или быть неполным / диагностика по возможности | 7 |

JSON не обещан до успешного разбора аргументов. Пропущенный путь, синтаксически
неверный UUID, нечисловое расстояние и неверный формат токена — ошибки clap.
У `edit-extrude --distance-mm` значения `NaN`, `inf`, ноль и отрицательное число
разбираются как число, но отказываются при выполнении. У `create --size` действуют
правила создания: нечисловые и бесконечные значения недопустимы; высота должна
быть положительной, а нулевая ширина/глубина допускаются сохранённым графом.
Успешное создание само по себе не обещает успешный rebuild геометрии.
При нескольких дефектах входа порядок отказов задаёт общий
маршрут; например, недоступное ядро может отказать раньше предметной проверки.
Неверная команда с `--json`, `validate --json`, корневой `--json` и неподдерживаемый
подкомандой `--version` также остаются текстовыми usage-ошибками. Глобального
флага нет. `inspect -- --json` читает файл с именем `--json` в текстовом режиме;
для JSON-чтения такого имени используйте `inspect --json -- --json`. То же для
create: `create -- --json` создаёт файл `--json` текстом; JSON-режим —
`create --json -- --json`.

Exit 7 — **ошибка доставки отчёта**, не обещание отката операции. Если stdout
закрыт после публикации, созданный документ или копия правки остаются целыми;
процесс не паникует, не удаляет файл и не запускает операцию снова. Не повторяйте
create или edit автоматически: проверьте назначение отдельным inspect и при
необходимости validate/rebuild. Даже если клиент не получил ответ, завершённая
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
text_command("export-stl", destination, "-o", root / "plate.stl")
text_command("export-fbx", destination, "-o", root / "plate.fbx")
assert source.read_bytes() == before
print("Verified JSON edit and native CLI checks:", root)
PY
```
