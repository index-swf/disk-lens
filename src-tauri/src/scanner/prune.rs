use crate::models::TreeNode;

/// How `top_n` is interpreted when pruning a node's children.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopNMode {
    /// Keep the `top_n` largest children; collapse the rest.
    Count,
    /// Keep children (largest first) until their cumulative size covers
    /// `top_n` percent of the parent's size; collapse the remainder. This mirrors
    /// TreeSize's behaviour: a handful of huge folders dominate, so a 99% cutoff
    /// shows everything that matters while keeping the node count tiny.
    Percent,
}

/// Build a synthetic aggregator node for the children that were dropped because
/// they fell outside the cutoff. All size/count fields are summed so the pruned
/// tree still accounts for 100% of the bytes. The node has no children and is
/// marked `truncated` so the frontend knows it can lazy-expand the parent.
fn aggregate_others(rest: &[&TreeNode]) -> TreeNode {
    let mut size = 0u64;
    let mut allocated_size = 0u64;
    let mut size_self = 0u64;
    let mut allocated_self = 0u64;
    let mut file_count = 0u32;
    let mut folder_count = 0u32;
    for n in rest {
        size += n.size;
        allocated_size += n.allocated_size;
        size_self += n.size_self;
        allocated_self += n.allocated_self;
        file_count += n.file_count;
        folder_count += n.folder_count;
    }
    TreeNode {
        name: format!("(其他 {} 项)", rest.len()),
        size,
        allocated_size,
        size_self,
        allocated_self,
        file_count,
        folder_count,
        last_modified: 0,
        children: Vec::new(),
        truncated: true,
    }
}

/// Build a synthetic node that folds every *file* directly under a directory into
/// a single row — TreeSize-style "show me the big folders, summarise the loose
/// files". Not drillable (it has no sub-folders of its own).
fn file_aggregate(files: &[&TreeNode]) -> TreeNode {
    let mut size = 0u64;
    let mut allocated_size = 0u64;
    let mut size_self = 0u64;
    let mut allocated_self = 0u64;
    let mut file_count = 0u32;
    for f in files {
        size += f.size;
        allocated_size += f.allocated_size;
        size_self += f.size_self;
        allocated_self += f.allocated_self;
        file_count += f.file_count;
    }
    TreeNode {
        name: format!("({} 个文件)", file_count),
        size,
        allocated_size,
        size_self,
        allocated_self,
        file_count,
        folder_count: 0,
        last_modified: 0,
        children: Vec::new(),
        truncated: true,
    }
}

/// True for a child that represents a directory, as opposed to a plain file leaf.
///
/// In the aggregated tree (built by `scanner/parallel.rs`) a *directory* node
/// always carries its immediate children — both sub-folders and loose files — in
/// `children`, and reports `folder_count` >= the number of nested sub-folders.
/// A *file* leaf, by contrast, is the only node that has exactly one file
/// (itself, `file_count == 1`), no sub-folders, and no child nodes. The only
/// ambiguous
/// case is an empty folder, which we also treat as a directory (`file_count ==
/// 0`). Classifying by negating the proven-file signature is exact for this
/// model: a real directory never has `file_count == 1 && folder_count == 0 &&
/// children.is_empty()` (its files live in `children`).
fn is_dir_child(c: &TreeNode) -> bool {
    !(c.file_count == 1 && c.folder_count == 0 && c.children.is_empty())
}

