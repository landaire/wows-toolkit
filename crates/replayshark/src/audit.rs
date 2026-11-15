//! Which entity definition type names have a domain newtype.

use std::collections::BTreeSet;

use serde::Serialize;
use wowsunpack::rpc::entitydefs::EntitySpec;
use wowsunpack::rpc::newtype_registry::newtype_for;

/// Every semantic type name the entity definitions use, in name order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeAudit {
    pub names: Vec<AuditedName>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuditedName {
    /// The definition's type name, such as `ENTITY_ID`.
    pub def_name: String,

    /// The Rust newtype for it. `None` when it has none.
    pub newtype: Option<&'static str>,
}

impl TypeAudit {
    /// How many names have a newtype.
    pub fn covered(&self) -> usize {
        self.names.iter().filter(|name| name.newtype.is_some()).count()
    }
}

pub fn audit_types(specs: &[EntitySpec]) -> TypeAudit {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for spec in specs {
        for p in spec.properties.iter().chain(&spec.internal_properties).chain(&spec.base_properties) {
            p.prop_type.collect_semantic_names(&mut names);
        }
        for m in spec.client_methods.iter().chain(&spec.base_methods).chain(&spec.cell_methods) {
            for arg in &m.args {
                arg.collect_semantic_names(&mut names);
            }
        }
    }

    let names = names
        .into_iter()
        .map(|def_name| {
            let newtype = newtype_for(&def_name).map(|newtype| newtype.rust_type_name());
            AuditedName { def_name, newtype }
        })
        .collect();

    TypeAudit { names }
}
