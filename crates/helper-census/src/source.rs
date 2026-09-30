// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The walk over first-party, non-test Rust source, and the symbol index it builds.
//!
//! Packages are the directories under `crates/` and `bindings/` that hold a
//! `Cargo.toml`. Each package's library and binary roots (and its `build.rs`) are
//! parsed and their `mod` declarations followed, exactly as rustc would reach them,
//! so the module path of every item is the one callers write. What the walk never
//! enters:
//!
//! * an item or module under a test-only `#[cfg]` (`test`, or an `all(…)` holding
//!   it) and every `#[test]` function;
//! * a file under a `tests/`, `benches/`, `examples/` or `generated/` directory of
//!   its package, however it is reached.
//!
//! Every function, constant and static that survives becomes a [`Unit`] with its
//! fingerprints. Every named item becomes an index entry, and every `use` an alias,
//! so a ledger path such as `purrdf_iri::Iri::resolve` resolves through the
//! re-export to the item that defines it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use purrassist_core::json::{Node, Value as JsonValue};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use proc_macro2::TokenStream;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::normalize::{self, BodyPrint};

/// Directory names whose contents are never shipping source.
const EXCLUDED_DIRS: [&str; 4] = ["tests", "benches", "examples", "generated"];

/// Where the source comes from: the repository on disk, or an in-memory tree in
/// the self-test and the unit tests.
pub(crate) trait Tree {
    /// The text of the file at `path` (repo-relative, `/`-separated).
    fn read(&self, path: &str) -> Option<String>;
    /// The entries of the directory at `path`: `(name, is_directory)`, sorted.
    fn list(&self, path: &str) -> Vec<(String, bool)>;
    /// Parse a package manifest, retaining failures rather than skipping packages.
    fn manifest(&self, _path: &str, text: &str) -> Result<Manifest, String> {
        Manifest::read(text)
    }
    /// Recorded read, directory and manifest validation failures.
    fn errors(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The repository on disk. Filesystem failures are retained until the walk ends.
#[derive(Debug)]
pub(crate) struct Disk {
    root: PathBuf,
    errors: RefCell<Vec<String>>,
}
impl Disk {
    /// Construct a source tree rooted in an existing directory.
    pub(crate) const fn new(root: PathBuf) -> Self {
        Self {
            root,
            errors: RefCell::new(Vec::new()),
        }
    }
    fn record(&self, path: &str, error: impl std::fmt::Display) {
        self.errors.borrow_mut().push(format!("{path}: {error}"));
    }
}
impl Tree for Disk {
    fn read(&self, path: &str) -> Option<String> {
        match std::fs::read_to_string(self.root.join(path)) {
            Ok(text) => Some(text),
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    self.record(path, error);
                }
                None
            }
        }
    }
    fn list(&self, path: &str) -> Vec<(String, bool)> {
        let entries = match std::fs::read_dir(self.root.join(path)) {
            Ok(entries) => entries,
            Err(error) => {
                // bindings and automatically discovered binary directories may be absent.
                if path == "crates" || error.kind() != std::io::ErrorKind::NotFound {
                    self.record(path, error);
                }
                return Vec::new();
            }
        };
        let mut listed = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.record(path, error);
                    continue;
                }
            };
            let Ok(name) = entry.file_name().into_string() else {
                self.record(path, "non-UTF-8 directory entry");
                continue;
            };
            match entry.file_type() {
                Ok(kind) if kind.is_symlink() => self.record(
                    &format!("{path}/{name}"),
                    "symlink entries require an explicit source layout",
                ),
                Ok(kind) => listed.push((name, kind.is_dir())),
                Err(error) => self.record(&format!("{path}/{name}"), error),
            }
        }
        listed.sort();
        listed
    }
    fn manifest(&self, path: &str, _text: &str) -> Result<Manifest, String> {
        // Cargo owns the complete manifest grammar and target discovery. This is
        // a read-only command: it neither resolves dependencies nor compiles code.
        let output = Command::new("cargo")
            .args(["read-manifest", "--manifest-path"])
            .arg(self.root.join(path))
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            // A valid virtual manifest owns no targets. Cargo metadata validates
            // its full syntax and declared members rather than silently skipping it.
            let virtual_output = Command::new("cargo")
                .args([
                    "metadata",
                    "--no-deps",
                    "--locked",
                    "--format-version",
                    "1",
                    "--manifest-path",
                ])
                .arg(self.root.join(path))
                .output()
                .map_err(|error| error.to_string())?;
            if virtual_output.status.success() {
                return Ok(Manifest::default());
            }
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let text = std::str::from_utf8(&output.stdout).map_err(|error| error.to_string())?;
        let document = purrassist_core::json::parse(text)
            .map_err(|error| format!("Cargo manifest JSON: {error:?}"))?;
        Manifest::from_cargo(&document, &self.root)
    }
    fn errors(&self) -> Vec<String> {
        self.errors.borrow().clone()
    }
}

