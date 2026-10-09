// SPDX-License-Identifier: MIT
//! §30R: the descriptor of the last window — its format, reading that writes
//! nothing, refusals that leave everything as it was, a publication that is whole
//! or absent, exact native paths, and two processes publishing at once.

use super::*;
use std::ffi::OsString;
use std::time::{Duration, SystemTime};

/// An absolute path on this platform.
fn absolute(parts: &[&str]) -> PathBuf {
    let mut path = std::env::temp_dir()
        .ancestors()
        .last()
        .expect("a root")
        .to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

/// Every entry of `folder`, by name, with its bytes (or `None` for a folder or
/// link), and its modification time.
fn snapshot(folder: &Path) -> Vec<(OsString, Option<Vec<u8>>, SystemTime)> {
    let mut entries: Vec<_> = std::fs::read_dir(folder)
        .expect("folder")
        .map(|entry| {
            let entry = entry.expect("entry");
            let meta = std::fs::symlink_metadata(entry.path()).expect("meta");
            let bytes = meta
                .is_file()
                .then(|| std::fs::read(entry.path()).expect("bytes"));
            (entry.file_name(), bytes, meta.modified().expect("mtime"))
        })
        .collect();
    entries.sort();
    entries
}

fn partials(folder: &Path) -> Vec<OsString> {
    std::fs::read_dir(folder)
        .expect("folder")
        .map(|entry| entry.expect("entry").file_name())
        .filter(|name| name.to_string_lossy().ends_with(PARTIAL_SUFFIX))
        .collect()
}

fn set(paths: &[PathBuf], active: Option<usize>) -> LastTabs {
    LastTabs {
        paths: paths.to_vec(),
        active,
    }
}

/// Whether permissions are enforced for this process (not root, not a
/// filesystem that ignores modes). A test that relies on them says N/A otherwise.
#[cfg(unix)]
fn modes_enforced(probe: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir(probe).expect("probe");
    std::fs::set_permissions(probe, std::fs::Permissions::from_mode(0o500)).expect("mode");
    let enforced = std::fs::write(probe.join("x"), b"x").is_err();
    std::fs::set_permissions(probe, std::fs::Permissions::from_mode(0o700)).expect("mode");
    std::fs::remove_dir_all(probe).expect("probe");
    enforced
}

/// A published list reads back exactly; reading makes no folder, writes nothing
/// and changes no time; an empty list is a list (it clears the offer); every
/// publication replaces the whole list.
#[test]
fn a_published_list_reads_back_exactly_and_reading_writes_nothing() {
    let home = tempfile::tempdir().expect("home");
    let folder = Folder::at(&home.path().join("state").join("tabs")).expect("folder");
    assert_eq!(folder.read().expect("nothing there"), None);
    assert!(
        !home.path().join("state").exists(),
        "reading made no folder"
    );

    let three = set(
        &[
            absolute(&["work", "a.fcad"]),
            absolute(&["work", "Пла ста", "ü plate .fcad"]),
            absolute(&["other", "a.fcad"]),
        ],
        Some(1),
    );
    folder.publish(&three).expect("published");
    assert_eq!(folder.read().expect("read"), Some(three.clone()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode =
            |path: &Path| std::fs::metadata(path).expect("mode").permissions().mode() & 0o777;
        assert_eq!(mode(folder.root()), 0o700, "a folder of its own");
        assert_eq!(mode(&folder.descriptor()), 0o600);
    }
    let before = snapshot(folder.root());
    std::thread::sleep(Duration::from_millis(20));
    for _ in 0..3 {
        assert_eq!(folder.read().expect("read"), Some(three.clone()));
    }
    assert_eq!(snapshot(folder.root()), before, "reading wrote nothing");
    assert!(partials(folder.root()).is_empty());

    // The whole list is replaced, never merged.
    let one = set(&[absolute(&["x.fcad"])], None);
    folder.publish(&one).expect("published");
    assert_eq!(folder.read().expect("read"), Some(one));
    // A window that ended with no saved file: an empty list, not "no list".
    folder.publish(&LastTabs::default()).expect("published");
    assert_eq!(folder.read().expect("read"), Some(LastTabs::default()));
    assert_eq!(
        std::fs::read(folder.descriptor()).expect("bytes"),
        format!("FERRITECAD-LAST-TABS 1\nplatform {PLATFORM}\ncount 0\nactive none\n").into_bytes()
    );
    assert!(
        partials(folder.root()).is_empty(),
        "no temporary file is left"
    );
    println!("\nFCAD_30R_DESCRIPTOR_ROUND_TRIP_EXECUTED");
}

/// A list that is damaged, cut short, of an unknown version, too large, a link or
/// not a file is refused with its own kind, and left exactly as it was.
#[test]
fn damaged_foreign_or_unknown_lists_are_refused_and_left_as_they_are() {
    let home = tempfile::tempdir().expect("home");
    let folder = Folder::at(&home.path().join("tabs")).expect("folder");
    let good = set(
        &[absolute(&["work", "a.fcad"]), absolute(&["work", "b.fcad"])],
        Some(0),
    );
    folder.publish(&good).expect("published");
    let valid =
        String::from_utf8(std::fs::read(folder.descriptor()).expect("bytes")).expect("text");
    let hex = encode(&absolute(&["work", "a.fcad"]));
    let header = |count: &str, active: &str| {
        format!("FERRITECAD-LAST-TABS 1\nplatform {PLATFORM}\ncount {count}\nactive {active}\n")
    };
    let other = if PLATFORM == "unix" {
        "windows"
    } else {
        "unix"
    };
    let long = "61".repeat(MAX_PATH_HEX / 2 + 1);
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("empty", Vec::new(), "damaged"),
        ("cut in the magic", b"FERRITECAD-LA".to_vec(), "damaged"),
        (
            "newer version",
            valid.replacen(" 1\n", " 2\n", 1).into_bytes(),
            "unknown-version",
        ),
        (
            "version not a number",
            valid.replacen(" 1\n", " x\n", 1).into_bytes(),
            "damaged",
        ),
        ("not ours", b"[tabs]\npath=/a.fcad\n".to_vec(), "damaged"),
        (
            "no last newline",
            valid.trim_end_matches('\n').as_bytes().to_vec(),
            "damaged",
        ),
        (
            "a path line missing",
            valid.as_bytes()[..valid.trim_end_matches('\n').rfind('\n').expect("a line") + 1]
                .to_vec(),
            "damaged",
        ),
        (
            "a line after the paths",
            format!("{valid}path {hex}\n").into_bytes(),
            "damaged",
        ),
        (
            "nine files",
            {
                let mut text = header("9", "none");
                for _ in 0..9 {
                    text.push_str(&format!("path {hex}\n"));
                }
                text.into_bytes()
            },
            "damaged",
        ),
        (
            "shown file out of range",
            format!("{}path {hex}\n", header("1", "1")).into_bytes(),
            "damaged",
        ),
        (
            "a leading zero",
            format!("{}path {hex}\n", header("01", "none")).into_bytes(),
            "damaged",
        ),
        (
            "upper-case digits",
            format!("{}path {}\n", header("1", "0"), hex.to_uppercase()).into_bytes(),
            "damaged",
        ),
        (
            "odd digits",
            format!("{}path {}\n", header("1", "0"), &hex[1..]).into_bytes(),
            "damaged",
        ),
        (
            "a relative path",
            format!("{}path {}\n", header("1", "0"), encode(Path::new("a.fcad"))).into_bytes(),
            "damaged",
        ),
        (
            "a NUL unit",
            format!(
                "{}path {}{}\n",
                header("1", "0"),
                hex,
                "0".repeat(UNIT_DIGITS)
            )
            .into_bytes(),
            "damaged",
        ),
        (
            "another platform",
            valid
                .replacen(
                    &format!("platform {PLATFORM}"),
                    &format!("platform {other}"),
                    1,
                )
                .into_bytes(),
            "damaged",
        ),
        (
            "not text",
            [valid.as_bytes(), &[0xff, b'\n']].concat(),
            "damaged",
        ),
        (
            "a path longer than any kept",
            format!("{}path {long}\n", header("1", "0")).into_bytes(),
            "damaged",
        ),
    ];
    for (name, bytes, kind) in &cases {
        std::fs::write(folder.descriptor(), bytes).expect("written");
        let before = snapshot(folder.root());
        let refused = folder.read().expect_err(name);
        assert_eq!(refused.kind(), *kind, "{name}: {refused}");
        assert_eq!(snapshot(folder.root()), before, "{name}: left as it was");
        assert_eq!(folder.publish(&good).expect_err(name).kind(), *kind);
        assert_eq!(
            snapshot(folder.root()),
            before,
            "{name}: Quit must preserve it too"
        );
    }

    // Too large: refused from its size, before it is read.
    let file = std::fs::File::create(folder.descriptor()).expect("file");
    file.set_len(MAX_BYTES + 1).expect("sparse");
    drop(file);
    assert!(matches!(folder.read(), Err(LastTabsError::TooLarge(n)) if n == MAX_BYTES + 1));
    assert_eq!(
        std::fs::metadata(folder.descriptor()).expect("kept").len(),
        MAX_BYTES + 1
    );

    // A folder in the descriptor's place, and (Unix) links, are never followed.
    std::fs::remove_file(folder.descriptor()).expect("removed");
    std::fs::create_dir(folder.descriptor()).expect("a folder in its place");
    assert_eq!(folder.read().expect_err("a folder").kind(), "foreign");
    std::fs::remove_dir(folder.descriptor()).expect("removed");
    #[cfg(unix)]
    {
        let elsewhere = home.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).expect("elsewhere");
        let target = elsewhere.join("document.fcad");
        std::fs::write(&target, valid.as_bytes()).expect("a user's file");
        std::os::unix::fs::symlink(&target, folder.descriptor()).expect("link");
        assert_eq!(folder.read().expect_err("a link").kind(), "foreign");
        // Publishing does not replace a link it did not make, nor write through it.
        assert_eq!(folder.publish(&good).expect_err("a link").kind(), "foreign");
        assert_eq!(std::fs::read(&target).expect("kept"), valid.as_bytes());
        assert!(
            std::fs::symlink_metadata(folder.descriptor())
                .expect("link")
                .file_type()
                .is_symlink()
        );
        std::fs::remove_file(folder.descriptor()).expect("removed");
        // A linked folder is refused for reading and publishing alike.
        let linked = Folder::at(&home.path().join("linked")).expect("folder");
        std::os::unix::fs::symlink(&elsewhere, linked.root()).expect("link");
        assert_eq!(
            linked.read().expect_err("a linked folder").kind(),
            "foreign"
        );
        assert_eq!(
            linked.publish(&good).expect_err("a linked folder").kind(),
            "foreign"
        );
        assert_eq!(
            std::fs::read_dir(&elsewhere).expect("dir").count(),
            1,
            "nothing written there"
        );
        // Unreadable: an I/O refusal, the file kept (N/A where modes are not enforced).
        if modes_enforced(&home.path().join("probe")) {
            use std::os::unix::fs::PermissionsExt as _;
            folder.publish(&good).expect("published");
            let bytes = std::fs::read(folder.descriptor()).expect("bytes");
            std::fs::set_permissions(folder.descriptor(), std::fs::Permissions::from_mode(0o000))
                .expect("mode");
            assert_eq!(folder.read().expect_err("unreadable").kind(), "io");
            std::fs::set_permissions(folder.descriptor(), std::fs::Permissions::from_mode(0o600))
                .expect("mode");
            assert_eq!(std::fs::read(folder.descriptor()).expect("kept"), bytes);
        } else {
            println!("FCAD_30R_UNREADABLE_LIST_N/A permissions are not enforced for this process");
        }
    }
    println!(
        "\nFCAD_30R_DESCRIPTOR_REFUSALS_EXECUTED\ncases={}",
        cases.len()
    );
}

