// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Complete callable bindings over an already admitted upstream `FnO` catalog.

use std::{cmp::Ordering, fmt};

use purrassist_core::{
    Governor, ScratchVec, StopReason,
    contract::{Resource, saturating_size},
};
use purrdf_core::fno::{
    FnFunction, FnImpl, FnMapping, FnOutput, FnParam, FnParamMapping, FnReturnMapping, FnoCatalog,
};

/// A callable-binding failure retaining the original catalog operands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError<'a> {
    /// The selected mapping is outside the catalog.
    MappingIndex {
        /// Requested index.
        index: usize,
        /// Number of available mappings.
        available: usize,
    },
    /// A callable parameter has no RDF predicate.
    ParameterPredicate(&'a FnParam),
    /// The callable output has no RDF predicate.
    OutputPredicate(&'a FnOutput),
    /// A declared parameter must have exactly one implementation binding.
    ParameterMappingCount {
        /// Original parameter.
        parameter: &'a FnParam,
        /// Actual number of bindings.
        count: usize,
    },
    /// The output must have exactly one implementation binding.
    ReturnMappingCount {
        /// Original selected mapping.
        mapping: &'a FnMapping,
        /// Actual number of bindings.
        count: usize,
    },
    /// Two callable parameters use the same RDF predicate.
    PredicateCollision {
        /// Earlier declared parameter.
        first: &'a FnParam,
        /// Later declared parameter.
        second: &'a FnParam,
    },
    /// Two callable parameters use the same JSON implementation property.
    PropertyCollision {
        /// Earlier declared binding.
        first: &'a FnParamMapping,
        /// Later declared binding.
        second: &'a FnParamMapping,
    },
    /// Governance refused further work or retained index storage.
    Resource(StopReason),
}

impl fmt::Display for BindingError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MappingIndex { index, available } => write!(
                f,
                "mapping index {index} is outside the {available} catalog mappings"
            ),
            Self::ParameterPredicate(parameter) => write!(
                f,
                "callable parameter {} requires an RDF predicate",
                parameter.iri
            ),
            Self::OutputPredicate(output) => write!(
                f,
                "callable output {} requires an RDF predicate",
                output.iri
            ),
            Self::ParameterMappingCount { parameter, count } => write!(
                f,
                "parameter {} requires one implementation binding, found {count}",
                parameter.iri
            ),
            Self::ReturnMappingCount { count, .. } => write!(
                f,
                "callable output requires one implementation binding, found {count}"
            ),
            Self::PredicateCollision { first, second } => write!(
                f,
                "parameters {} and {} share an input predicate",
                first.iri, second.iri
            ),
            Self::PropertyCollision { first, second } => write!(
                f,
                "bindings {} and {} share an input implementation property",
                first.node.label, second.node.label
            ),
            Self::Resource(reason) => write!(f, "callable binding admission stopped: {reason}"),
        }
    }
}
impl std::error::Error for BindingError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Original parameter and its unique implementation binding.
#[derive(Clone, Copy, Debug)]
pub struct ParameterBinding<'a> {
    /// Original parameter, retaining literal facets and predicate identity.
    pub parameter: &'a FnParam,
    /// Original mapping, retaining its native blank-node scope.
    pub mapping: &'a FnParamMapping,
}

/// Original output and its unique implementation binding.
#[derive(Clone, Copy, Debug)]
pub struct ReturnBinding<'a> {
    /// Original output and its RDF predicate.
    pub output: &'a FnOutput,
    /// Original return mapping and native blank-node scope.
    pub mapping: &'a FnReturnMapping,
}