/// An in-memory tree: repo-relative path to text.
#[derive(Debug, Default)]
pub(crate) struct Memory {
    /// Every file, by repo-relative path.
    pub(crate) files: BTreeMap<String, String>,
}

impl Tree for Memory {
    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn list(&self, path: &str) -> Vec<(String, bool)> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let mut listed = BTreeSet::new();
        for file in self.files.keys() {
            if let Some(rest) = file.strip_prefix(&prefix) {
                match rest.split_once('/') {
                    Some((directory, _)) => listed.insert((directory.to_owned(), true)),
                    None => listed.insert((rest.to_owned(), false)),
                };
            }
        }
        listed.into_iter().collect()
    }
}

/// What kind of unit a fingerprinted item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnitKind {
    /// A function or method with a body.
    Function,
    /// A `const` or `static` item.
    Constant,
}

/// One fingerprinted item of shipping source.
#[derive(Clone, Debug)]
pub(crate) struct Unit {
    /// Its resolved symbol path.
    pub(crate) symbol: String,
    /// Its own name (the last path segment).
    pub(crate) name: String,
    /// The package that defines it.
    pub(crate) package: String,
    /// The repo-relative file.
    pub(crate) file: String,
    /// The 1-based line of its name.
    pub(crate) line: usize,
    /// Function or constant.
    pub(crate) kind: UnitKind,
    /// A function body's fingerprints.
    pub(crate) print: Option<BodyPrint>,
    /// Every integer literal it holds, by value.
    pub(crate) constants: BTreeSet<u128>,
    /// The hex-digit tables it holds.
    pub(crate) tables: BTreeSet<&'static str>,
    /// Every string literal it holds, decoded, with its 1-based line.
    pub(crate) strings: Vec<(String, usize)>,
}

/// One named item in the index.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    /// The repo-relative file.
    pub(crate) file: String,
    /// The 1-based line of its name.
    pub(crate) line: usize,
    /// The package that defines it.
    pub(crate) package: String,
    /// The module it lives in (a method's module is its type's).
    pub(crate) module: String,
    /// Whether it carries documentation.
    pub(crate) documented: bool,
}

/// A `use` as written, resolved lazily once every module is known.
#[derive(Clone, Debug)]
struct Import {
    /// The module that holds the `use`.
    module: String,
    /// The imported path as written.
    target: Vec<String>,
}

/// An impl-block item whose symbol depends on where its self type is defined.
#[derive(Clone, Debug)]
struct Pending {
    module: String,
    self_type: Vec<String>,
    name: String,
    definition: Definition,
    unit: Option<usize>,
}

/// The walk's result.
#[derive(Debug, Default)]
pub(crate) struct Workspace {
    /// Every fingerprinted unit, in walk order.
    pub(crate) units: Vec<Unit>,
    /// Every named item by symbol path.
    pub(crate) definitions: BTreeMap<String, Definition>,
    /// `module::name` introduced by a `use`, to what it names.
    imports: BTreeMap<String, Import>,
    /// Glob imports per module.
    globs: BTreeMap<String, Vec<Import>>,
    /// Every crate root ident the walk entered.
    pub(crate) crates: BTreeSet<String>,
    /// Files the walk could not parse (a hard failure for the caller).
    pub(crate) errors: Vec<String>,
    /// Every census rule hit over every first-party `.rs` file, tests,
    /// benches and examples included.
    pub(crate) rule_hits: Vec<crate::rules::RuleHit>,
    pending: Vec<Pending>,
}

impl Workspace {
    /// Walk every package under `crates/` and `bindings/` of `tree`.
    pub(crate) fn walk(tree: &dyn Tree) -> Self {
        let mut workspace = Self::default();
        for parent in ["crates", "bindings"] {
            for (name, is_dir) in tree.list(parent) {
                let package_dir = format!("{parent}/{name}");
                if is_dir && let Some(manifest) = tree.read(&format!("{package_dir}/Cargo.toml")) {
                    workspace.walk_package(tree, &package_dir, &manifest);
                }
            }
        }
        workspace.settle();
        workspace.errors.extend(tree.errors());
        workspace
    }

    fn walk_package(&mut self, tree: &dyn Tree, package_dir: &str, manifest: &str) {
        let manifest = match tree.manifest(&format!("{package_dir}/Cargo.toml"), manifest) {
            Ok(manifest) => manifest,
            Err(error) => {
                self.errors
                    .push(format!("{package_dir}/Cargo.toml: {error}"));
                return;
            }
        };
        let Some(package) = manifest.package.clone() else {
            // A virtual manifest has no source of its own.
            return;
        };
        self.scan_rules(tree, &package, package_dir, package_dir);
        for (ident, root) in manifest.roots(tree, package_dir, &package) {
            self.crates.insert(ident.clone());
            let context = Context {
                tree,
                package: &package,
                package_dir,
            };
            let directory = parent_dir(&root);
            self.walk_file(&context, &root, &ident, &directory);
        }
    }

