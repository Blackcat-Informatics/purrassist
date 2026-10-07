// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Frozen official Draft 2020-12 and Draft-07 conformance corpora, with
//! explicit local resources.
use purrassist_core::json::{Member, Node, Value, parse};
use purrassist_core::schema::Schema;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// A dialect declaring the format-assertion vocabulary (either boolean;
/// requirement 1 treats both as "known", and the suite's own
/// `format-assertion.json` fixture exercises that this makes no difference
/// in behavior). Already present in the vendored `remotes/` resources.
const FORMAT_ASSERTION_DIALECT: &str =
    "http://localhost:1234/draft2020-12/format-assertion-true.json";

/// The Draft-07 dialect, injected into every Draft-07 suite schema object:
/// the suite's schemas omit `$schema`, and this engine's default is
/// Draft 2020-12. (A boolean root schema has no keywords to interpret, so it
/// behaves identically under either dialect.)
const DRAFT_07_DIALECT: &str = "http://json-schema.org/draft-07/schema#";

/// Required Draft-07 groups (`file`, `group description`, reason) allowed to
/// fail. Every other required group must pass completely, and every listed
/// group must still fail (a stale entry fails the test), so the measured
/// pass count cannot silently regress or silently improve.
const DRAFT_07_EXCLUDED: &[(&str, &str, &str)] = &[];

/// Optional Draft-07 groups allowed to fail, with the same stale-entry rule.
const DRAFT_07_OPTIONAL_EXCLUDED: &[(&str, &str, &str)] = &[
    (
        "content.json",
        "validation of string-encoded content based on media type",
        "contentMediaType is an annotation (Draft-07 makes content assertion optional; Draft 2020-12 likewise annotates)",
    ),
    (
        "content.json",
        "validation of binary string-encoding",
        "contentEncoding is an annotation, consistent with the Draft 2020-12 content vocabulary",
    ),
    (
        "content.json",
        "validation of binary-encoded media type documents",
        "contentEncoding/contentMediaType are annotations, consistent with Draft 2020-12",
    ),
    (
        "cross-draft.json",
        "refs to future drafts are processed as future drafts",
        "Draft 2019-09 is not an implemented dialect; its $schema is an unresolved dialect error",
    ),
];

