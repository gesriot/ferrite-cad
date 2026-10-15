// SPDX-License-Identifier: MIT
//! §31H: the launch journal's writer on temporary folders — the exact segment
//! bound, a record bigger than a segment, Unicode at the cut, rotation and
//! retention, folders and files that refuse, threads at once, a busy writer at a
//! panic, and instances side by side in one process and in two.

use super::*;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::io::BufRead as _;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

const CHILD_FOLDER: &str = "FCAD_LAUNCH_JOURNAL_CHILD_FOLDER";
const CHILD_GO: &str = "FCAD_LAUNCH_JOURNAL_CHILD_GO";
const SAY: &str = "FCAD-LAUNCH-JOURNAL-CHILD";
/// Far longer than anything here takes; only a waiting lock would reach it.
const DEADLINE: Duration = Duration::from_secs(120);

/// A raw line of exactly `bytes` bytes, its newline included: `append`'s input.
fn line_of(bytes: usize, fill: char) -> String {
    let mut line = fill.to_string().repeat(bytes - 1);
    line.push('\n');
    line
}

/// Every entry of `folder` by name, and whether it is a plain file. Nothing is
/// read: on Windows a running instance's lock file cannot be.
fn entries(folder: &Path) -> BTreeMap<String, bool> {
    std::fs::read_dir(folder)
        .expect("folder")
        .map(|entry| {
            let entry = entry.expect("entry");
            let name = entry.file_name().into_string().expect("a UTF-8 name");
            let kind = std::fs::symlink_metadata(entry.path()).expect("kind");
            (name, kind.is_file())
        })
        .collect()
}

/// The names of the files of the instance `stem` in `folder`.
fn files_of(folder: &Path, stem: &str) -> BTreeSet<String> {
    entries(folder)
        .into_keys()
        .filter(|name| name.starts_with(stem))
        .collect()
}

fn names(stem: &str, parts: &[&str]) -> BTreeSet<String> {
    parts.iter().map(|part| format!("{stem}{part}")).collect()
}

fn size(path: &Path) -> u64 {
    std::fs::metadata(path).expect("size").len()
}

fn text(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).expect("bytes")).expect("UTF-8")
}

/// Appends whole records until `journal` has moved to another segment.
fn fill(journal: &mut Journal) {
    let from = journal.number;
    while journal.number == from {
        assert!(!journal.stopped, "the journal stopped while filling");
        journal.append(&line_of(RECORD_LIMIT, 'f'));
    }
}

/// Whether `line` is a record: `<UTC time> <kind> <message>` and one newline.
fn is_record(line: &str, kind: &str) -> bool {
    let Some((time, rest)) = line.split_at_checked(23) else {
        return false;
    };
    time.ends_with(" UTC") && rest.starts_with(&format!(" {kind} "))
}

#[test]
fn a_segment_takes_records_up_to_exactly_sixteen_mebibytes_and_the_next_opens_segment_two() {
    let dir = tempfile::tempdir().expect("folder");
    let mut journal = Journal::open(dir.path()).expect("journal");
    let first = journal.segment(1);
    let header = size(&first);
    assert_eq!(journal.len, header);
    let mut left = SEGMENT_LIMIT - header;
    while left > RECORD_LIMIT as u64 {
        journal.append(&line_of(RECORD_LIMIT, 'a'));
        left -= RECORD_LIMIT as u64;
    }
    journal.append(&line_of(left as usize, 'b'));
    assert_eq!(
        size(&first),
        SEGMENT_LIMIT,
        "16 777 216 bytes, newlines included"
    );
    assert_eq!(SEGMENT_LIMIT, 16_777_216);
    assert_eq!((journal.number, journal.len), (1, SEGMENT_LIMIT));
    assert!(
        !journal.segment(2).exists(),
        "a segment that is exactly full stays"
    );

    // Two bytes more do not fit: they open segment 2, and segment 1 keeps every byte.
    let before = std::fs::read(&first).expect("first");
    assert!(before.ends_with(b"bb\n"));
    journal.append("c\n");
    // `assert!`, not `assert_eq!`: a failure must not print 16 MiB twice.
    assert!(
        std::fs::read(&first).expect("first") == before,
        "segment 1 changed: {} bytes",
        size(&first)
    );
    assert_eq!(journal.number, 2);
    let second = text(&journal.segment(2));
    assert!(
        second.starts_with(&format!("{SEGMENT_HEADER}segment=2 ")) && second.ends_with("\nc\n"),
        "{second}"
    );

    // A line of exactly RECORD_LIMIT bytes is a record; one byte more is not one.
    let len = journal.len;
    journal.append(&line_of(RECORD_LIMIT + 1, 'd'));
    assert_eq!(journal.len, len, "an over-long line is dropped");
    journal.append(&line_of(RECORD_LIMIT, 'e'));
    assert_eq!(journal.len, len + RECORD_LIMIT as u64);
    assert_eq!(size(&journal.segment(2)), journal.len);
    println!("\nFCAD_LAUNCH_JOURNAL_EXACT_LIMIT_EXECUTED");
}

