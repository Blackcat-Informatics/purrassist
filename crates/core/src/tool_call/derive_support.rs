// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Support for typed tool arguments: the traits a Rust type implements to
//! describe itself as a tool parameter, and the single-pass [`Members`]
//! reader that conversions from a JSON object are built on.
//!
//! These items are what `#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]`
//! generate against, and they are equally usable by hand. A type that is a
//! [`ToolParameter`] knows the [`ParameterSpec`] it declares and how to read
//! itself back from a validated argument ([`FromNode`]); a struct that is
//! [`ToolArguments`] also knows the whole [`ToolDefinition`].
//!
//! # Field dispatch
//!
//! Every property of a field's Rust type is carried by a trait, so an
//! unsupported combination is a compile error at the field type rather than a
//! runtime failure: [`ToolItem`] excludes `Option<T>` from `Vec<T>`,
//! [`ToolRepeatable`] exists only for `Vec<T>` and `Option<Vec<T>>`,
//! [`ToolPattern`] only for strings, and [`ToolExtra`] (sealed) only for
//! [`Node`].
//!
//! # Integer ranges
//!
//! The parameter model has no integer range keywords: every integer type
//! declares [`Param::integer`]. The range of the Rust type is enforced by
//! [`FromNode`] alone, which reports `expected: "integer within the u8 range"`
//! for `300` as a `u8`. `usize` and `isize` ranges differ between targets
//! (notably `wasm32`), so fixed-width integers are preferable in portable
//! tool definitions.
use super::{FromNode, FromNodeError, ObjectSpec, Param, ParameterSpec, ToolDefinition};
use crate::json::{Member, Node, Number, Value};
use core::str::FromStr;

/// A struct whose fields are the arguments of one tool.
///
/// The conversion from a validated argument object ([`FromNode`]) is the
/// supertrait. `#[derive(ToolDefinition)]` implements it; a hand-written
/// implementation is equally valid.
///
/// ```
/// use purrassist_core::{
///     Budget, json,
///     tool_call::{
///         FromNode, FromNodeError, Members, ObjectSpec, Slot, ToolArguments, ToolDefinition,
///         ToolParameter, ToolSet,
///     },
/// };
///
/// #[derive(Debug, PartialEq)]
/// struct ReadFile {
///     path: String,
///     limit: Option<u32>,
/// }
///
/// // Sorted declared names, and the declaration index of each.
/// const NAMES: &[&str] = &["limit", "path"];
/// const ORDER: &[usize] = &[1, 0];
///
/// impl FromNode for ReadFile {
///     fn from_node(node: &json::Node) -> Result<Self, FromNodeError> {
///         let mut members = Members::new(NAMES, ORDER, node)?;
///         let (mut path, mut limit) = (None, None);
///         for slot in &mut members {
///             match slot {
///                 Slot::Declared { index: 0, member } => {
///                     path = Some(String::from_node(&member.value).map_err(|e| e.within("path"))?);
///                 }
///                 Slot::Declared { member, .. } => {
///                     limit = Some(<Option<u32>>::from_node(&member.value).map_err(|e| e.within("limit"))?);
///                 }
///                 Slot::Duplicate { member, .. } => return Err(FromNodeError::duplicate(&member.key)),
///                 Slot::Undeclared(member) => return Err(FromNodeError::unknown(&member.key)),
///                 // `Slot` is `#[non_exhaustive]`: a new classification is not a breaking change.
///                 _ => return Err(FromNodeError::mismatch("a known member classification", node)),
///             }
///         }
///         Ok(Self {
///             path: path.ok_or_else(|| FromNodeError::missing("path"))?,
///             limit: limit.unwrap_or(None),
///         })
///     }
/// }
///
/// impl ToolArguments for ReadFile {
///     fn tool_definition() -> ToolDefinition {
///         ToolDefinition::new("read_file")
///             .param("path", <String as ToolParameter>::parameter().required())
///             .param("limit", <Option<u32> as ToolParameter>::parameter())
///     }
///     fn object_spec() -> ObjectSpec {
///         ObjectSpec::new()
///             .param("path", <String as ToolParameter>::parameter().required())
///             .param("limit", <Option<u32> as ToolParameter>::parameter())
///     }
/// }
///
/// let tools = ToolSet::compile([ReadFile::tool_definition()], Budget::default()).unwrap();
/// let arguments = json::parse(r#"{"path":"a.txt","limit":3}"#).unwrap();
/// assert!(tools.tool("read_file").unwrap().schema().validate(&arguments).unwrap().valid);
/// assert_eq!(ReadFile::from_node(&arguments), Ok(ReadFile { path: "a.txt".into(), limit: Some(3) }));
/// ```
pub trait ToolArguments: FromNode {
    /// The complete tool definition of this argument struct.
    #[must_use]
    fn tool_definition() -> ToolDefinition;

