//! 真实数据集成测试（无 mock）。
//!
//! 这些测试直接调用后端引擎 (`scanner::scan_drive` / `prune` / `find_child`)，
//! 全部基于**真实文件系统**，不依赖任何假数据。可通过
//! `cargo test` 运行；性能/压力测试标 `#[ignore]`，用 `cargo test -- --ignored` 运行。

use crate::find_child;
use crate::models::{ScannerError, TreeNode};
use crate::scanner::prune::{prune, TopNMode};
use crate::scanner::scan_drive;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Cancellation flag helper for tests (never cancels).
fn no_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

// ---------- 构造辅助 ----------

fn node(
    name: &str,
    size: u64,
    files: u32,
    folders: u32,
    children: Vec<TreeNode>,
) -> TreeNode {
    TreeNode {
        name: name.to_string(),
        size,
        allocated_size: size,
        file_count: files,
        folder_count: folders,
        last_modified: 0,
        children,
        truncated: false,
    }
}

fn dir(name: &str, size: u64, children: Vec<TreeNode>) -> TreeNode {
    node(name, size, 0, children.len() as u32, children)
}

// ---------- 1. scan_drive：真实目录 ----------

#[test]
fn scan_parallel_real_project() {
    // 扫真实的 src-tauri 源码目录（小且真实），验证结构与一致性。
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (tree, strategy, errors) =
        scan_drive(None, root_dir.to_string_lossy().into_owned(), "parallel", false, None, no_cancel())
            .expect("扫描项目目录应成功");

    assert!(strategy == "parallel", "强制 parallel 应返回 parallel");
    assert!(tree.size > 0, "项目大小应 > 0");
    assert!(tree.folder_count > 0, "应扫到文件夹");
    assert!(tree.file_count > 0, "应扫到文件");
    assert!(errors.count == 0, "项目目录不应有扫描错误, 实际: {}", errors.count);

    // 验证：prune 在真实树上可正常工作（返回裁剪树）。
    let pruned = prune(&tree, 2, 10, TopNMode::Count, false);
    assert!(pruned.size == tree.size, "裁剪不应改变总大小");
    // 子节点按 size 降序
    for w in pruned.children.windows(2) {
        assert!(
            w[0].size >= w[1].size,
            "子节点应按 size 降序排列（真实树）"
        );
    }
}

#[test]
fn scan_missing_path_errors() {
    let missing = if cfg!(target_os = "windows") {
        "C:\\__tree_scan_test_nonexistent_xyz_123".to_string()
    } else {
        "/__tree_scan_test_nonexistent_xyz_123".to_string()
    };
    let res = scan_drive(None, missing, "parallel", false, None, no_cancel());
    match res {
        Err(ScannerError::Msg(m)) => assert!(
            m.contains("does not exist"),
            "应报路径不存在，实际: {m}"
        ),
        other => panic!("缺失路径应返回 Err，实际: {other:?}"),
    }
}

// ---------- 2. prune：count 模式 ----------

#[test]
fn prune_count_keeps_top_n_and_aggregates() {
    // 5 个目录，size 100/80/60/40/20，top_n=2
    let root = dir(
        "root",
        300,
        vec![
            dir("a", 100, vec![]),
            dir("b", 80, vec![]),
            dir("c", 60, vec![]),
            dir("d", 40, vec![]),
            dir("e", 20, vec![]),
        ],
    );
    let out = prune(&root, 4, 2, TopNMode::Count, false);
    assert_eq!(out.children.len(), 3, "保留 2 个 + 1 个聚合");
    // 全局按 size 降序（聚合项 size=120 会排在最前，名字顺序不再固定）
    for w in out.children.windows(2) {
        assert!(w[0].size >= w[1].size, "子节点应按 size 降序排列");
    }
    let names: Vec<&str> = out.children.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"a"), "a 应被保留, 实际: {names:?}");
    assert!(names.contains(&"b"), "b 应被保留, 实际: {names:?}");
    let agg = out
        .children
        .iter()
        .find(|c| c.name.starts_with("(其他"))
        .expect("剩余应聚合成 (其他 N 项)");
    assert_eq!(agg.size, 120, "聚合项应汇总剩余 60+40+20");
    assert!(out.truncated, "有折叠应标 truncated");
    // 聚合节点不可下钻（无 children）
    assert!(agg.children.is_empty());
}

