// SPDX-License-Identifier: MIT
use ferritecad_document::{
    Document, EditChange, EndCondition, Expression, ExpressionSlot, FeatureAddition, SolidOperation,
};
use ferritecad_fixtures::feature_changes::{base_profile, circular_tool};
use ferritecad_types::{Dimension, ObjectId, Result, StableEntityId};
use std::path::Path;

pub struct Model {
    pub late: ObjectId,
    pub early: ObjectId,
    pub b: ObjectId,
    pub last: ObjectId,
    pub first: ObjectId,
}

pub fn write(path: &Path) -> Result<Model> {
    let chain = ferritecad_fixtures::write_fillet_chain(path, 1.5)?;
    let mut doc = Document::open(path)?;
    let mut last = chain.second;
    // Distinct pockets leave every earlier history and both corner rounds visible.
    for index in 0..28 {
        let cut = circular_tool(
            &doc,
            chain.body,
            [6. + (index % 7) as f64 * 6., 6. + (index / 7) as f64 * 6.],
            1.,
            blind(3.)?,
        )?;
        last = cut.feature;
        doc.write_changes(&cut.changes)?;
    }
    let (_, _, base) = base_profile(&doc, chain.body)?;
    let mut independent = Vec::new();
    for label in ["B", "C"] {
        let (sketch, body, first) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
        let mut profile = base.clone();
        for c in &mut profile.curves {
            c.id = StableEntityId::new();
        }
        doc.write_changes(&[
            EditChange::AddSketch {
                object: sketch,
                name: None,
                sketch: profile,
            },
            EditChange::AddBody {
                object: body,
                name: Some(label.into()),
                feature_object: first,
                feature_name: None,
                feature: FeatureAddition::Extrude {
                    profile: sketch,
                    regions: vec![],
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    end_condition: blind(10.)?,
                },
            },
        ])?;
        let cut = circular_tool(&doc, body, [24., 18.], 4., blind(3.)?)?;
        doc.write_changes(&cut.changes)?;
        independent.push(cut.feature);
    }
    let (early, late) = (ObjectId::new(), ObjectId::new());
    for (parameter, feature, name, value) in [
        (early, chain.extrude, "stock", "10 mm"),
        (late, last, "depth", "3 mm"),
    ] {
        doc.write_changes(&[
            EditChange::AddParameter {
                object: parameter,
                name: name.into(),
                dimension: Dimension::Length,
                expression: value.into(),
            },
            EditChange::SetExpressionText {
                object: feature,
                slot: ExpressionSlot::ExtrudeDistance,
                source: name.into(),
            },
        ])?;
    }
    doc.close()?;
    Ok(Model {
        early,
        late,
        b: independent[0],
        first: chain.first,
        last,
    })
}

fn blind(value: f64) -> Result<EndCondition> {
    Ok(EndCondition::Blind {
        distance: Expression::constant(value)?,
    })
}