#[test]
fn a_message_larger_than_a_segment_is_one_bounded_line_and_formatting_stops_at_the_limit() {
    // Twenty mebibytes of lines, given as one message.
    let big = "line\n".repeat(4 * 1024 * 1024);
    let line = record("error", format_args!("ferritecad: {big}"));
    assert!(line.len() <= RECORD_LIMIT, "{}", line.len());
    assert!(line.ends_with(" [truncated]\n"));
    assert_eq!(line.matches('\n').count(), 1, "one line");
    assert!(is_record(&line, "error"));
    assert!(line.contains(" error ferritecad: line\\nline\\n"));

    // A message that never ends is asked for only as much as fits.
    struct Endless(Cell<usize>);
    impl fmt::Display for Endless {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let piece = "x".repeat(1024);
            for _ in 0..1024 * 1024 {
                self.0.set(self.0.get() + 1);
                f.write_str(&piece)?;
            }
            Ok(())
        }
    }
    let endless = Endless(Cell::new(0));
    let cut = record("error", format_args!("{endless}"));
    assert!(cut.len() <= RECORD_LIMIT && cut.ends_with(" [truncated]\n"));
    assert!(
        endless.0.get() <= RECORD_LIMIT / 1024 + 1,
        "formatting went on for {} pieces",
        endless.0.get()
    );

    // Written, it is one record of the journal and nothing more.
    let dir = tempfile::tempdir().expect("folder");
    let mut journal = Journal::open(dir.path()).expect("journal");
    let header = journal.len;
    journal.append(&line);
    assert_eq!(size(&journal.segment(1)), header + line.len() as u64);

    // A message that fits is whole; control characters are escaped, a tab is not.
    let fits = record("error", format_args!("a\tb\u{1b}c\r\nd"));
    assert!(fits.ends_with(" error a\tb\\u{1b}c\\r\\nd\n"), "{fits}");
    println!("\nFCAD_LAUNCH_JOURNAL_LARGE_RECORD_EXECUTED");
}

#[test]
fn a_cut_never_splits_a_character_or_an_escape_and_keeps_as_much_as_fits() {
    for character in ['a', 'é', '€', '𝄞'] {
        for shift in 0..4 {
            let message = format!(
                "{}{}",
                "x".repeat(shift),
                character.to_string().repeat(RECORD_LIMIT)
            );
            let line = record("error", format_args!("{message}"));
            assert!(
                line.len() <= RECORD_LIMIT,
                "{character} {shift}: {}",
                line.len()
            );
            let body = line.strip_suffix(" [truncated]\n").expect("cut");
            let (prefix, content) = body.split_once(" error ").expect("kind");
            assert!(
                message.starts_with(content),
                "{character} {shift}: not a prefix"
            );
            let room = CONTENT_LIMIT - (prefix.len() + " error ".len());
            assert!(
                content.len() <= room && content.len() + character.len_utf8() > room,
                "{character} {shift}: {} of {room}",
                content.len()
            );
        }
    }
    // An escape is whole too: a run of ESC characters is cut between escapes.
    let line = record("error", format_args!("{}", "\u{1b}".repeat(RECORD_LIMIT)));
    let content = line
        .strip_suffix(" [truncated]\n")
        .and_then(|body| body.split_once(" error "))
        .expect("cut")
        .1;
    let escape = "\\u{1b}";
    assert_eq!(content, escape.repeat(content.len() / escape.len()));
    let prefix = line.len() - TRUNCATED.len() - 1 - content.len();
    assert!(
        content.len() + escape.len() > CONTENT_LIMIT - prefix,
        "as many as fit"
    );
    println!("\nFCAD_LAUNCH_JOURNAL_UNICODE_EXECUTED");
}