/// Replace `schema`'s `$schema` with `dialect` (a boolean schema is
/// returned unchanged).
fn with_dialect(schema: &Node, dialect: &str) -> Node {
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
            value: Node::new(Value::String(dialect.into())),
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
/// The `.json` files directly inside `directory`, sorted.
fn suite_files(directory: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .expect("suite")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths
}
fn suite_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema")
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

/// One failed case: file name, group description, and a full diagnostic.
struct Failure {
    file: String,
    group: String,
    line: String,
}

/// Run every case of every file in `paths`. `prepare` rewrites a group's
/// schema (given its file) before compilation; `judge` turns a compiled
/// schema and instance into the observed validity. Returns the case count
/// and every case whose observation differs from the suite's expectation.
fn run_cases(
    paths: &[PathBuf],
    resources: &BTreeMap<String, String>,
    prepare: impl Fn(&Path, &Node) -> Node,
    judge: impl Fn(&Schema, &Node) -> Result<bool, String>,
) -> (usize, Vec<Failure>) {
    let mut total = 0;
    let mut failures = Vec::new();
    for path in paths {
        let file = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .to_string();
        let suite = parse(&std::fs::read_to_string(path).expect("suite file")).expect("suite JSON");
        let Value::Array(groups) = suite.value else {
            panic!("suite array")
        };
        for group in groups {
            let description = group
                .get("description")
                .and_then(Node::as_str)
                .expect("description");
            let source = prepare(path, group.get("schema").expect("schema")).to_json();
            let compiled = Schema::with_resources(&source, resources);
            let Value::Array(tests) = &group.get("tests").expect("tests").value else {
                panic!("test array")
            };
            for test in tests {
                total += 1;
                let expected = test.get("valid").expect("expectation").value == Value::Bool(true);
                let actual = compiled
                    .as_ref()
                    .map_err(ToString::to_string)
                    .and_then(|schema| judge(schema, test.get("data").expect("instance")));
                if actual != Ok(expected) {
                    failures.push(Failure {
                        file: file.clone(),
                        group: description.to_owned(),
                        line: format!(
                            "{file} / {description} / {}: expected {expected}, got {actual:?}",
                            test.get("description").and_then(Node::as_str).unwrap_or(""),
                        ),
                    });
                }
            }
        }
    }
    (total, failures)
}

/// Observed validity: the report's verdict.
fn verdict(schema: &Schema, instance: &Node) -> Result<bool, String> {
    schema
        .validate(instance)
        .map(|report| report.valid)
        .map_err(|error| error.to_string())
}

/// Observed validity including annotation-only `format` results: the
/// verdict holds and every recorded format evaluation passed.
fn verdict_with_formats(schema: &Schema, instance: &Node) -> Result<bool, String> {
    schema
        .validate(instance)
        .map(|report| report.valid && report.format_evidence.iter().all(|entry| entry.passed))
        .map_err(|error| error.to_string())
}

fn report_failures(label: &str, total: usize, failures: &[Failure]) {
    for failure in failures.iter().take(80) {
        eprintln!("{}", failure.line);
    }
    assert!(
        failures.is_empty(),
        "{label}: {}/{total} passed, {} failures",
        total - failures.len(),
        failures.len()
    );
}

fn run_suite(extended: bool) {
    let root = suite_root();
    let resources = load_resources(&root);
    let paths = if extended {
        [
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
        .collect()
    } else {
        suite_files(&root.join("tests/draft2020-12"))
    };
    let (total, failures) = run_cases(&paths, &resources, |_, schema| schema.clone(), verdict);
    assert_eq!(
        total,
        if extended { 121 } else { 1301 },
        "frozen suite census changed"
    );
    report_failures("Draft 2020-12", total, &failures);
}

/// Every file the vendored `optional/format/` manifest lists for `draft`,
/// enumerated from the manifest itself (never the directory listing) so a
/// fixture file present on disk but missing from the manifest still fails.
/// The manifest and the on-disk directory must also agree exactly.
fn format_fixture_files(root: &Path, draft: &str) -> Vec<PathBuf> {
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
    let prefix = format!("tests/{draft}/optional/format/");
    let mut paths: Vec<PathBuf> = entries
        .into_iter()
        .map(|member| member.key)
        .filter(|name| name.starts_with(&prefix))
        .map(|name| root.join(name))
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        suite_files(&root.join(&prefix)),
        "{prefix} manifest and directory listing disagree"
    );
    paths
}

/// Requirement 1/acceptance: every vendored `optional/format/*.json` file
/// (enumerated from the manifest; this test fails if any is not wired) plus
/// `optional/format-assertion.json`, run under a dialect declaring
/// format-assertion. Census asserted; zero skipped cases.
#[test]
fn formats_under_assertion() {
    let root = suite_root();
    let resources = load_resources(&root);
    let mut paths = format_fixture_files(&root, "draft2020-12");
    paths.push(root.join("tests/draft2020-12/optional/format-assertion.json"));
    // `unknown.json` and `format-assertion.json` each test the
    // no-assertion/vocabulary-driven contract on their own declared
    // dialect; every other fixture assumes assertion is active (that is
    // the only reading under which its mixed valid/invalid expectations
    // are meaningful at all).
    let (total, failures) = run_cases(
        &paths,
        &resources,
        |path, schema| {
            if path.ends_with("format-assertion.json") || path.ends_with("unknown.json") {
                schema.clone()
            } else {
                with_dialect(schema, FORMAT_ASSERTION_DIALECT)
            }
        },
        verdict,
    );
    assert_eq!(total, 878, "format census changed");
    report_failures("formats under assertion", total, &failures);
}

/// Check `failures` against an exclusion list: only listed groups fail, and
/// every listed group still fails. Returns the number of passing cases.
fn check_exclusions(
    label: &str,
    total: usize,
    failures: &[Failure],
    excluded: &[(&str, &str, &str)],
) -> usize {
    let allowed: BTreeSet<(&str, &str)> = excluded
        .iter()
        .map(|(file, group, _)| (*file, *group))
        .collect();
    let unexpected: Vec<&Failure> = failures
        .iter()
        .filter(|failure| !allowed.contains(&(failure.file.as_str(), failure.group.as_str())))
        .collect();
    for failure in &unexpected {
        eprintln!("{}", failure.line);
    }
    assert!(
        unexpected.is_empty(),
        "{label}: {} failures outside the exclusion list",
        unexpected.len()
    );
    let failing: BTreeSet<(&str, &str)> = failures
        .iter()
        .map(|failure| (failure.file.as_str(), failure.group.as_str()))
        .collect();
    let stale: Vec<_> = allowed.difference(&failing).collect();
    assert!(stale.is_empty(), "{label}: stale exclusions {stale:?}");
    total - failures.len()
}

/// The required Draft-07 suite, each schema under the Draft-07 dialect.
#[test]
fn draft_07() {
    let root = suite_root();
    let resources = load_resources(&root);
    let paths = suite_files(&root.join("tests/draft7"));
    let (total, failures) = run_cases(
        &paths,
        &resources,
        |_, schema| with_dialect(schema, DRAFT_07_DIALECT),
        verdict,
    );
    assert_eq!(total, 929, "frozen Draft-07 suite census changed");
    let passed = check_exclusions("Draft-07", total, &failures, DRAFT_07_EXCLUDED);
    assert!(passed >= 929, "Draft-07: {passed}/{total} passed");
}

/// Draft-07 `optional/`: every non-format file under the Draft-07 dialect,
/// plus every `optional/format/` file judged by the annotation results
/// (Draft-07 `format` is an annotation, so a case is valid exactly when the
/// verdict holds and every recorded format evaluation passed).
#[test]
fn draft_07_optional() {
    let root = suite_root();
    let resources = load_resources(&root);
    let prepare = |_: &Path, schema: &Node| with_dialect(schema, DRAFT_07_DIALECT);
    let (plain_total, mut failures) = run_cases(
        &suite_files(&root.join("tests/draft7/optional")),
        &resources,
        prepare,
        verdict,
    );
    let (format_total, format_failures) = run_cases(
        &format_fixture_files(&root, "draft7"),
        &resources,
        prepare,
        verdict_with_formats,
    );
    failures.extend(format_failures);
    let total = plain_total + format_total;
    assert_eq!(total, 911, "frozen Draft-07 optional census changed");
    let passed = check_exclusions(
        "Draft-07 optional",
        total,
        &failures,
        DRAFT_07_OPTIONAL_EXCLUDED,
    );
    assert!(passed >= 905, "Draft-07 optional: {passed}/{total} passed");
}
