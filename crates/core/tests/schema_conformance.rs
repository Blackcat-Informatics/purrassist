// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Frozen official Draft 2020-12 conformance corpus, with explicit local resources.
use purrassist_core::json::{Node, Value, parse};
use purrassist_core::schema::Schema;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
fn files(directory: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory).expect("fixture directory") {
        let path = entry.expect("fixture entry").path();
        if path.is_dir() {
            paths.extend(files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}
#[test]
fn draft_2020_12() {
    run_suite(false);
}
#[test]
fn extended_2020_12_constraints() {
    run_suite(true);
}
fn run_suite(extended: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema");
    let mut resources = BTreeMap::new();
    for path in files(&root.join("remotes")) {
        let relative = path
            .strip_prefix(root.join("remotes"))
            .expect("relative path")
            .to_string_lossy()
            .replace('\\', "/");
        resources.insert(
            format!("http://localhost:1234/{relative}"),
            std::fs::read_to_string(path).expect("resource"),
        );
    }
    let mut failures = Vec::new();
    let mut total = 0;
    let mut passed = 0;
    let mut paths: Vec<_> = std::fs::read_dir(root.join("tests/draft2020-12"))
        .expect("suite")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    if extended {
        paths = [
            "bignum",
            "float-overflow",
            "ecmascript-regex",
            "non-bmp-regex",
            "refOfUnknownKeyword",
            "anchor",
            "dynamicRef",
            "id",
            "no-schema",
            "unknownKeyword",
        ]
        .iter()
        .map(|name| root.join(format!("tests/draft2020-12/optional/{name}.json")))
        .collect();
    }
    for path in paths {
        let suite =
            parse(&std::fs::read_to_string(&path).expect("suite file")).expect("suite JSON");
        let Value::Array(groups) = suite.value else {
            panic!("suite array")
        };
        for group in groups {
            let description = group
                .get("description")
                .and_then(Node::as_str)
                .expect("description");
            let source = group.get("schema").expect("schema").to_json();
            let compiled = Schema::with_resources(&source, &resources);
            let Value::Array(tests) = &group.get("tests").expect("tests").value else {
                panic!("test array")
            };
            for test in tests {
                total += 1;
                let expected = test.get("valid").expect("expectation").value == Value::Bool(true);
                let actual = compiled
                    .as_ref()
                    .map_err(ToString::to_string)
                    .and_then(|schema| {
                        schema
                            .validate(test.get("data").expect("instance"))
                            .map(|report| report.valid)
                            .map_err(|error| error.to_string())
                    });
                if actual == Ok(expected) {
                    passed += 1;
                } else {
                    failures.push(format!(
                        "{} / {} / {}: expected {}, got {:?}",
                        path.file_name().expect("name").to_string_lossy(),
                        description,
                        test.get("description").and_then(Node::as_str).unwrap_or(""),
                        expected,
                        actual
                    ));
                }
            }
        }
    }
    assert_eq!(
        total,
        if extended { 121 } else { 1301 },
        "frozen suite census changed"
    );
    for failure in failures.iter().take(60) {
        eprintln!("{failure}");
    }
    assert!(
        failures.is_empty(),
        "Draft 2020-12: {passed}/{total} passed, {} failures",
        failures.len()
    );
}
