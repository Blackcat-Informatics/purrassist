// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The first-party writer: a validated [`Item`] to Rust source text.
//!
//! The text is turned into a `TokenStream` by `FromStr` at the boundary. Two
//! kinds of names in it are special. `__pa_splice_N` stands for tokens copied
//! from the derive input (field types, the type name), so that rustc's errors
//! keep pointing at the user's tokens; `__pa_*` identifiers are generated
//! locals and are given `Span::mixed_site()` hygiene. Every path is absolute.

use crate::model::{
    DefaultValue, Effects, Enumeration, Extra, Item, Param, Policy, Span, Text, Tool, Unknown,
};
use core::fmt::Write as _;

/// Which crate path the generated code names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    /// `::purrassist_core`, for users of the core crate.
    Core,
    /// `::purrassist::__core`, for users of the facade crate.
    Facade,
}

impl Flavor {
    const fn root(self) -> &'static str {
        match self {
            Self::Core => "::purrassist_core",
            Self::Facade => "::purrassist::__core",
        }
    }
}

/// The prefix of an identifier standing for spliced input tokens.
pub(crate) const SPLICE_PREFIX: &str = "__pa_splice_";
/// The prefix of generated local names.
pub(crate) const LOCAL_PREFIX: &str = "__pa_";

/// Generated source text and the input tokens its placeholders stand for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Output {
    pub(crate) text: String,
    /// `splices[n]` are the spans of the input tokens that replace
    /// `__pa_splice_n`.
    pub(crate) splices: Vec<Vec<Span>>,
}

/// Write the impls for `item`.
pub(crate) fn generate(item: &Item, flavor: Flavor) -> Output {
    let mut writer = Writer {
        tc: format!("{}::tool_call", flavor.root()),
        js: format!("{}::json", flavor.root()),
        splices: Vec::new(),
    };
    let text = match item {
        Item::Tool(tool) => writer.tool(tool),
        Item::Enumeration(enumeration) => writer.enumeration(enumeration),
    };
    Output {
        text,
        splices: writer.splices,
    }
}

struct Writer {
    /// `<root>::tool_call`.
    tc: String,
    /// `<root>::json`.
    js: String,
    splices: Vec<Vec<Span>>,
}

impl Writer {
    fn splice(&mut self, spans: impl IntoIterator<Item = Span>) -> String {
        self.splices.push(spans.into_iter().collect());
        format!("{SPLICE_PREFIX}{}", self.splices.len() - 1)
    }

    fn ty(&mut self, ty: &[crate::model::Tree]) -> String {
        self.splice(ty.iter().map(|tree| tree.span))
    }

    fn default_node(&self, value: &DefaultValue) -> String {
        let js = &self.js;
        let inner = match value {
            DefaultValue::Null => format!("{js}::Value::Null"),
            DefaultValue::Bool(flag) => format!("{js}::Value::Bool({flag})"),
            DefaultValue::Number(spelling) => format!(
                "{js}::Value::Number({}::__derive_number({spelling:?}))",
                self.tc
            ),
            DefaultValue::Str(source) => {
                format!("{js}::Value::String(::std::string::String::from({source}))")
            }
            DefaultValue::Array(items) => {
                let items: Vec<String> = items.iter().map(|item| self.default_node(item)).collect();
                format!("{js}::Value::Array(::std::vec![{}])", items.join(", "))
            }
            DefaultValue::Object(members) => {
                let members: Vec<String> = members
                    .iter()
                    .map(|(key, member)| {
                        format!(
                            "{js}::Member {{ key: ::std::string::String::from({key}), key_span: \
                             ::core::option::Option::None, value: {} }}",
                            self.default_node(member)
                        )
                    })
                    .collect();
                format!("{js}::Value::Object(::std::vec![{}])", members.join(", "))
            }
        };
        format!("{js}::Node::new({inner})")
    }

