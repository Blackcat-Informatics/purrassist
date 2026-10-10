// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The scope graph of an adapter-built tool: the member names its exact
//! schema declares where the typed model stops (`$ref`, `anyOf`, `oneOf`,
//! `allOf`).
//!
//! The typed projection ([`super::ValueShape`]) drops the structure of a
//! value whose schema it cannot express, so a grammar reading nested XML
//! parameters under such a value would not know which names the value's
//! object scope declares. The scope graph restores exactly that and nothing
//! more: one [`ScopeNode`] per distinct schema location (a `$defs` or
//! `definitions` entry, an inline object, or an `anyOf`/`oneOf`/`allOf`
//! union), holding the member names the location declares and, per member,
//! whether its value nests as an object or as an array of objects
//! ([`Nesting`]). A recursive `$ref` is a back-edge by node index, never an
//! unrolling, so the graph is finite and its size is bounded by the schema's.
//! Compilation is charged to the governor (`Work` per schema object and
//! member visited, `ScratchBytes` per retained record) and is exhausted
//! explicitly. Typing stays with the exact schema
//! (`tool_call.value.schema-typed`); the graph only names scopes.
use crate::contract::{Governor, Resource, StopReason, saturating_size as size};
use crate::json::{Node, Value, pointer_segment};
use crate::schema::Dialect;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Portable charge for one retained scope node.
const NODE_RECORD: usize = 96;
/// Portable charge for one retained scope member (name bytes extra).
const MEMBER_RECORD: usize = 64;
/// Longest `$ref` chain followed from one location before the location is
/// treated as unresolvable.
const REF_HOPS: usize = 64;

/// Keywords that annotate a schema and never change which members it
/// declares: a schema holding only these beside `$ref` is that `$ref`.
const ANNOTATIONS: [&str; 9] = [
    "$ref",
    "$comment",
    "$defs",
    "default",
    "definitions",
    "deprecated",
    "description",
    "examples",
    "title",
];

/// How a member's value nests when its XML value begins with a nested
/// parameter opener.
///
/// ```
/// use purrassist_core::tool_call::Nesting;
/// assert_eq!(Nesting::default(), Nesting::None);
/// assert_ne!(Nesting::Object(0), Nesting::ArrayOfObject(0));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nesting {
    /// The value does not nest: tags in it stay text.
    #[default]
    None,
    /// The value is one object whose members are the scope at this node
    /// index of the [`ScopeGraph`].
    Object(usize),
    /// The value is an array of objects whose members are the scope at this
    /// node index; each repetition of the member is one item.
    ArrayOfObject(usize),
}

/// One member a scope declares, and how its value nests.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Nesting, ToolSet}};
/// let tools = ToolSet::from_openai(
///     r##"[{"type":"function","function":{"name":"t","parameters":{"type":"object",
///     "properties":{"n":{"$ref":"#/$defs/n"}},"$defs":{"n":{"type":"object",
///     "properties":{"label":{"type":"string"}}}}}}}]"##,
///     Budget::default(),
/// ).unwrap();
/// let shape = tools.argument_shape("t").unwrap();
/// let Nesting::Object(node) = shape.arguments.parameter("n").unwrap().nesting else { panic!() };
/// let member = &shape.scopes.node(node).unwrap().members()[0];
/// assert_eq!((member.name.as_str(), member.nesting, member.array), ("label", Nesting::None, false));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeMember {
    /// The exact member name the schema declares.
    pub name: String,
    /// How the member's value nests.
    pub nesting: Nesting,
    /// Whether the member's value is an array on every branch that admits a
    /// non-null value (an array of objects, or of anything else): XML text
    /// that is not a JSON array is then one item, and repeated occurrences
    /// collect into one array. The item type stays with the exact schema.
    pub array: bool,
}

/// One schema location's object scope: the member names it declares.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Nesting, ToolSet}};
/// let tools = ToolSet::from_openai(
///     r#"[{"type":"function","function":{"name":"t","parameters":{"type":"object",
///     "properties":{"u":{"anyOf":[{"type":"object","properties":{"a":{"type":"string"}}},
///     {"type":"object","properties":{"b":{"type":"string"}}}]}}}}}]"#,
///     Budget::default(),
/// ).unwrap();
/// let shape = tools.argument_shape("t").unwrap();
/// let Nesting::Object(node) = shape.arguments.parameter("u").unwrap().nesting else { panic!() };
/// let scope = shape.scopes.node(node).unwrap();
/// assert!(scope.declares("a") && scope.declares("b") && !scope.declares("c"));
/// assert!(scope.is_open());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeNode {
    members: Vec<ScopeMember>,
    open: bool,
}

