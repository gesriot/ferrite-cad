# §30N — named checkpoints inside the document

Decision: [ADR 0005, §30N](decisions/0005-document-session.md#30n-named-checkpoints-inside-the-document).
Verification and the macOS window recipe:
[named-document-checkpoints-verification.md](named-document-checkpoints-verification.md).

## What a person can do

The **Checkpoints** section (collapsed until opened; its heading shows how many there
are) lists the open document's checkpoints with their names and the UTC time each was
made; the one holding the model on screen is marked *on screen*.

* **Create checkpoint** keeps the accepted model — saved or not, Empty included — under
  the typed name. The picture is not rebuilt. The document becomes unsaved (`*name`):
  the checkpoint reaches the file only with **Save** / **Save As**. **Undo** takes it
  away again; **Redo** brings back the same checkpoint.
* **Restore** makes a checkpoint's model the working model as one step: its picture is
  prepared, and only then do model and picture change together. **Undo** returns to
  the previous working model, **Redo** to the same restored one. The file is not
  written, the history is kept, no new document opens, and the list stays as it was:
  Restore never brings back an older list.
* **Delete…** asks first and removes the checkpoint from the working document only;
  the model on screen stays. **Undo** brings it back; **Save** makes it final.
* Disabled buttons say why: no document, another operation running, a form open (a
  checkpoint keeps the accepted model, not a draft), an invalid name, the limit, or a
  list that cannot be read.

The command line lists, extracts, creates and deletes:

```text
ferritecad list-checkpoints SOURCE [--json]
ferritecad extract-checkpoint SOURCE --checkpoint UUID --output PATH [--expect-version V] [--json]
ferritecad create-checkpoint SOURCE --name NAME --expect-version V --output PATH [--json]
ferritecad delete-checkpoint SOURCE --checkpoint UUID --expect-version V --output PATH [--json]
```

`V` is the source's complete content version (`inspect` and `list-checkpoints` print
it). The source is never written: create and delete publish a *copy* with the change,
and extraction publishes one checkpoint's model alone — the command line's Restore.

## Rules

1. **Storage.** Table `checkpoints` of the `.fcad`, SQL schema v4: `id` (UUIDv7, the
   identity), `name`, `created_at`, `model`, `byte_len`, `image_hash` (BLAKE3), `image`
   (a complete document file whose own catalog is empty). Not the recovery folder, not
   `.fcad-cache`. `FORMAT_VERSION`, the minimum reader and capabilities are unchanged.
2. **Old files.** A schema v3 file is read without migration and has no checkpoints;
   opening, listing, validating, exporting and extracting never write it. A write
   produces v4 in the private version or the new output; the user's file becomes v4
   only on Save. An older FerriteCAD refuses a v4 file. Because the file would change
   schema, an Apply on a v3 document is an unsaved change even when its value is the
   same. `clear-cache` no longer migrates the document it is given.
3. **What a checkpoint holds.** The accepted version, copied by SQLite's online backup,
   its checkpoint rows removed, compacted with `VACUUM INTO` (row identities kept):
   every object, UUID, reference, dependency, parameter, imported source byte, unknown
   payload and unknown table of that version. Never draft form values, GPU state or the
   cache sidecar, and never other checkpoints (sizes add up; nothing nests).
4. **Ownership.** The list belongs to the working document. Rows are inserted and
   deleted, never changed. Restore takes the model from the image and keeps the
   working version's rows exactly (row identities included).
5. **Dirty, Undo, Save.** The list is document content: Create and Delete are steps of
   the one session history, dirty until saved, version-guarded and two-phase like any
   Apply. A step that changes only the list keeps the picture (no rebuild, no kernel);
   the forms' facts follow the new version. Restoring the model on screen is "No
   change". A recovered crash copy (§30M) carries the list, since it is the accepted
   version's file.
6. **Names.** Surrounding whitespace removed; 1–80 characters; no control characters.
   Equal names are allowed; every operation takes the UUID.
7. **Limits.** At most 32 checkpoints and 16 MiB of images per document. A Create past
   either is refused with the reason; nothing is removed to make room. The catalogue query reads no
   image; computing the complete content version still hashes every SQL cell, including
   stored images. Extraction and Restore stream one and check its length, hash, document id,
   empty catalog and recorded model before use.
8. **Damage.** A damaged image refuses that checkpoint only (extraction/Restore leave no
   file). A row this build would not have written makes the list unreadable — said in
   words — while the model still opens and edits.
9. **One operation at a time.** Create, Restore and Delete reserve the operation slot of
   Apply, Undo/Redo, Save, Open/New and Recover; while it is held none of the others
   starts, and Cancel or a replaced document turns a late answer into nothing. Disk,
   SQL and rebuild run on workers.
10. **CLI wire (JSON v1, additive).** Operations `list-checkpoints`, `create-checkpoint`,
    `delete-checkpoint`, `extract-checkpoint` in the existing envelope.
    `--expect-version` is required to create or delete and optional to extract (the
    UUID and the stored hash pin what is written). Every output is no-clobber; the
    source as output, an alias of it or an occupied path is refused. Usage errors stay
    clap text (exit 2), refusals are structured (exit 2), a lost report after
    publication is exit 7 and the file stays. No kernel is needed.

`list-checkpoints` result:

```json
{"document_id":"<uuid>","content_version":"<hex>","checkpoints":[
  {"checkpoint_id":"<uuid>","name":"A","created_at":"2026-10-08T07:12:33.123Z",
   "bytes":61440,"holds_current_model":true}]}
```

`create-checkpoint`, `delete-checkpoint`, `extract-checkpoint` result:

```json
{"destination":"…","document_id":"<uuid>","content_version":"<hex of the output>",
 "checkpoint":{"checkpoint_id":"<uuid>","name":"A","created_at":"…","bytes":61440}}
```

## Executable recipe (CLI)

```sh
ferritecad create plate.fcad --sample --size 80 40 12
V=$(ferritecad list-checkpoints plate.fcad --json | jq -r .result.content_version)
ferritecad create-checkpoint plate.fcad --name "12 mm" --expect-version "$V" -o with-a.fcad --json
A=$(ferritecad list-checkpoints with-a.fcad --json | jq -r '.result.checkpoints[0].checkpoint_id')
ferritecad extract-checkpoint with-a.fcad --checkpoint "$A" -o a.fcad
W=$(ferritecad list-checkpoints with-a.fcad --json | jq -r .result.content_version)
ferritecad delete-checkpoint with-a.fcad --checkpoint "$A" --expect-version "$W" -o without.fcad
```

## Not here

Tabs, a persisted Undo history, branching or merging, automatic checkpoints on Apply or
Save, cloud sync, a CLI session. Power-loss durability is not claimed. §30, Milestone 5C
and the product remain open.