    /// Run the census rules over every `.rs` file under `directory`, whatever
    /// compiles it: a test's randomly seeded map is as nondeterministic as a
    /// library's. The byte-layout rules read shipping code only: a file under
    /// one of the package's test, bench, example or generated directories is a
    /// test's independent oracle, not a second implementation.
    fn scan_rules(&mut self, tree: &dyn Tree, package: &str, package_dir: &str, directory: &str) {
        for (name, is_dir) in tree.list(directory) {
            let path = format!("{directory}/{name}");
            if is_dir {
                if name != "target" && !name.starts_with('.') {
                    self.scan_rules(tree, package, package_dir, &path);
                }
                continue;
            }
            if Path::new(&name)
                .extension()
                .is_none_or(|extension| extension != "rs")
            {
                continue;
            }
            let Some(source) = tree.read(&path) else {
                self.errors
                    .push(format!("{path}: listed Rust source could not be read"));
                continue;
            };
            match syn::parse_file(&source) {
                Ok(parsed) => {
                    self.rule_hits
                        .extend(crate::rules::std_default_hasher(package, &path, &parsed));
                    self.rule_hits
                        .extend(crate::rules::hex_rules(package, &path, &parsed));
                    self.rule_hits
                        .extend(crate::rules::lex_rules(package, &path, &parsed));
                    if !is_excluded(package_dir, &path) {
                        self.rule_hits
                            .extend(crate::layout::layout_rules(package, &path, &parsed));
                    }
                }
                Err(error) => self.errors.push(format!("{path}: {error}")),
            }
        }
    }