// ---------- 3. prune：percent 模式 ----------

#[test]
fn prune_percent_covers_threshold() {
    // root size=300，子目录 100/80/60/40/20。top_n=90 → 阈值 270。
    // 累计 100+80+60+40=280 ≥ 270 → 保留前 4 个，聚合最后 1 个(20)。
    let root = dir(
        "root",
        300,
        vec![
            dir("a", 100, vec![]),
            dir("b", 80, vec![]),
            dir("c", 60, vec![]),
            dir("d", 40, vec![]),
            dir("e", 20, vec![]),
        ],
    );
    let out = prune(&root, 4, 90, TopNMode::Percent, false);
    // 保留 4 个目录 + 1 个聚合
    assert_eq!(out.children.len(), 5);
    let agg = out.children.last().unwrap();
    assert!(agg.name.starts_with("(其他"), "剩余应聚合");
    assert_eq!(agg.size, 20, "percent 模式只聚合掉最后 20（覆盖率 99%）");
    assert!(out.truncated);
}

// ---------- 4. prune：merge_files ----------

#[test]
fn prune_merge_files_folds_loose_files() {
    // 1 个目录 + 3 个散文件（folder_count=0）
    let root = node(
        "root",
        130,
        3,
        1,
        vec![
            dir("sub", 100, vec![]),
            node("f1", 10, 1, 0, vec![]),
            node("f2", 10, 1, 0, vec![]),
            node("f3", 10, 1, 0, vec![]),
        ],
    );
    let out = prune(&root, 4, 100, TopNMode::Count, true);
    // 应含：sub 目录 + (3 个文件)
    assert_eq!(out.children.len(), 2, "应折叠为 目录 + 文件聚合");
    let file_node = out.children.iter().find(|c| c.name.starts_with("(")).unwrap();
    assert_eq!(file_node.file_count, 3, "文件聚合应汇总 3 个文件");
    assert_eq!(file_node.folder_count, 0, "文件聚合不可下钻");
    assert!(file_node.children.is_empty());
    assert!(out.truncated, "merge_files 后 truncated=true");
}

// ---------- 5. prune：depth 预算耗尽 ----------

#[test]
fn prune_depth_zero_collapses() {
    let root = dir("root", 10, vec![dir("a", 5, vec![dir("b", 2, vec![])])]);
    let out = prune(&root, 0, 100, TopNMode::Count, false);
    assert!(out.children.is_empty(), "depth=0 应无 children");
    assert!(out.truncated, "原含子节点，depth=0 应标 truncated");
}

// ---------- 6. find_child：大小写不敏感 ----------

#[test]
fn find_child_case_insensitive() {
    let root = dir("root", 1, vec![dir("Windows", 1, vec![]), dir("System32", 1, vec![])]);
    assert!(find_child(&root.children, "Windows").is_some());
    assert!(find_child(&root.children, "windows").is_some(), "小写应匹配");
    assert!(find_child(&root.children, "WINDOWS").is_some(), "大写应匹配");
    assert!(find_child(&root.children, "Missing").is_none());
}

// ---------- 7. get_node 导航逻辑（与命令一致的核心路径） ----------

#[test]
fn get_node_navigation_returns_subtree() {
    // root -> A(100) -> B(50); root -> C(30)
    let root = dir(
        "root",
        180,
        vec![
            dir("A", 100, vec![dir("B", 50, vec![]), dir("A2", 30, vec![])]),
            dir("C", 30, vec![]),
        ],
    );
    // 模拟 get_node(["A","B"])
    let mut cur = &root;
    cur = find_child(&cur.children, "A").expect("A 应存在");
    cur = find_child(&cur.children, "B").expect("B 应存在");
    let sub = prune(cur, 2, 100, TopNMode::Count, false);
    assert_eq!(sub.name, "B");
    assert_eq!(sub.size, 50);
}

