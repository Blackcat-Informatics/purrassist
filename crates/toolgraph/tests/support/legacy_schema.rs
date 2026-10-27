// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Frozen legacy JSON-LD schema and `OpenAPI` fixtures.
//!
//! From `PurRDF`
//! `crates/shapes/tests/fixtures/iri-only-{ontology,shapes}.ttl` and the
//! shaped-only schema/OpenAPI sections of `iri-only-surface.golden.txt`
//! at pre-adoption source `2ed9845b0ca3d38fe74f71b19320f0d46dcaeb26`.
//! These bytes are unchanged at `8d0a1c6d9d094e5642ab8af3e96d4262b84a8d77`.

pub(crate) const ONTOLOGY: &str = r"@prefix ex: <https://example.org/schema/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

ex:Agent a owl:Class .
ex:Person a owl:Class ; rdfs:subClassOf ex:Agent .
ex:Org a owl:Class ; rdfs:subClassOf ex:Agent ; owl:disjointWith ex:Person .
ex:Team a owl:Class ; owl:equivalentClass ex:Group .
ex:Group a owl:Class .
ex:name a owl:DatatypeProperty , owl:FunctionalProperty ;
    rdfs:domain ex:Agent ; rdfs:range xsd:string .
ex:nick rdfs:subPropertyOf ex:name .
ex:age a owl:DatatypeProperty ; rdfs:domain ex:Person ; rdfs:range xsd:integer .
ex:knows a owl:ObjectProperty , owl:SymmetricProperty ;
    rdfs:domain ex:Person ; rdfs:range ex:Person .
ex:memberOf a owl:ObjectProperty ; owl:inverseOf ex:member ;
    rdfs:domain ex:Person ; rdfs:range ex:Org .
ex:member a owl:ObjectProperty .
ex:Code a rdfs:Datatype .
ex:code a owl:DatatypeProperty ; rdfs:domain ex:Org ; rdfs:range ex:Code .
ex:payload a owl:ObjectProperty ; rdfs:domain ex:Org ; rdfs:range rdf:JSON .
ex:label a owl:AnnotationProperty .
";

pub(crate) const SHAPES: &str = r"@prefix ex: <https://example.org/schema/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ; sh:datatype xsd:string ] .
";

pub(crate) const SCHEMA: &str = r##"{
  "$defs": {
    "Annotation": {
      "additionalProperties": {
        "anyOf": [
          {
            "type": "string"
          },
          {
            "type": "number"
          },
          {
            "type": "boolean"
          },
          {
            "properties": {
              "@id": {
                "type": "string"
              }
            },
            "required": [
              "@id"
            ],
            "type": "object"
          },
          {
            "properties": {
              "@type": {
                "type": "string"
              },
              "@value": {}
            },
            "required": [
              "@value"
            ],
            "type": "object"
          }
        ]
      },
      "description": "Free-form metadata about an asserted triple (e.g. meta:accordingTo, meta:confidence, meta:assertedAt). Permissive.",
      "title": "RDF-1.2 statement metadata (reifier annotation)",
      "type": "object"
    },
    "Node": {
      "allOf": [
        {
          "if": {
            "properties": {
              "@type": {
                "anyOf": [
                  {
                    "const": "ex:Person"
                  },
                  {
                    "contains": {
                      "const": "ex:Person"
                    },
                    "type": "array"
                  }
                ]
              }
            },
            "required": [
              "@type"
            ]
          },
          "then": {
            "$ref": "#/$defs/Person"
          }
        }
      ],
      "description": "Validated by @type: a node typed ex:Foo MUST satisfy #/$defs/Foo (closed-world). Nodes typed only by unmodeled classes are permissively allowed.",
      "properties": {
        "@annotation": {
          "$ref": "#/$defs/Annotation"
        },
        "@id": {
          "type": "string"
        },
        "@type": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "items": {
                "type": "string"
              },
              "type": "array"
            }
          ]
        }
      },
      "title": "A single discriminated PURRDF instance node",
      "type": "object"
    },
    "Person": {
      "properties": {
        "@annotation": {
          "$ref": "#/$defs/Annotation"
        },
        "@id": {
          "type": "string"
        },
        "@type": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "items": {
                "type": "string"
              },
              "type": "array"
            }
          ]
        },
        "ex:name": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "items": {
                "type": "string"
              },
              "minItems": 1,
              "type": "array"
            }
          ]
        }
      },
      "required": [
        "ex:name"
      ],
      "type": "object"
    }
  },
  "$id": "https://example.org/schema/schema/instance.schema.json",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "anyOf": [
    {
      "properties": {
        "@context": true,
        "@graph": {
          "items": {
            "$ref": "#/$defs/Node"
          },
          "type": "array"
        }
      },
      "required": [
        "@graph"
      ],
      "type": "object"
    },
    {
      "$ref": "#/$defs/Node"
    }
  ],
  "properties": {
    "@context": true,
    "@graph": {
      "items": {
        "$ref": "#/$defs/Node"
      },
      "type": "array"
    }
  },
  "title": "PURRDF instance schema (SHACL-derived, closed-world)",
  "type": "object"
}
"##;

