//! Import resolver for the Altium spec language.
//!
//! Loads all files referenced by `import` declarations, detects cycles,
//! validates cross-domain rules, and builds a [`ResolvedSpec`] ready for
//! the compiler.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::ast::{SpecFile, SpecItem};
use crate::diagnostic::Span;
use crate::eval::{SpecError, SpecErrorCode};
use crate::parser::parse_spec;

// ── Public types ──────────────────────────────────────────────────────────────

/// Root AST and its direct imports after validating the complete import graph.
///
/// Transitive dependencies are validated but are not retained in this legacy
/// view. It is not a captured input snapshot for saved-plan replay.
#[derive(Debug)]
pub struct ResolvedSpec {
    /// The root file's AST.
    pub root: SpecFile,
    /// Named imports: alias → (file_path, parsed SpecFile)
    pub named_imports: IndexMap<String, (PathBuf, SpecFile)>,
    /// Direct bare imports in topological order (dependencies first), deduplicated.
    pub bare_imports: Vec<(PathBuf, SpecFile)>,
}

/// Resolve all imports starting from the given root file.
///
/// Parses all referenced files, detects cycles, validates cross-domain rules,
/// checks alias uniqueness, then builds a
/// [`ResolvedSpec`].
pub fn resolve_imports(root_path: &Path, root_ast: SpecFile) -> Result<ResolvedSpec, SpecError> {
    let root_path = root_path.canonicalize().map_err(|e| {
        SpecError::no_span(
            SpecErrorCode::FileNotFound,
            format!("failed to resolve root spec '{}': {e}", root_path.display()),
        )
    })?;

    // Cache: canonical path -> parsed SpecFile
    let mut cache: HashMap<PathBuf, SpecFile> = HashMap::new();
    cache.insert(root_path.clone(), root_ast.clone());

    // Build dependency graph by recursively parsing imports.
    // graph[file] = list of imported file paths in declaration order.
    let mut graph: IndexMap<PathBuf, Vec<ImportEdge>> = IndexMap::new();
    collect_imports(&root_path, &root_ast, &mut cache, &mut graph)?;

    // Topological sort with cycle detection.
    let topo_order = topo_sort(&root_path, &graph)?;

    // Validate cross-domain rules.
    for (from_path, edges) in &graph {
        for edge in edges {
            validate_cross_domain(from_path, &edge.to_path, edge.span).map_err(|mut error| {
                if from_path != &root_path {
                    // SpecError spans are rendered against the root by callers.
                    error.span = None;
                }
                error
            })?;
        }
    }

    // Validate alias uniqueness within each file.
    for (file_path, edges) in &graph {
        let mut seen_aliases: HashMap<String, Span> = HashMap::new();
        for edge in edges {
            if let Some(ref alias) = edge.alias {
                if let Some(prev_span) = seen_aliases.get(alias) {
                    return Err(SpecError::new(
                        SpecErrorCode::DuplicateImportAlias,
                        format!(
                            "duplicate import alias '{}' in {} (first defined at {}..{})",
                            alias,
                            file_path.display(),
                            prev_span.start,
                            prev_span.end,
                        ),
                        (file_path == &root_path).then_some(edge.span),
                    ));
                }
                seen_aliases.insert(alias.clone(), edge.span);
            }
        }
    }

    // Build named_imports and bare_imports from the root file's imports only.
    // Transitive imports are already parsed and cached.
    let root_edges = graph.get(&root_path).ok_or_else(|| {
        SpecError::no_span(
            SpecErrorCode::FileNotFound,
            format!(
                "resolver invariant violated: no import graph for '{}'",
                root_path.display()
            ),
        )
    })?;

    let mut named_imports: IndexMap<String, (PathBuf, SpecFile)> = IndexMap::new();
    let mut bare_import_paths: Vec<PathBuf> = Vec::new();

    for edge in root_edges {
        let spec_file = cached_spec(&cache, &edge.to_path)?.clone();
        if let Some(ref alias) = edge.alias {
            named_imports.insert(alias.clone(), (edge.to_path.clone(), spec_file));
        } else {
            bare_import_paths.push(edge.to_path.clone());
        }
    }

    // Build bare_imports in topological order (leaves first, skip root and named).
    // Note: bare-import entity collisions are NOT checked because each import
    // targets a different output file (reference semantics).
    let named_paths: std::collections::HashSet<PathBuf> =
        named_imports.values().map(|(p, _)| p.clone()).collect();
    let bare_imports: Vec<(PathBuf, SpecFile)> = topo_order
        .into_iter()
        .filter(|p| *p != root_path && !named_paths.contains(p) && bare_import_paths.contains(p))
        .map(|p| {
            let file = cached_spec(&cache, &p)?.clone();
            Ok((p, file))
        })
        .collect::<Result<_, SpecError>>()?;

    Ok(ResolvedSpec {
        root: root_ast,
        named_imports,
        bare_imports,
    })
}

