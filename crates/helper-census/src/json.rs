// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Report construction adapter; `PurRAssist` core owns all JSON serialization.
use purrassist_core::json::{Member, Node, Number, Value as CoreValue};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Index, IndexMut};

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Self>),
    Object(Object),
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Object(BTreeMap<String, Value>);
impl Object {
    pub(crate) const fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub(crate) fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.insert(key.to_owned(), value);
        self
    }
    pub(crate) fn insert(&mut self, key: String, value: impl Into<Value>) {
        self.0.insert(key, value.into());
    }
}
impl Value {
    pub(crate) fn object<K: Into<String>>(entries: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::Object(Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        ))
    }
    pub(crate) const fn as_array(&self) -> Option<&Vec<Self>> {
        if let Self::Array(items) = self {
            Some(items)
        } else {
            None
        }
    }
    fn node(&self) -> Node {
        Node::new(match self {
            Self::Null => CoreValue::Null,
            Self::Bool(value) => CoreValue::Bool(*value),
            Self::Number(value) => CoreValue::Number(value.clone()),
            Self::String(value) => CoreValue::String(value.clone()),
            Self::Array(items) => CoreValue::Array(items.iter().map(Self::node).collect()),
            Self::Object(object) => CoreValue::Object(
                object
                    .0
                    .iter()
                    .map(|(key, value)| Member {
                        key: key.clone(),
                        key_span: None,
                        value: value.node(),
                    })
                    .collect(),
            ),
        })
    }
}
impl fmt::Display for Value {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.node().to_json())
    }
}
impl Index<&str> for Value {
    type Output = Self;
    fn index(&self, key: &str) -> &Self {
        let Self::Object(object) = self else {
            panic!("report object required")
        };
        object.0.get(key).expect("report member exists")
    }
}
impl IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Self {
        let Self::Object(object) = self else {
            panic!("report object required")
        };
        object.0.entry(key.to_owned()).or_insert(Self::Null)
    }
}
impl From<Object> for Value {
    fn from(value: Object) -> Self {
        Self::Object(value)
    }
}
impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<&String> for Value {
    fn from(value: &String) -> Self {
        Self::String(value.clone())
    }
}
impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}
impl From<usize> for Value {
    fn from(value: usize) -> Self {
        Self::Number(Number::from_count(value))
    }
}
impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Number(value.to_string().parse().expect("i64 is a JSON integer"))
    }
}
impl<T: Into<Self>> FromIterator<T> for Value {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self::Array(items.into_iter().map(Into::into).collect())
    }
}
impl<T: Clone + Into<Self>> From<&[T]> for Value {
    fn from(items: &[T]) -> Self {
        items.iter().cloned().collect()
    }
}
impl<T: Into<Self>> From<Vec<T>> for Value {
    fn from(items: Vec<T>) -> Self {
        items.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Object, Value};

    #[test]
    fn serialization_is_sorted_escaped_and_exact() {
        let report = Value::from(
            Object::new()
                .with("z", "quote\"\n\u{2603}")
                .with("a", usize::MAX)
                .with("negative", -7_i64)
                .with("bool", true),
        );
        let text = report.to_string();
        let parsed = purrassist_core::json::parse(&text).expect("generated report is strict JSON");
        assert_eq!(
            parsed
                .get("z")
                .and_then(purrassist_core::json::Node::as_str),
            Some("quote\"\n\u{2603}")
        );
        assert!(text.starts_with("{\"a\":"));
        assert!(text.contains(&usize::MAX.to_string()));
        assert!(text.contains("\"negative\":-7"));
        assert!(text.contains("\"bool\":true"));
    }
}