pub(crate) const OPENAPI: &str = r##"{
  "components": {
    "schemas": {
      "Annotation": {
        "additionalProperties": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "type": "number"
            },
            {
              "type": "boolean"
            },
            {
              "properties": {
                "@id": {
                  "type": "string"
                }
              },
              "required": [
                "@id"
              ],
              "type": "object"
            },
            {
              "properties": {
                "@type": {
                  "type": "string"
                },
                "@value": {}
              },
              "required": [
                "@value"
              ],
              "type": "object"
            }
          ]
        },
        "description": "Free-form metadata about an asserted triple (e.g. meta:accordingTo, meta:confidence, meta:assertedAt). Permissive.",
        "title": "RDF-1.2 statement metadata (reifier annotation)",
        "type": "object"
      },
      "Node": {
        "allOf": [
          {
            "if": {
              "properties": {
                "@type": {
                  "anyOf": [
                    {
                      "const": "ex:Person"
                    },
                    {
                      "contains": {
                        "const": "ex:Person"
                      },
                      "type": "array"
                    }
                  ]
                }
              },
              "required": [
                "@type"
              ]
            },
            "then": {
              "$ref": "#/components/schemas/Person"
            }
          }
        ],
        "description": "Validated by @type: a node typed ex:Foo MUST satisfy #/$defs/Foo (closed-world). Nodes typed only by unmodeled classes are permissively allowed.",
        "properties": {
          "@annotation": {
            "$ref": "#/components/schemas/Annotation"
          },
          "@id": {
            "type": "string"
          },
          "@type": {
            "anyOf": [
              {
                "type": "string"
              },
              {
                "items": {
                  "type": "string"
                },
                "type": "array"
              }
            ]
          }
        },
        "title": "A single discriminated PURRDF instance node",
        "type": "object"
      },
      "Person": {
        "properties": {
          "@annotation": {
            "$ref": "#/components/schemas/Annotation"
          },
          "@id": {
            "type": "string"
          },
          "@type": {
            "anyOf": [
              {
                "type": "string"
              },
              {
                "items": {
                  "type": "string"
                },
                "type": "array"
              }
            ]
          },
          "ex:name": {
            "anyOf": [
              {
                "type": "string"
              },
              {
                "items": {
                  "type": "string"
                },
                "minItems": 1,
                "type": "array"
              }
            ]
          }
        },
        "required": [
          "ex:name"
        ],
        "type": "object"
      }
    }
  },
  "info": {
    "title": "PURRDF",
    "version": "3.0.1"
  },
  "openapi": "3.1.0",
  "paths": {
    "/entities/{id}": {
      "get": {
        "parameters": [
          {
            "in": "path",
            "name": "id",
            "required": true,
            "schema": {
              "type": "string"
            }
          }
        ],
        "responses": {
          "200": {
            "content": {
              "application/ld+json": {
                "schema": {
                  "type": "object"
                }
              }
            },
            "description": "The requested entity as a JSON-LD node."
          }
        },
        "summary": "Fetch a single PURRDF entity by id"
      }
    }
  }
}
"##;