fn cached_spec<'a>(
    cache: &'a HashMap<PathBuf, SpecFile>,
    path: &Path,
) -> Result<&'a SpecFile, SpecError> {
    cache.get(path).ok_or_else(|| {
        SpecError::no_span(
            SpecErrorCode::FileNotFound,
            format!(
                "resolver invariant violated: parsed import '{}' is missing",
                path.display()
            ),
        )
    })
}

// ── Import edge (one import declaration) ─────────────────────────────────────

#[derive(Debug, Clone)]
struct ImportEdge {
    to_path: PathBuf,
    /// Present if `import "..." as alias`; absent for bare imports.
    alias: Option<String>,
    /// Source span of the import path string (for error messages).
    span: Span,
}

// ── Recursive import collection ───────────────────────────────────────────────

/// Recursively parse all imported files and populate `cache` and `graph`.
fn collect_imports(
    file_path: &Path,
    file_ast: &SpecFile,
    cache: &mut HashMap<PathBuf, SpecFile>,
    graph: &mut IndexMap<PathBuf, Vec<ImportEdge>>,
) -> Result<(), SpecError> {
    if graph.contains_key(file_path) {
        // Already processed this file's imports.
        return Ok(());
    }

    let mut edges: Vec<ImportEdge> = Vec::new();

    for item in &file_ast.items {
        let import_decl = match &item.node {
            SpecItem::Import(d) => d,
            _ => continue,
        };

        let import_path_str = &import_decl.path.node;
        let import_span = import_decl.path.span;

        // Absolute paths are forbidden.
        if Path::new(import_path_str.as_str()).is_absolute() {
            return Err(SpecError::new(
                SpecErrorCode::FileNotFound,
                format!(
                    "absolute import paths are not allowed: '{import_path_str}' (from '{}')",
                    file_path.display()
                ),
                Some(import_span),
            ));
        }

        let resolved = resolve_import_path(file_path, import_path_str);
        let canonical = resolved.canonicalize().map_err(|e| {
            SpecError::new(
                SpecErrorCode::FileNotFound,
                format!(
                    "failed to resolve imported file '{}' (from '{}'): {e}",
                    resolved.display(),
                    file_path.display()
                ),
                Some(import_span),
            )
        })?;

        let alias = import_decl.alias.as_ref().map(|a| a.node.clone());
        edges.push(ImportEdge {
            to_path: canonical.clone(),
            alias,
            span: import_span,
        });

        // Parse if not yet cached.
        if !cache.contains_key(&canonical) {
            let source = std::fs::read_to_string(&canonical).map_err(|e| {
                SpecError::new(
                    SpecErrorCode::FileNotFound,
                    format!(
                        "failed to read import '{}' (from '{}'): {e}",
                        canonical.display(),
                        file_path.display()
                    ),
                    Some(import_span),
                )
            })?;
            let ast = parse_spec(&source).map_err(|e| {
                SpecError::no_span(
                    SpecErrorCode::ParseError,
                    format!(
                        "parse error in import from '{}':\n{}",
                        file_path.display(),
                        e.render(&canonical.display().to_string(), &source)
                    ),
                )
            })?;
            cache.insert(canonical.clone(), ast.clone());
            // Recurse.
            collect_imports(&canonical, &ast, cache, graph).map_err(|mut error| {
                // This span belongs to a nested importer, not the root source
                // used by the caller's renderer. The message retains its path.
                error.span = None;
                error
            })?;
        }
    }

    graph.insert(file_path.to_path_buf(), edges);
    Ok(())
}