/// A publication that fails — a list the format cannot hold, a folder that is a
/// file or cannot be written — leaves the previous list byte for byte and no
/// temporary file of its own; another process's temporary file, an interrupted
/// publication's leftover and unrelated files are never touched or read.
#[test]
fn a_failed_publication_keeps_the_previous_list_whole_and_touches_nothing_else() {
    let home = tempfile::tempdir().expect("home");
    let folder = Folder::at(&home.path().join("tabs")).expect("folder");
    let previous = set(&[absolute(&["work", "a.fcad"])], Some(0));
    folder.publish(&previous).expect("published");
    let kept = std::fs::read(folder.descriptor()).expect("bytes");
    // What another process, or one killed between its write and its rename, left.
    let theirs = folder
        .root()
        .join(".last-window.0190aaaa-0000-7000-8000-000000000000.partial");
    std::fs::write(&theirs, b"FERRITECAD-LAST-TABS 1\nplatform").expect("theirs");
    let unrelated = folder.root().join("notes.txt");
    std::fs::write(&unrelated, b"mine").expect("unrelated");
    let others = || {
        (
            std::fs::read(&theirs).expect("theirs"),
            std::fs::read(&unrelated).expect("unrelated"),
        )
    };
    let untouched = others();
    let before = snapshot(folder.root());
    assert!(
        replace_with_new(
            &theirs,
            &folder.descriptor(),
            &serialize(&previous).expect("valid previous list")
        )
        .is_err()
    );
    assert_eq!(
        snapshot(folder.root()),
        before,
        "exclusive create failed: no file belongs to this publication"
    );

    // A failure after our own write removes only our partial.
    let own_partial = folder.root().join("own.partial");
    let blocked_destination = folder.root().join("occupied-directory");
    std::fs::create_dir(&blocked_destination).expect("occupied destination");
    std::fs::write(blocked_destination.join("keep"), b"keep").expect("foreign bytes");
    assert!(replace_with_new(&own_partial, &blocked_destination, b"new list").is_err());
    assert!(!own_partial.exists());
    assert_eq!(
        std::fs::read(blocked_destination.join("keep")).expect("foreign bytes preserved"),
        b"keep"
    );
    assert_eq!(
        std::fs::read(folder.descriptor()).expect("old descriptor preserved"),
        kept
    );
    assert_eq!(
        folder.read().expect("read"),
        Some(previous.clone()),
        "a leftover is never read"
    );

    let long = PathBuf::from(format!(
        "{}{}",
        absolute(&[""]).display(),
        "a".repeat(MAX_PATH_HEX / UNIT_DIGITS + 1)
    ));
    let nine = vec![absolute(&["n.fcad"]); MAX_TABS + 1];
    for (name, refused) in [
        ("relative", set(&[PathBuf::from("a.fcad")], None)),
        ("too long", set(&[long], None)),
        ("too many", set(&nine, None)),
        (
            "shown file not listed",
            set(&[absolute(&["a.fcad"])], Some(1)),
        ),
    ] {
        let error = folder.publish(&refused).expect_err(name);
        assert_eq!(error.kind(), "refused", "{name}: {error}");
        assert_eq!(
            std::fs::read(folder.descriptor()).expect("kept"),
            kept,
            "{name}"
        );
        assert_eq!(
            partials(folder.root()),
            vec![theirs.file_name().expect("name").to_owned()]
        );
    }

    // A folder that is a file: nothing written anywhere.
    let blocked = home.path().join("blocked");
    std::fs::write(&blocked, b"a file").expect("file");
    let error = Folder::at(&blocked)
        .expect("folder")
        .publish(&previous)
        .expect_err("a file");
    assert_eq!(error.kind(), "foreign", "{error}");
    assert_eq!(std::fs::read(&blocked).expect("kept"), b"a file");
    // A folder under a file cannot be made.
    let under = Folder::at(&blocked.join("tabs")).expect("folder");
    assert_eq!(
        under.publish(&previous).expect_err("under a file").kind(),
        "io"
    );

    #[cfg(unix)]
    if modes_enforced(&home.path().join("probe")) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(folder.root(), std::fs::Permissions::from_mode(0o500))
            .expect("mode");
        let error = folder
            .publish(&set(&[absolute(&["b.fcad"])], None))
            .expect_err("a folder that cannot be written");
        std::fs::set_permissions(folder.root(), std::fs::Permissions::from_mode(0o700))
            .expect("mode");
        assert_eq!(error.kind(), "io", "{error}");
        assert_eq!(std::fs::read(folder.descriptor()).expect("kept"), kept);
    } else {
        println!("FCAD_30R_READ_ONLY_FOLDER_N/A permissions are not enforced for this process");
    }

    // A later publication succeeds beside the leftovers and leaves them alone.
    let next = set(&[absolute(&["work", "c.fcad"])], None);
    folder.publish(&next).expect("published");
    assert_eq!(folder.read().expect("read"), Some(next));
    assert_eq!(others(), untouched);
    assert_eq!(partials(folder.root()).len(), 1, "only the other process's");
    println!("\nFCAD_30R_FAILED_PUBLICATION_EXECUTED");
}