    /// The `ParameterSpec` expression of one field.
    fn param_spec(&mut self, param: &Param) -> String {
        let tc = self.tc.clone();
        let ty = self.ty(&param.ty);
        let base = match (&param.pattern, param.repeat) {
            (None, false) => format!("<{ty} as {tc}::ToolParameter>::parameter()"),
            (None, true) => format!("<{ty} as {tc}::ToolRepeatable>::repeated()"),
            (Some(pattern), false) => {
                format!(
                    "<{ty} as {tc}::ToolPattern>::with_pattern({})",
                    pattern.source
                )
            }
            (Some(pattern), true) => {
                // The item carries the pattern; the bound is asserted without
                // calling the array-shaped `with_pattern`.
                let ty_again = self.ty(&param.ty);
                format!(
                    "{{ let _: fn(&'static str) -> {tc}::ParameterSpec = \
                     <{ty_again} as {tc}::ToolPattern>::with_pattern; \
                     <{ty} as {tc}::ToolRepeatable>::repeated().pattern({}) }}",
                    pattern.source
                )
            }
        };
        let mut spec = if param.default.is_some() {
            base
        } else {
            let ty_again = self.ty(&param.ty);
            format!(
                "{{ let __pa_spec = {base}; if <{ty_again} as {tc}::ToolParameter>::REQUIRED {{ \
                 __pa_spec.required() }} else {{ __pa_spec }} }}"
            )
        };
        for alias in &param.aliases {
            let _ = write!(spec, ".alias({})", alias.source);
        }
        if let Some(description) = &param.description {
            let _ = write!(spec, ".description({})", description.source);
        }
        if let Some(default) = &param.default {
            let _ = write!(spec, ".default({})", self.default_node(default));
        }
        spec
    }

    fn unknown_call(&self, unknown: Option<Unknown>) -> String {
        unknown.map_or_else(String::new, |unknown| {
            let variant = match unknown {
                Unknown::Error => "Error",
                Unknown::Drop => "DropWithLoss",
                Unknown::Pass => "PassThrough",
            };
            format!(
                ".unknown_parameters({}::UnknownParameters::{variant})",
                self.tc
            )
        })
    }

    fn params_calls(&mut self, tool: &Tool) -> String {
        let mut calls = String::new();
        for param in &tool.params {
            let _ = write!(
                calls,
                ".param({}, {})",
                param.key.source,
                self.param_spec(param)
            );
        }
        calls
    }

    fn tool(&mut self, tool: &Tool) -> String {
        let tc = self.tc.clone();
        let name = self.splice([tool.ident.span]);
        let mut definition = format!("{tc}::ToolDefinition::new({})", tool.name.source);
        for alias in &tool.aliases {
            let _ = write!(definition, ".alias({})", alias.source);
        }
        if let Some(description) = &tool.description {
            let _ = write!(definition, ".description({})", description.source);
        }
        if let Some(effects) = tool.effects {
            let variant = match effects {
                Effects::Mutating => "Mutating",
                Effects::ReadOnly => "ReadOnly",
            };
            let _ = write!(definition, ".effects({tc}::Effects::{variant})");
        }
        if let Some(policy) = tool.policy {
            let variant = match policy {
                Policy::Structural => "Structural",
                Policy::Tolerant => "Tolerant",
            };
            let _ = write!(definition, ".policy({tc}::RepairPolicy::{variant})");
        }
        definition.push_str(&self.unknown_call(tool.unknown));
        definition.push_str(&self.params_calls(tool));
        let object = format!(
            "{tc}::ObjectSpec::new(){}{}",
            self.unknown_call(tool.unknown),
            self.params_calls(tool)
        );
        let description = tool
            .description
            .as_ref()
            .map_or_else(String::new, |text| format!(".description({})", text.source));
        let depths: Vec<String> = tool
            .params
            .iter()
            .map(|param| {
                let ty = self.ty(&param.ty);
                format!("<{ty} as {tc}::ToolParameter>::DEPTH")
            })
            .collect();
        let from_node = self.tool_from_node(tool);
        let name_checks = Self::name_checks(&tc, tool);
        let extra_check = tool.extra.as_ref().map_or_else(String::new, |extra| {
            let ty = self.ty(&extra.ty);
            format!(
                "const _: () = {{ fn __pa_extra<__pa_T: {tc}::ToolExtra>() {{}} let _ = \
                 __pa_extra::<{ty}>; }};"
            )
        });
        format!(
            "#[automatically_derived] impl {tc}::ToolArguments for {name} {{ \
                fn tool_definition() -> {tc}::ToolDefinition {{ {definition} }} \
                fn object_spec() -> {tc}::ObjectSpec {{ {object} }} \
             }} \
             #[automatically_derived] impl {tc}::ToolParameter for {name} {{ \
                fn parameter() -> {tc}::ParameterSpec {{ \
                    {tc}::Param::object(<{name} as {tc}::ToolArguments>::object_spec()){description} \
                }} \
                const REQUIRED: bool = true; \
                fn absent() -> ::core::option::Option<Self> {{ ::core::option::Option::None }} \
                const DEPTH: usize = 1 + {tc}::max_depth(&[{depths}]); \
             }} \
             #[automatically_derived] impl {tc}::ToolItem for {name} {{}} \
             {from_node} \
             {extra_check} \
             {name_checks} \
             const _: usize = <{name} as {tc}::ToolParameter>::DEPTH;",
            depths = depths.join(", "),
        )
    }

