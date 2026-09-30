// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Frozen official Draft 2020-12 conformance corpus, with explicit local resources.
use purrassist_core::json::{Member, Node, Value, parse};
use purrassist_core::schema::Schema;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// A dialect declaring the format-assertion vocabulary (either boolean;
/// requirement 1 treats both as "known", and the suite's own
/// `format-assertion.json` fixture exercises that this makes no difference
/// in behavior). Already present in the vendored `remotes/` resources.
const FORMAT_ASSERTION_DIALECT: &str =
    "http://localhost:1234/draft2020-12/format-assertion-true.json";

/// Force `schema`'s `$schema` to [`FORMAT_ASSERTION_DIALECT`] so the
/// official `optional/format/*.json` fixtures (written against the plain
/// default dialect) exercise assertion, not just annotation.
fn under_format_assertion(schema: &Node) -> Node {
    let Value::Object(members) = &schema.value else {
        return schema.clone();
    };
    let mut members: Vec<Member> = members
        .iter()
        .filter(|member| member.key != "$schema")
        .cloned()
        .collect();
    members.insert(
        0,
        Member {
            key: "$schema".into(),
            key_span: None,
            value: Node::new(Value::String(FORMAT_ASSERTION_DIALECT.into())),
        },
    );
    Node::new(Value::Object(members))
}
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
fn load_resources(root: &Path) -> BTreeMap<String, String> {
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
    resources
}
fn run_suite(extended: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema");
    let resources = load_resources(&root);
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

/// Every file the vendored `optional/format/` manifest lists, enumerated
/// from the manifest itself (never the directory listing) so a fixture file
/// present on disk but missing from the manifest still fails this test.
fn format_fixture_files(root: &Path) -> Vec<PathBuf> {
    let manifest: Value =
        parse(&std::fs::read_to_string(root.join("manifest.json")).expect("format manifest"))
            .expect("manifest JSON")
            .value;
    let Value::Object(top) = manifest else {
        panic!("manifest object")
    };
    let files = top
        .into_iter()
        .find(|member| member.key == "files")
        .expect("files entry")
        .value;
    let Value::Object(entries) = files.value else {
        panic!("files object")
    };
    let mut paths: Vec<PathBuf> = entries
        .into_iter()
        .map(|member| member.key)
        .filter(|name| name.starts_with("tests/draft2020-12/optional/format/"))
        .map(|name| root.join(name))
        .collect();
    paths.sort();
    paths
}

/// Requirement 1/acceptance: every vendored `optional/format/*.json` file
/// (enumerated from the manifest; this test fails if any is not wired) plus
/// `optional/format-assertion.json`, run under a dialect declaring
/// format-assertion. Census asserted; zero skipped cases.
#[test]
fn formats_under_assertion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema");
    let resources = load_resources(&root);
    let format_files = format_fixture_files(&root);
    // The manifest and the on-disk directory must agree exactly, or a new
    // fixture could silently go unwired.
    let mut on_disk = std::fs::read_dir(root.join("tests/draft2020-12/optional/format"))
        .expect("format directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    on_disk.sort();
    assert_eq!(
        format_files, on_disk,
        "optional/format/ manifest and directory listing disagree"
    );
    let mut total = 0;
    let mut failures = Vec::new();
    for path in format_files.into_iter().chain(std::iter::once(
        root.join("tests/draft2020-12/optional/format-assertion.json"),
    )) {
        // `unknown.json` and `format-assertion.json` each test the
        // no-assertion/vocabulary-driven contract on their own declared
        // dialect; every other fixture assumes assertion is active (that is
        // the only reading under which its mixed valid/invalid expectations
        // are meaningful at all).
        let force_assertion =
            !path.ends_with("format-assertion.json") && !path.ends_with("unknown.json");
        let suite =
            parse(&std::fs::read_to_string(&path).expect("format suite")).expect("suite JSON");
        let Value::Array(groups) = suite.value else {
            panic!("suite array")
        };
        for group in groups {
            let description = group
                .get("description")
                .and_then(Node::as_str)
                .expect("description");
            let mut schema_node = group.get("schema").expect("schema").clone();
            if force_assertion {
                schema_node = under_format_assertion(&schema_node);
            }
            let source = schema_node.to_json();
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
                    // counted below via `total - failures.len()`
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
    assert_eq!(total, 878, "format census changed");
    for failure in failures.iter().take(80) {
        eprintln!("{failure}");
    }
    assert!(
        failures.is_empty(),
        "formats under assertion: {}/{total} passed, {} failures",
        total - failures.len(),
        failures.len()
    );
}