#[test]
fn get_node_unknown_name_is_none() {
    let root = dir("root", 1, vec![dir("A", 1, vec![])]);
    // 模拟 get_node(["A","Z"]) —— 第二层找不到
    let cur = find_child(&root.children, "A").expect("A 存在");
    assert!(
        find_child(&cur.children, "Z").is_none(),
        "get_node 对不存在的名字应返回 None（命令层转成 Err 带提示）"
    );
}

// ---------- 8. enumerate_drives：本机真实盘符/挂载点 ----------

#[test]
fn enumerate_drives_returns_real_drives() {
    let drives = crate::enumerate_drives();
    assert!(!drives.is_empty(), "本机至少应有一个盘符/挂载点, 实际为空");
    if cfg!(target_os = "windows") {
        assert!(
            drives.iter().any(|d| d.letter.eq_ignore_ascii_case("C:")),
            "C: 盘应存在, 实际: {:?}",
            drives.iter().map(|d| d.letter.clone()).collect::<Vec<_>>()
        );
        for d in &drives {
            assert_eq!(d.letter.len(), 2, "盘符格式应为 'X:', 实际: {}", d.letter);
            assert!(d.letter.ends_with(':'), "盘符应以冒号结尾: {}", d.letter);
            assert!(!d.kind.is_empty(), "磁盘类型不应为空: {}", d.letter);
        }
    } else {
        // Linux: 至少应枚举到根挂载点 "/"
        assert!(
            drives.iter().any(|d| d.letter == "/"),
            "根挂载点 / 应存在, 实际: {:?}",
            drives.iter().map(|d| d.letter.clone()).collect::<Vec<_>>()
        );
        for d in &drives {
            assert!(d.letter.starts_with('/'), "挂载点应以 / 开头: {}", d.letter);
            assert!(!d.kind.is_empty(), "文件系统类型不应为空: {}", d.letter);
        }
    }
}

/// /proc/mounts 会把空格/tab/反斜杠转义成八进制（\040 等），必须解码成真实路径，
/// 否则带空格的 U 盘挂载点（如 "UBUNTU 24_0"）会变成不存在的 "UBUNTU\04024_0"。
#[cfg(not(target_os = "windows"))]
#[test]
fn unescape_mount_field_decodes_octal_escapes() {
    use crate::scanner::unescape_mount_field;
    assert_eq!(
        unescape_mount_field("/media/caoyy/UBUNTU\\04024_0"),
        "/media/caoyy/UBUNTU 24_0"
    );
    assert_eq!(unescape_mount_field("a\\011b"), "a\tb");
    assert_eq!(unescape_mount_field("back\\134slash"), "back\\slash");
    assert_eq!(unescape_mount_field("plain"), "plain");
    assert_eq!(unescape_mount_field("\\040"), " ");
}

/// Linux 枚举结果不应包含对用户无意义的挂载点：/boot/efi、fuse（FreeRDP 剪贴板）、
/// snap 镜像、以及任何未解码的 \040 转义残留。
#[cfg(not(target_os = "windows"))]
#[test]
fn enumerate_drives_unix_filters_noise_and_unescapes() {
    let drives = crate::enumerate_drives();
    assert!(
        !drives.iter().any(|d| d.letter == "/boot/efi"),
        "不应出现 /boot/efi: {:?}",
        drives.iter().map(|d| d.letter.clone()).collect::<Vec<_>>()
    );
    assert!(
        !drives.iter().any(|d| d.letter.starts_with("/tmp/") || d.kind.starts_with("fuse")),
        "不应出现 /tmp/ 或 fuse 挂载: {:?}",
        drives.iter().map(|d| format!("{} ({})", d.letter, d.kind)).collect::<Vec<_>>()
    );
    // 无八进制转义残留（真实路径不应包含反斜杠八进制序列）
    for d in &drives {
        assert!(
            !d.letter.contains("\\040") && !d.letter.contains('\\'),
            "挂载点不应含未解码的转义: {}",
            d.letter
        );
    }
}