    fn tool_from_node(&mut self, tool: &Tool) -> String {
        let (tc, js) = (self.tc.clone(), self.js.clone());
        let name = self.splice([tool.ident.span]);
        let count = tool.params.len();
        let mut sorted: Vec<usize> = (0..count).collect();
        sorted.sort_by(|&a, &b| tool.params[a].key.value.cmp(&tool.params[b].key.value));
        let names: Vec<&str> = sorted
            .iter()
            .map(|&index| tool.params[index].key.source.as_str())
            .collect();
        let order: Vec<String> = sorted.iter().map(ToString::to_string).collect();

        let mut declarations = String::new();
        for (index, param) in tool.params.iter().enumerate() {
            let ty = self.ty(&param.ty);
            let _ = write!(
                declarations,
                "let mut __pa_f{index}: ::core::option::Option<{ty}> = \
                 ::core::option::Option::None; "
            );
        }
        if tool.extra.is_some() {
            let _ = write!(
                declarations,
                "let mut __pa_extra: ::std::vec::Vec<{js}::Member> = ::std::vec::Vec::new(); "
            );
        }

        let arms = self.member_arms(tool);

        let mut finals = String::new();
        let mut fields = String::new();
        for (index, param) in tool.params.iter().enumerate() {
            let absent = self.absent_value(param);
            let _ = write!(
                finals,
                "let __pa_v{index} = match __pa_f{index} {{ \
                 ::core::option::Option::Some(__pa_value) => __pa_value, \
                 ::core::option::Option::None => {absent} }}; "
            );
            let _ = write!(fields, "{}: __pa_v{index}, ", param.ident.text);
        }
        if let Some(Extra { ident, .. }) = &tool.extra {
            let _ = write!(
                fields,
                "{}: {js}::Node::new({js}::Value::Object(__pa_extra)), ",
                ident.text
            );
        }

        format!(
            "#[automatically_derived] impl {tc}::FromNode for {name} {{ \
                fn from_node(__pa_node: &{js}::Node) -> \
                    ::core::result::Result<Self, {tc}::FromNodeError> {{ \
                    const __PA_NAMES: &[&str] = &[{names}]; \
                    const __PA_ORDER: &[usize] = &[{order}]; \
                    let mut __pa_members = {tc}::Members::new(__PA_NAMES, __PA_ORDER, __pa_node)?; \
                    {declarations} \
                    for __pa_slot in &mut __pa_members {{ match __pa_slot {{ {arms} }} }} \
                    {finals} \
                    ::core::result::Result::Ok(Self {{ {fields} }}) \
                }} \
             }}",
            names = names.join(", "),
            order = order.join(", "),
        )
    }

    /// Compile-time assertions that every tool, parameter and alias name is
    /// valid (the rule itself lives in core).
    fn name_checks(tc: &str, tool: &Tool) -> String {
        const TOOL: &str = "#[tool(name = \"..\")]";
        const PARAM: &str = "#[param(name = \"..\")]";
        // Aliases are always written explicitly, so their remedy is a valid
        // spelling; derived names can also be replaced with `name = ".."`.
        let mut names: Vec<(&str, Option<&str>, &Text)> =
            vec![("tool name", Some(TOOL), &tool.name)];
        names.extend(tool.aliases.iter().map(|text| ("tool alias", None, text)));
        for param in &tool.params {
            names.push(("parameter name", Some(PARAM), &param.key));
            names.extend(
                param
                    .aliases
                    .iter()
                    .map(|text| ("parameter alias", None, text)),
            );
        }
        let mut out = String::new();
        for (what, attribute, text) in names {
            let remedy = attribute.map_or_else(String::new, |attribute| {
                format!(", or set it with {attribute}")
            });
            let message = format!(
                "{what} {:?} is not valid: use ASCII letters, digits and `_ . : -`{remedy}",
                text.value
            );
            let _ = write!(
                out,
                "const _: () = assert!({tc}::__valid_name({}), {message:?}); ",
                text.source
            );
        }
        out
    }

