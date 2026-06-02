use petgraph::graph::DiGraph;

use crate::error::Result;

pub struct DependencyGraph {
    graph: DiGraph<String, ()>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
        }
    }

    pub fn add_package(&mut self, name: &str) -> usize {
        self.graph.add_node(name.to_string()).index()
    }

    pub fn add_dependency(&mut self, from: usize, to: usize) -> Result<()> {
        self.graph.add_edge(
            petgraph::graph::NodeIndex::new(from),
            petgraph::graph::NodeIndex::new(to),
            (),
        );
        Ok(())
    }

    pub fn topological_sort(&self) -> Vec<String> {
        petgraph::algo::toposort(&self.graph, None)
            .unwrap_or_default()
            .into_iter()
            .map(|idx| self.graph[idx].clone())
            .collect()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}