    /// The parameters as a nested object, for use as a field of another
    /// struct ([`Param::object`]) or as the argument scope of a tool.
    ///
    /// For a derived struct the object always states its unknown-member
    /// handling: the struct's `unknown` value, or
    /// [`DropWithLoss`](super::UnknownParameters::DropWithLoss) when it
    /// declares none. [`tool_definition`](Self::tool_definition) does not:
    /// a top-level tool keeps the default of its effects.
    #[must_use]
    fn object_spec() -> ObjectSpec;
}

/// A Rust type that can be one parameter of a tool.
///
/// [`parameter`](Self::parameter) is the *unrequired* shape of the type: it
/// never calls [`ParameterSpec::required`], so the caller (the derive, or a
/// hand-written definition) applies `.required()` exactly when
/// [`REQUIRED`](Self::REQUIRED) is true and the field declares no default.
/// `Option<T>` shares the parameter shape of `T` and is the only type that is
/// not required.
///
/// Implemented here for `String`, the twelve integer types, `f32`, `f64`,
/// [`Number`], `bool`, `Vec<T: ToolItem>`, `Option<T: ToolItem>` and
/// `Box<T: ToolItem>`; the derive implements it (and [`ToolItem`]) for
/// derived structs and enums. `Option<Option<T>>` is deliberately not a
/// parameter: JSON has one `null`, so the nesting is meaningless.
///
/// # Hand-written implementations
/// Only [`parameter`](Self::parameter) is required. The other items default
/// to a required, scalar-depth parameter: `REQUIRED = true`,
/// `absent() = None` and `DEPTH = 0`. `REQUIRED` and `absent()` must agree:
/// `REQUIRED` is `false` exactly when `absent()` is `Some(..)`. Declaring
/// `REQUIRED = false` while `absent()` stays `None` makes the schema accept a
/// missing member that [`FromNode`] then rejects as
/// [`FromNodeError::missing`]. Only a type that nests derived types needs a
/// `DEPTH` other than `0`.
///
/// ```
/// use purrassist_core::{json::Node, tool_call::{FromNode, FromNodeError, Param, ParameterSpec, ToolParameter}};
///
/// struct FilePath(String);
///
/// impl FromNode for FilePath {
///     fn from_node(node: &Node) -> Result<Self, FromNodeError> {
///         String::from_node(node).map(Self)
///     }
/// }
///
/// impl ToolParameter for FilePath {
///     fn parameter() -> ParameterSpec {
///         Param::string()
///     }
/// }
///
/// assert!(FilePath::REQUIRED);
/// assert!(FilePath::absent().is_none());
/// assert_eq!(FilePath::DEPTH, 0);
/// ```
///
/// ```
/// use purrassist_core::tool_call::{Param, ToolParameter};
/// assert_eq!(<u16 as ToolParameter>::parameter(), Param::integer());
/// assert_eq!(<Option<String> as ToolParameter>::parameter(), Param::string());
/// assert_eq!(<Vec<bool> as ToolParameter>::parameter(), Param::array(Param::boolean()));
/// assert!(<String as ToolParameter>::REQUIRED);
/// assert!(!<Option<String> as ToolParameter>::REQUIRED);
/// assert_eq!(<Option<String> as ToolParameter>::absent(), Some(None));
/// assert_eq!(<String as ToolParameter>::absent(), None);
/// ```
pub trait ToolParameter: FromNode {
    /// The unrequired parameter specification of this type.
    ///
    /// For a derived struct this is an object that states its own
    /// unknown-member handling (the struct's `unknown` value, or
    /// [`super::UnknownParameters::DropWithLoss`] when it declares none), so it does
    /// not inherit the enclosing tool's handling, including inside a
    /// hand-built [`super::UnknownParameters::PassThrough`] tool.
    #[must_use]
    fn parameter() -> ParameterSpec;

    /// Whether a member of this type must be present (false only for
    /// `Option<T>`). Defaults to `true`.
    ///
    /// This must be `false` exactly when [`absent`](Self::absent) returns
    /// `Some(..)`.
    const REQUIRED: bool = true;

    /// The value an absent member converts to: `Some(None)` for `Option<T>`,
    /// otherwise `None` (absence is then a [`FromNodeError::missing`] error).
    /// Defaults to `None`; see [`REQUIRED`](Self::REQUIRED) for the relation.
    #[must_use]
    fn absent() -> Option<Self> {
        None
    }

    /// Nesting depth of derived types: `0` for scalars, `T::DEPTH` for
    /// `Vec<T>` and `Option<T>`, and `1 + max_depth(..)` over its fields for a
    /// derived type. A derive evaluates it in a `const` so that indirectly
    /// recursive types become a compile-time cycle error. Defaults to `0`.
    const DEPTH: usize = 0;
}