    fn walk_file(&mut self, context: &Context<'_>, file: &str, module: &str, directory: &str) {
        if is_excluded(context.package_dir, file) {
            return;
        }
        let Some(source) = context.tree.read(file) else {
            self.errors
                .push(format!("{file}: declared as a module but not found"));
            return;
        };
        let parsed = match syn::parse_file(&source) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.errors.push(format!("{file}: {error}"));
                return;
            }
        };
        let place = Place {
            file,
            source: &source,
            module,
            directory,
        };
        self.walk_items(context, &place, &parsed.items);
    }

    fn walk_items(&mut self, context: &Context<'_>, place: &Place<'_>, items: &[syn::Item]) {
        for item in items {
            if is_test_only(item_attrs(item)) {
                continue;
            }
            match item {
                syn::Item::Fn(function) => {
                    let unit = Self::function_unit(
                        context,
                        place,
                        &function.sig,
                        &function.block,
                        format!("{}::{}", place.module, function.sig.ident),
                    );
                    self.define(
                        context,
                        place,
                        &function.sig.ident,
                        &function.attrs,
                        place.module,
                    );
                    self.units.push(unit);
                }
                syn::Item::Const(constant) => {
                    self.constant_unit(
                        context,
                        place,
                        &constant.ident,
                        &constant.expr,
                        &constant.attrs,
                    );
                }
                syn::Item::Static(item_static) => {
                    self.constant_unit(
                        context,
                        place,
                        &item_static.ident,
                        &item_static.expr,
                        &item_static.attrs,
                    );
                }
                syn::Item::Struct(item) => {
                    self.define(context, place, &item.ident, &item.attrs, place.module);
                }
                syn::Item::Enum(item) => {
                    self.define(context, place, &item.ident, &item.attrs, place.module);
                }
                syn::Item::Union(item) => {
                    self.define(context, place, &item.ident, &item.attrs, place.module);
                }
                syn::Item::Type(item) => {
                    self.define(context, place, &item.ident, &item.attrs, place.module);
                }
                syn::Item::TraitAlias(item) => {
                    self.define(context, place, &item.ident, &item.attrs, place.module);
                }
                syn::Item::Trait(item) => self.walk_trait(context, place, item),
                syn::Item::Impl(item) => self.walk_impl(context, place, item),
                syn::Item::Mod(item) => self.walk_mod(context, place, item),
                syn::Item::Use(item) => self.record_use(place.module, &item.tree, &mut Vec::new()),
                syn::Item::Macro(item) => {
                    if let Some(ident) = &item.ident {
                        let exported = item
                            .attrs
                            .iter()
                            .any(|attr| attr.path().is_ident("macro_export"));
                        let module = if exported {
                            place
                                .module
                                .split("::")
                                .next()
                                .unwrap_or(place.module)
                                .to_owned()
                        } else {
                            place.module.to_owned()
                        };
                        self.define(context, place, ident, &item.attrs, &module);
                    }
                }
                _ => {}
            }
        }
    }

    fn walk_trait(&mut self, context: &Context<'_>, place: &Place<'_>, item: &syn::ItemTrait) {
        self.define(context, place, &item.ident, &item.attrs, place.module);
        let owner = format!("{}::{}", place.module, item.ident);
        for trait_item in &item.items {
            match trait_item {
                syn::TraitItem::Fn(method) if !is_test_only(&method.attrs) => {
                    self.define(context, place, &method.sig.ident, &method.attrs, &owner);
                    if let Some(block) = &method.default {
                        let unit = Self::function_unit(
                            context,
                            place,
                            &method.sig,
                            block,
                            format!("{owner}::{}", method.sig.ident),
                        );
                        self.units.push(unit);
                    }
                }
                syn::TraitItem::Const(constant) if !is_test_only(&constant.attrs) => {
                    self.define(context, place, &constant.ident, &constant.attrs, &owner);
                }
                _ => {}
            }
        }
    }

    fn walk_impl(&mut self, context: &Context<'_>, place: &Place<'_>, item: &syn::ItemImpl) {
        let self_type = match item.self_ty.as_ref() {
            syn::Type::Path(path) => path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
            _ => return,
        };
        for impl_item in &item.items {
            match impl_item {
                syn::ImplItem::Fn(method) if !is_test_only(&method.attrs) => {
                    let unit = Self::function_unit(
                        context,
                        place,
                        &method.sig,
                        &method.block,
                        String::new(),
                    );
                    self.units.push(unit);
                    self.pending.push(Pending {
                        module: place.module.to_owned(),
                        self_type: self_type.clone(),
                        name: method.sig.ident.to_string(),
                        definition: Self::definition(
                            context,
                            place,
                            &method.sig.ident,
                            &method.attrs,
                            "",
                        ),
                        unit: Some(self.units.len() - 1),
                    });
                }
                syn::ImplItem::Const(constant) if !is_test_only(&constant.attrs) => {
                    let mut unit = Self::constant(context, place, &constant.ident, &constant.expr);
                    unit.symbol = String::new();
                    self.units.push(unit);
                    self.pending.push(Pending {
                        module: place.module.to_owned(),
                        self_type: self_type.clone(),
                        name: constant.ident.to_string(),
                        definition: Self::definition(
                            context,
                            place,
                            &constant.ident,
                            &constant.attrs,
                            "",
                        ),
                        unit: Some(self.units.len() - 1),
                    });
                }
                _ => {}
            }
        }
    }

    fn walk_mod(&mut self, context: &Context<'_>, place: &Place<'_>, item: &syn::ItemMod) {
        self.define(context, place, &item.ident, &item.attrs, place.module);
        let module = format!("{}::{}", place.module, item.ident);
        let child_directory = format!("{}/{}", place.directory, item.ident);
        if let Some((_, items)) = &item.content {
            let inner = Place {
                file: place.file,
                source: place.source,
                module: &module,
                directory: &child_directory,
            };
            self.walk_items(context, &inner, items);
            return;
        }
        let (file, directory) = path_attribute(&item.attrs).map_or_else(
            || {
                let flat = format!("{child_directory}.rs");
                if context.tree.read(&flat).is_some() {
                    (flat, child_directory.clone())
                } else {
                    (format!("{child_directory}/mod.rs"), child_directory.clone())
                }
            },
            |explicit| {
                let file = join_normalised(&parent_dir(place.file), &explicit);
                let directory = parent_dir(&file);
                (file, directory)
            },
        );
        self.walk_file(context, &file, &module, &directory);
    }

    fn record_use(&mut self, module: &str, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.record_use(module, &path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(name) => {
                let ident = name.ident.to_string();
                if ident == "self" {
                    if let Some(last) = prefix.last() {
                        self.import(module, last, prefix.clone());
                    }
                } else {
                    let mut target = prefix.clone();
                    target.push(ident.clone());
                    self.import(module, &ident, target);
                }
            }
            syn::UseTree::Rename(rename) => {
                let mut target = prefix.clone();
                let ident = rename.ident.to_string();
                if ident != "self" {
                    target.push(ident);
                }
                self.import(module, &rename.rename.to_string(), target);
            }
            syn::UseTree::Glob(_) => {
                self.globs
                    .entry(module.to_owned())
                    .or_default()
                    .push(Import {
                        module: module.to_owned(),
                        target: prefix.clone(),
                    });
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.record_use(module, item, prefix);
                }
            }
        }
    }

    fn import(&mut self, module: &str, name: &str, target: Vec<String>) {
        if name == "_" {
            return;
        }
        self.imports
            .entry(format!("{module}::{name}"))
            .or_insert_with(|| Import {
                module: module.to_owned(),
                target,
            });
    }

    fn definition(
        context: &Context<'_>,
        place: &Place<'_>,
        ident: &syn::Ident,
        attrs: &[syn::Attribute],
        module: &str,
    ) -> Definition {
        Definition {
            file: place.file.to_owned(),
            line: ident.span().start().line,
            package: context.package.to_owned(),
            module: module.to_owned(),
            documented: attrs.iter().any(|attr| attr.path().is_ident("doc")),
        }
    }

    fn define(
        &mut self,
        context: &Context<'_>,
        place: &Place<'_>,
        ident: &syn::Ident,
        attrs: &[syn::Attribute],
        module: &str,
    ) {
        let definition = Self::definition(context, place, ident, attrs, module);
        self.definitions
            .entry(format!("{module}::{ident}"))
            .or_insert(definition);
    }

    fn function_unit(
        context: &Context<'_>,
        place: &Place<'_>,
        signature: &syn::Signature,
        block: &syn::Block,
        symbol: String,
    ) -> Unit {
        let body = relex(place.source, block.brace_token.span.join(), true);
        let mut params = ParamNames::default();
        for input in &signature.inputs {
            params.visit_fn_arg(input);
        }
        let mut constants = BTreeSet::new();
        let mut tables = BTreeSet::new();
        normalize::integer_constants(&body, &mut constants);
        normalize::hex_tables(&body, &mut tables);
        let strings = absolute_lines(&body, block.brace_token.span.join().start().line);
        Unit {
            symbol,
            name: signature.ident.to_string(),
            package: context.package.to_owned(),
            file: place.file.to_owned(),
            line: signature.ident.span().start().line,
            kind: UnitKind::Function,
            print: Some(normalize::body_print(&body, &params.0)),
            constants,
            tables,
            strings,
        }
    }

    fn constant(
        context: &Context<'_>,
        place: &Place<'_>,
        ident: &syn::Ident,
        expr: &syn::Expr,
    ) -> Unit {
        let tokens = relex(place.source, expr.span(), false);
        let mut constants = BTreeSet::new();
        let mut tables = BTreeSet::new();
        normalize::integer_constants(&tokens, &mut constants);
        normalize::hex_tables(&tokens, &mut tables);
        let strings = absolute_lines(&tokens, expr.span().start().line);
        Unit {
            symbol: format!("{}::{ident}", place.module),
            name: ident.to_string(),
            package: context.package.to_owned(),
            file: place.file.to_owned(),
            line: ident.span().start().line,
            kind: UnitKind::Constant,
            print: None,
            constants,
            tables,
            strings,
        }
    }

    fn constant_unit(
        &mut self,
        context: &Context<'_>,
        place: &Place<'_>,
        ident: &syn::Ident,
        expr: &syn::Expr,
        attrs: &[syn::Attribute],
    ) {
        let unit = Self::constant(context, place, ident, expr);
        self.define(context, place, ident, attrs, place.module);
        self.units.push(unit);
    }

    /// Place every impl item under its self type's defining module.
    fn settle(&mut self) {
        for pending in std::mem::take(&mut self.pending) {
            let owner = self
                .resolve_written(&pending.module, &pending.self_type)
                .unwrap_or_else(|| {
                    format!(
                        "{}::{}",
                        pending.module,
                        pending.self_type.last().map_or("_", String::as_str)
                    )
                });
            let symbol = format!("{owner}::{}", pending.name);
            let mut definition = pending.definition;
            definition.module.clone_from(&owner);
            self.definitions.entry(symbol.clone()).or_insert(definition);
            if let Some(index) = pending.unit {
                self.units[index].symbol = symbol;
            }
        }
    }

    /// Resolve a path as written inside `module` to a defined symbol.
    fn resolve_written(&self, module: &str, written: &[String]) -> Option<String> {
        let absolute = self.absolutise(module, written)?;
        self.resolve(&absolute)
    }

    /// The absolute form of a path written inside `module`.
    fn absolutise(&self, module: &str, written: &[String]) -> Option<String> {
        let (first, rest) = written.split_first()?;
        let crate_root = module.split("::").next().unwrap_or(module);
        let mut base: Vec<String> = match first.as_str() {
            "crate" => vec![crate_root.to_owned()],
            "self" => module.split("::").map(str::to_owned).collect(),
            "super" => {
                let mut parts: Vec<String> = module.split("::").map(str::to_owned).collect();
                parts.pop();
                parts
            }
            _ => {
                let local = format!("{module}::{first}");
                if self.definitions.contains_key(&local) || self.imports.contains_key(&local) {
                    module
                        .split("::")
                        .map(str::to_owned)
                        .chain([first.clone()])
                        .collect()
                } else {
                    vec![first.clone()]
                }
            }
        };
        for segment in rest {
            if segment == "super" {
                base.pop();
            } else {
                base.push(segment.clone());
            }
        }
        Some(base.join("::"))
    }

    /// Resolve `path` (absolute) to a defined symbol, following `use` aliases.
    ///
    /// A breadth-first walk over rewritten candidates with a visited set, so a
    /// glob cycle (`use super::*` both ways) costs one visit per distinct path.
    pub(crate) fn resolve(&self, path: &str) -> Option<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        queue.push_back(path.to_owned());
        while let Some(candidate) = queue.pop_front() {
            if self.definitions.contains_key(&candidate) {
                return Some(candidate);
            }
            if !seen.insert(candidate.clone()) || seen.len() > 4096 {
                continue;
            }
            let segments: Vec<&str> = candidate.split("::").collect();
            for split in (1..=segments.len()).rev() {
                let prefix = segments[..split].join("::");
                let rest = &segments[split..];
                if let Some(import) = self.imports.get(&prefix)
                    && let Some(target) = self.absolutise(&import.module, &import.target)
                    && target != prefix
                {
                    queue.push_back(join_path(&target, rest));
                }
                if split < segments.len()
                    && let Some(globs) = self.globs.get(&prefix)
                {
                    for glob in globs {
                        if let Some(target) = self.absolutise(&glob.module, &glob.target)
                            && target != prefix
                        {
                            queue.push_back(join_path(&target, rest));
                        }
                    }
                }
            }
        }
        None
    }
}

