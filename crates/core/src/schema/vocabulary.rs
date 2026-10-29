// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::reference::{copy, lookup_work, work};
use super::{CompilationError, DIALECT, DRAFT_07, SchemaError, build::Context};
use crate::contract::format_scratch_governed;
use crate::json::{Node, Value};
use crate::{Governor, ScratchString, ScratchVec, StopReason};

/// A JSON Schema dialect this engine implements natively.
///
/// A schema resource selects its dialect with `$schema`; one without
/// `$schema` inherits the dialect of its enclosing schema (or, for a
/// separately supplied resource, of the schema that first referenced it),
/// and a root without `$schema` is Draft 2020-12. Any other `$schema` URI
/// must name a caller-supplied Draft 2020-12 style meta-schema.
///
/// ```
/// use purrassist_core::schema::{Dialect, Schema};
/// let draft07 = Schema::compile(
///     r#"{"$schema":"http://json-schema.org/draft-07/schema#","items":[{"type":"string"}]}"#,
/// )?;
/// assert_eq!(draft07.dialect(), Dialect::Draft07);
/// assert_eq!(draft07.dialect().validator(), "purrassist.schema.draft-07.v1");
/// assert_eq!(Schema::compile("{}")?.dialect(), Dialect::Draft202012);
/// assert_eq!(Dialect::from_uri("http://json-schema.org/draft-07/schema"), Some(Dialect::Draft07));
/// assert_eq!(Dialect::from_uri("http://json-schema.org/draft-04/schema#"), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dialect {
    /// Draft 2020-12 with the vocabularies its `$schema` meta-schema declares.
    #[default]
    Draft202012,
    /// Draft-07: tuple `items` with `additionalItems`, `definitions`,
    /// `dependencies`, `$ref` overriding its sibling keywords, and `$id`
    /// plain-name fragments as anchors; `format` is an annotation only.
    Draft07,
}

impl Dialect {
    /// The dialect a `$schema` URI names, ignoring an empty trailing
    /// fragment; `None` for any URI this engine does not implement natively
    /// (including a custom meta-schema, which compilation then requires as an
    /// explicitly supplied resource).
    #[must_use]
    pub fn from_uri(uri: &str) -> Option<Self> {
        match uri.trim_end_matches('#') {
            DIALECT => Some(Self::Draft202012),
            DRAFT_07 => Some(Self::Draft07),
            _ => None,
        }
    }

