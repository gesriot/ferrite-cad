// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{EndCondition, Expression};
use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};

fn output(path: &Path, command: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg(command)
        .arg(path)
        .args(args)
        .output()?;
    assert!(result.status.success(), "{result:?}");
    Ok(String::from_utf8(result.stdout).expect("UTF-8 output"))
}

fn marker(body: ObjectId, feature: Option<ObjectId>) -> EditChange {
    EditChange::SetRollback {
        object: body,
        feature,
    }
}

#[test]
fn native_rollback_cli_edits_inspects_rebuilds_and_exports_the_shown_result() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("full.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let mut doc = Document::open(&path)?;
    let body = body(&doc)?;
    let (base, sketch, _) = base_profile(&doc, body)?;
    let short = root.path().join("short.fcad");
    doc.snapshot_to(&short)?;
    let cut = circular_tool(
        &doc,
        body,
        [20., 20.],
        2.,
        EndCondition::Blind {
            distance: Expression::constant(3.)?,
        },
    )?;
    doc.write_changes(&cut.changes)?;
    doc.close()?;
    let marked = root.path().join("marked.fcad");
    accepted(edit(&path, &marked, &[marker(body, Some(base))])?);
    let json: serde_json::Value =
        serde_json::from_str(&output(&marked, "inspect", &["--json"])?).expect("JSON inspect");
    assert_eq!(json["result"]["bodies"][0]["rollback"], base.to_string());
    assert_eq!(
        json["result"]["bodies"][0]["history"],
        serde_json::json!([base, cut.feature])
    );
    let text = output(&marked, "inspect", &[])?;
    let body_heading = format!("  {body} {:<16} Plate", "body");
    let rows = text.lines().collect::<Vec<_>>();
    let at = rows
        .iter()
        .position(|row| *row == body_heading)
        .expect("Body heading");
    assert_eq!(
        rows[at + 1],
        format!("      tip feature {}, rollback {base}", cut.feature)
    );
    let text = output(&marked, "rebuild", &["--cold"])?;
    let row = text
        .lines()
        .find(|row| row.split_whitespace().next() == Some(&body.to_string()))
        .expect("Body rebuild row");
    assert_eq!(
        row,
        format!(
            "  {body}  {:<12}  {:<16}Built; tip Cut; shows Extrude1; 1 solid",
            "Plate", "body"
        )
    );
    let mut stls = Vec::new();
    for (source, name) in [(&marked, "marked.stl"), (&short, "short.stl")] {
        let stl = root.path().join(name);
        output(
            source,
            "export-stl",
            &["--output", stl.to_str().expect("path")],
        )?;
        stls.push(std::fs::read(stl)?);
    }
    assert_eq!(
        stls[0], stls[1],
        "the shown result exports byte-identically to the short history"
    );
    let cleared = root.path().join("cleared.fcad");
    accepted(edit(&marked, &cleared, &[marker(body, None)])?);
    let json: serde_json::Value =
        serde_json::from_str(&output(&cleared, "inspect", &["--json"])?).expect("JSON inspect");
    assert!(json["result"]["bodies"][0].get("rollback").is_none());
    equivalent(&facts(&path)?, &facts(&cleared)?);
    for (index, change, expected) in [
        (
            0,
            marker(sketch, Some(base)),
            format!(
                "invalid input: sketch 'Profile' ({sketch}) is not a Body; \
                 only a Body can be rolled back."
            ),
        ),
        (
            1,
            marker(body, Some(sketch)),
            format!(
                "invalid input: sketch 'Profile' ({sketch}) is not in the history of \
                 body 'Plate' ({body})."
            ),
        ),
        (
            2,
            marker(body, Some(cut.feature)),
            format!(
                "invalid input: Extrude 'Cut' ({}) is the tip of body 'Plate' ({body}); \
             use clear_rollback to show the end of its history.",
                cut.feature
            ),
        ),
    ] {
        let refused = edit(
            &path,
            &root.path().join(format!("refused-{index}.fcad")),
            &[change],
        )?;
        assert_eq!(refused["ok"], false);
        assert_eq!(refused["error"]["kind"], "input");
        assert_eq!(refused["error"]["message"], expected);
    }
    let mut reference = Document::open(&short)?;
    let fillet =
        ferritecad_fixtures::feature_changes::edge_addition(&reference, body, 0, 2., false)?;
    reference.write_changes(std::slice::from_ref(&fillet))?;
    reference.write_changes(&cut.changes)?;
    reference.close()?;
    let inserted = root.path().join("inserted.fcad");
    accepted(edit(
        &path,
        &inserted,
        &[marker(body, Some(base)), fillet, marker(body, None)],
    )?);
    equivalent(&facts(&inserted)?, &facts(&short)?);
    Ok(())
}
