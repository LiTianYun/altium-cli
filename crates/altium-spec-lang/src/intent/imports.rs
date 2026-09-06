//! Evaluation over captured source text. No files are read during compilation.
use super::compile::error;
use super::{SchLibIntent, compile_schlib_with_scope};
use crate::ast::*;
use crate::eval::DefinitionReference;
use crate::import::CapturedSourceGraph;
use crate::{ScopeStack, SpecError, SpecErrorCode, Value, eval_expr};
use indexmap::IndexMap;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Root definitions are the only scheduled output. Imported libraries remain
/// available for explicit references without acquiring output/write scope.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSchLib {
    pub root: SchLibIntent,
    pub imported_libraries: IndexMap<PathBuf, SchLibIntent>,
}

pub fn compile_captured_schlib(graph: &CapturedSourceGraph) -> Result<CompiledSchLib, SpecError> {
    if graph.root_path().extension().and_then(|e| e.to_str()) != Some("schlib-spec") {
        return Err(SpecError::no_span(
            SpecErrorCode::CrossDomainViolation,
            "SchLib compilation requires a .schlib-spec root",
        ));
    }
    let mut exports: IndexMap<PathBuf, IndexMap<String, Value>> = IndexMap::new();
    let mut libraries = IndexMap::new();
    let mut root = None;
    for path in graph.topological_order() {
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("schlib-spec" | "pcblib-spec")
        ) {
            return Err(SpecError::no_span(
                SpecErrorCode::CrossDomainViolation,
                format!("unsupported imported domain: {}", path.display()),
            ));
        }
        let source = graph.sources().get(path).ok_or_else(|| {
            SpecError::no_span(
                SpecErrorCode::FileNotFound,
                format!("captured source missing: {}", path.display()),
            )
        })?;
        let is_pcb = path.extension().and_then(|ext| ext.to_str()) == Some("pcblib-spec");
        let compile_file =
            || -> Result<(IndexMap<String, Value>, Option<SchLibIntent>), SpecError> {
                let mut scope = ScopeStack::new();
                scope.push();
                let mut names = BTreeMap::new();
                let mut exposed = IndexMap::new();
                for edge in source.imports() {
                    let imported = exports.get(edge.path()).ok_or_else(|| {
                        error("captured dependency must precede its importer", edge.span())
                    })?;
                    match edge.alias() {
                        Some(alias) => {
                            declare(&mut names, alias, edge.span())?;
                            scope.define(alias.to_string(), Value::Object(imported.clone().into()));
                        }
                        None => {
                            for (name, value) in imported {
                                declare(&mut names, name, edge.span())?;
                                scope.define(name.clone(), value.clone());
                                exposed.insert(name.clone(), value.clone());
                            }
                        }
                    }
                }
                for item in &source.ast().items {
                    if let SpecItem::SwapGroup(group) = &item.node {
                        if is_pcb {
                            return Err(error(
                                "swap_group is not legal in a PcbLib definition file",
                                item.span,
                            ));
                        }
                        super::compile::bind_swap(group, &mut scope, &mut names)?;
                        let name = group.name.node.as_str();
                        exposed.insert(name.clone(), Value::SwapGroup(name.clone()));
                        if let Some(binding) = &group.binding {
                            exposed.insert(binding.node.clone(), Value::SwapGroup(name));
                        }
                    }
                }
                // Validate the namespace before compiling: no definition can hide an
                // imported alias/definition or another binding in the same scope.
                for item in &source.ast().items {
                    match &item.node {
                        SpecItem::LetBinding(binding) => {
                            declare(&mut names, &binding.name.node, binding.name.span)?;
                            scope.mark_evaluating(&binding.name.node);
                            let value = eval_expr(&binding.value, &scope)?;
                            scope.define(binding.name.node.clone(), value.clone());
                            exposed.insert(binding.name.node.clone(), value);
                        }
                        SpecItem::Component(decl) => {
                            if is_pcb {
                                return Err(error(
                                    "component is not legal in a PcbLib definition file",
                                    item.span,
                                ));
                            }
                            let name = decl.name.node.as_str();
                            declare(&mut names, &name, decl.name.span)?;
                            exposed.insert(name.clone(), component_reference(path, &name));
                            if let Some(binding) = &decl.binding {
                                if binding.node != name {
                                    declare(&mut names, &binding.node, binding.span)?;
                                    exposed.insert(
                                        binding.node.clone(),
                                        component_reference(path, &name),
                                    );
                                }
                            }
                        }
                        SpecItem::Footprint(decl) => {
                            if !is_pcb {
                                return Err(error(
                                    "footprint definition is not legal in a SchLib definition file",
                                    item.span,
                                ));
                            }
                            let name = decl.name.node.as_str();
                            declare(&mut names, &name, decl.name.span)?;
                            let value = super::footprint_exports::export(path, decl, &scope)?;
                            exposed.insert(name.clone(), value.clone());
                            if let Some(binding) = &decl.binding {
                                if binding.node != name {
                                    declare(&mut names, &binding.node, binding.span)?;
                                    exposed.insert(binding.node.clone(), value);
                                }
                            }
                        }
                        SpecItem::Collection(_) if is_pcb => {
                            return Err(error(
                                "SchLib collection scope is not legal in a PcbLib definition file",
                                item.span,
                            ));
                        }
                        SpecItem::Import(_) | SpecItem::Collection(_) | SpecItem::SwapGroup(_) => {}
                        _ => {
                            return Err(error(
                                "unsupported imported domain for SchLib intent",
                                item.span,
                            ));
                        }
                    }
                }
                let intent = if is_pcb {
                    None
                } else {
                    Some(compile_schlib_with_scope(source.ast(), scope)?)
                };
                Ok((exposed, intent))
            };
        let (exposed, intent) = compile_file().map_err(|mut err| {
            if path != graph.root_path() {
                err.message = err.render(&path.display().to_string(), source.source());
                err.span = None;
            } else {
                err.message = format!(
                    "compiling captured source '{}': {}",
                    path.display(),
                    err.message
                );
            }
            err
        })?;
        exports.insert(path.clone(), exposed);
        if let Some(intent) = intent {
            if path == graph.root_path() {
                root = Some(intent);
            } else {
                libraries.insert(path.clone(), intent);
            }
        }
    }
    Ok(CompiledSchLib {
        root: root.ok_or_else(|| {
            SpecError::no_span(
                SpecErrorCode::CrossDomainViolation,
                "SchLib compilation requires a .schlib-spec root",
            )
        })?,
        imported_libraries: libraries,
    })
}
fn declare(names: &mut BTreeMap<String, Span>, name: &str, span: Span) -> Result<(), SpecError> {
    if let Some(first) = names.insert(name.to_string(), span) {
        return Err(SpecError::at(
            SpecErrorCode::DuplicateEntity,
            format!(
                "duplicate name '{name}', first declared at {}..{}",
                first.start, first.end
            ),
            span,
        ));
    }
    Ok(())
}

fn component_reference(source: &std::path::Path, name: &str) -> Value {
    Value::Definition(DefinitionReference::Component {
        source: source.to_path_buf(),
        name: name.to_string(),
    })
}
