// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Reference label changes preserve display text, alt text and link semantics.
use purrassist_core::{
    Budget, Governor, Outcome,
    markdown::{Dialect, Document},
};

#[test]
fn relabel_preserves_shortcut_collapsed_full_and_image_rendering_and_inverse() {
    for newline in ["\n", "\r\n"] {
        let source = "Before [tea] [tea][] [*text*][tea] ![tea] ![tea][] ![alt][tea].\n\n[tea]: /tea \"Title\"\n".replace('\n', newline);
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(&source, &mut governor).unwrap();
        let site = document.reference_sites(&mut governor).unwrap().remove(0);
        let plan = document
            .relabel_reference(&site.path, site.occurrence, "chai", &mut governor)
            .unwrap();
        let Outcome::Resolved(applied) = plan.apply(&source, Budget::default()).outcome else {
            panic!("apply")
        };
        assert_eq!(applied.text, "Before [tea][chai] [tea][chai] [*text*][chai] ![tea][chai] ![tea][chai] ![alt][chai].\n\n[chai]: /tea \"Title\"\n".replace('\n', newline));
        let updated = Document::parse(&applied.text, &mut governor).unwrap();
        for dialect in [Dialect::CommonMark, Dialect::Gfm] {
            assert_eq!(
                document.render_html(dialect, &mut governor).unwrap(),
                updated.render_html(dialect, &mut governor).unwrap()
            );
        }
        let inverse = plan.invert(&source, &mut governor).unwrap();
        let Outcome::Resolved(restored) = inverse.apply(&applied.text, Budget::default()).outcome
        else {
            panic!("inverse")
        };
        assert_eq!(restored.text, source);
    }
}

#[test]
fn relabel_preserves_escaped_and_emphasized_display_text() {
    let source = "[t\\*ea][] ![t\\*ea] [**text**][t\\*ea]\n\n[t\\*ea]: /tea\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    let site = document.reference_sites(&mut governor).unwrap().remove(0);
    let plan = document
        .relabel_reference(&site.path, 0, "chai", &mut governor)
        .unwrap();
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    let updated = Document::parse(&applied.text, &mut governor).unwrap();
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
        updated.render_html(Dialect::Gfm, &mut governor).unwrap()
    );
    assert!(applied.text.contains("[t\\*ea][chai]"));
}

#[test]
fn relabel_rejects_collisions_and_newly_activated_unrelated_references() {
    for source in [
        "[tea] [chai]\n\n[tea]: /tea\n[chai]: /other\n",
        "[tea] [chai]\n\n[tea]: /tea\n",
    ] {
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(source, &mut governor).unwrap();
        let site = document.reference_sites(&mut governor).unwrap().remove(0);
        assert!(
            document
                .relabel_reference(&site.path, 0, "chai", &mut governor)
                .is_err()
        );
        assert_eq!(document.source(), source);
    }
}