impl ScopeNode {
    /// The declared members, sorted by name.
    #[must_use]
    pub fn members(&self) -> &[ScopeMember] {
        &self.members
    }

    /// The member with this exact name, if the scope declares it.
    #[must_use]
    pub fn member(&self, name: &str) -> Option<&ScopeMember> {
        super::by_name(&self.members, name, |member| member.name.as_str())
    }

    /// Whether the scope declares a member of this exact name.
    #[must_use]
    pub fn declares(&self, name: &str) -> bool {
        self.member(name).is_some()
    }

    /// Whether the scope may hold members it does not declare: some branch
    /// does not close its object, is not an object, or could not be
    /// resolved. A scope that declares no member and is open has unknown
    /// membership.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the scope's membership is unknown: it declares no member and
    /// may hold any.
    #[must_use]
    pub const fn is_opaque(&self) -> bool {
        self.open && self.members.is_empty()
    }
}

/// The scope graph of one tool: the [`ScopeNode`]s its unmodeled values
/// refer to by index. Empty for a tool built from typed definitions.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("read").param("path", Param::string());
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// assert!(tools.argument_shape("read").unwrap().scopes.is_empty());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeGraph {
    nodes: Vec<ScopeNode>,
}

impl ScopeGraph {
    /// The node at `index`, if any.
    #[must_use]
    pub fn node(&self, index: usize) -> Option<&ScopeNode> {
        self.nodes.get(index)
    }

    /// The number of nodes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no node.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// The schema objects reachable from one location through `$ref` and the
/// `allOf`/`anyOf`/`oneOf` keywords, and whether some part of them could
/// not be resolved.
struct Reach {
    pointers: Vec<String>,
    unresolved: bool,
}

/// Compiles a [`ScopeGraph`] from the exact parameters schema of one tool,
/// lazily: a location is interned when a value refers to it, and its
/// members are read when the graph is finished.
pub(super) struct ScopeCompiler<'d> {
    root: &'d Node,
    dialect: Dialect,
    /// Node index by (reference-resolved) location pointer.
    index: BTreeMap<String, usize>,
    /// Node index by the (reference-resolved, sorted) pointers of a scope
    /// declared at several locations at once ([`Self::scope_many`]).
    joint: BTreeMap<Vec<String>, usize>,
    nodes: Vec<ScopeNode>,
    pending: Vec<(usize, Vec<String>)>,
}