/// Complete bindings for one explicitly selected upstream `FnO` mapping.
///
/// Catalog construction is caller-owned and outside this operation's governance.
/// Admission uses binary searches over upstream canonical catalog records and
/// explicitly charged pairwise checks local to this callable's parameter count.
/// Those local checks are quadratic; storage is one fallible caller-order index.
/// This value is neither a loaded tool graph nor an execution authorization.
///
/// The executable `admit_callable` example constructs a catalog, runs the actual
/// upstream reader and admits a complete binding without copying its source records.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{BindingError, CallableBinding};
/// use purrdf_core::fno::FnoCatalog;
/// fn inspect(catalog: &FnoCatalog) -> Result<(), BindingError<'_>> {
///     let binding = CallableBinding::admit(catalog, 0, &mut Governor::new(Budget::default()))?;
///     for parameter in binding.parameters() {
///         assert!(parameter.parameter.predicate.is_some());
///     }
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct CallableBinding<'a> {
    catalog: &'a FnoCatalog,
    mapping_index: usize,
    function_index: usize,
    implementation_index: usize,
    parameters: Vec<(usize, usize)>,
}

impl<'a> CallableBinding<'a> {
    /// Admit one mapping with complete, unambiguous input and output bindings.
    ///
    /// Input predicates and implementation properties must be distinct. Output
    /// bindings occupy a separate scope and may reuse an input property/predicate.
    /// JSON property names are not restricted to identifiers or nonempty strings.
    ///
    /// # Errors
    /// Returns original operands in [`BindingError`] for missing predicates,
    /// incomplete/ambiguous mappings, input collisions or an invalid selection.
    /// Resource exhaustion and cancellation expose no partial successful binding.
    pub fn admit(
        catalog: &'a FnoCatalog,
        mapping_index: usize,
        governor: &mut Governor,
    ) -> Result<Self, BindingError<'a>> {
        governor
            .charge(Resource::Work, 1)
            .map_err(BindingError::Resource)?;
        let data = catalog.as_input();
        let mapping = data
            .mappings
            .get(mapping_index)
            .ok_or(BindingError::MappingIndex {
                index: mapping_index,
                available: data.mappings.len(),
            })?;
        let function_index = lookup(
            &data.functions,
            &mapping.function,
            |value| &value.iri,
            governor,
        )
        .map_err(BindingError::Resource)?;
        let implementation_index = lookup(
            &data.implementations,
            &mapping.implementation,
            |value| &value.iri,
            governor,
        )
        .map_err(BindingError::Resource)?;
        let function = &data.functions[function_index];
        if function.output.predicate.is_none() {
            return Err(BindingError::OutputPredicate(&function.output));
        }
        if mapping.return_mappings.len() != 1 {
            return Err(BindingError::ReturnMappingCount {
                mapping,
                count: mapping.return_mappings.len(),
            });
        }
        let mut parameters: ScratchVec<(usize, usize)> = ScratchVec::new(governor);
        for iri in &function.expects {
            governor
                .charge(Resource::Work, 1)
                .map_err(BindingError::Resource)?;
            let parameter_index = lookup(&data.params, iri, |value| &value.iri, governor)
                .map_err(BindingError::Resource)?;
            let parameter = &data.params[parameter_index];
            let Some(predicate) = parameter.predicate.as_deref() else {
                return Err(BindingError::ParameterPredicate(parameter));
            };
            let selected = parameter_mapping(mapping, parameter, governor)?;
            let binding = &mapping.parameter_mappings[selected];
            for &(previous_parameter, previous_mapping) in parameters.as_slice() {
                let previous = &data.params[previous_parameter];
                // Every earlier parameter was admitted with a predicate above.
                let previous_predicate = previous
                    .predicate
                    .as_deref()
                    .ok_or(BindingError::ParameterPredicate(previous))?;
                if compare(previous_predicate, predicate, governor)
                    .map_err(BindingError::Resource)?
                    == Ordering::Equal
                {
                    return Err(BindingError::PredicateCollision {
                        first: previous,
                        second: parameter,
                    });
                }
                let previous = &mapping.parameter_mappings[previous_mapping];
                if compare(
                    &previous.implementation_property,
                    &binding.implementation_property,
                    governor,
                )
                .map_err(BindingError::Resource)?
                    == Ordering::Equal
                {
                    return Err(BindingError::PropertyCollision {
                        first: previous,
                        second: binding,
                    });
                }
            }
            parameters
                .push((parameter_index, selected))
                .map_err(BindingError::Resource)?;
        }
        governor.check().map_err(BindingError::Resource)?;
        Ok(Self {
            catalog,
            mapping_index,
            function_index,
            implementation_index,
            parameters: parameters.into_vec(),
        })
    }

    /// The original selected mapping, without scans or allocation.
    #[must_use]
    pub fn mapping(&self) -> &'a FnMapping {
        &self.catalog.as_input().mappings[self.mapping_index]
    }

    /// The original function, including declared parameter order and literal facets.
    #[must_use]
    pub fn function(&self) -> &'a FnFunction {
        &self.catalog.as_input().functions[self.function_index]
    }

    /// The original selected implementation identity and media type.
    #[must_use]
    pub fn implementation(&self) -> &'a FnImpl {
        &self.catalog.as_input().implementations[self.implementation_index]
    }

    /// Retrieve a declared parameter in constant time through the admitted index.
    #[must_use]
    pub fn parameter(&self, index: usize) -> Option<ParameterBinding<'a>> {
        self.parameters
            .get(index)
            .map(|&(parameter, mapping)| ParameterBinding {
                parameter: &self.catalog.as_input().params[parameter],
                mapping: &self.mapping().parameter_mappings[mapping],
            })
    }

    /// Iterate in declaration order, with no searches, cloning or allocation.
    #[must_use = "consume the iterator to inspect the admitted parameter bindings"]
    pub fn parameters(&self) -> impl ExactSizeIterator<Item = ParameterBinding<'a>> + '_ {
        self.parameters
            .iter()
            .map(|&(parameter, mapping)| ParameterBinding {
                parameter: &self.catalog.as_input().params[parameter],
                mapping: &self.mapping().parameter_mappings[mapping],
            })
    }

    /// The unique output binding, independent of input naming scope.
    #[must_use]
    pub fn output(&self) -> ReturnBinding<'a> {
        ReturnBinding {
            output: &self.function().output,
            mapping: &self.mapping().return_mappings[0],
        }
    }
}

