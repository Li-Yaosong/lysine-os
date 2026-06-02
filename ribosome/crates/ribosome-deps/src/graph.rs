use std::collections::HashMap;
use std::path::{Path, PathBuf};

use petgraph::algo::is_cyclic_directed;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use ribosome_parser::{parse_mrna_file, MrnaFile};
use walkdir::WalkDir;

use crate::error::{DepsError, Result};

/// Genome DAG of package build dependencies.
pub struct DependencyGraph {
    graph: DiGraph<String, ()>,
    index: HashMap<String, NodeIndex>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
        }
    }

    pub fn add_package(&mut self, name: &str) -> NodeIndex {
        if let Some(&idx) = self.index.get(name) {
            return idx;
        }
        let idx = self.graph.add_node(name.to_string());
        self.index.insert(name.to_string(), idx);
        idx
    }

    pub fn add_dependency(&mut self, from: &str, to: &str) -> Result<()> {
        let from_idx = self.add_package(from);
        let to_idx = self.add_package(to);
        if from_idx != to_idx && !self.graph.contains_edge(from_idx, to_idx) {
            self.graph.add_edge(from_idx, to_idx, ());
        }
        Ok(())
    }

    /// Load all `.mRNA` files under `root` and add build-time edges.
    pub fn load_mrna_directory(&mut self, root: &Path) -> Result<Vec<PathBuf>> {
        let mut loaded = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("mRNA") {
                continue;
            }
            let mrna = parse_mrna_file(path).map_err(|e| DepsError::Parse {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;
            self.add_mrna(&mrna);
            loaded.push(path.to_path_buf());
        }
        Ok(loaded)
    }

    pub fn add_mrna(&mut self, mrna: &MrnaFile) {
        self.add_package(&mrna.name);
        if let Some(depends) = &mrna.depends {
            if let Some(build_deps) = &depends.build {
                for dep in build_deps {
                    if let Ok(spec) = ribosome_parser::parse_dependency_spec(dep) {
                        self.add_dependency(&mrna.name, &spec.name)
                            .expect("internal graph error");
                    }
                }
            }
        }
    }

    pub fn topological_sort(&self) -> Vec<String> {
        petgraph::algo::toposort(&self.graph, None)
            .unwrap_or_default()
            .into_iter()
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    pub fn has_cycle(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    pub fn cycle_packages(&self) -> Vec<String> {
        if !self.has_cycle() {
            return Vec::new();
        }
        // Return nodes that participate in any cycle (approximation: nodes with back-edges in DFS)
        let mut cyclic = Vec::new();
        for idx in self.graph.node_indices() {
            for neighbor in self.graph.neighbors_directed(idx, Direction::Outgoing) {
                if petgraph::algo::has_path_connecting(&self.graph, neighbor, idx, None) {
                    let name = &self.graph[idx];
                    if !cyclic.contains(name) {
                        cyclic.push(name.clone());
                    }
                }
            }
        }
        cyclic
    }

    /// Emit Graphviz DOT format.
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph genome {\n");
        for idx in self.graph.node_indices() {
            let name = &self.graph[idx];
            let id = dot_id(name);
            out.push_str(&format!("  {id} [label=\"{name}\"];\n"));
        }
        for edge in self.graph.edge_indices() {
            let (a, b) = self.graph.edge_endpoints(edge).unwrap();
            let from = dot_id(&self.graph[a]);
            let to = dot_id(&self.graph[b]);
            out.push_str(&format!("  {from} -> {to};\n"));
        }
        out.push_str("}\n");
        out
    }

    pub fn package_count(&self) -> usize {
        self.graph.node_count()
    }
}

fn dot_id(name: &str) -> String {
    name.replace('-', "_")
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}