impl<'d> ScopeCompiler<'d> {
    pub(super) const fn new(root: &'d Node, dialect: Dialect) -> Self {
        Self {
            root,
            dialect,
            index: BTreeMap::new(),
            joint: BTreeMap::new(),
            nodes: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// The node index of the scope at `pointer` in the parameters schema.
    pub(super) fn scope(
        &mut self,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let located = self.locate(pointer, governor)?;
        self.intern(located, governor)
    }

    /// The node index of the scope at the already located `located`.
    fn intern(&mut self, located: String, governor: &mut Governor) -> Result<usize, StopReason> {
        if let Some(&index) = self.index.get(&located) {
            return Ok(index);
        }
        governor.charge(
            Resource::ScratchBytes,
            size(NODE_RECORD.saturating_add(located.len().saturating_mul(2))),
        )?;
        let index = self.nodes.len();
        self.nodes.push(ScopeNode::default());
        self.index.insert(located.clone(), index);
        self.pending.push((index, vec![located]));
        Ok(index)
    }

    /// The node index of the scope that is the union of the schemas at
    /// `pointers`: a value declared at several locations at once (a name
    /// several `anyOf`/`oneOf` branches of the root declare). One location
    /// is [`Self::scope`]; the members of several are the union of what
    /// each declares.
    pub(super) fn scope_many(
        &mut self,
        pointers: &[String],
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let mut located = Vec::with_capacity(pointers.len());
        for pointer in pointers {
            located.push(self.locate(pointer, governor)?);
        }
        located.sort();
        located.dedup();
        if located.len() == 1 {
            let only = located.swap_remove(0);
            return self.intern(only, governor);
        }
        if let Some(&index) = self.joint.get(&located) {
            return Ok(index);
        }
        let bytes = located.iter().fold(NODE_RECORD, |total, at| {
            total.saturating_add(at.len().saturating_mul(2))
        });
        governor.charge(Resource::ScratchBytes, size(bytes))?;
        let index = self.nodes.len();
        self.nodes.push(ScopeNode::default());
        self.joint.insert(located.clone(), index);
        self.pending.push((index, located));
        Ok(index)
    }

    /// Read every interned location's members and return the graph.
    pub(super) fn finish(mut self, governor: &mut Governor) -> Result<ScopeGraph, StopReason> {
        while let Some((index, pointers)) = self.pending.pop() {
            let node = self.read(&pointers, governor)?;
            self.nodes[index] = node;
        }
        Ok(ScopeGraph { nodes: self.nodes })
    }

    /// The location a pure `$ref` at `pointer` denotes, following chains;
    /// `pointer` itself for anything else.
    fn locate(&self, pointer: &str, governor: &mut Governor) -> Result<String, StopReason> {
        let mut at = pointer.to_owned();
        let mut seen = BTreeSet::new();
        for _ in 0..REF_HOPS {
            governor.charge(Resource::Work, size(1 + at.len() / 16))?;
            let Some(node) = self.root.pointer(&at) else {
                break;
            };
            let Value::Object(members) = &node.value else {
                break;
            };
            let pure = members
                .iter()
                .all(|member| ANNOTATIONS.contains(&member.key.as_str()));
            let target = node
                .get("$ref")
                .and_then(Node::as_str)
                .and_then(local_pointer);
            let Some(target) = target.filter(|_| pure) else {
                break;
            };
            if !seen.insert(at.clone()) {
                break;
            }
            at = target;
        }
        Ok(at)
    }

    /// Every schema object reachable from `start`, in discovery order.
    fn reach(&self, start: &str, governor: &mut Governor) -> Result<Reach, StopReason> {
        let mut queue = VecDeque::from([start.to_owned()]);
        let mut seen = BTreeSet::new();
        let mut reach = Reach {
            pointers: Vec::new(),
            unresolved: false,
        };
        let mut charged = 0_u64;
        while let Some(at) = queue.pop_front() {
            if !seen.insert(at.clone()) {
                continue;
            }
            let bytes = size(at.len().saturating_add(32));
            governor.charge(Resource::ScratchBytes, bytes)?;
            charged = charged.saturating_add(bytes);
            let Some(node) = self.root.pointer(&at) else {
                reach.unresolved = true;
                continue;
            };
            let Value::Object(members) = &node.value else {
                reach.pointers.push(at);
                continue;
            };
            governor.charge(Resource::Work, size(members.len().saturating_add(1)))?;
            // Draft-07 `$ref` overrides every sibling keyword.
            let draft07_ref = if let Some(reference) = node.get("$ref") {
                match reference.as_str().and_then(local_pointer) {
                    Some(target) => queue.push_back(target),
                    None => reach.unresolved = true,
                }
                self.dialect == Dialect::Draft07
            } else {
                false
            };
            if !draft07_ref {
                for keyword in ["allOf", "anyOf", "oneOf"] {
                    if let Some(Value::Array(branches)) = node.get(keyword).map(|n| &n.value) {
                        governor.charge(Resource::Work, size(branches.len()))?;
                        for branch in 0..branches.len() {
                            queue.push_back(format!("{at}/{keyword}/{branch}"));
                        }
                    }
                }
            }
            reach.pointers.push(at);
        }
        governor.release_scratch(charged)?;
        Ok(reach)
    }

    /// Whether the schema object `node` is an object scope of its own: it
    /// declares `properties` or a `type` naming `object`.
    fn objectish(&self, at: &str) -> bool {
        let Some(node) = self.root.pointer(at) else {
            return false;
        };
        if self.dialect == Dialect::Draft07 && node.get("$ref").is_some() {
            return false;
        }
        names_type(node, "object")
            || matches!(
                node.get("properties").map(|n| &n.value),
                Some(Value::Object(_))
            )
    }

    /// The members the locations declare together and whether the scope is
    /// open.
    fn read(
        &mut self,
        pointers: &[String],
        governor: &mut Governor,
    ) -> Result<ScopeNode, StopReason> {
        let mut reach = Reach {
            pointers: Vec::new(),
            unresolved: false,
        };
        let mut known = BTreeSet::new();
        for pointer in pointers {
            let part = self.reach(pointer, governor)?;
            reach.unresolved |= part.unresolved;
            for at in part.pointers {
                if known.insert(at.clone()) {
                    reach.pointers.push(at);
                }
            }
        }
        let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut open = reach.unresolved || reach.pointers.is_empty();
        for at in &reach.pointers {
            let Some(node) = self.root.pointer(at) else {
                continue;
            };
            let Value::Object(_) = &node.value else {
                // `true` admits anything; `false` admits nothing.
                open |= node.value != Value::Bool(false);
                continue;
            };
            let overridden = self.dialect == Dialect::Draft07 && node.get("$ref").is_some();
            let delegates = node.get("$ref").is_some()
                || ["allOf", "anyOf", "oneOf"]
                    .iter()
                    .any(|keyword| node.get(keyword).is_some());
            let properties = match node.get("properties").map(|n| &n.value) {
                Some(Value::Object(properties)) if !overridden => Some(properties),
                _ => None,
            };
            if let Some(properties) = properties {
                governor.charge(Resource::Work, size(properties.len()))?;
                for property in properties {
                    declared
                        .entry(property.key.clone())
                        .or_default()
                        .push(format!(
                            "{at}/properties/{}",
                            pointer_segment(&property.key)
                        ));
                }
            }
            let object = !overridden && self.objectish(at);
            if object {
                open |= !closed(node, self.dialect);
            } else if !delegates {
                // A schema that is no object and delegates nowhere admits
                // some non-object value (or any value, when empty).
                open = true;
            }
        }
        let mut members = Vec::with_capacity(declared.len());
        for (name, candidates) in declared {
            governor.charge(
                Resource::ScratchBytes,
                size(MEMBER_RECORD.saturating_add(name.len())),
            )?;
            let mut member = ScopeMember {
                name,
                nesting: Nesting::None,
                array: false,
            };
            for candidate in &candidates {
                let (nesting, array) = self.classify(candidate, governor)?;
                member.nesting = nesting;
                member.array = array || matches!(nesting, Nesting::ArrayOfObject(_));
                if nesting != Nesting::None || array {
                    break;
                }
            }
            members.push(member);
        }
        Ok(ScopeNode { members, open })
    }

    /// How a member whose schema sits at `pointer` nests, and whether its
    /// value is an array: an object scope when any branch is an object,
    /// else an array of objects when any branch is an array whose items are
    /// objects. A value is an array when every non-null leaf branch is
    /// typed `array`.
    fn classify(
        &mut self,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<(Nesting, bool), StopReason> {
        let location = self.locate(pointer, governor)?;
        let reach = self.reach(&location, governor)?;
        let mut items: Option<String> = None;
        let (mut leaves, mut arrays) = (0_usize, 0_usize);
        for at in &reach.pointers {
            let Some(node) = self.root.pointer(at) else {
                continue;
            };
            if self.objectish(at) {
                return Ok((Nesting::Object(self.scope(&location, governor)?), false));
            }
            let delegates = node.get("$ref").is_some()
                || ["allOf", "anyOf", "oneOf"]
                    .iter()
                    .any(|keyword| node.get(keyword).is_some());
            if !delegates && !names_type_only(node, "null") {
                leaves += 1;
                arrays += usize::from(names_type_only(node, "array"));
            }
            if items.is_none()
                && names_type(node, "array")
                && matches!(node.get("items").map(|n| &n.value), Some(Value::Object(_)))
            {
                let target = format!("{at}/items");
                let located = self.locate(&target, governor)?;
                let inner = self.reach(&located, governor)?;
                if inner.pointers.iter().any(|item| self.objectish(item)) {
                    items = Some(located);
                }
            }
        }
        let array = leaves > 0 && leaves == arrays && !reach.unresolved;
        match items {
            Some(located) => Ok((
                Nesting::ArrayOfObject(self.scope(&located, governor)?),
                array,
            )),
            None => Ok((Nesting::None, array)),
        }
    }
}

/// Whether the schema object `node` closes its object: no member beyond the
/// declared ones is admitted.
fn closed(node: &Node, dialect: Dialect) -> bool {
    let false_of = |keyword: &str| {
        node.get(keyword)
            .is_some_and(|value| value.value == Value::Bool(false))
    };
    let closing = false_of("additionalProperties")
        || (dialect != Dialect::Draft07 && false_of("unevaluatedProperties"));
    closing && node.get("patternProperties").is_none()
}

/// Whether the `type` keyword of `node` names `kind` (as a string or in a
/// type array).
fn names_type(node: &Node, kind: &str) -> bool {
    match node.get("type").map(|n| &n.value) {
        Some(Value::String(name)) => name == kind,
        Some(Value::Array(names)) => names.iter().any(|name| name.as_str() == Some(kind)),
        _ => false,
    }
}

/// Whether the `type` keyword of `node` is exactly the string `kind`.
fn names_type_only(node: &Node, kind: &str) -> bool {
    matches!(node.get("type").map(|n| &n.value), Some(Value::String(name)) if name == kind)
}

/// The JSON Pointer a local `$ref` (`#` or `#/...`) denotes within the
/// document; `None` for any other reference (remote, anchor).
pub(super) fn local_pointer(reference: &str) -> Option<String> {
    let fragment = reference.strip_prefix('#')?;
    if !fragment.is_empty() && !fragment.starts_with('/') {
        return None;
    }
    let bytes = fragment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = fragment.get(at + 1..at + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}