/// A [`ToolParameter`] that is not an `Option`: the bound of `Vec<T>` items.
///
/// `Vec<Option<T>>` therefore has no [`ToolParameter`] impl and is rejected
/// by rustc at the type. `Vec<Vec<T>>` is allowed (an array of arrays).
///
/// ```
/// use purrassist_core::tool_call::ToolItem;
/// fn item<T: ToolItem>() {}
/// item::<String>();
/// item::<Vec<u8>>();
/// item::<Box<String>>();
/// ```
///
/// ```compile_fail,E0277
/// use purrassist_core::tool_call::ToolItem;
/// fn item<T: ToolItem>() {}
/// item::<Option<String>>();
/// ```
pub trait ToolItem: ToolParameter {}

/// A parameter type whose members may repeat in markup (`Vec<T>` and
/// `Option<Vec<T>>`).
///
/// [`repeated`](Self::repeated) is the item's specification with
/// [`ParameterSpec::repeatable`] applied: each occurrence supplies one item
/// and the value is an array. [`ToolSet::compile`](super::ToolSet::compile)
/// rejects a repeatable array item (`Vec<Vec<T>>` with repeat) as an invalid
/// definition, because array parameters already accept repeated occurrences.
///
/// ```
/// use purrassist_core::tool_call::{Param, ToolRepeatable};
/// assert_eq!(<Vec<String> as ToolRepeatable>::repeated(), Param::string().repeatable());
/// assert_eq!(<Option<Vec<u8>> as ToolRepeatable>::repeated(), Param::integer().repeatable());
/// ```
///
/// ```compile_fail
/// use purrassist_core::tool_call::ToolRepeatable;
/// let _ = <String as ToolRepeatable>::repeated();
/// ```
pub trait ToolRepeatable {
    /// The repeatable, unrequired specification of the item type.
    #[must_use]
    fn repeated() -> ParameterSpec;
}

