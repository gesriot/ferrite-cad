# §30O — several documents in tabs of one window

Marker `FCAD_30O_DOCUMENT_TABS`. Decision:
[ADR 0005, §30O](decisions/0005-document-session.md#30o-several-documents-in-tabs-of-one-window).
Verification and the real-window recipe:
[document-tabs-verification.md](document-tabs-verification.md).

## What a person can do

Open plate A, change it without saving, **Open…** B: B opens in a second tab and A
stays open, unsaved, without any question. Change B, press A's tab: A comes back as it
was left — its model, its own Undo/Redo, its checkpoints, its file, and the view it was
looked at from. Each tab saves to its own file.

* A row of tabs under the toolbar shows every open document: its name, `*` while it
  has unsaved changes, and **×**. The shown tab is selected. Two copies of one file
  may both be called `plate.fcad`; they are still two tabs.
* **Open…**, **New** (and the drawing forms), and **Recover** add a tab. Opening a
  file that is already open — by the same path, a symbolic link, a hard link or
  another spelling — shows its tab instead of opening it twice.
* Pressing a tab shows its document. The picture is rebuilt from the tab's current
  version; until it is ready the old tab stays shown and **Cancel** stops the switch.
* **×** on a tab with nothing unsaved closes it. On an unsaved or **Untitled** tab the
  tab is shown first, then *Save / Discard / Cancel* is asked. Closing the last tab
  leaves an empty window.
* **Quit** (closing the window, `Cmd+Q`) asks about every unsaved tab in turn: the
  shown one first, then each other in the row's order, each shown before it is asked.
* Every existing action — Apply, Add, Undo/Redo, checkpoints, STL/FBX export, Save,
  Save As — works on the shown tab's accepted model, saved or not.

## Rules

1. **One owner per tab.** A tab is one window controller (`sessions::Sessions`)
   holding one `DocumentSession`, with a runtime `TabId` made once and never reused —
   never a position and never the `DocumentId`. Path or Untitled, dirty, accepted
   history, checkpoints, Save and the crash-copy lane are the session's; the tab row
   reads names, `*` and availability from it. `tabs::Tabs` owns which tabs exist,
   their order, the hidden tabs' controllers and their view state (camera, typed
   checkpoint name). No rule of Save, Apply or Undo is repeated in it.
2. **One picture.** Only the shown tab has a scene on the GPU; hidden tabs keep
   files and a camera only, and nothing rebuilds them in the background. Switching:
   the target's current version is drawn on a worker (the scene route Undo uses),
   then in one statement the target becomes active, the previous tab is hidden with
   its camera, and the picture is replaced. A kernel refusal, a picture the device
   refuses, a Cancel, or a late answer leaves the previous tab shown and active.
   Camera is kept per tab; selection and visibility are reset on every switch.
3. **One foreground operation.** A switch holds the shown tab's operation slot (as
   Recover does). Showing or closing another tab and Quit wait while any operation,
   Open, New, export or Recover runs, or while a form is open (forms describe one
   picture and are never carried to another document). *Since §30P an idle edit form
   no longer holds the window: it stays with its own tab
   ([tab-edit-drafts.md](tab-edit-drafts.md)).* Every session answer carries
   `Address { tab, generation }`; an answer for another tab, a closed tab or an older
   generation changes nothing and releases no slot. Cancel of a switch releases the
   slot at once; its late picture is dropped.
4. **Adding a tab.** A candidate (Open, New, Recover) becomes a tab only through the
   `Bind` that shows its picture; a failed, cancelled or stale candidate adds nothing
   and its private files go. At most **8** tabs: a ninth Open, New or Recover is
   refused in words before anything is read or made. A file a tab already names is
   shown, not read again (also re-checked at the bind).
5. **Save As.** A destination that another tab names (by any name, on disk or not) is
   refused: *"Not saved: … is open in another tab."* Everything else is unchanged:
   no-clobber, the version guard, the private-folder refusal.
6. **Close.** Clean → closed. Unsaved → shown, then *Save / Discard / Cancel*: Cancel,
   a cancelled dialog or a failed Save keep the tab and its model; a published Save or
   Discard close it. Closing frees that tab's private files and retires its crash copy
   only. Closing the shown tab replaces the picture with an empty one in the same
   statement (prepared first; a device that refuses keeps the tab), then the
   neighbouring tab is shown.
7. **Quit.** The pass asks the shown tab, then shows and asks each unsaved hidden tab.
   *Save* goes on after the file is published; *Discard* goes on and acts only when the
   window ends. *Cancel*, a failed or cancelled Save, or a tab that cannot be shown stop
   the pass: no tab is closed, files saved during the pass stay saved, and tabs answered
   Discard stay open, unsaved and recoverable. When every unsaved tab is answered, the
   window ends and every tab's crash copy is retired.
8. **Recovery belongs to the session.** One recorder worker per process; each tab
   writes through its own lane (`RecoveryRecorder::lane`), so work, Save or Close in one
   tab never ends another tab's record. An exit nobody decided keeps every unsaved tab's
   last published copy. **Recover** opens the copy as a new tab and asks nothing about
   the open ones. Leases, the 32-record limit and the §30M rules are unchanged.
9. **Limits are per window.** Up to 8 tabs × (64 versions, 512 MiB) of private history
   = at most 4 GiB of private files in the temporary directory; one crash copy per
   unsaved tab within the store's 32 records; one GPU scene, plus the transient picture
   being prepared for a switch. The limit of one session is not the limit of the window.

## Not here

Restoring the tabs or their order after a restart (*§30R reopens the last window's saved
files: [restore-saved-tabs.md](restore-saved-tabs.md)*), per-tab form drafts (*added by
§30P: [tab-edit-drafts.md](tab-edit-drafts.md)*), background
work in hidden tabs, several windows, docking, a workspace database, a CLI session
protocol. The command line is unchanged: each command works on the file it is given
and reaches the same models (see the verification record). §30, Milestone 5C and the
product remain open; nothing here claims the earlier memory (OOM) investigation is
closed.
