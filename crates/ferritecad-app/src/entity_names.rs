// SPDX-License-Identifier: MIT
use ferritecad_scene::FaceMeaning;

pub(crate) struct TopologyWords {
    pub(crate) feature: String,
    pub(crate) kind: String,
    pub(crate) digest: String,
    pub(crate) description: String,
}

pub(crate) fn topology_words(meaning: &FaceMeaning) -> TopologyWords {
    TopologyWords {
        feature: meaning.feature.to_string(),
        kind: meaning.kind.as_str().to_owned(),
        digest: meaning.digest.to_string(),
        description: meaning.description.clone(),
    }
}

pub(crate) fn topology_name(words: &TopologyWords) -> ferritecad_ui::TopologyName<'_> {
    ferritecad_ui::TopologyName {
        feature: &words.feature,
        kind: &words.kind,
        digest: &words.digest,
        description: &words.description,
    }
}