fn join_path(base: &str, rest: &[&str]) -> String {
    if rest.is_empty() {
        base.to_owned()
    } else {
        format!("{base}::{}", rest.join("::"))
    }
}

/// Which package is being walked.
struct Context<'a> {
    tree: &'a dyn Tree,
    package: &'a str,
    package_dir: &'a str,
}

/// Where in the source the walk is.
struct Place<'a> {
    file: &'a str,
    source: &'a str,
    module: &'a str,
    /// The directory a child `mod x;` of this module is looked up in.
    directory: &'a str,
}

/// Every name a parameter pattern binds.
#[derive(Default)]
struct ParamNames(BTreeSet<String>);

impl<'ast> Visit<'ast> for ParamNames {
    fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
        self.0.insert(pat.ident.to_string());
        syn::visit::visit_pat_ident(self, pat);
    }

    fn visit_type(&mut self, _: &'ast syn::Type) {}
}

/// Re-lex the source a span covers. With `strip_braces`, the span is a braced
/// block and only its contents are returned.
fn relex(source: &str, span: proc_macro2::Span, strip_braces: bool) -> TokenStream {
    let range = span.byte_range();
    let (start, end) = if strip_braces {
        (range.start + 1, range.end.saturating_sub(1))
    } else {
        (range.start, range.end)
    };
    source
        .get(start..end.max(start))
        .and_then(|slice| TokenStream::from_str(slice).ok())
        .unwrap_or_default()
}