/// Paths keep their native units exactly: Unicode, spaces, a newline in a name,
/// and (Unix) bytes that are not UTF-8 or (Windows) an unpaired surrogate. A real
/// file with such a name is the file read back.
#[test]
fn paths_keep_their_native_units_exactly() {
    let home = tempfile::tempdir().expect("home");
    let folder = Folder::at(&home.path().join("tabs")).expect("folder");
    let user = home.path().join("Пла ста ü");
    std::fs::create_dir(&user).expect("user folder");
    let mut paths = vec![user.join(" a b .fcad"), user.join("日本語.fcad")];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        paths.push(user.join("new\nline.fcad"));
        let raw = OsString::from_vec(b"/data/caf\xe9 \xff.fcad".to_vec());
        assert!(raw.to_str().is_none(), "not UTF-8");
        paths.push(PathBuf::from(raw));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
        let mut wide: Vec<u16> = user.join("x").as_os_str().encode_wide().collect();
        wide.pop();
        wide.extend([0xd800, u16::from(b'.'), u16::from(b'f')]);
        let raw = OsString::from_wide(&wide);
        assert!(raw.to_str().is_none(), "not valid UTF-16");
        paths.push(PathBuf::from(raw));
    }
    let listed = set(&paths, Some(paths.len() - 1));
    folder.publish(&listed).expect("published");
    let read = folder.read().expect("read").expect("a list");
    assert_eq!(read, listed);
    for (written, back) in paths.iter().zip(&read.paths) {
        assert_eq!(written.as_os_str(), back.as_os_str(), "unit for unit");
    }
    // The files themselves: each name made on disk is found again by the path read.
    for path in &read.paths[..2] {
        std::fs::write(path, b"x").expect("made");
        assert_eq!(
            std::fs::read(path).expect("found by the path read back"),
            b"x"
        );
    }
    let native = read.paths.last().expect("native");
    match std::fs::write(home.path().join(native.file_name().expect("name")), b"y") {
        Ok(()) => println!("FCAD_30R_NATIVE_NAME_ON_DISK_EXECUTED"),
        // APFS and others refuse names that are not UTF-8: the round trip above
        // still held; the name on disk is N/A here, not a pass.
        Err(error) => println!("FCAD_30R_NATIVE_NAME_ON_DISK_N/A {error}"),
    }
    println!("\nFCAD_30R_NATIVE_PATHS_EXECUTED\npaths={}", paths.len());
}

