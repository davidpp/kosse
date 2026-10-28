//! Types serialized to the browser. TypeScript declarations are generated from these definitions,
//! so the web client never hand-maintains a copy of the model.

use std::{any::TypeId, collections::HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS, TypeVisitor};

/// Identifies the Stackscan instance answering on this endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub name: String,
    pub version: String,
}

/// Every contract type in one TypeScript module.
#[must_use]
pub fn typescript_bindings() -> String {
    let mut module = BindingsModule::new();
    // Roots: the types API handlers serialize. Types they reference are included automatically.
    module.add::<Health>();
    module.finish()
}

const BINDINGS_HEADER: &str = "// Generated from crates/core/src/contract.rs. Do not edit.\n\
// Regenerate with `bun run contracts`.\n";

/// Collects declarations dependency-first, each exactly once, so the module needs no imports.
struct BindingsModule {
    cfg: Config,
    declared: HashSet<TypeId>,
    source: String,
}

impl BindingsModule {
    fn new() -> Self {
        Self {
            cfg: Config::new(),
            declared: HashSet::new(),
            source: String::from(BINDINGS_HEADER),
        }
    }

    fn add<T: TS + 'static + ?Sized>(&mut self) {
        // Only derived types have an output path; std containers and primitives are inlined,
        // but whatever they wrap (e.g. `Vec<Endpoint>`) still needs declaring.
        if T::output_path().is_none() {
            T::visit_dependencies(self);
            return;
        }
        if !self.declared.insert(TypeId::of::<T>()) {
            return;
        }
        T::visit_dependencies(self);
        self.source.push('\n');
        if let Some(docs) = T::docs() {
            self.source.push_str(&docs);
        }
        self.source.push_str("export ");
        self.source.push_str(&T::decl(&self.cfg));
        self.source.push('\n');
    }

    fn finish(self) -> String {
        self.source
    }
}

impl TypeVisitor for BindingsModule {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        self.add::<T>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Leaf {
        value: u32,
    }

    #[derive(TS)]
    #[expect(dead_code, reason = "only the type shape matters to the generator")]
    struct Branch {
        first: Leaf,
        rest: Vec<Leaf>,
        maybe: Option<Leaf>,
    }

    #[test]
    fn referenced_types_are_declared_once_before_their_users() {
        let mut module = BindingsModule::new();
        module.add::<Branch>();
        let source = module.finish();

        assert_eq!(source.matches("export type Leaf =").count(), 1, "{source}");
        let leaf = source.find("export type Leaf =").expect("Leaf declared");
        let branch = source
            .find("export type Branch =")
            .expect("Branch declared");
        assert!(leaf < branch, "{source}");
    }
}