/// The string literals of a re-lexed stream whose first line is `first_line` of
/// its file, each carrying its line in the file.
fn absolute_lines(stream: &TokenStream, first_line: usize) -> Vec<(String, usize)> {
    let mut strings = Vec::new();
    normalize::string_literals(stream, &mut strings);
    for (_, line) in &mut strings {
        *line += first_line.saturating_sub(1);
    }
    strings
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// Whether the attributes make an item test-only: `#[test]`, or a `#[cfg]` whose
/// predicate can only hold under `cfg(test)`.
pub(crate) fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Meta>()
            .is_ok_and(|meta| predicate_needs_test(&meta))
    })
}

fn predicate_needs_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) => {
            let Ok(children) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            if list.path.is_ident("all") {
                children.iter().any(predicate_needs_test)
            } else if list.path.is_ident("any") {
                !children.is_empty() && children.iter().all(predicate_needs_test)
            } else {
                false
            }
        }
        syn::Meta::NameValue(_) => false,
    }
}

/// The value of a `#[path = "…"]` attribute.
pub(crate) fn path_attribute(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let syn::Meta::NameValue(pair) = &attr.meta else {
            return None;
        };
        if !pair.path.is_ident("path") {
            return None;
        }
        match &pair.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            }) => Some(text.value()),
            _ => None,
        }
    })
}

fn parent_dir(file: &str) -> String {
    file.rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_owned())
}

/// `base/relative` with `.` and `..` folded.
fn join_normalised(base: &str, relative: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(base).join(relative).components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

/// Whether `file` sits in a directory of its package that never holds shipping
/// source, or outside the package altogether.
fn is_excluded(package_dir: &str, file: &str) -> bool {
    let Some(inside) = file
        .strip_prefix(package_dir)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return true;
    };
    let mut components: Vec<&str> = inside.split('/').collect();
    components.pop();
    components
        .iter()
        .any(|directory| EXCLUDED_DIRS.contains(directory))
}

/// The parts of a `Cargo.toml` the walk needs.
#[derive(Debug, Default)]
pub(crate) struct Manifest {
    package: Option<String>,
    lib_name: Option<String>,
    lib_path: Option<String>,
    bins: Vec<(Option<String>, Option<String>)>,
    declared_roots: Option<Vec<(String, String)>>,
}

