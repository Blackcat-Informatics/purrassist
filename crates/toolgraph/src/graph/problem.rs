// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Typed refusals of [`ToolGraph::load`](super::ToolGraph::load).

use std::fmt;

use purrassist_core::{StopReason, output::FieldMaskError};

/// How a node violates the vocabulary's structural shape.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Violation {
    /// A required property is absent.
    Missing,
    /// A single-valued property has more than one value.
    TooMany {
        /// Number of values found.
        found: usize,
    },
    /// The value has the wrong RDF kind or literal datatype.
    WrongKind {
        /// The accepted kind, such as `xsd:string literal`.
        expected: &'static str,
    },
    /// The value is of the right kind but outside the accepted value space.
    OutOfRange {
        /// The accepted values, such as `a positive integer that fits u64`.
        expected: &'static str,
    },
    /// The predicate belongs to a different node role than the subject's.
    Domain {
        /// The role the subject was recognised as.
        subject_role: &'static str,
    },
    /// The value names a node that is absent or lacks the required role.
    Unresolved,
    /// A value that must be unique is repeated.
    Duplicate,
    /// A nesting relation that must be acyclic reaches itself.
    Cycle,
    /// A mutually exclusive property pair is both present or both absent.
    Exclusive,
    /// A free-form explanation from an upstream check.
    Detail(String),
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("required property is missing"),
            Self::TooMany { found } => write!(f, "single-valued property has {found} values"),
            Self::WrongKind { expected } => write!(f, "expected {expected}"),
            Self::OutOfRange { expected } => write!(f, "value must be {expected}"),
            Self::Domain { subject_role } => {
                write!(f, "predicate is not valid on a {subject_role} node")
            }
            Self::Unresolved => f.write_str("value names no node with the required role"),
            Self::Duplicate => f.write_str("value must be unique"),
            Self::Cycle => f.write_str("nesting relation is cyclic"),
            Self::Exclusive => f.write_str("exactly one of the exclusive properties is required"),
            Self::Detail(text) => f.write_str(text),
        }
    }
}

/// An RDF construct the loader does not support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Construct {
    /// A statement in a named graph.
    NamedGraph,
    /// A statement no view owns: not `FnO`, vocabulary, shape or declaration.
    UnownedStatement,
    /// A reifier of a statement that is not a parameter path declaration.
    Reifier,
    /// A runtime fact predicate or class that a static tool graph must not assert.
    RuntimeFact,
    /// An `FnO` function that is not typed as a tool.
    FunctionWithoutTool,
    /// A shape path that is not a single IRI.
    ComplexPath,
}

impl fmt::Display for Construct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NamedGraph => "named-graph statement",
            Self::UnownedStatement => "statement owned by no view",
            Self::Reifier => "reifier of a statement that is not a parameter path",
            Self::RuntimeFact => "runtime fact asserted in a static tool graph",
            Self::FunctionWithoutTool => "FnO function not typed as a tool",
            Self::ComplexPath => "shape path that is not a single IRI",
        })
    }
}

/// The exact extent of a statement in the source text.
///
/// It spans the statement's predicate and object as written (or, for a term the
/// grammar generates, the syntax that introduced it), so it points into the
/// original input rather than at a rebuilt serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceExtent {
    /// Byte offset of the first byte, from the start of the input.
    pub start: usize,
    /// Byte offset one past the last byte.
    pub end: usize,
    /// The 1-based line of `start`.
    pub line: u64,
    /// The 1-based column of `start`, counted in characters.
    pub column: u32,
}

impl SourceExtent {
    /// The extent of `range` within `source`, with its starting line and column.
    pub(super) fn new(source: &[u8], range: std::ops::Range<usize>) -> Self {
        let start = range.start.min(source.len());
        let before = &source[..start];
        let line_start = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |at| at + 1);
        let line = before.split(|&b| b == b'\n').count() as u64;
        let column = std::str::from_utf8(&before[line_start..])
            .map_or(start - line_start, |text| text.chars().count());
        Self {
            start: range.start,
            end: range.end,
            line,
            column: u32::try_from(column + 1).unwrap_or(u32::MAX),
        }
    }
}

