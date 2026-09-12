# §24J — общий read-only validate

`ferritecad_jobs::validate_document(source: &Path) -> Result<ValidatedDocument>`
— минимальная операция: путь является всем запросом. Результат содержит owned
`document_id: DocumentId` и `report: ValidationReport`; живого SQLite connection,
kernel или geometry handle в нём нет. Нет параметров записи или отмены, UI-команды,
repair, миграции, нового content hash либо version token.

CLI `validate_result(&ValidateArgs)` — один адаптер text/JSON. Только JSON проверяет
UTF-8 путь до обращения к job; обычная lossy presentation текста не менялась.
Одна операция открывает `Document::open_read_only`, читает identity и вызывает
существующий `Document::validate` на закреплённом снимке. После чтения соединение
явно закрывается также при ошибке report; исходная ошибка чтения имеет приоритет
перед ошибкой close. Отчёт отдаётся только после успешного закрытия.

Валидация и diagnostic codes остаются в `ferritecad-document`. Jobs не решает
exit code, не печатает и не сериализует JSON. CLI проецирует owned факты в явные
DTO; counts считаются по тем же diagnostics, без повторного чтения документа.
Текст использует прежний `render::validation`. JSON использует прежний
`emit_with_exit`: 0 без errors, 1 с errors, 2 при operational refusal, 7 при
ошибке доставки любого исхода. Для 0/1 `ok:true`; только `valid` показывает,
есть ли errors. [Точная схема и рецепт](cli-json-v1.md#результат-validate-24j).

## Граница read-only

Схема, application ID, minimum reader и WAL проверяются общими guards.
Найден конкретный дефект существующего reader: DELETE-заголовок не мешал
SQLite изменять чужой SHM при наличии stale WAL. Теперь общий read-only open
до SQLite отказывает при наличии `-wal` либо `-shm` у запрошенного пути или
разрешённой цели symlink. Даже пустая/чужая sidecar не удаляется. Это защита
от записи, не новое правило ValidationReport; остальные read-only клиенты
получают ту же защиту. Hot rollback journal без WAL остаётся I/O refusal, когда
SQLite требует recovery; recovery не выполняется.
Ни fallback на writable open, ни SQLite backup, ни journal-mode rewrite не нужны.
Отсутствующий путь не создаётся. Чужие sidecars не удаляются и не обновляются.
Все сведения в одном report принадлежат одному SQLite reading; последующая
команда export открывает отдельный снимок. Между validate и export нет guard,
который запрещает изменение файла другим процессом.

Намеренно изменилось поведение **text validate для старых схем и WAL**: вместо
миграции/нормализации он отказывает с exit 2. Для текущих поддерживаемых файлов
сохранены тексты, порядок findings и exit 0/1. Dump-graph и clear-cache сохраняют
прежний `Document::open`; общий writable путь не изменён.

Часть повреждений позволяет закончить report: например, несовпадение сохранённого
payload hash, отсутствие dependency edge, повреждение embedded source bytes или
недостижимая source row. Невалидный CBOR envelope, несогласованная колонка kind,
неизвестная dependency role, недекодируемая metadata, старая схема или
несовместимый reader могут отказать раньше — это operational error, не пустой
успешный report. Набор правил не расширен ради JSON.

Warnings не становятся errors: `object.unknown-type` означает, что объект
сохранён, но не интерпретирован. Неизвестный code обрабатывается по severity;
неизвестная severity требует остановки клиента. Порядок и повторения findings
сохраняются, object UUID может именовать отсутствующий объект, None даёт null.
Message не является машинным кодом.

## Что результат не доказывает

Внутренняя согласованность сохранённых фактов не доказывает, что native rebuild
даст solid, STEP был правильным, повторный reader восстановит импортированную
геометрию или FBX будет complete. Historical STEP diagnostics, validation findings
и текущие FBX omissions независимы. Reader silence не подтверждает качество STEP.

Новые проверки работают без OCCT/PlaneGCS и без skips. Process suite проверяет
text/JSON/direct job, bytes/mtime/каталог, warnings/errors/отказы и OS pipes.
Два транзакционных теста проверяют pinned metadata/report и закрытие SQLite:
один вызывает внутреннее завершение job между writer update и commit; другой
проверяет немедленный SQLITE_BUSY при попытке commit с изменёнными metadata и
findings, затем успешный commit после close. Sleeps и публичного test callback нет.

[Исполненные проверки, failing-first, временные поломки и ограничения платформ](read-only-validation-verification.md).