impl Manifest {
    /// Parse the fixture subset with the same strict string grammar as the ledger.
    /// Production disk manifests instead use Cargo's complete grammar.
    fn read(text: &str) -> Result<Self, String> {
        let parsed = crate::toml::parse(text).map_err(|error| error.to_string())?;
        let string = |table: &BTreeMap<String, crate::toml::Value>,
                      key: &str|
         -> Result<Option<String>, String> {
            match table.get(key) {
                Some(crate::toml::Value::Str(value)) if !value.is_empty() => {
                    Ok(Some(value.clone()))
                }
                None => Ok(None),
                _ => Err(format!("manifest `{key}` must be a nonempty string")),
            }
        };
        let table = |key: &str| -> Result<Option<&BTreeMap<String, crate::toml::Value>>, String> {
            match parsed.get(key) {
                Some(crate::toml::Value::Table(table)) => Ok(Some(table)),
                None => Ok(None),
                _ => Err(format!("manifest `{key}` must be a table")),
            }
        };
        let mut manifest = Self::default();
        if let Some(package) = table("package")? {
            manifest.package =
                Some(string(package, "name")?.ok_or("manifest package has no name")?);
        } else if table("workspace")?.is_none() {
            return Err("manifest has neither a package nor a workspace".to_owned());
        }
        if let Some(lib) = table("lib")? {
            manifest.lib_name = string(lib, "name")?;
            manifest.lib_path = string(lib, "path")?;
        }
        if let Some(bins) = parsed.get("bin") {
            let crate::toml::Value::Array(bins) = bins else {
                return Err("manifest bins must be an array of tables".to_owned());
            };
            for bin in bins {
                let crate::toml::Value::Table(bin) = bin else {
                    return Err("manifest bin must be a table".to_owned());
                };
                manifest
                    .bins
                    .push((string(bin, "name")?, string(bin, "path")?));
            }
        }
        Ok(manifest)
    }

    /// Extract actual package target names and roots from Cargo's validated JSON.
    fn from_cargo(document: &Node, root: &Path) -> Result<Self, String> {
        let required = |node: &Node, field: &str| -> Result<String, String> {
            node.get(field)
                .and_then(Node::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("Cargo manifest omitted string `{field}`"))
        };
        let package = required(document, "name")?;
        let Some(JsonValue::Array(targets)) = document.get("targets").map(|node| &node.value)
        else {
            return Err("Cargo manifest omitted targets".to_owned());
        };
        let mut roots = Vec::new();
        for target in targets {
            let Some(JsonValue::Array(kinds)) = target.get("kind").map(|node| &node.value) else {
                return Err("Cargo target omitted kind".to_owned());
            };
            let kinds: Vec<&str> = kinds.iter().filter_map(Node::as_str).collect();
            if kinds
                .iter()
                .any(|kind| matches!(*kind, "test" | "bench" | "example"))
            {
                continue;
            }
            let name = required(target, "name")?.replace('-', "_");
            let name = if kinds.contains(&"custom-build") {
                format!("{}::build_script", package.replace('-', "_"))
            } else {
                name
            };
            let source = PathBuf::from(required(target, "src_path")?);
            let relative = source
                .strip_prefix(root)
                .map_err(|_| format!("Cargo target {} escapes census root", source.display()))?;
            let relative = relative
                .to_str()
                .ok_or("non-UTF-8 Cargo target path")?
                .replace('\\', "/");
            roots.push((name, relative));
        }
        Ok(Self {
            package: Some(package),
            declared_roots: Some(roots),
            ..Self::default()
        })
    }