/// A string-valued parameter type that accepts a `pattern` constraint
/// (`String`, `Option<String>`, `Vec<String>` and `Option<Vec<String>>`; for
/// vectors the pattern constrains each item).
///
/// A malformed pattern is reported by
/// [`ToolSet::compile`](super::ToolSet::compile), not here.
///
/// ```
/// use purrassist_core::tool_call::{Param, ToolPattern};
/// assert_eq!(<String as ToolPattern>::with_pattern("^a"), Param::string().pattern("^a"));
/// assert_eq!(
///     <Vec<String> as ToolPattern>::with_pattern("^a"),
///     Param::array(Param::string().pattern("^a")),
/// );
/// ```
///
/// ```compile_fail
/// use purrassist_core::tool_call::ToolPattern;
/// let _ = <u8 as ToolPattern>::with_pattern("^a");
/// ```
pub trait ToolPattern {
    /// The unrequired specification with `pattern` applied to the string.
    #[must_use]
    fn with_pattern(pattern: &'static str) -> ParameterSpec;
}

mod sealed {
    /// The private supertrait that closes [`super::ToolExtra`].
    pub trait Sealed {}
    impl Sealed for crate::json::Node {}
}

/// Marker for the type of a field that collects undeclared members
/// (`unknown = pass`): [`Node`], holding an object of those members.
///
/// The trait is sealed: `json::Node` is the only implementor, and the
/// derive builds the field through the hidden `__from_members` constructor.
/// It is not an extension point. Sealing it now leaves room to accept more
/// collection types later without breaking an implementor; unsealing a
/// sealed trait is compatible, sealing a published one is not.
///
/// ```
/// use purrassist_core::{json::Node, tool_call::ToolExtra};
/// fn extra<T: ToolExtra>() {}
/// extra::<Node>();
/// ```
///
/// ```compile_fail,E0277
/// use purrassist_core::tool_call::ToolExtra;
/// fn extra<T: ToolExtra>() {}
/// extra::<String>();
/// ```
///
/// A downstream implementation is rejected (the supertrait is private and
/// cannot be named, so it cannot be implemented):
///
/// ```compile_fail,E0277
/// use purrassist_core::{json::Member, tool_call::ToolExtra};
/// struct Mine;
/// impl ToolExtra for Mine {
///     fn __from_members(_: Vec<Member>) -> Self { Self }
/// }
/// ```
pub trait ToolExtra: sealed::Sealed + Sized {
    /// Build the field from the collected undeclared members, in member
    /// order. For generated code only.
    #[doc(hidden)]
    fn __from_members(members: Vec<Member>) -> Self;
}

impl ToolExtra for Node {
    fn __from_members(members: Vec<Member>) -> Self {
        Self::new(Value::Object(members))
    }
}

macro_rules! scalar_parameter {
    ($($target:ty => $spec:expr),* $(,)?) => {$(
        impl ToolParameter for $target {
            fn parameter() -> ParameterSpec {
                $spec
            }
        }
        impl ToolItem for $target {}
    )*};
}

scalar_parameter!(
    String => Param::string(),
    bool => Param::boolean(),
    f32 => Param::number(),
    f64 => Param::number(),
    Number => Param::number(),
    i8 => Param::integer(),
    i16 => Param::integer(),
    i32 => Param::integer(),
    i64 => Param::integer(),
    i128 => Param::integer(),
    isize => Param::integer(),
    u8 => Param::integer(),
    u16 => Param::integer(),
    u32 => Param::integer(),
    u64 => Param::integer(),
    u128 => Param::integer(),
    usize => Param::integer(),
);

impl<T: ToolItem> ToolParameter for Vec<T> {
    fn parameter() -> ParameterSpec {
        Param::array(T::parameter())
    }
    const DEPTH: usize = T::DEPTH;
}

impl<T: ToolItem> ToolItem for Vec<T> {}

/// `Option<T>` requires `T: ToolItem`, which rejects `Option<Option<T>>`
/// (JSON has a single `null`).
///
/// ```compile_fail
/// use purrassist_core::tool_call::ToolParameter;
/// fn parameter<T: ToolParameter>() {}
/// parameter::<Option<Option<String>>>();
/// ```
///
/// ```compile_fail
/// use purrassist_core::tool_call::ToolParameter;
/// fn parameter<T: ToolParameter>() {}
/// parameter::<Vec<Option<String>>>();
/// ```
impl<T: ToolItem> ToolParameter for Option<T> {
    fn parameter() -> ParameterSpec {
        T::parameter()
    }
    const REQUIRED: bool = false;
    fn absent() -> Option<Self> {
        Some(None)
    }
    const DEPTH: usize = T::DEPTH;
}

/// `Box<T>` is a parameter exactly as `T` is, forwarding `REQUIRED`,
/// `absent()` and `DEPTH`. It lets a field own a large nested value on the
/// heap, but it does **not** make recursion possible: `DEPTH` still forms a
/// cycle through the box, so a recursive tool definition remains a compile
/// error (`E0391`), as does `Option<Box<Self>>`.
///
/// ```
/// use purrassist_core::tool_call::{Param, ToolParameter};
/// assert_eq!(<Box<u8> as ToolParameter>::parameter(), Param::integer());
/// assert_eq!(<Option<Box<u8>> as ToolParameter>::absent(), Some(None));
/// assert_eq!(<Vec<Box<String>> as ToolParameter>::parameter(), Param::array(Param::string()));
/// ```
///
/// An indirect cycle through a `Box` is still a cycle:
///
/// ```compile_fail,E0391
/// use purrassist_core::tool_call::{FromNode, FromNodeError, ParameterSpec, ToolParameter, ToolItem};
/// use purrassist_core::json::Node;
///
/// struct A(Box<B>);
/// struct B(Vec<A>);
/// impl FromNode for A {
///     fn from_node(_: &Node) -> Result<Self, FromNodeError> { unimplemented!() }
/// }
/// impl FromNode for B {
///     fn from_node(_: &Node) -> Result<Self, FromNodeError> { unimplemented!() }
/// }
/// impl ToolParameter for A {
///     fn parameter() -> ParameterSpec { unimplemented!() }
///     const DEPTH: usize = 1 + <Box<B> as ToolParameter>::DEPTH;
/// }
/// impl ToolItem for A {}
/// impl ToolParameter for B {
///     fn parameter() -> ParameterSpec { unimplemented!() }
///     const DEPTH: usize = 1 + <Vec<A> as ToolParameter>::DEPTH;
/// }
/// impl ToolItem for B {}
/// const _: usize = <A as ToolParameter>::DEPTH;
/// ```
///
/// The twin whose chain ends in a scalar compiles:
///
/// ```
/// use purrassist_core::tool_call::{FromNode, FromNodeError, ParameterSpec, ToolParameter, ToolItem};
/// use purrassist_core::json::Node;
///
/// struct A(Box<B>);
/// struct B(Vec<u8>);
/// impl FromNode for A {
///     fn from_node(_: &Node) -> Result<Self, FromNodeError> { unimplemented!() }
/// }
/// impl FromNode for B {
///     fn from_node(_: &Node) -> Result<Self, FromNodeError> { unimplemented!() }
/// }
/// impl ToolParameter for A {
///     fn parameter() -> ParameterSpec { unimplemented!() }
///     const DEPTH: usize = 1 + <Box<B> as ToolParameter>::DEPTH;
/// }
/// impl ToolItem for A {}
/// impl ToolParameter for B {
///     fn parameter() -> ParameterSpec { unimplemented!() }
///     const DEPTH: usize = 1 + <Vec<u8> as ToolParameter>::DEPTH;
/// }
/// impl ToolItem for B {}
/// const _: usize = <A as ToolParameter>::DEPTH;
/// assert_eq!(<A as ToolParameter>::DEPTH, 2);
/// ```
impl<T: ToolItem> ToolParameter for Box<T> {
    fn parameter() -> ParameterSpec {
        T::parameter()
    }
    const REQUIRED: bool = T::REQUIRED;
    fn absent() -> Option<Self> {
        T::absent().map(Self::new)
    }
    const DEPTH: usize = T::DEPTH;
}

impl<T: ToolItem> ToolItem for Box<T> {}

impl<T: ToolItem> ToolRepeatable for Vec<T> {
    fn repeated() -> ParameterSpec {
        T::parameter().repeatable()
    }
}

impl<T: ToolItem> ToolRepeatable for Option<Vec<T>> {
    fn repeated() -> ParameterSpec {
        <Vec<T>>::repeated()
    }
}

impl ToolPattern for String {
    fn with_pattern(pattern: &'static str) -> ParameterSpec {
        Param::string().pattern(pattern)
    }
}

impl ToolPattern for Option<String> {
    fn with_pattern(pattern: &'static str) -> ParameterSpec {
        String::with_pattern(pattern)
    }
}

impl ToolPattern for Vec<String> {
    fn with_pattern(pattern: &'static str) -> ParameterSpec {
        Param::array(String::with_pattern(pattern))
    }
}

impl ToolPattern for Option<Vec<String>> {
    fn with_pattern(pattern: &'static str) -> ParameterSpec {
        <Vec<String>>::with_pattern(pattern)
    }
}

/// The greatest of `depths`, or `0` when empty: the `const` building block of
/// [`ToolParameter::DEPTH`] for derived types (`1 + max_depth(&[..])`).
///
/// ```
/// use purrassist_core::tool_call::{ToolParameter, max_depth};
/// const DEPTH: usize = 1 + max_depth(&[<String as ToolParameter>::DEPTH, 2, 1]);
/// assert_eq!(DEPTH, 3);
/// assert_eq!(max_depth(&[]), 0);
/// ```
#[must_use]
pub const fn max_depth(depths: &[usize]) -> usize {
    let mut greatest = 0;
    let mut index = 0;
    while index < depths.len() {
        if depths[index] > greatest {
            greatest = depths[index];
        }
        index += 1;
    }
    greatest
}

/// Build a [`Number`] from a spelling that a macro already validated against
/// the JSON number grammar.
///
/// Intended for generated code only (default values lowered by the derive).
///
/// # Panics
/// Panics when `spelling` is not a JSON number; callers must have validated
/// it, so a panic indicates a defect in the generator, never in user input.
///
/// ```
/// let number = purrassist_core::tool_call::__derive_number("-1.5e3");
/// assert_eq!(number.as_str(), "-1.5e3");
/// ```
///
/// ```should_panic
/// purrassist_core::tool_call::__derive_number("0x10");
/// ```
#[doc(hidden)]
#[must_use]
pub fn __derive_number(spelling: &'static str) -> Number {
    match Number::from_str(spelling) {
        Ok(number) => number,
        Err(error) => panic!("generated number {spelling:?} is not a JSON number: {error}"),
    }
}

/// [`is_valid_name`](super::is_valid_name) as a `const` check for generated
/// code: derives assert explicit tool, parameter and alias names with it, so
/// an invalid name is a compile error at the derive.
///
/// ```
/// const _: () = assert!(purrassist_core::tool_call::__valid_name("read_file"));
/// assert!(!purrassist_core::tool_call::__valid_name("read file"));
/// ```
#[doc(hidden)]
#[must_use]
pub const fn __valid_name(name: &str) -> bool {
    super::is_valid_name(name)
}

/// Whether `names` is in strictly ascending byte order (no duplicates), as a
/// `const` check for generated code. A derive asserts it for the sorted table
/// of declared member names that [`Members`] binary-searches, so a table that
/// the generator and the compiler disagree about is a compile error rather
/// than a release-only misclassification.
///
/// ```
/// use purrassist_core::tool_call::__sorted;
/// const _: () = assert!(__sorted(&["a", "ab", "b", "é"]));
/// assert!(__sorted(&[]));
/// assert!(__sorted(&["only"]));
/// assert!(!__sorted(&["b", "a"]));
/// assert!(!__sorted(&["a", "a"]));
/// assert!(!__sorted(&["é", "z"])); // byte order: 0xc3 sorts after `z`
/// ```
#[doc(hidden)]
#[must_use]
pub const fn __sorted(names: &[&str]) -> bool {
    let mut index = 1;
    while index < names.len() {
        if !bytes_less(names[index - 1].as_bytes(), names[index].as_bytes()) {
            return false;
        }
        index += 1;
    }
    true
}

/// Lexicographic byte order, strictly less.
const fn bytes_less(left: &[u8], right: &[u8]) -> bool {
    let mut at = 0;
    while at < left.len() && at < right.len() {
        if left[at] != right[at] {
            return left[at] < right[at];
        }
        at += 1;
    }
    left.len() < right.len()
}

/// How one member of an object relates to the declared member names.
///
/// ```
/// use purrassist_core::{json, tool_call::{Members, Slot}};
/// let node = json::parse(r#"{"a":1,"z":2}"#).unwrap();
/// let slots: Vec<_> = Members::new(&["a"], &[0], &node).unwrap().collect();
/// assert!(matches!(slots[0], Slot::Declared { index: 0, .. }));
/// assert!(matches!(slots[1], Slot::Undeclared(_)));
/// ```
///
/// The enum is `#[non_exhaustive]`, so a match outside this crate needs a
/// wildcard arm; generated conversions report a [`FromNodeError`] for it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Slot<'a> {
    /// The first occurrence of a declared member.
    Declared {
        /// Declaration-order index of the member.
        index: usize,
        /// The member occurrence.
        member: &'a Member,
    },
    /// A second or later occurrence of a declared member.
    Duplicate {
        /// Declaration-order index of the member.
        index: usize,
        /// The repeated occurrence.
        member: &'a Member,
    },
    /// A member whose name is not declared.
    Undeclared(&'a Member),
}

/// A single-pass classifier over the members of a JSON object.
///
/// `declared` is the sorted (strictly ascending, byte order) table of declared
/// member names and `order[i]` the declaration-order index of `declared[i]`.
/// Iterating yields one [`Slot`] per member, in member order, each found by
/// binary search over the table: O(n log k) for n members and k declared
/// names, with no per-field [`Node::get`] (which cannot tell a duplicate from
/// an absent member). A generated conversion matches on the slots, stores
/// each declared value by index in its own slot, and afterwards turns every
/// slot that stayed empty into the field's default, its
/// [`ToolParameter::absent`] value, or [`FromNodeError::missing`]. (Derived
/// conversions do not need [`is_present`](Self::is_present); hand-written
/// ones may use it instead of per-field slots.) Undeclared members are an
/// [`FromNodeError::unknown`] error, or are cloned into an `extra` object
/// under `unknown = pass`.
///
/// `new` always checks that the two tables have equal length. Debug builds
/// additionally check that `declared` is strictly sorted and that `order` is a
/// permutation of `0..declared.len()`. Release builds do not, so an unsorted
/// table or a non-permutation is a caller defect that yields misclassified
/// members (or an out-of-range panic), never undefined behavior. Generated
/// code removes the first hazard at compile time with the hidden `__sorted`
/// const check on its table.
///
/// ```
/// use purrassist_core::{json, tool_call::{FromNodeError, Members, Slot}};
/// // Declared in the order `path`, `limit`; sorted for lookup.
/// const NAMES: &[&str] = &["limit", "path"];
/// const ORDER: &[usize] = &[1, 0];
/// let node = json::parse(r#"{"path":"a","extra":1,"path":"b"}"#).unwrap();
/// let mut members = Members::new(NAMES, ORDER, &node).unwrap();
/// let mut errors = Vec::new();
/// for slot in &mut members {
///     match slot {
///         Slot::Declared { .. } => {}
///         Slot::Duplicate { member, .. } => errors.push(FromNodeError::duplicate(&member.key)),
///         Slot::Undeclared(member) => errors.push(FromNodeError::unknown(&member.key)),
///         _ => unreachable!("no other classification exists"),
///     }
/// }
/// assert!(members.is_present(0)); // `path`
/// assert!(!members.is_present(1)); // `limit`
/// assert_eq!(errors, [FromNodeError::unknown("extra"), FromNodeError::duplicate("path")]);
/// ```
#[derive(Debug)]
pub struct Members<'a> {
    declared: &'static [&'static str],
    order: &'static [usize],
    rest: core::slice::Iter<'a, Member>,
    seen: Vec<bool>,
    #[cfg(test)]
    comparisons: core::cell::Cell<usize>,
    #[cfg(test)]
    visited: usize,
}