fn parameter_mapping<'a>(
    mapping: &FnMapping,
    parameter: &'a FnParam,
    governor: &mut Governor,
) -> Result<usize, BindingError<'a>> {
    let mut count = 0;
    let mut selected = 0;
    for (index, candidate) in mapping.parameter_mappings.iter().enumerate() {
        if compare(&candidate.function_parameter, &parameter.iri, governor)
            .map_err(BindingError::Resource)?
            == Ordering::Equal
        {
            count += 1;
            selected = index;
        }
    }
    if count == 1 {
        Ok(selected)
    } else {
        Err(BindingError::ParameterMappingCount { parameter, count })
    }
}

fn compare(left: &str, right: &str, governor: &mut Governor) -> Result<Ordering, StopReason> {
    governor.charge(
        Resource::Work,
        saturating_size(left.len().min(right.len())).saturating_add(1),
    )?;
    Ok(left.cmp(right))
}

fn lookup<T>(
    values: &[T],
    key: &str,
    iri: impl Fn(&T) -> &str,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let mut start = 0;
    let mut end = values.len();
    while start < end {
        let middle = start + (end - start) / 2;
        match compare(iri(&values[middle]), key, governor)? {
            Ordering::Less => start = middle + 1,
            Ordering::Greater => end = middle,
            Ordering::Equal => return Ok(middle),
        }
    }
    // FnoCatalog has private storage: native admission proves every mapping and
    // function reference exists, and canonicalization sorts these records by IRI.
    unreachable!("upstream admitted catalog contains every referenced IRI")
}