// ── Path resolution ───────────────────────────────────────────────────────────

fn resolve_import_path(importing_file: &Path, import_path: &str) -> PathBuf {
    importing_file
        .parent()
        .unwrap_or(Path::new("."))
        .join(import_path)
}

// ── Topological sort with cycle detection ─────────────────────────────────────

#[derive(Clone, PartialEq)]
enum Color {
    White,
    Gray,
    Black,
}

fn topo_sort(
    root: &Path,
    graph: &IndexMap<PathBuf, Vec<ImportEdge>>,
) -> Result<Vec<PathBuf>, SpecError> {
    let mut colors: HashMap<PathBuf, Color> = HashMap::new();
    let mut order: Vec<PathBuf> = Vec::new();
    let mut stack: Vec<PathBuf> = Vec::new();

    // Start DFS from root; then visit any other nodes not yet reached.
    dfs(root, graph, &mut colors, &mut order, &mut stack)?;
    for node in graph.keys() {
        if colors.get(node).map_or(true, |c| *c == Color::White) {
            dfs(node, graph, &mut colors, &mut order, &mut stack)?;
        }
    }

    Ok(order)
}

fn dfs(
    node: &Path,
    graph: &IndexMap<PathBuf, Vec<ImportEdge>>,
    colors: &mut HashMap<PathBuf, Color>,
    order: &mut Vec<PathBuf>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), SpecError> {
    let color = colors.get(node).cloned().unwrap_or(Color::White);
    if color == Color::Black {
        return Ok(());
    }
    if color == Color::Gray {
        // Found a cycle — build the cycle path for the error message.
        let cycle_start = stack.iter().position(|p| p == node).unwrap_or(0);
        let mut cycle: Vec<PathBuf> = stack[cycle_start..].to_vec();
        cycle.push(node.to_path_buf());
        let path_str = cycle
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        return Err(SpecError::no_span(
            SpecErrorCode::CircularImport,
            format!("circular import detected: {path_str}"),
        ));
    }

    colors.insert(node.to_path_buf(), Color::Gray);
    stack.push(node.to_path_buf());

    if let Some(edges) = graph.get(node) {
        for edge in edges {
            dfs(&edge.to_path, graph, colors, order, stack)?;
        }
    }

    stack.pop();
    colors.insert(node.to_path_buf(), Color::Black);
    order.push(node.to_path_buf());
    Ok(())
}

// ── Cross-domain validation ───────────────────────────────────────────────────

/// Determine the spec domain from a file path's extension.
///
/// `foo.schlib-spec` → SchLib, `bar.pcblib-spec` → PcbLib,
/// `baz.schdoc-spec` → SchDoc, `qux.pcbdoc-spec` → PcbDoc,
/// `project.prjpcb-spec` → PrjPcb.
fn file_domain(path: &Path) -> FileDomain {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".schlib-spec") {
        FileDomain::SchLib
    } else if name.ends_with(".pcblib-spec") {
        FileDomain::PcbLib
    } else if name.ends_with(".schdoc-spec") {
        FileDomain::SchDoc
    } else if name.ends_with(".pcbdoc-spec") {
        FileDomain::PcbDoc
    } else if name.ends_with(".prjpcb-spec") {
        FileDomain::PrjPcb
    } else {
        FileDomain::Unknown
    }
}

#[derive(Debug, PartialEq)]
enum FileDomain {
    SchLib,
    PcbLib,
    SchDoc,
    PcbDoc,
    PrjPcb,
    Unknown,
}