    /// `(crate ident, root file)` for the library, every binary and the build
    /// script, in a fixed order.
    fn roots(&self, tree: &dyn Tree, package_dir: &str, package: &str) -> Vec<(String, String)> {
        if let Some(roots) = &self.declared_roots {
            return roots.clone();
        }
        let ident = |name: &str| name.replace('-', "_");
        let mut roots: Vec<(String, String)> = Vec::new();
        let lib = self
            .lib_path
            .clone()
            .unwrap_or_else(|| "src/lib.rs".to_owned());
        if self.lib_path.is_some()
            || self.lib_name.is_some()
            || tree.read(&format!("{package_dir}/{lib}")).is_some()
        {
            let name = self.lib_name.clone().unwrap_or_else(|| package.to_owned());
            roots.push((ident(&name), format!("{package_dir}/{lib}")));
        }
        let mut bin_files: BTreeSet<String> = BTreeSet::new();
        for (name, path) in &self.bins {
            let name = name.clone().unwrap_or_else(|| package.to_owned());
            let path = path.clone().unwrap_or_else(|| {
                if name == package {
                    "src/main.rs".to_owned()
                } else {
                    format!("src/bin/{name}.rs")
                }
            });
            if bin_files.insert(path.clone()) {
                roots.push((ident(&name), format!("{package_dir}/{path}")));
            }
        }
        if bin_files.insert("src/main.rs".to_owned())
            && tree.read(&format!("{package_dir}/src/main.rs")).is_some()
        {
            roots.push((ident(package), format!("{package_dir}/src/main.rs")));
        }
        for (name, is_dir) in tree.list(&format!("{package_dir}/src/bin")) {
            let (bin, path) = if is_dir {
                (name.clone(), format!("src/bin/{name}/main.rs"))
            } else if let Some(stem) = name.strip_suffix(".rs") {
                (stem.to_owned(), format!("src/bin/{name}"))
            } else {
                continue;
            };
            if bin_files.insert(path.clone())
                && tree.read(&format!("{package_dir}/{path}")).is_some()
            {
                roots.push((ident(&bin), format!("{package_dir}/{path}")));
            }
        }
        if tree.read(&format!("{package_dir}/build.rs")).is_some() {
            roots.push((
                format!("{}::build_script", ident(package)),
                format!("{package_dir}/build.rs"),
            ));
        }
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::{Memory, Workspace, is_excluded, join_normalised};

    fn workspace(files: &[(&str, &str)]) -> Workspace {
        let memory = Memory {
            files: files
                .iter()
                .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
                .collect(),
        };
        let walked = Workspace::walk(&memory);
        assert!(walked.errors.is_empty(), "{:?}", walked.errors);
        walked
    }

    #[test]
    fn fixture_manifests_accept_strings_and_reject_missing_or_malformed_fields() {
        let valid = super::Manifest::read(
            "[package]\nname='example' # comment\n[lib]\npath='root.rs' # comment\n",
        )
        .expect("valid strings");
        assert_eq!(valid.package.as_deref(), Some("example"));
        assert_eq!(valid.lib_path.as_deref(), Some("root.rs"));
        for invalid in [
            "[package]\n",
            "[package]\nname=3\n",
            "[package]\nname='a'\n[lib]\npath=false\n",
            "[package\nname='a'\n",
        ] {
            assert!(super::Manifest::read(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn a_listed_but_unreadable_rust_file_fails_the_walk() {
        struct Failing(Memory);
        impl super::Tree for Failing {
            fn read(&self, path: &str) -> Option<String> {
                if path.ends_with("hidden.rs") {
                    None
                } else {
                    self.0.read(path)
                }
            }
            fn list(&self, path: &str) -> Vec<(String, bool)> {
                self.0.list(path)
            }
        }
        let files = [
            (
                "crates/a/Cargo.toml".to_owned(),
                "[package]\nname='a'\n".to_owned(),
            ),
            (
                "crates/a/src/lib.rs".to_owned(),
                "pub fn present() {}".to_owned(),
            ),
            (
                "crates/a/src/hidden.rs".to_owned(),
                "pub fn missing() {}".to_owned(),
            ),
        ]
        .into_iter()
        .collect();
        let walked = Workspace::walk(&Failing(Memory { files }));
        assert!(
            walked
                .errors
                .iter()
                .any(|error| error.contains("hidden.rs") && error.contains("could not be read"))
        );
    }

    #[test]
    fn modules_resolve_through_reexports_and_impls_land_on_their_type() {
        let walked = workspace(&[
            ("crates/a/Cargo.toml", "[package]\nname = \"demo-a\"\n"),
            (
                "crates/a/src/lib.rs",
                "mod base;\nmod ops;\npub use base::Thing;\n#[cfg(test)]\nmod tests;\n",
            ),
            ("crates/a/src/base.rs", "/// A thing.\npub struct Thing;\n"),
            (
                "crates/a/src/ops.rs",
                "use crate::base::Thing;\nimpl Thing {\n    /// Go.\n    pub(crate) fn go(&self) -> u8 { 1 }\n}\n#[cfg(test)]\nmod tests { fn hidden() {} }\n",
            ),
        ]);
        assert_eq!(
            walked.resolve("demo_a::Thing::go").as_deref(),
            Some("demo_a::base::Thing::go")
        );
        assert!(walked.definitions["demo_a::base::Thing::go"].documented);
        assert_eq!(
            walked.definitions["demo_a::base::Thing::go"].module,
            "demo_a::base::Thing"
        );
        assert!(walked.resolve("demo_a::ops::tests::hidden").is_none());
        assert!(walked.resolve("demo_a::Thing::gone").is_none());
    }

    #[test]
    fn test_only_items_and_test_directories_are_not_walked() {
        let walked = workspace(&[
            ("crates/a/Cargo.toml", "[package]\nname = \"demo-a\"\n"),
            (
                "crates/a/src/lib.rs",
                "#[test]\nfn t() {}\n#[cfg(all(test, unix))]\nfn u() {}\n#[cfg(any(test, unix))]\nfn shipped() {}\n#[cfg(not(test))]\nfn also_shipped() {}\n#[path = \"../tests/support.rs\"]\nmod support;\n",
            ),
            ("crates/a/tests/support.rs", "pub(crate) fn helper() {}\n"),
        ]);
        let names: Vec<&str> = walked.units.iter().map(|unit| unit.name.as_str()).collect();
        assert_eq!(names, ["shipped", "also_shipped"]);
    }

    #[test]
    fn paths_fold_and_escapes_are_excluded() {
        assert_eq!(
            join_normalised("crates/a/src", "../../b/src/x.rs"),
            "crates/b/src/x.rs"
        );
        assert!(is_excluded("crates/a", "crates/b/src/x.rs"));
        assert!(is_excluded("crates/a", "crates/a/generated/x.rs"));
        assert!(!is_excluded("crates/a", "crates/a/src/x.rs"));
    }
}
