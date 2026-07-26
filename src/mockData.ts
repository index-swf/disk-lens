import type { TreeNode } from "./types";

// 后端 scan_drive 就绪前用于跑通 UI 的 mock 数据。
// 结构与真实 TreeNode 完全一致。真实扫描结果返回后会被替换。

function node(
  name: string,
  size: number,
  allocated_size: number,
  file_count: number,
  folder_count: number,
  children: TreeNode[] = []
): TreeNode {
  return { name, size, allocated_size, file_count, folder_count, children };
}

export const mockTree: TreeNode = node(
  "C:",
  5_400_000_000,
  5_700_000_000,
  12_500,
  1_800,
  [
    node("Windows", 2_100_000_000, 2_250_000_000, 6_200, 900, [
      node("System32", 1_200_000_000, 1_300_000_000, 4_100, 420, [
        node("drivers", 300_000_000, 320_000_000, 800, 60),
        node("wbem", 120_000_000, 130_000_000, 400, 30),
      ]),
      node("WinSxS", 600_000_000, 650_000_000, 1_500, 300),
    ]),
    node("Program Files", 1_800_000_000, 1_900_000_000, 3_100, 500, [
      node("Node.js", 400_000_000, 430_000_000, 1_200, 120),
      node("Git", 350_000_000, 370_000_000, 900, 80),
      node("Docker", 700_000_000, 740_000_000, 700, 120),
    ]),
    node("Users", 1_200_000_000, 1_280_000_000, 2_400, 320, [
      node("index", 1_100_000_000, 1_170_000_000, 2_200, 300, [
        node("Documents", 500_000_000, 530_000_000, 800, 120),
        node("Downloads", 480_000_000, 510_000_000, 600, 90),
        node("Pictures", 90_000_000, 95_000_000, 700, 60),
      ]),
    ]),
    node("ProgramData", 300_000_000, 320_000_000, 800, 80),
  ]
);
