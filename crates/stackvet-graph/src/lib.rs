use petgraph::algo::tarjan_scc;
use petgraph::graphmap::DiGraphMap;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct ImportGraph {
    edges: Vec<(String, String)>,
    nodes: HashSet<String>,
}

impl ImportGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: impl Into<String>) {
        self.nodes.insert(node.into());
    }

    pub fn add_import(&mut self, from: impl Into<String>, to: impl Into<String>) {
        let f = from.into();
        let t = to.into();
        self.nodes.insert(f.clone());
        self.nodes.insert(t.clone());
        self.edges.push((f, t));
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn incoming_count(&self, node: &str) -> usize {
        self.edges.iter().filter(|(_, to)| to == node).count()
    }

    pub fn outgoing_count(&self, node: &str) -> usize {
        self.edges.iter().filter(|(from, _)| from == node).count()
    }

    /// Finds all circular dependencies using Tarjan's Strongly Connected Components algorithm
    pub fn find_cycles(&self) -> Vec<Vec<String>> {
        let mut graph = DiGraphMap::<&str, ()>::new();
        for (f, t) in &self.edges {
            graph.add_edge(f.as_str(), t.as_str(), ());
        }

        let sccs = tarjan_scc(&graph);
        let mut cycles = Vec::new();

        for component in sccs {
            if component.len() > 1 {
                // Multi-node cycle
                let mut cycle_nodes: Vec<String> =
                    component.into_iter().map(|s| s.to_string()).collect();
                // Ensure deterministic ordering
                cycle_nodes.sort();
                cycles.push(cycle_nodes);
            } else if component.len() == 1 {
                // Self-loop
                let node = component[0];
                if graph.contains_edge(node, node) {
                    cycles.push(vec![node.to_string()]);
                }
            }
        }

        cycles.sort_by(|a, b| a.first().cmp(&b.first()));
        cycles
    }

    /// Formats graph in Mermaid flowchart syntax
    pub fn to_mermaid(&self) -> String {
        let mut out = String::from("flowchart TD\n");
        let mut id_map: HashMap<&str, String> = HashMap::new();
        let mut counter = 0;

        for node in &self.nodes {
            let id = format!("N{}", counter);
            id_map.insert(node.as_str(), id.clone());
            out.push_str(&format!("    {}[\"{}\"]\n", id, node));
            counter += 1;
        }

        for (from, to) in &self.edges {
            if let (Some(f_id), Some(t_id)) = (id_map.get(from.as_str()), id_map.get(to.as_str()))
            {
                out.push_str(&format!("    {} --> {}\n", f_id, t_id));
            }
        }
        out
    }

    /// Formats graph in Graphviz DOT format
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph G {\n    rankdir=LR;\n");
        for (from, to) in &self.edges {
            out.push_str(&format!("    \"{}\" -> \"{}\";\n", from, to));
        }
        out.push_str("}\n");
        out
    }

    /// Formats graph in clean ASCII representation
    pub fn to_ascii(&self) -> String {
        let mut out = String::new();
        let mut sorted_nodes: Vec<&String> = self.nodes.iter().collect();
        sorted_nodes.sort();

        for node in sorted_nodes {
            let targets: Vec<&str> = self
                .edges
                .iter()
                .filter(|(f, _)| f == node)
                .map(|(_, t)| t.as_str())
                .collect();
            if targets.is_empty() {
                out.push_str(&format!("* {}\n", node));
            } else {
                out.push_str(&format!("* {} -> {}\n", node, targets.join(", ")));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_cycles() {
        let mut graph = ImportGraph::new();
        // A -> B -> C -> A (cycle of 3)
        graph.add_import("a.py", "b.py");
        graph.add_import("b.py", "c.py");
        graph.add_import("c.py", "a.py");

        // D -> E (no cycle)
        graph.add_import("d.py", "e.py");

        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 3);
        assert!(cycles[0].contains(&"a.py".to_string()));
        assert!(cycles[0].contains(&"b.py".to_string()));
        assert!(cycles[0].contains(&"c.py".to_string()));
    }

    #[test]
    fn test_self_loop() {
        let mut graph = ImportGraph::new();
        graph.add_import("loop.py", "loop.py");
        let cycles = graph.find_cycles();
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec!["loop.py".to_string()]);
    }
}