#[test]
fn an_instance_keeps_its_first_and_current_segment_and_removes_only_its_own_full_ones() {
    let dir = tempfile::tempdir().expect("folder");
    let foreign = dir.path().join("viewer-notes.log");
    std::fs::write(&foreign, b"somebody's notes").expect("foreign");
    let mut journal = Journal::open(dir.path()).expect("journal");
    let stem = journal.stem.clone();
    journal.append(&record(
        "error",
        format_args!("ferritecad: the first error"),
    ));
    assert_eq!(
        files_of(dir.path(), &stem),
        names(&stem, &[".lock", ".1.log"])
    );

    fill(&mut journal);
    assert_eq!(journal.number, 2);
    assert_eq!(
        files_of(dir.path(), &stem),
        names(&stem, &[".lock", ".1.log", ".2.log"])
    );
    let first = std::fs::read(journal.segment(1)).expect("first");
    assert!(first.len() as u64 <= SEGMENT_LIMIT);
    assert!(String::from_utf8_lossy(&first).contains(" error ferritecad: the first error\n"));

    for (number, expected) in [
        (3, [".lock", ".1.log", ".3.log"]),
        (4, [".lock", ".1.log", ".4.log"]),
    ] {
        fill(&mut journal);
        assert_eq!(journal.number, number);
        assert_eq!(files_of(dir.path(), &stem), names(&stem, &expected));
        assert!(
            std::fs::read(journal.segment(1)).expect("first") == first,
            "segment 1 is kept whole"
        );
    }
    for number in [1, 4] {
        let segment = text(&journal.segment(number));
        assert!(segment.len() as u64 <= SEGMENT_LIMIT);
        let header = segment.lines().next().expect("header");
        assert!(header.starts_with(&format!(
            "{SEGMENT_HEADER}segment={number} ferritecad-viewer "
        )));
        assert!(header.contains(&format!(
            " pid={} instance={} ",
            std::process::id(),
            journal.id
        )));
    }
    assert!(!journal.stopped);
    assert_eq!(
        std::fs::read(&foreign).expect("foreign"),
        b"somebody's notes"
    );
    println!("\nFCAD_LAUNCH_JOURNAL_ROTATION_EXECUTED");
}