    /// The `match` arms that classify each member of the object.
    fn member_arms(&mut self, tool: &Tool) -> String {
        let (tc, count) = (self.tc.clone(), tool.params.len());
        let mut arms = String::new();
        if count == 0 {
            let _ = write!(arms, "{tc}::Slot::Declared {{ .. }} => {{}} ");
        }
        for (index, param) in tool.params.iter().enumerate() {
            let ty = self.ty(&param.ty);
            let pattern = if index + 1 == count {
                format!("{tc}::Slot::Declared {{ member: __pa_member, .. }}")
            } else {
                format!("{tc}::Slot::Declared {{ index: {index}, member: __pa_member }}")
            };
            let _ = write!(
                arms,
                "{pattern} => {{ __pa_f{index} = ::core::option::Option::Some(\
                 <{ty} as {tc}::FromNode>::from_node(&__pa_member.value)\
                 .map_err(|__pa_error| __pa_error.within({key}))?); }} ",
                key = param.key.source
            );
        }
        let _ = write!(
            arms,
            "{tc}::Slot::Duplicate {{ member: __pa_member, .. }} => \
             return ::core::result::Result::Err({tc}::FromNodeError::duplicate(&__pa_member.key)), "
        );
        if tool.extra.is_some() {
            let _ = write!(
                arms,
                "{tc}::Slot::Undeclared(__pa_member) => __pa_extra.push(__pa_member.clone()), "
            );
        } else {
            let _ = write!(
                arms,
                "{tc}::Slot::Undeclared(__pa_member) => \
                 return ::core::result::Result::Err({tc}::FromNodeError::unknown(&__pa_member.key)), "
            );
        }

        arms
    }

    /// What an absent member of `param` becomes: its default, its `absent()`
    /// value (`None` for an `Option`), or a `missing` error.
    fn absent_value(&mut self, param: &Param) -> String {
        let tc = self.tc.clone();
        if let Some(default) = &param.default {
            let ty = self.ty(&param.ty);
            return format!(
                "<{ty} as {tc}::FromNode>::from_node(&{})\
                 .map_err(|__pa_error| __pa_error.within({}))?",
                self.default_node(default),
                param.key.source
            );
        }
        let ty = self.ty(&param.ty);
        format!(
            "match <{ty} as {tc}::ToolParameter>::absent() {{ \
             ::core::option::Option::Some(__pa_value) => __pa_value, \
             ::core::option::Option::None => return ::core::result::Result::Err(\
             {tc}::FromNodeError::missing({})) }}",
            param.key.source
        )
    }