fn validate_cross_domain(from_path: &Path, to_path: &Path, span: Span) -> Result<(), SpecError> {
    let from_domain = file_domain(from_path);
    let to_domain = file_domain(to_path);
    if from_domain == FileDomain::Unknown || to_domain == FileDomain::Unknown {
        return Err(SpecError::new(
            SpecErrorCode::CrossDomainViolation,
            format!(
                "unsupported spec import extension: {} imports {}",
                from_path.display(),
                to_path.display()
            ),
            Some(span),
        ));
    }

    // All known cross-domain imports are allowed. The spec language permits any
    // spec file to import any other spec file type (e.g. SchLib importing
    // PcbLib for typed footprint references).
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{ImportDecl, SpecFile, SpecItem};
    use crate::diagnostic::{Span, Spanned};

    fn zero_span() -> Span {
        Span { start: 0, end: 1 }
    }

    fn spanned<T>(node: T) -> Spanned<T> {
        Spanned {
            node,
            span: zero_span(),
        }
    }

    fn import_decl(path: &str, alias: Option<&str>) -> ImportDecl {
        ImportDecl {
            path: spanned(path.to_string()),
            alias: alias.map(|a| spanned(a.to_string())),
        }
    }

    fn spec_with_imports(imports: Vec<ImportDecl>) -> SpecFile {
        SpecFile {
            items: imports
                .into_iter()
                .map(|d| spanned(SpecItem::Import(d)))
                .collect(),
        }
    }

    fn empty_spec() -> SpecFile {
        SpecFile { items: vec![] }
    }

    // ── Test: empty root (no imports) ─────────────────────────────────────────

    #[test]
    fn no_imports_returns_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let ast = empty_spec();
        let resolved = resolve_imports(&root_path, ast.clone()).unwrap();

        assert!(resolved.named_imports.is_empty());
        assert!(resolved.bare_imports.is_empty());
    }

    // ── Test: named import ────────────────────────────────────────────────────

    #[test]
    fn named_import_stored_under_alias() {
        let tmp = tempfile::TempDir::new().unwrap();

        let fp_path = tmp.path().join("pads.pcblib-spec");
        std::fs::write(&fp_path, "").unwrap();

        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("pads.pcblib-spec", Some("fp"))]);
        let resolved = resolve_imports(&root_path, root_ast).unwrap();

        assert_eq!(resolved.named_imports.len(), 1);
        assert!(resolved.named_imports.contains_key("fp"));
        assert!(resolved.bare_imports.is_empty());
    }

    // ── Test: bare import ─────────────────────────────────────────────────────

    #[test]
    fn bare_import_appears_in_bare_imports() {
        let tmp = tempfile::TempDir::new().unwrap();

        let passives_path = tmp.path().join("passives.schlib-spec");
        std::fs::write(&passives_path, "").unwrap();

        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("passives.schlib-spec", None)]);
        let resolved = resolve_imports(&root_path, root_ast).unwrap();

        assert!(resolved.named_imports.is_empty());
        assert_eq!(resolved.bare_imports.len(), 1);
    }

    // ── Test: cycle detection ─────────────────────────────────────────────────

    #[test]
    fn cycle_detection_error() {
        let tmp = tempfile::TempDir::new().unwrap();

        // a.schlib-spec imports b.schlib-spec
        // b.schlib-spec imports a.schlib-spec  → cycle
        let a_path = tmp.path().join("a.schlib-spec");
        let b_path = tmp.path().join("b.schlib-spec");

        std::fs::write(&a_path, r#"import "b.schlib-spec""#).unwrap();
        std::fs::write(&b_path, r#"import "a.schlib-spec""#).unwrap();

        let a_source = std::fs::read_to_string(&a_path).unwrap();
        let a_ast = parse_spec(&a_source).unwrap();

        let err = resolve_imports(&a_path, a_ast).unwrap_err();
        assert_eq!(err.code, SpecErrorCode::CircularImport);
        assert!(
            err.message.contains("circular import"),
            "got: {}",
            err.message
        );
    }

    // ── Test: duplicate alias ─────────────────────────────────────────────────

    #[test]
    fn duplicate_alias_error() {
        let tmp = tempfile::TempDir::new().unwrap();

        let f1_path = tmp.path().join("f1.schlib-spec");
        let f2_path = tmp.path().join("f2.schlib-spec");
        std::fs::write(&f1_path, "").unwrap();
        std::fs::write(&f2_path, "").unwrap();

        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![
            import_decl("f1.schlib-spec", Some("fp")),
            import_decl("f2.schlib-spec", Some("fp")), // duplicate alias
        ]);

        let err = resolve_imports(&root_path, root_ast).unwrap_err();
        assert_eq!(err.code, SpecErrorCode::DuplicateImportAlias);
        assert!(err.message.contains("fp"), "got: {}", err.message);
    }

    // ── Test: cross-domain imports are allowed ──────────────────────────────

    #[test]
    fn schlib_can_import_schlib() {
        let tmp = tempfile::TempDir::new().unwrap();

        let sch_path = tmp.path().join("comps.schlib-spec");
        std::fs::write(&sch_path, "").unwrap();

        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("comps.schlib-spec", None)]);
        let resolved = resolve_imports(&root_path, root_ast).unwrap();
        assert_eq!(resolved.bare_imports.len(), 1);
    }

    // ── Test: bare import collision is now allowed (reference semantics) ─────

    #[test]
    fn bare_import_collision_allowed() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Both files define component "R" — no longer an error because each
        // import targets a different output file.
        let f1_path = tmp.path().join("f1.schlib-spec");
        let f2_path = tmp.path().join("f2.schlib-spec");
        std::fs::write(&f1_path, "component R {}").unwrap();
        std::fs::write(&f2_path, "component R {}").unwrap();

        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![
            import_decl("f1.schlib-spec", None),
            import_decl("f2.schlib-spec", None),
        ]);

        let resolved = resolve_imports(&root_path, root_ast).unwrap();
        assert_eq!(resolved.bare_imports.len(), 2);
    }

    // ── Test: PrjPcb cross-domain rules ───────────────────────────────────────

    #[test]
    fn prjpcb_can_import_schlib() {
        let tmp = tempfile::TempDir::new().unwrap();

        let sch_path = tmp.path().join("comps.schlib-spec");
        std::fs::write(&sch_path, "").unwrap();

        let root_path = tmp.path().join("root.prjpcb-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("comps.schlib-spec", None)]);
        let resolved = resolve_imports(&root_path, root_ast).unwrap();
        assert_eq!(resolved.bare_imports.len(), 1);
    }

    #[test]
    fn prjpcb_can_import_pcblib() {
        let tmp = tempfile::TempDir::new().unwrap();

        let pcb_path = tmp.path().join("pads.pcblib-spec");
        std::fs::write(&pcb_path, "").unwrap();

        let root_path = tmp.path().join("root.prjpcb-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("pads.pcblib-spec", None)]);
        let resolved = resolve_imports(&root_path, root_ast).unwrap();
        assert_eq!(resolved.bare_imports.len(), 1);
    }

    #[test]
    fn prjpcb_can_import_prjpcb() {
        let tmp = tempfile::TempDir::new().unwrap();

        let nested_path = tmp.path().join("other.prjpcb-spec");
        std::fs::write(&nested_path, "").unwrap();

        let root_path = tmp.path().join("root.prjpcb-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("other.prjpcb-spec", None)]);
        let resolved = resolve_imports(&root_path, root_ast).unwrap();
        assert_eq!(resolved.bare_imports.len(), 1);
    }

    // ── Test: file not found ──────────────────────────────────────────────────

    #[test]
    fn file_not_found_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root_path = tmp.path().join("root.schlib-spec");
        std::fs::write(&root_path, "").unwrap();

        let root_ast = spec_with_imports(vec![import_decl("nonexistent.schlib-spec", None)]);
        let err = resolve_imports(&root_path, root_ast).unwrap_err();
        assert_eq!(err.code, SpecErrorCode::FileNotFound);
        assert_eq!(
            err.span,
            Some(zero_span()),
            "direct import preserves root span"
        );
    }

    fn write_spec(dir: &Path, name: &str, source: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, source).unwrap();
        path
    }

    fn resolve_file(path: &Path) -> Result<ResolvedSpec, SpecError> {
        let source = std::fs::read_to_string(path).unwrap();
        resolve_imports(path, parse_spec(&source).unwrap())
    }

    #[test]
    fn diamond_import_order_is_deterministic_and_canonical_paths_are_deduplicated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let shared = write_spec(tmp.path(), "shared.schlib-spec", "component Shared {}");
        let left = write_spec(
            tmp.path(),
            "left.schlib-spec",
            r#"import "shared.schlib-spec""#,
        );
        let right = write_spec(
            tmp.path(),
            "right.schlib-spec",
            r#"import "./shared.schlib-spec""#,
        );
        let root = write_spec(
            tmp.path(),
            "root.schlib-spec",
            r#"
            import "left.schlib-spec"
            import "right.schlib-spec"
            import "shared.schlib-spec"
            import "./shared.schlib-spec"
        "#,
        );
        let expected = vec![
            shared.canonicalize().unwrap(),
            left.canonicalize().unwrap(),
            right.canonicalize().unwrap(),
        ];
        for _ in 0..3 {
            let resolved = resolve_file(&root).unwrap();
            assert_eq!(
                resolved
                    .bare_imports
                    .iter()
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn aliases_are_local_to_each_importing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_spec(tmp.path(), "leaf.schlib-spec", "component Leaf {}");
        write_spec(
            tmp.path(),
            "middle.schlib-spec",
            r#"import "leaf.schlib-spec" as lib"#,
        );
        let root = write_spec(
            tmp.path(),
            "root.schlib-spec",
            r#"import "middle.schlib-spec" as lib"#,
        );
        let resolved = resolve_file(&root).unwrap();
        assert_eq!(
            resolved
                .named_imports
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["lib"]
        );
        assert!(
            resolved.named_imports["lib"]
                .0
                .ends_with("middle.schlib-spec")
        );
        assert!(resolved.bare_imports.is_empty());
    }

    #[test]
    fn duplicate_alias_in_transitive_file_reports_that_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_spec(tmp.path(), "leaf.schlib-spec", "");
        write_spec(
            tmp.path(),
            "middle.schlib-spec",
            r#"
            import "leaf.schlib-spec" as lib
            import "leaf.schlib-spec" as lib
        "#,
        );
        let root = write_spec(
            tmp.path(),
            "root.schlib-spec",
            r#"import "middle.schlib-spec""#,
        );
        let error = resolve_file(&root).unwrap_err();
        assert_eq!(error.code, SpecErrorCode::DuplicateImportAlias);
        assert!(
            error.span.is_none(),
            "nested alias span must not render in the root"
        );
        assert!(
            error.message.contains("middle.schlib-spec"),
            "{}",
            error.message
        );
    }

    #[test]
    fn transitive_missing_import_reports_importer_and_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_spec(
            tmp.path(),
            "middle.schlib-spec",
            r#"import "missing.schlib-spec""#,
        );
        let root = write_spec(
            tmp.path(),
            "root.schlib-spec",
            r#"import "middle.schlib-spec" as lib"#,
        );
        let error = resolve_file(&root).unwrap_err();
        assert_eq!(error.code, SpecErrorCode::FileNotFound);
        assert!(
            error.span.is_none(),
            "nested import span must not render in the root"
        );
        assert!(
            error.message.contains("middle.schlib-spec"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("missing.schlib-spec"),
            "{}",
            error.message
        );
    }

    #[test]
    fn imported_parse_error_renders_the_imported_source_location() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_spec(tmp.path(), "bad.schlib-spec", "\ncomponent Bad {\n  ???\n}");
        let root = write_spec(
            tmp.path(),
            "root.schlib-spec",
            r#"import "bad.schlib-spec""#,
        );
        let error = resolve_file(&root).unwrap_err();
        assert_eq!(error.code, SpecErrorCode::ParseError);
        assert!(
            error.message.contains("bad.schlib-spec:3:"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("root.schlib-spec"),
            "{}",
            error.message
        );
        assert!(error.message.contains("???"), "{}", error.message);
        assert!(
            error.span.is_none(),
            "imported span must not be interpreted in root source"
        );
    }

    #[test]
    fn imported_non_utf8_source_is_rejected_with_path_context() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("bad.schlib-spec"), [0xff]).unwrap();
        let root = write_spec(
            tmp.path(),
            "root.schlib-spec",
            r#"import "bad.schlib-spec""#,
        );
        let error = resolve_file(&root).unwrap_err();
        assert_eq!(error.code, SpecErrorCode::FileNotFound);
        assert!(
            error.message.contains("bad.schlib-spec"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("root.schlib-spec"),
            "{}",
            error.message
        );
    }

    #[test]
    fn missing_root_has_path_context() {
        let tmp = tempfile::TempDir::new().unwrap();
        let error =
            resolve_imports(&tmp.path().join("absent.schlib-spec"), empty_spec()).unwrap_err();
        assert_eq!(error.code, SpecErrorCode::FileNotFound);
        assert!(
            error.message.contains("absent.schlib-spec"),
            "{}",
            error.message
        );
    }
}