/// Produce a size-bounded copy of `node` for IPC transport.
///
/// `max_depth` is a *remaining-depth budget*: the root is pruned with the full
/// budget, and each child recurses with `budget - 1`. When the budget hits 0 the
/// node keeps no children and is flagged `truncated` (only if it actually had
/// children). Because recursion is bounded by `max_depth` (a small number such as
/// 4), the call depth can never overflow the stack even on very deep trees.
///
/// At every level whose budget is still positive we:
///   1. split children into directory-children and file-children,
///   2. sort the directory-children by `size` descending,
///   3. keep the first `top_n` (Count mode) or until cumulative size reaches
///      `top_n` percent of the parent (Percent mode),
///   4. aggregate the remaining directory-children into a synthetic
///      "(其他 N 项)" node,
///   5. when `merge_files` is set, fold all file-children into one
///      "(N 个文件)" node; **when it is not set, the file-children are kept as
///      individual child nodes** (the frontend groups them under a `[files]`
///      virtual node). Never drop them silently — that made directories with
///      only loose files appear empty after expanding.
///
/// A node is reported as `truncated` when its children were not all individually
/// present in the output — either because the depth budget ran out, some
/// directory-children were collapsed, or (in `merge_files` mode) loose files were
/// folded into the synthetic node.
pub fn prune(
    node: &TreeNode,
    max_depth: u32,
    top_n: usize,
    mode: TopNMode,
    merge_files: bool,
) -> TreeNode {
    if max_depth == 0 {
        let truncated = !node.children.is_empty();
        return TreeNode {
            name: node.name.clone(),
            size: node.size,
            allocated_size: node.allocated_size,
            size_self: node.size_self,
            allocated_self: node.allocated_self,
            file_count: node.file_count,
            folder_count: node.folder_count,
            last_modified: node.last_modified,
            children: Vec::new(),
            truncated,
        };
    }

    // Split + sort directory children by reference to avoid deep-cloning the
    // whole subtree under every node.
    let mut dirs: Vec<&TreeNode> = node.children.iter().filter(|c| is_dir_child(c)).collect();
    let files: Vec<&TreeNode> = node.children.iter().filter(|c| !is_dir_child(c)).collect();
    dirs.sort_by(|a, b| b.size.cmp(&a.size));

    // Decide which directory-children to keep vs. aggregate.
    let (keep, rest_dirs): (Vec<&TreeNode>, &[&TreeNode]) = match mode {
        TopNMode::Count => {
            let n = top_n.max(1);
            if dirs.len() > n {
                (dirs[..n].to_vec(), &dirs[n..])
            } else {
                (dirs.clone(), &[])
            }
        }
        TopNMode::Percent => {
            if node.size == 0 {
                // Empty directory: fall back to keeping everything.
                (dirs.clone(), &[])
            } else {
                let threshold = (top_n as f64).clamp(1.0, 100.0) / 100.0 * (node.size as f64);
                let mut acc = 0u64;
                let mut idx = 0;
                while idx < dirs.len() {
                    acc += dirs[idx].size;
                    idx += 1;
                    if (acc as f64) >= threshold {
                        break;
                    }
                }
                if idx < dirs.len() {
                    (dirs[..idx].to_vec(), &dirs[idx..])
                } else {
                    (dirs.clone(), &[])
                }
            }
        }
    };

    let mut out: Vec<TreeNode> = Vec::with_capacity(keep.len() + 2);
    // `keep` holds `&TreeNode`; iterate by value (references are `Copy`) so each
    // `d` is already `&TreeNode` — no double-reference ambiguity.
    for d in keep {
        out.push(prune(d, max_depth - 1, top_n, mode, merge_files));
    }
    if !rest_dirs.is_empty() {
        out.push(aggregate_others(rest_dirs));
    }
    if merge_files && !files.is_empty() {
        out.push(file_aggregate(&files));
    } else {
        // merge_files=false：散文件必须保留为独立子节点（前端统一折叠进
        // [files] 虚拟节点）。之前这里直接丢弃，导致只含散文件的目录
        // 展开后一片空白（"扫描不是全量的"假象）。
        for f in &files {
            out.push((*f).clone());
        }
    }
    // 保持"子节点按 size 降序"的全局不变式（文件混入后也统一排序）。
    out.sort_by(|a, b| b.size.cmp(&a.size));

    let truncated = !rest_dirs.is_empty() || (merge_files && !files.is_empty());
    TreeNode {
        name: node.name.clone(),
        size: node.size,
        allocated_size: node.allocated_size,
        size_self: node.size_self,
        allocated_self: node.allocated_self,
        file_count: node.file_count,
        folder_count: node.folder_count,
        last_modified: node.last_modified,
        children: out,
        truncated,
    }
}