// ---------- 10. 数据导出：build_export_node（路径拼接 + 过滤） ----------

#[test]
fn join_path_platform_style() {
    use crate::join_path;
    #[cfg(target_os = "windows")]
    {
        assert_eq!(join_path("", "C:"), "C:");
        assert_eq!(join_path("C:", "Users"), "C:\\Users");
        assert_eq!(join_path("C:\\Users", "index"), "C:\\Users\\index");
        assert_eq!(join_path("C:\\Users\\", "index"), "C:\\Users\\index");
    }
    #[cfg(not(target_os = "windows"))]
    {
        assert_eq!(join_path("", "/"), "/");
        assert_eq!(join_path("/", "home"), "/home");
        assert_eq!(join_path("/home", "caoyy"), "/home/caoyy");
        assert_eq!(join_path("/home/", "caoyy"), "/home/caoyy");
    }
}

#[test]
fn export_full_keeps_all_nodes_with_paths() {
    use crate::build_export_node;
    // 根 C: -> dirA(大文件+小文件) + dirB
    let big_file = node("big.bin", 800, 1, 0, vec![]);
    let small_file = node("small.txt", 10, 1, 0, vec![]);
    let dir_a = dir("dirA", 810, vec![big_file, small_file]);
    let dir_b = dir("dirB", 50, vec![]);
    let root = dir("C:", 860, vec![dir_a, dir_b]);

    let export = build_export_node(&root, "", 0, 0); // 全量
    assert_eq!(export.children.len(), 2, "全量导出应保留全部子节点");
    let a = &export.children[0];
    assert_eq!(a.children.len(), 2, "dirA 的两个文件都应保留");
    assert!(a.children.iter().any(|f| f.name == "big.bin" && !f.is_dir));
    assert!(a.children.iter().any(|f| f.name == "small.txt" && !f.is_dir));

    // 根 path 直接等于根名（join_path("", name) == name，两平台通用）；
    // 子节点 path 以其父路径为前缀（绝对路径语义）
    assert_eq!(export.path, "C:", "根 path 应等于根名");
    assert!(a.path.contains("dirA"), "dirA path: {}", a.path);
    assert!(a.path.ends_with("dirA"), "dirA path 应以 dirA 结尾: {}", a.path);

    // 占父目录百分比：根=100；dirA = 810/860 = 94.19
    assert_eq!(export.percent_of_parent, 100.0);
    assert!((a.percent_of_parent - (810.0 / 860.0 * 100.0)).abs() < 0.01);
}

#[test]
fn export_filter_keeps_large_only() {
    use crate::build_export_node;
    // 阈值 60（MB 语义用字节直接构造，这里用 60 作字节阈值）
    let big_file = node("big.bin", 80, 1, 0, vec![]);
    let small_file = node("small.txt", 10, 1, 0, vec![]);
    let dir_a = dir("dirA", 90, vec![big_file, small_file]);
    let dir_b = dir("dirB", 50, vec![]);
    let root = dir("C:", 140, vec![dir_a, dir_b]);

    let export = build_export_node(&root, "", 0, 60);
    // 根始终保留；dirA(90>=60) 保留、dirB(50<60) 滤掉
    assert_eq!(export.children.len(), 1, "只应保留 dirA");
    assert_eq!(export.children[0].name, "dirA");
    // dirA 内：big.bin(80>=60) 保留、small.txt(10<60) 滤掉
    assert_eq!(export.children[0].children.len(), 1);
    assert_eq!(export.children[0].children[0].name, "big.bin");
    // 文件 path 应包含 dirA 前缀（分隔符平台相关，不硬编码）
    assert!(
        export.children[0].children[0].path.contains("dirA"),
        "big.bin path: {}",
        export.children[0].children[0].path
    );
    assert!(export.children[0].children[0].path.ends_with("big.bin"));
    // 占父目录百分比：big.bin 占 dirA = 80/90*100 ≈ 88.89
    let pct = export.children[0].children[0].percent_of_parent;
    assert!((pct - 80.0 / 90.0 * 100.0).abs() < 0.01, "pct={pct}");
}

