import type { NavItem } from "../types";

interface BreadcrumbProps {
  /** 导航栈：从根到当前节点的完整路径 */
  navStack: NavItem[];
  /** 点击某一层时回溯到该层（index 为该层在栈中的位置） */
  onNavigate: (index: number) => void;
}

// 面包屑导航，维护由 App 持有的导航栈。点击任意层级回溯到对应路径。
export default function Breadcrumb({ navStack, onNavigate }: BreadcrumbProps) {
  if (navStack.length === 0) return null;

  return (
    <nav className="breadcrumb" aria-label="路径导航">
      {navStack.map((item, index) => {
        const isLast = index === navStack.length - 1;
        return (
          <span key={`${item.node.name}-${index}`} className="breadcrumb-item">
            <button
              type="button"
              className={isLast ? "breadcrumb-current" : "breadcrumb-link"}
              disabled={isLast}
              onClick={() => onNavigate(index)}
            >
              {item.node.name}
            </button>
            {!isLast && <span className="breadcrumb-sep"> › </span>}
          </span>
        );
      })}
    </nav>
  );
}