/// Whether `order` holds each of `0..order.len()` exactly once.
fn is_permutation(order: &[usize]) -> bool {
    let mut used = vec![false; order.len()];
    order
        .iter()
        .all(|&index| index < used.len() && !core::mem::replace(&mut used[index], true))
}

impl<'a> Members<'a> {
    /// Begin classifying the members of the object `node`.
    ///
    /// # Errors
    /// [`FromNodeError::mismatch`] with expected `"object"` when `node` is
    /// not a JSON object.
    ///
    /// # Panics
    /// Panics when `declared` and `order` differ in length (a generator
    /// defect); debug builds also panic on an unsorted table.
    ///
    /// ```
    /// use purrassist_core::{json, tool_call::Members};
    /// let node = json::parse("[]").unwrap();
    /// let error = Members::new(&["a"], &[0], &node).unwrap_err();
    /// assert_eq!(error.expected, "object");
    /// ```
    pub fn new(
        declared: &'static [&'static str],
        order: &'static [usize],
        node: &'a Node,
    ) -> Result<Self, FromNodeError> {
        assert_eq!(
            declared.len(),
            order.len(),
            "declared names and declaration order must have equal length"
        );
        debug_assert!(
            declared.windows(2).all(|pair| pair[0] < pair[1]),
            "declared member names must be strictly sorted"
        );
        debug_assert!(
            is_permutation(order),
            "declaration indexes must be a permutation of 0..field count"
        );
        let Value::Object(members) = &node.value else {
            return Err(FromNodeError::mismatch("object", node));
        };
        Ok(Self {
            declared,
            order,
            rest: members.iter(),
            seen: vec![false; declared.len()],
            #[cfg(test)]
            comparisons: core::cell::Cell::new(0),
            #[cfg(test)]
            visited: 0,
        })
    }

    /// Whether a member with declaration index `index` has been yielded as
    /// [`Slot::Declared`] so far (after full iteration: whether it occurred).
    ///
    /// # Panics
    /// Panics when `index` is not below the number of declared names.
    #[must_use]
    pub fn is_present(&self, index: usize) -> bool {
        self.seen[index]
    }

    /// Binary search with early exit, counting comparisons under test.
    fn lookup(&self, key: &str) -> Option<usize> {
        let (mut low, mut high) = (0, self.declared.len());
        while low < high {
            let middle = low + (high - low) / 2;
            #[cfg(test)]
            {
                self.comparisons.set(self.comparisons.get() + 1);
            }
            match key.cmp(self.declared[middle]) {
                core::cmp::Ordering::Less => high = middle,
                core::cmp::Ordering::Greater => low = middle + 1,
                core::cmp::Ordering::Equal => return Some(self.order[middle]),
            }
        }
        None
    }
}