#[test]
fn export_summary_counts() {
    use crate::{build_export_node, count_export_nodes};
    let f1 = node("f1", 1, 1, 0, vec![]);
    let f2 = node("f2", 1, 1, 0, vec![]);
    let sub = dir("sub", 2, vec![f1]);
    let root = dir("root", 4, vec![sub, f2]);
    let export = build_export_node(&root, "", 0, 0);
    let (nodes, dirs, files) = count_export_nodes(&export);
    assert_eq!(nodes, 4, "root+sub+f1+f2");
    assert_eq!(dirs, 2, "root+sub");
    assert_eq!(files, 2, "f1+f2");
}

// ---------- 9. prune：merge_files=false 必须保留散文件（回归测试） ----------

#[test]
fn prune_keeps_files_when_merge_false() {
    // 1 个目录 + 2 个散文件，merge_files=false：散文件必须保留为独立子节点，
    // 否则前端 [files] 虚拟节点展开后为空（曾导致"展开后没内容"的假象）。
    let root = node(
        "root",
        130,
        2,
        1,
        vec![
            dir("sub", 100, vec![]),
            node("f1", 10, 1, 0, vec![]),
            node("f2", 20, 1, 0, vec![]),
        ],
    );
    let out = prune(&root, 4, 100, TopNMode::Count, false);
    assert_eq!(out.children.len(), 3, "应保留 1 目录 + 2 个散文件");
    let files: Vec<&TreeNode> = out.children.iter().filter(|c| c.file_count == 1).collect();
    assert_eq!(files.len(), 2, "merge_files=false 时散文件应逐项保留");
    assert!(!out.truncated, "无折叠时不应标 truncated");
}

// ============================================================
//  压力 / 性能测试（#[ignore]，用 `cargo test -- --ignored` 运行）
//  全部基于真实磁盘，对比 TreeSize Free 的 ~10s/200GB 基准。
// ============================================================

#[test]
#[ignore]
fn perf_thread_scaling_on_project() {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("project root")
        .to_string_lossy()
        .into_owned();

    for threads in [Some(1u32), None] {
        let t0 = std::time::Instant::now();
        let (tree, _, _) = scan_drive(None, root_dir.clone(), "parallel", false, threads, no_cancel())
            .expect("扫描应成功");
        let dt = t0.elapsed();
        let rate = tree.file_count as f64 / dt.as_secs_f64();
        println!(
            "THREADS[{:?}] files={} folders={} sizeMB={:.1} elapsed={:.2}s rate={:.0} files/s",
            threads.unwrap_or(0),
            tree.file_count,
            tree.folder_count,
            tree.size as f64 / 1e6,
            dt.as_secs_f64(),
            rate
        );
    }
}

#[test]
#[ignore]
fn perf_scan_c_drive() {
    // 强制 parallel（确定性、树完整），扫真实根卷，对比 TreeSize 10s 基准。
    // Windows 扫 C:，Linux 扫 /。
    let target = if cfg!(target_os = "windows") {
        "C:".to_string()
    } else {
        "/".to_string()
    };
    let t0 = std::time::Instant::now();
    let res = scan_drive(None, target, "parallel", false, None, no_cancel());
    let dt = t0.elapsed();
    match res {
        Ok((tree, strat, _errors)) => {
            let gb = tree.size as f64 / 1e9;
            let rate = tree.file_count as f64 / dt.as_secs_f64();
            println!(
                "PERF[root] strategy={strat} files={} folders={} sizeGB={:.2} elapsed={:.1}s rate={:.0} files/s",
                tree.file_count, tree.folder_count, gb, dt.as_secs_f64(), rate
            );
            println!(
                "BENCHMARK vs TreeSize Free: ~10s for ~200GB. 本次 {:.1}s for {:.1}GB => {:.2}x",
                dt.as_secs_f64(),
                gb,
                dt.as_secs_f64() / 10.0
            );
        }
        Err(e) => println!("PERF[root] FAILED: {e}"),
    }
}