#[test]
fn a_starting_instance_keeps_the_four_newest_finished_a_running_one_and_everything_foreign() {
    let dir = tempfile::tempdir().expect("folder");
    let folder = dir.path();
    let outside = tempfile::tempdir().expect("outside");
    let target = outside.path().join("target.txt");
    std::fs::write(&target, b"not the journal's").expect("target");

    // Foreign: names that are not exactly this journal's, and one of its names
    // whose instance has no lock file, or a lock file without its header.
    let stray = ObjectId::new();
    let unlocked = ObjectId::new();
    let foreign = [
        ("viewer-notes.log".to_owned(), b"notes".to_vec()),
        ("other-app.log".to_owned(), b"other".to_vec()),
        (
            format!("viewer-20261015T031300Z-1-{stray}.1.log"),
            format!("{SEGMENT_HEADER}segment=1 x\n").into_bytes(),
        ),
        (
            format!("viewer-20261015T031300Z-01-{unlocked}.1.log"),
            b"leading zero".to_vec(),
        ),
        (
            format!("viewer-20261015T031300Z-7-{unlocked}.lock"),
            b"not a lock header\n".to_vec(),
        ),
        (
            format!("viewer-20261015T031300Z-7-{unlocked}.1.log"),
            format!("{SEGMENT_HEADER}segment=1 x\n").into_bytes(),
        ),
    ];
    for (name, bytes) in &foreign {
        std::fs::write(folder.join(name), bytes).expect("foreign");
    }

    // A running instance, older than every finished one: the first to go if it
    // were taken for finished.
    let mut running = Journal::open(folder).expect("running");
    running.append(&record("error", format_args!("still running")));
    let mut finished = Vec::new();
    for at in 0..6 {
        std::thread::sleep(Duration::from_millis(3));
        let mut journal = Journal::open(folder).expect("finished");
        // One PID, a new instance every time.
        assert!(journal.stem.contains(&format!("-{}-", std::process::id())));
        assert_ne!(journal.stem, running.stem);
        journal.append(&record("error", format_args!("finished {at}")));
        finished.push(journal.stem.clone());
        drop(journal);
        if at == 0 {
            // Beside the oldest finished one: a link and a folder named like its
            // segments. Removing that instance follows neither.
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, folder.join(format!("{}.2.log", finished[0])))
                .expect("link");
            std::fs::create_dir(folder.join(format!("{}.3.log", finished[0]))).expect("folder");
        }
    }
    std::thread::sleep(Duration::from_millis(3));
    let starting = Journal::open(folder).expect("starting");

    let left = entries(folder);
    for stem in &finished[..2] {
        assert!(
            !left.contains_key(&format!("{stem}.lock")),
            "{stem} was kept"
        );
        assert!(
            !left.contains_key(&format!("{stem}.1.log")),
            "{stem} was kept"
        );
    }
    for stem in &finished[2..] {
        assert_eq!(
            files_of(folder, stem),
            names(stem, &[".lock", ".1.log"]),
            "{stem}"
        );
    }
    assert_eq!(
        files_of(folder, &running.stem),
        names(&running.stem, &[".lock", ".1.log"])
    );
    assert_eq!(
        files_of(folder, &starting.stem),
        names(&starting.stem, &[".lock", ".1.log"])
    );
    for (name, bytes) in &foreign {
        assert_eq!(left.get(name), Some(&true), "{name}");
        assert_eq!(
            &std::fs::read(folder.join(name)).expect("foreign"),
            bytes,
            "{name}"
        );
    }
    #[cfg(unix)]
    assert_eq!(
        left.get(&format!("{}.2.log", finished[0])),
        Some(&false),
        "the link stays"
    );
    assert_eq!(
        left.get(&format!("{}.3.log", finished[0])),
        Some(&false),
        "the folder stays"
    );
    assert_eq!(
        std::fs::read(&target).expect("target"),
        b"not the journal's"
    );
    #[cfg(unix)]
    let links = 1;
    #[cfg(not(unix))]
    let links = 0;
    assert_eq!(left.len(), foreign.len() + 2 * (4 + 2) + 1 + links);

    // The running instance goes on writing into the file it always had.
    running.append(&record("error", format_args!("after the sweep")));
    let own = text(&running.segment(1));
    assert!(own.contains(" error still running\n") && own.ends_with(" error after the sweep\n"));
    retention_retries_a_segment_after_access_is_restored();
    println!("\nFCAD_LAUNCH_JOURNAL_RETENTION_EXECUTED");
}

/// An inaccessible segment must keep its instance's lock file. Otherwise a later
/// sweep can never recognise and remove it, even after access is restored.
fn retention_retries_a_segment_after_access_is_restored() {
    let dir = tempfile::tempdir().expect("folder");
    let old = Journal::open(dir.path()).expect("old instance");
    let segment = old.segment(1);
    let lock = dir.path().join(format!("{}{LOCK_SUFFIX}", old.stem));
    drop(old);
    for _ in 0..KEEP_FINISHED {
        std::thread::sleep(Duration::from_millis(3));
        drop(Journal::open(dir.path()).expect("newer finished instance"));
    }
    let permissions = std::fs::metadata(&segment).expect("segment").permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&segment, std::fs::Permissions::from_mode(0o000))
            .expect("refuse reading the header");
        assert!(File::open(&segment).is_err(), "run unprivileged");
    }
    #[cfg(windows)]
    {
        let mut denied = permissions.clone();
        denied.set_readonly(true);
        std::fs::set_permissions(&segment, denied).expect("refuse removing the segment");
    }
    let started = Journal::open(dir.path()).expect("a sweep with an inaccessible segment");
    let lock_kept = lock.exists();
    let segment_kept = segment.exists();
    std::fs::set_permissions(&segment, permissions).expect("restore access");
    assert!(segment_kept, "an inaccessible segment was removed");
    assert!(
        lock_kept,
        "cleanup lost the lock needed to retry the segment"
    );

    let next = Journal::open(dir.path()).expect("a sweep after access is restored");
    assert!(!segment.exists(), "cleanup must retry the old segment");
    assert!(
        !lock.exists(),
        "the completed cleanup removes its lock last"
    );
    drop((started, next));
}

