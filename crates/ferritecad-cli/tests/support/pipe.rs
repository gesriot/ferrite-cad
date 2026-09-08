// SPDX-License-Identifier: MIT
use std::process::Stdio;

pub fn closed_pipe() -> Stdio {
    // A direct OS pipe whose only reader is already closed. Converting a
    // ChildStdin to Stdio on Windows inserts a relay thread and another pipe:
    // that live relay can accept bytes after the ultimate reader has exited.
    // PipeWriter goes directly to the child, with no intermediate reader.
    let (reader, writer) = std::io::pipe().expect("OS pipe");
    drop(reader);
    Stdio::from(writer)
}
