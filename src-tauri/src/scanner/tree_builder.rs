use crate::models::TreeNode;
use std::collections::{HashMap, HashSet};

/// A flat record produced by the USN enumeration. `frn` is the file reference
/// number (parent/child link), `size`/`allocated_size` are filled afterwards by
/// stat-ing each file.
#[derive(Clone)]
pub struct FlatEntry {
    pub frn: u64,
    pub parent_frn: u64,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub allocated_size: u64,
}

/// Build an aggregated `TreeNode` forest from a flat list of file records.
///
/// Uses an explicit stack so deeply nested trees do not overflow the call stack.
/// Post-order processing (reverse of a pre-order DFS) guarantees every node is
/// aggregated only after all of its descendants.
pub fn build_tree(entries: Vec<FlatEntry>) -> Option<TreeNode> {
    let mut node_map: HashMap<u64, FlatEntry> = HashMap::with_capacity(entries.len());
    let mut pairs: Vec<(u64, u64)> = Vec::with_capacity(entries.len());
    for e in entries {
        pairs.push((e.frn, e.parent_frn));
        node_map.insert(e.frn, e);
    }

    let mut children_map: HashMap<u64, Vec<u64>> = HashMap::new();
    for (frn, parent) in &pairs {
        children_map.entry(*parent).or_default().push(*frn);
    }

    // Find the volume root: a node whose parent is 0, points to itself, or is not
    // present in the record set.
    let root_frn = {
        let mut cand = None;
        for (frn, e) in &node_map {
            if e.parent_frn == 0 || e.parent_frn == *frn {
                cand = Some(*frn);
                break;
            }
        }
        cand.or_else(|| {
            node_map
                .iter()
                .find(|(frn, e)| **frn != e.parent_frn && !node_map.contains_key(&e.parent_frn))
                .map(|(frn, _)| *frn)
        })?
    };

    // Pre-order DFS, then reverse => valid bottom-up order.
    let mut order = Vec::new();
    let mut stack = vec![root_frn];
    let mut seen: HashSet<u64> = HashSet::new();
    while let Some(frn) = stack.pop() {
        if !seen.insert(frn) {
            continue;
        }
        order.push(frn);
        if let Some(ch) = children_map.get(&frn) {
            for c in ch {
                stack.push(*c);
            }
        }
    }

    let mut agg: HashMap<u64, TreeNode> = HashMap::new();
    for frn in order.into_iter().rev() {
        let e = match node_map.get(&frn) {
            Some(e) => e.clone(),
            None => continue,
        };
        let child_frns = children_map.get(&frn).cloned().unwrap_or_default();
        let mut child_nodes: Vec<TreeNode> = child_frns
            .iter()
            .filter_map(|c| agg.remove(c))
            .collect();
        child_nodes.sort_by(|a, b| b.size.cmp(&a.size));

        let (size, alloc, fc, dc) = if e.is_dir {
            let mut s = 0u64;
            let mut a = 0u64;
            let mut f = 0u32;
            let mut d = 0u32;
            for cn in &child_nodes {
                s += cn.size;
                a += cn.allocated_size;
                f += cn.file_count;
                d += cn.folder_count;
            }
            // Add the count of immediate directory children.
            let dir_children = child_frns
                .iter()
                .filter(|c| node_map.get(*c).map(|n| n.is_dir).unwrap_or(false))
                .count() as u32;
            d += dir_children;
            (s, a, f, d)
        } else {
            (e.size, e.allocated_size, 1, 0)
        };

        agg.insert(
            frn,
            TreeNode {
                name: e.name.clone(),
                size,
                allocated_size: alloc,
                file_count: fc,
                folder_count: dc,
                children: child_nodes,
                truncated: false,
            },
        );
    }

    agg.remove(&root_frn)
}