#[test]
fn folders_and_files_that_refuse_never_panic_and_stop_the_journal_once() {
    let dir = tempfile::tempdir().expect("folder");

    // A file where the folder should be, and a folder below a file.
    let file = dir.path().join("not-a-folder");
    std::fs::write(&file, b"a file").expect("file");
    assert!(Journal::open(&file).is_err());
    assert!(Journal::open(&file.join("below")).is_err());
    assert_eq!(std::fs::read(&file).expect("file"), b"a file");

    // A link to a folder is refused, and nothing is written behind it.
    #[cfg(unix)]
    {
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("real");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        assert!(Journal::open(&link).is_err());
        assert!(entries(&real).is_empty());
    }

    // A folder this user may not write into. A privileged user would be let in,
    // and then this proves nothing: that is a failure, not a pass.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let closed = dir.path().join("closed");
        std::fs::create_dir(&closed).expect("closed");
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o500)).expect("mode");
        let probe = std::fs::write(closed.join("probe"), b"");
        let refused = Journal::open(&closed).err();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o700)).expect("mode");
        assert!(
            probe.is_err(),
            "this user can write into a 0500 folder; run unprivileged"
        );
        let refused = refused.expect("an unwritable folder is refused");
        assert!(refused.to_string().contains("creating"), "{refused}");
        assert!(entries(&closed).is_empty());
    }

    // A write the operating system refuses: the segment's handle is read-only.
    let mut journal = Journal::open(&dir.path().join("logs")).expect("journal");
    journal.append(&record("error", format_args!("before")));
    let path = journal.segment(1);
    let before = std::fs::read(&path).expect("before");
    journal.current = File::open(&path).expect("read-only");
    journal.append(&record("error", format_args!("refused")));
    assert!(journal.stopped);
    for _ in 0..1000 {
        journal.append(&record("error", format_args!("after")));
    }
    assert_eq!(
        std::fs::read(&path).expect("after"),
        before,
        "no retry, no growth"
    );

    // A rotation that cannot make its next segment stops at the bound.
    let mut journal = Journal::open(&dir.path().join("full")).expect("journal");
    std::fs::write(journal.segment(2), b"in the way").expect("in the way");
    let mut appended = 0;
    while !journal.stopped {
        journal.append(&line_of(RECORD_LIMIT, 'g'));
        appended += 1;
        assert!(
            appended <= 2 * SEGMENT_LIMIT as usize / RECORD_LIMIT,
            "it never stopped"
        );
    }
    assert!(size(&journal.segment(1)) <= SEGMENT_LIMIT);
    assert_eq!(
        std::fs::read(journal.segment(2)).expect("in the way"),
        b"in the way"
    );
    assert!(!journal.segment(3).exists());
    for _ in 0..1000 {
        journal.append(&line_of(RECORD_LIMIT, 'h'));
    }
    assert!(size(&journal.segment(1)) <= SEGMENT_LIMIT);
    println!("\nFCAD_LAUNCH_JOURNAL_FAILURES_EXECUTED");
}

