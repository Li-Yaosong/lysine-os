use std::collections::HashMap;
use std::path::{Path, PathBuf};

use petgraph::algo::is_cyclic_directed;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use ribosome_parser::{parse_mrna_file, MrnaFile};
use walkdir::WalkDir;

use crate::error::{DepsError, Result};

/// Genome DAG of package build dependencies.
///
/// Edge direction: `A → B` means "A depends on B" (A requires B to build).
/// Therefore `topological_sort()` returns packages in "build-first" order:
/// leaves (no outgoing edges, i.e. packages with no dependencies) come first.
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

    /// Add a dependency edge: `from` depends on `to`.
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

    /// Return packages in topological order.
    ///
    /// For our graph convention (`A → B` means A depends on B), petgraph's
    /// `toposort` returns nodes so that if there is an edge `u → v`, then `u`
    /// comes before `v`. This means dependents come first, dependencies last.
    ///
    /// We **reverse** the result so that dependencies come first (build order).
    ///
    /// Returns `DepsError::CircularDependency` if the graph contains a cycle.
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        let topo = petgraph::algo::toposort(&self.graph, None).map_err(|cycle| {
            let cycle_node = &self.graph[cycle.node_id()];
            DepsError::CircularDependency {
                cycle: cycle_node.clone(),
            }
        })?;

        // Reverse: dependencies first (leaves first), dependents last.
        Ok(topo
            .into_iter()
            .rev()
            .map(|idx| self.graph[idx].clone())
            .collect())
    }

    pub fn has_cycle(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    pub fn cycle_packages(&self) -> Vec<String> {
        if !self.has_cycle() {
            return Vec::new();
        }
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

    /// Returns all packages needed to build `name` (including transitive deps),
    /// in build order (dependencies first, `name` last).
    ///
    /// Returns an error if `name` is not in the graph or the graph has a cycle.
    pub fn build_order(&self, name: &str) -> Result<Vec<String>> {
        let start = self.index.get(name).ok_or_else(|| {
            DepsError::MissingDependency(format!("package '{name}' not found in graph"))
        })?;

        // BFS to collect all reachable nodes from `name` following outgoing edges
        // (i.e. all transitive dependencies).
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(*start);
        visited.insert(*start);

        while let Some(node) = queue.pop_front() {
            for neighbor in self.graph.neighbors_directed(node, Direction::Outgoing) {
                if visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }

        // Topological sort the full graph (returns Err on cycle), then filter.
        let topo = petgraph::algo::toposort(&self.graph, None).map_err(|cycle| {
            let cycle_node = &self.graph[cycle.node_id()];
            DepsError::CircularDependency {
                cycle: cycle_node.clone(),
            }
        })?;

        // Reverse so dependencies come first, then filter to only visited nodes.
        let order: Vec<String> = topo
            .into_iter()
            .rev()
            .filter(|idx| visited.contains(idx))
            .map(|idx| self.graph[idx].clone())
            .collect();

        Ok(order)
    }

    /// Returns the direct dependencies of `name`.
    pub fn direct_dependencies(&self, name: &str) -> Vec<String> {
        let Some(&idx) = self.index.get(name) else {
            return Vec::new();
        };
        self.graph
            .neighbors_directed(idx, Direction::Outgoing)
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Returns all packages that depend on `name` (reverse dependencies).
    pub fn reverse_dependencies(&self, name: &str) -> Vec<String> {
        let Some(&idx) = self.index.get(name) else {
            return Vec::new();
        };

        // BFS over incoming edges to find all reverse dependents.
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(idx);

        while let Some(node) = queue.pop_front() {
            for neighbor in self.graph.neighbors_directed(node, Direction::Incoming) {
                if visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }

        visited
            .into_iter()
            .filter(|&idx| idx != self.index.get(name).copied().unwrap_or(idx))
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    /// Returns packages from `names` that can be safely removed — i.e., not
    /// depended upon by any package in `installed` (excluding the removal candidates).
    pub fn removable(&self, names: &[&str], installed: &[&str]) -> Vec<String> {
        let name_set: std::collections::HashSet<&str> = names.iter().copied().collect();

        let mut safe = Vec::new();
        for &name in names {
            let reverse_deps = self.reverse_dependencies(name);
            let blocked = reverse_deps
                .iter()
                .any(|dep| installed.contains(&dep.as_str()) && !name_set.contains(dep.as_str()));
            if !blocked {
                safe.push(name.to_string());
            }
        }
        safe
    }

    /// Returns the transitive runtime dependency closure for the given packages,
    /// resolved from the graph edges. Order is build order (deps first).
    ///
    /// Silently ignores cycle errors (best-effort for install planning).
    pub fn resolve_install_order(&self, names: &[&str]) -> Vec<String> {
        let mut all_deps = std::collections::HashSet::new();
        for name in names {
            if let Ok(order) = self.build_order(name) {
                for dep in order {
                    all_deps.insert(dep);
                }
            } else {
                all_deps.insert(name.to_string());
            }
        }

        // Sort topologically (best-effort; ignore cycle errors for this helper).
        let topo = self.topological_sort().unwrap_or_else(|_| {
            // Fallback: return packages in arbitrary order.
            self.graph
                .node_indices()
                .map(|idx| self.graph[idx].clone())
                .collect::<Vec<_>>()
        });
        let mut result: Vec<String> = topo
            .into_iter()
            .filter(|name| all_deps.contains(name))
            .collect();

        // Add any packages not in the graph at the end.
        for name in names {
            if !self.index.contains_key(*name) && !result.contains(&name.to_string()) {
                result.push(name.to_string());
            }
        }

        result
    }

    /// Check if a package exists in the graph.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn build_sample_graph() -> DependencyGraph {
        // A → B → D
        // A → C → D
        // B → E
        let mut g = DependencyGraph::new();
        g.add_dependency("a", "b").unwrap();
        g.add_dependency("a", "c").unwrap();
        g.add_dependency("b", "d").unwrap();
        g.add_dependency("c", "d").unwrap();
        g.add_dependency("b", "e").unwrap();
        g
    }

    #[test]
    fn test_topological_sort_dependencies_first() {
        let graph = build_sample_graph();
        let order = graph.topological_sort().unwrap();

        // Dependencies must come before dependents.
        let d_pos = order.iter().position(|n| n == "d").unwrap();
        let b_pos = order.iter().position(|n| n == "b").unwrap();
        let c_pos = order.iter().position(|n| n == "c").unwrap();
        let a_pos = order.iter().position(|n| n == "a").unwrap();
        let e_pos = order.iter().position(|n| n == "e").unwrap();

        assert!(d_pos < b_pos, "d (dependency) should come before b");
        assert!(d_pos < c_pos, "d (dependency) should come before c");
        assert!(b_pos < a_pos, "b (dependency) should come before a");
        assert!(c_pos < a_pos, "c (dependency) should come before a");
        assert!(e_pos < b_pos, "e (dependency) should come before b");
    }

    #[test]
    fn test_build_order_returns_all_deps() {
        let graph = build_sample_graph();
        let order = graph.build_order("a").unwrap();

        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));
        assert!(order.contains(&"d".to_string()));
        assert!(order.contains(&"e".to_string()));
        assert_eq!(order.len(), 5);

        // Dependencies should come first.
        let d_pos = order.iter().position(|n| n == "d").unwrap();
        let a_pos = order.iter().position(|n| n == "a").unwrap();
        assert!(d_pos < a_pos, "d should be built before a");
    }

    #[test]
    fn test_build_order_leaf_node() {
        let graph = build_sample_graph();
        let order = graph.build_order("d").unwrap();
        assert_eq!(order, vec!["d"]);
    }

    #[test]
    fn test_build_order_missing_package() {
        let graph = build_sample_graph();
        assert!(graph.build_order("nonexistent").is_err());
    }

    #[test]
    fn test_direct_dependencies() {
        let graph = build_sample_graph();
        let mut deps = graph.direct_dependencies("a");
        deps.sort();
        assert_eq!(deps, vec!["b", "c"]);

        let deps_d = graph.direct_dependencies("d");
        assert!(deps_d.is_empty());
    }

    #[test]
    fn test_reverse_dependencies() {
        let graph = build_sample_graph();
        let rdeps = graph.reverse_dependencies("d");
        assert!(rdeps.contains(&"b".to_string()));
        assert!(rdeps.contains(&"c".to_string()));
        assert!(rdeps.contains(&"a".to_string()));
    }

    #[test]
    fn test_reverse_dependencies_leaf() {
        let graph = build_sample_graph();
        let rdeps = graph.reverse_dependencies("a");
        assert!(rdeps.is_empty());
    }

    #[test]
    fn test_removable_all_safe() {
        let graph = build_sample_graph();
        let safe = graph.removable(&["e"], &["a", "b", "c", "d", "e"]);
        assert!(safe.is_empty());
    }

    #[test]
    fn test_removable_leaf_is_safe() {
        let graph = build_sample_graph();
        let safe = graph.removable(&["e"], &["a", "c", "d", "e"]);
        assert!(safe.is_empty());
    }

    #[test]
    fn test_removable_truly_safe() {
        let graph = build_sample_graph();
        let safe = graph.removable(&["e"], &["e"]);
        assert_eq!(safe, vec!["e"]);
    }

    #[test]
    fn test_removable_blocked() {
        let graph = build_sample_graph();
        let safe = graph.removable(&["d"], &["a", "b", "c", "d", "e"]);
        assert!(safe.is_empty());
    }

    #[test]
    fn test_resolve_install_order() {
        let graph = build_sample_graph();
        let order = graph.resolve_install_order(&["a"]);
        assert!(order.contains(&"d".to_string()));
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
        assert!(order.contains(&"c".to_string()));
        assert!(order.contains(&"e".to_string()));
    }

    #[test]
    fn test_contains() {
        let graph = build_sample_graph();
        assert!(graph.contains("a"));
        assert!(graph.contains("d"));
        assert!(!graph.contains("z"));
    }

    #[test]
    fn test_topological_sort_rejects_cycle() {
        let mut g = DependencyGraph::new();
        g.add_dependency("a", "b").unwrap();
        g.add_dependency("b", "a").unwrap(); // cycle!
        assert!(g.has_cycle());
        assert!(g.topological_sort().is_err());
    }
}
