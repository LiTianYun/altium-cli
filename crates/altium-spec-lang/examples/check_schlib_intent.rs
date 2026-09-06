//! Read-only diagnostic for the evolving SchLib intent API; does not plan/apply.
use altium_spec_lang::{import::capture_imports, intent::compile_captured_schlib};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err("usage: check_schlib_intent FILE.schlib-spec [...]".into());
    }
    for path in paths {
        let graph = capture_imports(&path)?;
        let captured = graph
            .sources()
            .get(graph.root_path())
            .ok_or("captured graph has no root source")?;
        let intent = compile_captured_schlib(&graph)
            .map_err(|error| error.render(&path.display().to_string(), captured.source()))?;
        let repeated = compile_captured_schlib(&graph)
            .map_err(|error| error.render(&path.display().to_string(), captured.source()))?;
        if intent != repeated {
            return Err(format!("non-deterministic intent compilation: {}", path.display()).into());
        }
        let components = &intent.root.components.items;
        let pins: usize = components
            .iter()
            .map(|component| {
                component.pins.items.len()
                    + component
                        .parts
                        .items
                        .iter()
                        .map(|part| part.pins.items.len())
                        .sum::<usize>()
            })
            .sum();
        let graphics: usize = components
            .iter()
            .map(|component| {
                component.graphics.items.len()
                    + component
                        .parts
                        .items
                        .iter()
                        .map(|part| part.graphics.items.len())
                        .sum::<usize>()
            })
            .sum();
        println!(
            "{}: {} components, {pins} pins, {graphics} graphics",
            path.display(),
            components.len()
        );
    }
    Ok(())
}