#[test]
fn threads_writing_at_once_leave_whole_records_in_bounded_segments() {
    const THREADS: usize = 8;
    const EACH: usize = 400;
    let dir = tempfile::tempdir().expect("folder");
    let journal = Mutex::new(Journal::open(dir.path()).expect("journal"));
    let message = |thread: usize, at: usize| {
        let run = (at * 37 + thread * 1013) % RECORD_LIMIT;
        format!("t{thread}-{at}-{}", "z".repeat(run))
    };
    std::thread::scope(|scope| {
        for thread in 0..THREADS {
            let journal = &journal;
            scope.spawn(move || {
                for at in 0..EACH {
                    let line = record("error", format_args!("{}", message(thread, at)));
                    hold(journal).append(&line);
                }
            });
        }
    });
    let journal = journal.into_inner().expect("whole");
    assert!(!journal.stopped);
    assert_eq!(
        journal.number, 2,
        "about 26 MiB: two segments, none removed"
    );

    let mut seen = BTreeSet::new();
    for number in 1..=2 {
        let path = journal.segment(number);
        assert!(size(&path) <= SEGMENT_LIMIT);
        let segment = text(&path);
        assert!(segment.ends_with('\n'));
        for line in segment.lines().skip(1) {
            assert!(
                is_record(line, "error"),
                "an interleaved line: {:.80}",
                line
            );
            let content = &line[23 + " error ".len()..];
            let (key, _) = content.split_once("-z").unwrap_or((content, ""));
            let (thread, at) = key
                .strip_prefix('t')
                .and_then(|key| key.split_once('-'))
                .and_then(|(t, a)| Some((t.parse().ok()?, a.trim_end_matches('-').parse().ok()?)))
                .expect("a key");
            let whole = message(thread, at);
            let room = CONTENT_LIMIT - 23 - " error ".len();
            let expected = if whole.len() > room {
                format!("{}{TRUNCATED}", &whole[..room])
            } else {
                whole
            };
            assert_eq!(content, expected, "t{thread}-{at}");
            assert!(seen.insert((thread, at)), "t{thread}-{at} twice");
        }
    }
    assert_eq!(seen.len(), THREADS * EACH, "every record once");
    println!("\nFCAD_LAUNCH_JOURNAL_THREADS_EXECUTED");
}

#[test]
fn a_panic_never_waits_for_a_busy_writer() {
    let dir = tempfile::tempdir().expect("folder");
    let journal = Mutex::new(Journal::open(dir.path()).expect("journal"));
    let path = hold(&journal).segment(1);
    {
        // The writer is busy: another thread's record, or the panicking thread's own.
        let busy = hold(&journal);
        let started = Instant::now();
        let written = append_if_free(&journal, &record("panic", format_args!("while busy")));
        let waited = started.elapsed();
        assert!(!written);
        assert!(waited >= PANIC_WAIT * PANIC_TRIES, "{waited:?}");
        assert!(waited < Duration::from_secs(10), "{waited:?}");
        drop(busy);
    }
    assert!(!text(&path).contains("while busy"));
    assert!(append_if_free(
        &journal,
        &record("panic", format_args!("when free"))
    ));
    assert!(text(&path).ends_with(" panic when free\n"));
    println!("\nFCAD_LAUNCH_JOURNAL_BUSY_PANIC_EXECUTED");
}

/// Reads `child`'s stdout on a thread and hands over each line.
fn lines_of(child: &mut std::process::Child) -> mpsc::Receiver<String> {
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// The first line from `lines` that starts with `prefix`, all lines read so far.
fn wait_for(
    lines: &mpsc::Receiver<String>,
    prefix: &str,
    said: &mut Vec<String>,
) -> Option<String> {
    let deadline = Instant::now() + DEADLINE;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let line = lines.recv_timeout(left).ok()?;
        said.push(line.clone());
        if line.starts_with(prefix) {
            return Some(line);
        }
    }
    None
}