impl<'a> Iterator for Members<'a> {
    type Item = Slot<'a>;

    fn next(&mut self) -> Option<Slot<'a>> {
        let member = self.rest.next()?;
        #[cfg(test)]
        {
            self.visited += 1;
        }
        Some(match self.lookup(&member.key) {
            None => Slot::Undeclared(member),
            Some(index) if self.seen[index] => Slot::Duplicate { index, member },
            Some(index) => {
                self.seen[index] = true;
                Slot::Declared { index, member }
            }
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rest.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    /// 64 names in sorted order; declaration order is the reverse.
    fn names() -> &'static [&'static str] {
        let names: Vec<&'static str> = (0..64)
            .map(|index| &*Box::leak(format!("field_{index:02}").into_boxed_str()))
            .collect();
        Box::leak(names.into_boxed_slice())
    }

    fn reverse_order() -> &'static [usize] {
        Box::leak((0..64).rev().collect::<Vec<_>>().into_boxed_slice())
    }

    #[test]
    fn classifies_64_members_in_one_pass_within_the_comparison_bound() {
        let declared = names();
        // Members arrive in an order unrelated to the table.
        let text = format!(
            "{{{}}}",
            (0..64)
                .map(|index| format!("\"field_{:02}\":{index}", (index * 37) % 64))
                .collect::<Vec<_>>()
                .join(",")
        );
        let node = json::parse(&text).unwrap();
        let mut members = Members::new(declared, reverse_order(), &node).unwrap();
        let mut yielded = 0;
        for slot in &mut members {
            yielded += 1;
            let Slot::Declared { index, member } = slot else {
                panic!("every member is declared once");
            };
            assert_eq!(member.key, format!("field_{:02}", 63 - index));
        }
        assert_eq!(yielded, 64);
        assert_eq!(members.visited, 64, "each member is visited exactly once");
        assert!((0..64).all(|index| members.is_present(index)));
        let bound = 64 * 6; // 64 * ceil(log2 64)
        assert!(
            members.comparisons.get() <= bound,
            "{} comparisons exceed {bound}",
            members.comparisons.get()
        );
        assert!(
            members.comparisons.get() >= 64,
            "every member needs a comparison"
        );
    }