// --- two processes ----------------------------------------------------------------

const CHILD_ROOT: &str = "FCAD_30R_CHILD_ROOT";
const CHILD_SET: &str = "FCAD_30R_CHILD_SET";
const ROUNDS: usize = 200;

/// The two lists the two processes publish: eight long paths, or three others.
fn child_list(which: &str) -> LastTabs {
    let long =
        |tag: &str, n: usize| absolute(&["p", &format!("{tag}{n}-{}.fcad", "x".repeat(900))]);
    match which {
        "eight" => set(
            &(0..8).map(|n| long("eight", n)).collect::<Vec<_>>(),
            Some(7),
        ),
        _ => set(
            &(0..3).map(|n| long("three", n)).collect::<Vec<_>>(),
            Some(0),
        ),
    }
}

/// One of the two processes: publishes its own list `ROUNDS` times into the shared
/// folder. Run only by the test below, as a child.
#[test]
#[ignore = "a child process of two_processes_publish_whole_lists_and_one_of_them_stays"]
fn publisher_child() {
    let root = std::env::var_os(CHILD_ROOT).expect("the parent names the folder");
    let which = std::env::var(CHILD_SET).expect("the parent names the list");
    let folder = Folder::at(Path::new(&root)).expect("folder");
    let list = child_list(&which);
    let mut refused = 0;
    for _ in 0..ROUNDS {
        if let Err(error) = folder.publish(&list) {
            // Windows may refuse a rename onto a file another process is renaming
            // at that moment; that is a failed publication (said, the old list
            // whole), never a mixed one. Elsewhere it is a failure of this test.
            assert!(cfg!(windows) && error.kind() == "io", "{error}");
            refused += 1;
        }
    }
    println!(
        "\nFCAD-30R-CHILD {which} published={} refused={refused}",
        ROUNDS - refused
    );
}