    /// The canonical meta-schema URI of this dialect, without a fragment.
    #[must_use]
    pub const fn uri(self) -> &'static str {
        match self {
            Self::Draft202012 => DIALECT,
            Self::Draft07 => DRAFT_07,
        }
    }

    /// The validator identity recorded by
    /// [`super::ValidationReport::to_validation`] for this dialect.
    #[must_use]
    pub const fn validator(self) -> &'static str {
        match self {
            Self::Draft202012 => "purrassist.schema.draft2020-12.v1",
            Self::Draft07 => "purrassist.schema.draft-07.v1",
        }
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "JSON Schema vocabularies are independent capability flags"
)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Vocabularies {
    pub validation: bool,
    applicator: bool,
    unevaluated: bool,
    content: bool,
    /// Whether the dialect's `$vocabulary` declared the format-assertion
    /// vocabulary URI at all (`true` or `false` both count: the dialect
    /// treats either boolean as "known vocabulary", since we recognize it
    /// either way; only its *absence* leaves `format` annotation-only).
    pub format_assert: bool,
    /// Which keyword set and `$ref`/`$id` rules apply.
    pub dialect: Dialect,
}
impl Default for Vocabularies {
    fn default() -> Self {
        Self {
            validation: true,
            applicator: true,
            unevaluated: true,
            content: true,
            format_assert: false,
            dialect: Dialect::Draft202012,
        }
    }
}
impl Vocabularies {
    /// Loads the dialect's vocabulary flags, plus every unrecognized
    /// `$vocabulary` URI declared with `false` (required-but-unknown fails
    /// compilation immediately below instead).
    pub(super) fn load(
        dialect: &str,
        context: &mut Context<'_>,
    ) -> Result<(Self, ScratchVec<ScratchString>), CompilationError> {
        let mut end = dialect.len();
        while end > 0 {
            work(context.governor, 1)?;
            if dialect.as_bytes()[end - 1] != b'#' {
                break;
            }
            end -= 1;
        }
        let uri = &dialect[..end];
        work(
            context.governor,
            uri.len().min(DIALECT.len()).saturating_add(1),
        )?;
        if uri == DIALECT {
            return Ok((Self::default(), ScratchVec::new(context.governor)));
        }
        work(
            context.governor,
            uri.len().min(DRAFT_07.len()).saturating_add(1),
        )?;
        if uri == DRAFT_07 {
            return Ok((Self::draft07(), ScratchVec::new(context.governor)));
        }
        lookup_work(context.resources.len(), uri.len(), context.governor)?;
        let Some(source) = context.resources.get(uri) else {
            return Err(vocabulary_error(
                uri,
                format_args!("unresolved dialect; supply its meta-schema explicitly"),
                context.governor,
            ));
        };
        // DOM parsing and its syntax diagnostics retain their separate admission.
        let meta = context.parse(source, uri)?;
        let Some(declaration) = super::compile::member(&meta, "$vocabulary", context.governor)?
        else {
            return Ok((Self::default(), ScratchVec::new(context.governor)));
        };
        let Value::Object(entries) = &declaration.value else {
            return Err(vocabulary_error(
                uri,
                format_args!("$vocabulary must be an object"),
                context.governor,
            ));
        };
        let mut result = Self {
            validation: false,
            applicator: false,
            unevaluated: false,
            content: false,
            format_assert: false,
            dialect: Dialect::Draft202012,
        };
        let mut unsupported = ScratchVec::new(context.governor);
        for entry in entries {
            // Bound the eight fixed URI alternatives and entry classification.
            work(
                context.governor,
                entry.key.len().saturating_add(1).saturating_mul(9),
            )?;
            let Value::Bool(required) = entry.value.value else {
                return Err(vocabulary_error(
                    uri,
                    format_args!("vocabulary requirement must be boolean"),
                    context.governor,
                ));
            };
            match entry.key.as_str() {
                "https://json-schema.org/draft/2020-12/vocab/core"
                | "https://json-schema.org/draft/2020-12/vocab/meta-data"
                | "https://json-schema.org/draft/2020-12/vocab/format-annotation" => {}
                "https://json-schema.org/draft/2020-12/vocab/validation" => {
                    result.validation = true;
                }
                "https://json-schema.org/draft/2020-12/vocab/applicator" => {
                    result.applicator = true;
                }
                "https://json-schema.org/draft/2020-12/vocab/unevaluated" => {
                    result.unevaluated = true;
                }
                "https://json-schema.org/draft/2020-12/vocab/content" => result.content = true,
                "https://json-schema.org/draft/2020-12/vocab/format-assertion" => {
                    result.format_assert = true;
                }
                unknown if required => {
                    return Err(vocabulary_error(
                        uri,
                        format_args!("unsupported required vocabulary: {unknown}"),
                        context.governor,
                    ));
                }
                unknown => {
                    super::reference::work(context.governor, 1)?;
                    if unsupported.len() == unsupported.capacity() {
                        super::reference::work(context.governor, unsupported.len())?;
                    }
                    unsupported.push(super::reference::copy(unknown, context.governor)?)?;
                }
            }
        }
        Ok((result, unsupported))
    }
    /// Draft-07's fixed keyword set: no vocabularies, and `format` is an
    /// annotation only.
    pub(super) const fn draft07() -> Self {
        Self {
            validation: true,
            applicator: true,
            unevaluated: false,
            content: true,
            format_assert: false,
            dialect: Dialect::Draft07,
        }
    }
    /// Whether `keyword` has defined behavior under this dialect and its
    /// declared vocabularies. Keywords that belong only to the other dialect
    /// are unknown, hence ignored, annotations.
    pub(super) fn enabled(self, keyword: &str) -> bool {
        let foreign = match self.dialect {
            Dialect::Draft202012 => matches!(keyword, "additionalItems" | "dependencies"),
            Dialect::Draft07 => matches!(
                keyword,
                "$defs"
                    | "$anchor"
                    | "$dynamicRef"
                    | "$dynamicAnchor"
                    | "$vocabulary"
                    | "prefixItems"
                    | "dependentRequired"
                    | "dependentSchemas"
                    | "minContains"
                    | "maxContains"
                    | "contentSchema"
            ),
        };
        if foreign {
            return false;
        }
        match keyword {
            "type" | "enum" | "const" | "multipleOf" | "maximum" | "exclusiveMaximum"
            | "minimum" | "exclusiveMinimum" | "maxLength" | "minLength" | "pattern"
            | "maxItems" | "minItems" | "uniqueItems" | "maxContains" | "minContains"
            | "maxProperties" | "minProperties" | "required" | "dependentRequired" => {
                self.validation
            }
            "prefixItems"
            | "items"
            | "additionalItems"
            | "dependencies"
            | "contains"
            | "additionalProperties"
            | "properties"
            | "patternProperties"
            | "dependentSchemas"
            | "propertyNames"
            | "if"
            | "then"
            | "else"
            | "allOf"
            | "anyOf"
            | "oneOf"
            | "not" => self.applicator,
            "unevaluatedItems" | "unevaluatedProperties" => self.unevaluated,
            "contentSchema" | "contentEncoding" | "contentMediaType" => self.content,
            _ => true,
        }
    }
    /// Whether `keyword` on schema object `node` takes effect: it is
    /// [`Self::enabled`], and, under Draft-07, `node` has no `$ref` or the
    /// keyword is `$ref` itself (Draft-07 `$ref` overrides every sibling;
    /// `$schema` still selects the dialect that decides this).
    pub(super) fn applies(self, node: &Node, keyword: &str) -> bool {
        self.enabled(keyword)
            && (self.dialect != Dialect::Draft07
                || matches!(keyword, "$ref" | "$schema")
                || node.get("$ref").is_none())
    }
}
pub(super) struct Keywords<'a> {
    pub node: &'a Node,
    pub vocabularies: Vocabularies,
}
impl<'a> Keywords<'a> {
    pub(super) fn get(&self, keyword: &str) -> Option<&'a Node> {
        self.vocabularies
            .applies(self.node, keyword)
            .then(|| self.node.get(keyword))
            .flatten()
    }
}
impl core::ops::Deref for Keywords<'_> {
    type Target = Node;
    fn deref(&self) -> &Node {
        self.node
    }
}

fn vocabulary_error(
    uri: &str,
    message: core::fmt::Arguments<'_>,
    governor: &mut Governor,
) -> CompilationError {
    let result = (|| {
        let location = copy(uri, governor)?;
        let message = format_scratch_governed(message, governor)?;
        governor.check()?;
        Ok::<_, StopReason>(SchemaError {
            location: location.into_string(),
            message: message.into_string(),
        })
    })();
    match result {
        Ok(error) => CompilationError::InvalidSchema(error),
        Err(reason) => CompilationError::Resource(reason),
    }
}