impl fmt::Display for SourceExtent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// Why a tool graph was refused.
///
/// Every variant names its subject; predicate-level variants also name the
/// predicate. [`UnknownTerm`](Self::UnknownTerm) and
/// [`UnsupportedConstruct`](Self::UnsupportedConstruct) also carry the exact
/// [`SourceExtent`] of the statement they refuse when the governed parser
/// retained it; variants that name a node rather than one statement do not (see
/// the stated limits in the `graph` module documentation).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum LoadError {
    /// The RDF text did not parse.
    Parse {
        /// The parser's stable diagnostic code.
        code: String,
        /// The parser's message.
        message: String,
        /// The parser's 1-based line, when known.
        line: Option<u64>,
        /// The parser's 1-based column, when known.
        column: Option<u32>,
    },
    /// A term in the caller's namespace is not one of the vocabulary's bound terms,
    /// or a predicate is not allowed on a node of this role.
    UnknownTerm {
        /// The subject of the offending statement.
        subject: String,
        /// The predicate of the offending statement.
        predicate: String,
        /// The offending term.
        term: String,
        /// The first matching statement in the source, when the parser retained it.
        at: Option<SourceExtent>,
    },
    /// A tool, context, or shape-reference node violates the vocabulary's shape.
    InvalidShape {
        /// The offending node.
        subject: String,
        /// The offending or missing predicate.
        predicate: String,
        /// The violation.
        violation: Violation,
    },
    /// The `FnO` catalog or a tool's callable binding was refused.
    InvalidFnoBinding {
        /// The offending function or catalog node.
        subject: String,
        /// The offending predicate when the upstream check names one.
        predicate: Option<String>,
        /// The upstream explanation.
        detail: String,
    },
    /// A reified parameter hint is malformed or attaches to no parameter.
    InvalidHint {
        /// The offending hint or reifier node.
        subject: String,
        /// The offending or missing predicate.
        predicate: String,
        /// The violation.
        violation: Violation,
    },
    /// An execution profile, waiver or rule node is malformed.
    InvalidExecutionProfile {
        /// The offending node.
        subject: String,
        /// The offending or missing predicate.
        predicate: String,
        /// The violation.
        violation: Violation,
    },
    /// A construct the loader refuses rather than skips.
    UnsupportedConstruct {
        /// The subject of the offending statement.
        subject: String,
        /// The predicate of the offending statement.
        predicate: String,
        /// The refused construct.
        construct: Construct,
        /// The first matching statement in the source, when the parser retained it.
        at: Option<SourceExtent>,
    },
    /// `outputProjectionJq` is not in the field-mask subset.
    UnsupportedProjection {
        /// The context node.
        subject: String,
        /// The projection text, verbatim.
        projection: String,
        /// The field-mask parser's refusal with its byte offset.
        error: FieldMaskError,
    },
    /// The caller's budget (work, input bytes or scratch storage) stopped the load,
    /// in this crate's accounting or in the governed parser or `FnO` reader.
    Exhausted(StopReason),
    /// The caller cancelled the load.
    Cancelled,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                code,
                message,
                line,
                column,
            } => write!(
                f,
                "RDF parse failure {code} at {line:?}:{column:?}: {message}"
            ),
            Self::UnknownTerm {
                subject,
                predicate,
                term,
                at,
            } => {
                write!(
                    f,
                    "{subject} {predicate}: term {term} is not in the vocabulary"
                )?;
                at.iter().try_for_each(|at| write!(f, " ({at})"))
            }
            Self::InvalidShape {
                subject,
                predicate,
                violation,
            }
            | Self::InvalidHint {
                subject,
                predicate,
                violation,
            }
            | Self::InvalidExecutionProfile {
                subject,
                predicate,
                violation,
            } => write!(f, "{subject} {predicate}: {violation}"),
            Self::InvalidFnoBinding {
                subject,
                predicate,
                detail,
            } => write!(
                f,
                "{subject} {}: {detail}",
                predicate.as_deref().unwrap_or("-")
            ),
            Self::UnsupportedConstruct {
                subject,
                predicate,
                construct,
                at,
            } => {
                write!(f, "{subject} {predicate}: unsupported {construct}")?;
                at.iter().try_for_each(|at| write!(f, " ({at})"))
            }
            Self::UnsupportedProjection {
                subject,
                projection,
                error,
            } => write!(
                f,
                "{subject}: projection {projection:?} is not a field mask: {error}"
            ),
            Self::Exhausted(reason) => write!(f, "tool graph load stopped: {reason:?}"),
            Self::Cancelled => f.write_str("tool graph load cancelled"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnsupportedProjection { error, .. } => Some(error),
            _ => None,
        }
    }
}