/// Two processes publish their own whole lists into one folder at once while this
/// one reads: every reading is nothing yet, or one of the two whole lists — never
/// a mix, a part or a damaged one — and the last publication stays. Each process's
/// temporary files are its own and none is left.
#[test]
fn two_processes_publish_whole_lists_and_one_of_them_stays() {
    let home = tempfile::tempdir().expect("home");
    let root = home.path().join("tabs");
    let folder = Folder::at(&root).expect("folder");
    let lists = [child_list("eight"), child_list("three")];
    let me = std::env::current_exe().expect("this test binary");
    let mut children: Vec<_> = ["eight", "three"]
        .into_iter()
        .map(|which| {
            std::process::Command::new(&me)
                .args([
                    "last_tabs::tests::publisher_child",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_ROOT, &root)
                .env(CHILD_SET, which)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("a child")
        })
        .collect();
    let (mut reads, mut seen, mut io) = (0, [0, 0], 0);
    while children
        .iter_mut()
        .any(|child| child.try_wait().expect("wait").is_none())
    {
        reads += 1;
        match folder.read() {
            Ok(None) => {}
            Ok(Some(read)) => {
                let which = lists
                    .iter()
                    .position(|list| *list == read)
                    .expect("a mixed or partial list was read");
                seen[which] += 1;
            }
            // Windows may refuse to open a file that is being renamed over; that
            // is a refusal said as such, never a mixed list.
            Err(error) => {
                assert!(
                    cfg!(windows) && error.kind() == "io",
                    "a reading during publication was refused: {error}"
                );
                io += 1;
            }
        }
    }
    let mut said = Vec::new();
    for child in children {
        let output = child.wait_with_output().expect("output");
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "{text}");
        assert!(
            text.contains("test result: ok. 1 passed; 0 failed"),
            "{text}"
        );
        said.push(
            text.lines()
                .find(|line| line.starts_with("FCAD-30R-CHILD"))
                .expect("the child said what it did")
                .to_owned(),
        );
    }
    let last = folder.read().expect("read").expect("a list");
    assert!(lists.contains(&last), "the last publication stays whole");
    assert!(
        partials(&root).is_empty(),
        "no temporary file of either is left"
    );
    assert!(
        seen[0] + seen[1] > 0,
        "the readings overlapped the publications"
    );
    println!(
        "\nFCAD_30R_TWO_PROCESSES_EXECUTED\nreads={reads} eight={} three={} io_refusals={io} {}",
        seen[0],
        seen[1],
        said.join(" ")
    );
}