    #[test]
    fn undeclared_lookups_cost_at_most_log_plus_one() {
        let node = json::parse(r#"{"aaa":1,"zzz":2,"field_10x":3}"#).unwrap();
        let mut members = Members::new(names(), reverse_order(), &node).unwrap();
        assert!(
            members
                .by_ref()
                .all(|slot| matches!(slot, Slot::Undeclared(_)))
        );
        assert!(members.comparisons.get() <= 3 * 7);
    }

    #[test]
    fn duplicates_and_undeclared_are_distinguished_from_missing() {
        // Declared `b` then `a`; the sorted table is `a`, `b`.
        const SORTED: &[&str] = &["a", "b"];
        const ORDER: &[usize] = &[1, 0];
        let node = json::parse(r#"{"a":1,"a":2,"c":3}"#).unwrap();
        let mut members = Members::new(SORTED, ORDER, &node).unwrap();
        let slots: Vec<_> = members.by_ref().collect();
        assert!(matches!(slots[0], Slot::Declared { index: 1, .. }));
        assert!(matches!(slots[1], Slot::Duplicate { index: 1, .. }));
        assert!(matches!(slots[2], Slot::Undeclared(m) if m.key == "c"));
        assert!(members.is_present(1));
        assert!(!members.is_present(0));
    }

    #[test]
    fn empty_table_and_empty_object() {
        let node = json::parse("{}").unwrap();
        assert_eq!(Members::new(&[], &[], &node).unwrap().count(), 0);
        let node = json::parse(r#"{"a":1}"#).unwrap();
        let slot = Members::new(&[], &[], &node).unwrap().next().unwrap();
        assert!(matches!(slot, Slot::Undeclared(_)));
    }

    #[test]
    fn max_depth_is_const_evaluable() {
        const DEPTH: usize = max_depth(&[0, 3, 2]);
        assert_eq!(DEPTH, 3);
        assert_eq!(max_depth(&[]), 0);
    }

    #[test]
    fn parameters_match_the_builders() {
        assert_eq!(
            <Option<Vec<String>>>::parameter(),
            Param::array(Param::string())
        );
        assert_eq!(
            <Vec<Vec<u8>>>::parameter(),
            Param::array(Param::array(Param::integer()))
        );
        assert_eq!(<Number as ToolParameter>::parameter(), Param::number());
        assert_eq!(
            <Option<Vec<String>>>::with_pattern("x"),
            Param::array(Param::string().pattern("x"))
        );
        assert_eq!(<Vec<Vec<u8>> as ToolParameter>::DEPTH, 0);
    }

    #[test]
    fn sorted_is_strict_byte_order_and_const_evaluable() {
        const { assert!(__sorted(&["", "a", "a_", "b", "é", "日"])) };
        assert!(__sorted(&[]));
        for unsorted in [&["b", "a"][..], &["a", "a"], &["ab", "a"], &["é", "z"]] {
            assert!(!__sorted(unsorted), "{unsorted:?}");
        }
        // Agrees with `str` ordering on a spread of names.
        let names = ["a", "a.b", "a_b", "a-b", "A", "ab", "z", "é"];
        for left in names {
            for right in names {
                assert_eq!(__sorted(&[left, right]), left < right, "{left} {right}");
            }
        }
    }

    #[test]
    fn extra_collects_members_as_an_object() {
        let node = json::parse(r#"{"a":1}"#).unwrap();
        let Value::Object(members) = node.value.clone() else {
            unreachable!()
        };
        assert_eq!(
            <Node as ToolExtra>::__from_members(members).value,
            node.value
        );
        assert_eq!(
            <Node as ToolExtra>::__from_members(Vec::new()).value,
            Value::Object(Vec::new())
        );
    }

    #[test]
    fn box_and_f32_parameters_match_their_targets() {
        assert_eq!(<f32 as ToolParameter>::parameter(), Param::number());
        assert_eq!(<Box<f32> as ToolParameter>::parameter(), Param::number());
        const { assert!(<Box<String> as ToolParameter>::REQUIRED) };
        assert_eq!(<Box<String> as ToolParameter>::absent(), None);
        assert_eq!(<Box<Vec<Vec<u8>>> as ToolParameter>::DEPTH, 0);
        assert_eq!(<Option<Box<u8>> as ToolParameter>::absent(), Some(None));
    }

    #[test]
    fn non_ascii_names_order_by_bytes() {
        // "z" (0x7a) < "é" (0xc3 0xa9) < "日" (0xe6 ..) by bytes.
        const NAMES: &[&str] = &["z", "é", "日"];
        const ORDER: &[usize] = &[2, 0, 1];
        let node = json::parse(r#"{"日":1,"é":2,"z":3,"ê":4}"#).unwrap();
        let slots: Vec<_> = Members::new(NAMES, ORDER, &node).unwrap().collect();
        assert!(matches!(slots[0], Slot::Declared { index: 1, .. }));
        assert!(matches!(slots[1], Slot::Declared { index: 0, .. }));
        assert!(matches!(slots[2], Slot::Declared { index: 2, .. }));
        assert!(matches!(slots[3], Slot::Undeclared(_)));
    }

    #[test]
    fn member_errors_escape_pointer_segments() {
        for error in [
            FromNodeError::missing("a/b~c"),
            FromNodeError::duplicate("a/b~c"),
            FromNodeError::unknown("a/b~c"),
        ] {
            assert_eq!(error.pointer, "/a~1b~0c");
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "strictly sorted")]
    fn unsorted_table_panics_in_debug() {
        let node = json::parse("{}").unwrap();
        let _ = Members::new(&["b", "a"], &[0, 1], &node);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "permutation")]
    fn non_permutation_order_panics_in_debug() {
        let node = json::parse("{}").unwrap();
        let _ = Members::new(&["a", "b"], &[0, 0], &node);
    }
}