/// An instance in another process: it opens, writes, says so, waits, then fills
/// its own segments while the parent's instances hold theirs.
#[test]
#[ignore = "a child process of instances_side_by_side_keep_their_own_files_and_never_wait"]
fn instance_child() {
    let folder =
        PathBuf::from(std::env::var_os(CHILD_FOLDER).expect("the parent names the folder"));
    let go = PathBuf::from(std::env::var_os(CHILD_GO).expect("the parent names the signal"));
    let mut journal = Journal::open(&folder).expect("journal");
    journal.append(&record(
        "error",
        format_args!("child {}", std::process::id()),
    ));
    println!("\n{SAY} READY {}", journal.stem);
    let _ = std::io::stdout().flush();
    let deadline = Instant::now() + DEADLINE;
    while !go.exists() {
        // A parent that failed took its folder with it: nobody will say go.
        assert!(folder.exists(), "the parent is gone");
        assert!(
            Instant::now() < deadline,
            "the parent never let the child go on"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fill(&mut journal);
    fill(&mut journal);
    assert_eq!(
        files_of(&folder, &journal.stem),
        names(&journal.stem, &[".lock", ".1.log", ".3.log"])
    );
    println!("\n{SAY} DONE {} segments={}", journal.stem, journal.number);
}

/// A child process that is killed and reaped if the test ends before it does: a
/// failed assertion must not leave it waiting for a signal that never comes.
struct Reaped(std::process::Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// Two instances in this process (one PID) and one in another: each has its own
/// files, nobody's opening waits for another's lock, nobody removes or rotates
/// another's files, and a running instance is never taken for a finished one.
#[test]
fn instances_side_by_side_keep_their_own_files_and_never_wait() {
    let dir = tempfile::tempdir().expect("folder");
    let folder = dir.path().join("logs");
    let go = dir.path().join("go");
    let mut child = Reaped(
        Command::new(std::env::current_exe().expect("this test binary"))
            .args([
                "journal::tests::instance_child",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_FOLDER, &folder)
            .env(CHILD_GO, &go)
            .stdout(Stdio::piped())
            .spawn()
            .expect("a child"),
    );
    let lines = lines_of(&mut child.0);
    let mut said = Vec::new();
    let Some(ready) = wait_for(&lines, &format!("{SAY} READY "), &mut said) else {
        panic!("the child never opened its journal: {said:?}");
    };
    let other = ready.rsplit(' ').next().expect("stem").to_owned();

    // Two instances here, newer than the child's, then five finished ones; the
    // child's instance is the oldest, so it would be the first removed if it were
    // taken for finished. No opening may wait for the child's lock.
    let opening = |at: &str| {
        let started = Instant::now();
        let journal = Journal::open(&folder).expect(at);
        assert!(started.elapsed() < Duration::from_secs(10), "{at} waited");
        journal
    };
    let mut first = opening("first here");
    let mut second = opening("second here");
    assert_ne!(first.stem, second.stem);
    for at in 0..5 {
        std::thread::sleep(Duration::from_millis(3));
        let mut done = opening("finished");
        done.append(&record("error", format_args!("finished {at}")));
    }
    let last = opening("last");
    assert_eq!(
        files_of(&folder, &other),
        names(&other, &[".lock", ".1.log"]),
        "the child's files"
    );
    assert_eq!(
        files_of(&folder, &first.stem),
        names(&first.stem, &[".lock", ".1.log"])
    );
    assert_eq!(
        files_of(&folder, &second.stem),
        names(&second.stem, &[".lock", ".1.log"])
    );

    // The child fills its own segments while these hold theirs.
    std::fs::write(&go, b"go").expect("go");
    first.append(&record("error", format_args!("first {}", first.id)));
    second.append(&record("error", format_args!("second {}", second.id)));
    let done = wait_for(&lines, &format!("{SAY} DONE "), &mut said);
    let status = child.0.wait().expect("the child ends");
    let done = done.unwrap_or_else(|| panic!("the child did not finish: {said:?}"));
    assert!(status.success(), "{said:?}");
    assert_eq!(done, format!("{SAY} DONE {other} segments=3"));
    while let Ok(line) = lines.recv_timeout(Duration::from_secs(10)) {
        said.push(line);
    }
    assert!(
        said.iter()
            .any(|line| line.starts_with("test result: ok. 1 passed")),
        "{said:?}"
    );

    // Everyone's files are their own and only theirs.
    assert_eq!(
        files_of(&folder, &other),
        names(&other, &[".lock", ".1.log", ".3.log"])
    );
    let child_first = text(&folder.join(format!("{other}.1.log")));
    assert!(child_first.contains(" error child "));
    for journal in [&first, &second, &last] {
        assert_eq!(
            files_of(&folder, &journal.stem),
            names(&journal.stem, &[".lock", ".1.log"])
        );
        let own = text(&journal.segment(1));
        assert!(
            own.lines()
                .next()
                .expect("header")
                .contains(&format!(" instance={} ", journal.id))
        );
        assert!(!own.contains(" error child ") && !own.contains("fff"));
    }
    assert!(text(&first.segment(1)).ends_with(&format!(" error first {}\n", first.id)));
    assert!(text(&second.segment(1)).ends_with(&format!(" error second {}\n", second.id)));
    println!("\nFCAD_LAUNCH_JOURNAL_INSTANCES_EXECUTED");
}