    fn enumeration(&mut self, enumeration: &Enumeration) -> String {
        let (tc, js) = (self.tc.clone(), self.js.clone());
        let name = self.splice([enumeration.ident.span]);
        let values: Vec<&str> = enumeration
            .variants
            .iter()
            .map(|(_, value)| value.source.as_str())
            .collect();
        let description = enumeration
            .description
            .as_ref()
            .map_or_else(String::new, |text: &Text| {
                format!(".description({})", text.source)
            });
        let mut arms = String::new();
        for (variant, value) in &enumeration.variants {
            let _ = write!(
                arms,
                "::core::option::Option::Some({}) => ::core::result::Result::Ok(Self::{}), ",
                value.source, variant.text
            );
        }
        let expected = format!(
            "{:?}",
            format!(
                "one of {}",
                enumeration
                    .variants
                    .iter()
                    .map(|(_, value)| format!("{:?}", value.value))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        );
        format!(
            "#[automatically_derived] impl {tc}::FromNode for {name} {{ \
                fn from_node(__pa_node: &{js}::Node) -> \
                    ::core::result::Result<Self, {tc}::FromNodeError> {{ \
                    match __pa_node.as_str() {{ {arms} \
                        _ => ::core::result::Result::Err(\
                            {tc}::FromNodeError::mismatch({expected}, __pa_node)), }} \
                }} \
             }} \
             #[automatically_derived] impl {tc}::ToolParameter for {name} {{ \
                fn parameter() -> {tc}::ParameterSpec {{ \
                    {tc}::Param::enumeration(&[{values}]){description} \
                }} \
                const REQUIRED: bool = true; \
                fn absent() -> ::core::option::Option<Self> {{ ::core::option::Option::None }} \
                const DEPTH: usize = 0; \
             }} \
             #[automatically_derived] impl {tc}::ToolItem for {name} {{}} \
             const _: usize = <{name} as {tc}::ToolParameter>::DEPTH;",
            values = values.join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{lex::trees, resolve};
    use crate::parse::{Target, parse};

    fn write(target: Target, flavor: Flavor, source: &str) -> Output {
        let item = resolve(parse(target, &trees(source)).unwrap()).unwrap();
        generate(&item, flavor)
    }

    /// The generated text is a balanced token tree (the boundary's `FromStr`
    /// cannot fail on it).
    fn assert_balanced(text: &str) {
        let _ = trees(text);
    }

    #[test]
    fn struct_output_names_the_requested_crate_only() {
        let source =
            r#"#[tool(name = "t", unknown = pass)] struct A { a: u8, #[param(extra)] rest: Node }"#;
        let core = write(Target::Definition, Flavor::Core, source);
        let facade = write(Target::Definition, Flavor::Facade, source);
        assert!(
            core.text
                .contains("::purrassist_core::tool_call::ToolArguments")
        );
        assert!(!core.text.contains("::purrassist::__core"));
        assert!(
            facade
                .text
                .contains("::purrassist::__core::tool_call::ToolArguments")
        );
        assert!(!facade.text.contains("::purrassist_core"));
        assert_balanced(&core.text);
        assert_balanced(&facade.text);
        assert_eq!(
            core.text
                .replace("::purrassist_core", "::purrassist::__core"),
            facade.text
        );
    }

    #[test]
    fn declared_names_are_a_sorted_table_with_the_declaration_order() {
        let output = write(
            Target::Definition,
            Flavor::Core,
            "struct A { zed: u8, alpha: u8, mid: u8 }",
        );
        assert!(
            output
                .text
                .contains(r#"&[&str] = &["alpha", "mid", "zed"];"#),
            "{}",
            output.text
        );
        assert!(
            output.text.contains("&[usize] = &[1, 2, 0];"),
            "{}",
            output.text
        );
    }

    #[test]
    fn placeholders_are_spliced_from_the_input_spans() {
        // `struct A { a : Vec < u8 > }`: ids `struct`=0 `A`=1 group=2 `a`=3 `:`=4 `Vec`=5 `<`=6 `u8`=7 `>`=8.
        let output = write(Target::Definition, Flavor::Core, "struct A { a: Vec<u8> }");
        assert!(output.splices.contains(&vec![1]), "type name");
        assert!(
            output.splices.contains(&vec![5, 6, 7, 8]),
            "{:?}",
            output.splices
        );
        for (index, _) in output.splices.iter().enumerate() {
            assert!(output.text.contains(&format!("{SPLICE_PREFIX}{index}")));
        }
    }

    #[test]
    fn zero_parameter_tools_and_flags_generate() {
        for source in ["struct Ping;", "struct Ping {}"] {
            let output = write(Target::Definition, Flavor::Core, source);
            assert!(output.text.contains("Slot::Declared { .. } => {}"));
            assert!(output.text.contains("max_depth(&[])"));
            assert_balanced(&output.text);
        }
    }

    #[test]
    fn locals_are_prefixed_and_paths_absolute() {
        let output = write(
            Target::Definition,
            Flavor::Core,
            r#"struct A { #[param(default = [1, {"k": null}], repeat, pattern = "^a")] a: Vec<String> }"#,
        );
        assert_balanced(&output.text);
        // No bare `Option`, `Result`, `Some`, `Vec` or `String` outside `::` paths.
        for word in [
            "Option", "Result", "Some", "None", "Ok", "Err", "Vec", "String", "Self",
        ] {
            let bare = output
                .text
                .match_indices(word)
                .filter(|(at, _)| {
                    let before = output.text[..*at].chars().last();
                    let after = output.text[at + word.len()..].chars().next();
                    !matches!(before, Some(c) if c.is_alphanumeric() || c == '_')
                        && !matches!(after, Some(c) if c.is_alphanumeric() || c == '_')
                        && !output.text[..*at].ends_with("::")
                })
                .count();
            if word != "Self" {
                assert_eq!(bare, 0, "bare `{word}` in {}", output.text);
            }
        }
        assert!(output.text.contains("__derive_number(\"1\")"));
    }

    #[test]
    fn enumerations_generate_a_matching_conversion() {
        let output = write(
            Target::Enumeration,
            Flavor::Core,
            r#"#[tool(description = "d")] enum Mode { Asc, #[param(name = "DESC\"")] Desc }"#,
        );
        assert_balanced(&output.text);
        assert!(
            output
                .text
                .contains(r#"Param::enumeration(&["Asc", "DESC\""]).description("d")"#)
        );
        assert!(
            output
                .text
                .contains(r#"mismatch("one of \"Asc\", \"DESC\\\"\"", __pa_node)"#),
            "{}",
            output.text
        );
        assert!(output.text.contains("DEPTH: usize = 0;"));
    }

    #[test]
    fn semantic_errors_have_exact_messages() {
        let message = |target: Target, source: &str| {
            let raw = parse(target, &trees(source)).unwrap();
            resolve(raw).unwrap_err().message
        };
        let tool = |source: &str| message(Target::Definition, source);
        assert_eq!(
            tool("struct 日本 {}"),
            "the tool name `日本` derived from `日本` is not a valid tool name (ASCII letters, digits and `_ . : -`); add #[tool(name = \"..\")]"
        );
        assert_eq!(
            tool("struct A { a: Vec<A> }"),
            "field `a` refers to `A` itself; a tool's arguments cannot contain themselves, so a recursive schema must be declared by hand"
        );
        assert_eq!(
            tool("struct A { a: Option<Self> }"),
            "field `a` refers to `A` itself; a tool's arguments cannot contain themselves, so a recursive schema must be declared by hand"
        );
        assert_eq!(
            tool(r#"struct A { a: u8, #[param(name = "a")] b: u8 }"#),
            "parameter name `a` is declared twice (first by field `a`)"
        );
        assert_eq!(
            tool("#[tool(unknown = pass)] struct A { a: u8 }"),
            "`unknown = pass` needs exactly one field marked #[param(extra)] of type `json::Node` to receive the undeclared members"
        );
        assert_eq!(
            tool("struct A { #[param(extra)] x: Node }"),
            "a #[param(extra)] field needs #[tool(unknown = pass)] on the struct"
        );
        assert_eq!(
            tool("#[tool(unknown = drop)] struct A { #[param(extra)] x: Node }"),
            "a #[param(extra)] field needs #[tool(unknown = pass)] on the struct"
        );
        assert_eq!(
            tool(
                "#[tool(unknown = pass)] struct A { #[param(extra)] x: Node, #[param(extra)] y: Node }"
            ),
            "only one field can be marked #[param(extra)]"
        );
        assert_eq!(
            tool(r#"#[tool(unknown = pass)] struct A { #[param(extra, name = "n")] x: Node }"#),
            "`extra` collects undeclared members and cannot be combined with other #[param] keys"
        );
        assert_eq!(
            message(Target::Enumeration, "enum E {}"),
            "a ToolEnum needs at least one variant: an enumeration without values accepts nothing"
        );
        assert_eq!(
            message(
                Target::Enumeration,
                r#"enum E { A, #[param(name = "A")] B }"#
            ),
            "enumeration value `A` is declared twice"
        );
    }

    #[test]
    fn error_spans_select_the_offending_token() {
        let span = |source: &str| {
            let raw = parse(Target::Definition, &trees(source)).unwrap();
            resolve(raw).unwrap_err().span
        };
        // `struct A { a : Vec < A > }`: the inner `A` is span 7.
        assert_eq!(span("struct A { a: Vec<A> }"), 7);
        // The `unknown = pass` error points at `pass`.
        // `#`=0 `[..]`=1 `tool`=2 `(..)`=3 `unknown`=4 `=`=5 `pass`=6.
        assert_eq!(span("#[tool(unknown = pass)] struct A { a: u8 }"), 6);
        // The undeclared-`extra` error points at the field name.
        assert_eq!(span("struct A { #[param(extra)] x: Node }"), 8);
    }
}
