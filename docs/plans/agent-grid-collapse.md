# Agent Grid 折叠展开方案

## 背景

当前 `DashboardView` 中 Agent Grid 使用 CSS Grid 多行排列，Agent 数量较多时会占据大量垂直空间，挤压下方 sessions/activity 区域。

### 当前布局

```
┌──────────────────────────────────────────────────────┐
│ header (Agent Dashboard + 统计 chips)                │
├──────────────────────────────────────────────────────┤
│ AgentGrid — grid 多行, 无高度限制                      │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐        │
│  │ dev    │ │ react   │ │ tui    │ │ test   │  R1    │
│  └────────┘ └────────┘ └────────┘ └────────┘        │
│  ┌────────┐ ┌────────┐ ┌────────┐                   │
│  │ explore│ │ architect│ │ pm    │          R2       │
│  └────────┘ └────────┘ └────────┘                   │
│  ┌────────┐                                          │
│  │ qa     │                                 R3       │
│  └────────┘                                          │
├──────────────────────────────────────────────────────┤
│ TabContent (sessions / activity)  ← 空间被挤压       │
└──────────────────────────────────────────────────────┘
```

## 目标

Agent Grid 默认只显示 1 行，超出部分可点击展开/收起，保证 sessions/activity 区域始终有充足的显示空间。

```
收起状态（默认）:
┌──────────────────────────────────────────────────────┐
│ header                                               │
├──────────────────────────────────────────────────────┤
│ AgentGrid — 仅 1 行                                  │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐        │
│  │ dev    │ │ react   │ │ tui    │ │ test   │        │
│  └────────┘ └────────┘ └────────┘ └────────┘        │
│                              +5 more ▼               │
├──────────────────────────────────────────────────────┤
│ TabContent (sessions / activity)  ← 充足空间         │
└──────────────────────────────────────────────────────┘

展开状态:
┌──────────────────────────────────────────────────────┐
│ header                                               │
├──────────────────────────────────────────────────────┤
│ AgentGrid — 全部显示                                 │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐        │
│  │ dev    │ │ react   │ │ tui    │ │ test   │        │
│  └────────┘ └────────┘ └────────┘ └────────┘        │
│  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐        │
│  │ explore│ │architect│ │  pm    │ │  qa    │        │
│  └────────┘ └────────┘ └────────┘ └────────┘        │
│  ┌────────┐                                          │
│  │ other  │                                          │
│  └────────┘                                          │
│                               收起 ▲                 │
├──────────────────────────────────────────────────────┤
│ TabContent                                           │
└──────────────────────────────────────────────────────┘
```

## 实现方案

### 改动范围

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `web/src/components/dashboard/AgentGrid.tsx` | **主要修改** | 新增折叠/展开逻辑 |
| `web/src/components/dashboard/AgentCard.tsx` | 不变 | — |
| `web/src/components/dashboard/DashboardView.tsx` | 不变 | — |

### 核心设计

#### 1. 状态管理

```tsx
// AgentGrid.tsx
const [expanded, setExpanded] = useState(false)
const gridRef = useRef<HTMLDivElement>(null)
const [columnCount, setColumnCount] = useState(1)
```

- `expanded`：控制折叠/展开
- `columnCount`：当前 grid 实际列数，用于计算"一行能放几个"
- `gridRef`：指向 grid 容器 DOM 节点

#### 2. 列数计算 — ResizeObserver

通过 `ResizeObserver` 监听容器宽度变化，结合 grid 断点规则推算列数：

```
当前 grid CSS:
  grid-cols-1      → < 640px  → 1 列
  sm:grid-cols-2   → ≥ 640px  → 2 列
  lg:grid-cols-3   → ≥ 1024px → 3 列
  xl:grid-cols-4   → ≥ 1280px → 4 列
```

```tsx
useEffect(() => {
  const el = gridRef.current
  if (!el) return

  const observer = new ResizeObserver(([entry]) => {
    const width = entry.contentRect.width
    const cols =
      width >= 1280 ? 4 :
      width >= 1024 ? 3 :
      width >= 640  ? 2 : 1
    setColumnCount(cols)
  })

  observer.observe(el)
  return () => observer.disconnect()
}, [])
```

使用 `ResizeObserver` 而非 `window.matchMedia`，因为 grid 容器不一定等于窗口宽度（受 FileTreeSidebar、AgentChatSidebar 挤压）。

#### 3. 可见项计算

```tsx
const visibleAgents = expanded ? sorted : sorted.slice(0, columnCount)
const hiddenCount = sorted.length - columnCount
const canExpand = hiddenCount > 0
```

- 收起时只渲染前 `columnCount` 个
- 展开时渲染全部
- `hiddenCount ≤ 0` 时隐藏展开按钮

#### 4. 展开按钮

```
收起态:   "+5 more  ▼"
展开态:   "收起  ▲"
```

位于 grid 下方，右对齐，作为独立按钮（非 grid 子元素）：

```tsx
{canExpand && (
  <button
    type="button"
    onClick={() => setExpanded(!expanded)}
    className="mt-2 w-full text-center text-xs text-muted-foreground
               hover:text-foreground transition-colors py-1"
  >
    {expanded
      ? <>收起 ▲</>
      : <>+{hiddenCount} more ▼</>
    }
  </button>
)}
```

#### 5. 动画（可选）

**方案 A — 无动画（推荐首选）**：展开/收起直接切换。简单可靠。

**方案 B — CSS Grid 行过渡**：通过动态设置 `grid-template-rows` 实现行级展开动画。需要将额外行包裹在一个 `overflow: hidden; grid-row: span N` 的容器中，实现复杂度较高。

**方案 C — 高度过渡**：用一个 wrapper div 包裹 grid，切换 `max-height` + `overflow: hidden` + `transition`。需要估算展开后的最大高度。

推荐先实现方案 A，后续可升级为方案 C。

#### 6. 边界情况

| 场景 | 行为 |
|---|---|
| Agent 数 ≤ 列数 | 不显示展开按钮，全部显示 |
| Agent 数 = 0 | 保持现有的空状态 UI |
| 容器宽度变化（窗口缩放/sidebar 展开） | ResizeObserver 自动更新列数 |
| 展开后列数变化 | expanded 状态不变，显示全部 agent |
| 选中 agent 在折叠区 | 可选优化：将选中 agent 提升到可见区域 |

#### 7. 完整渲染结构

```tsx
return (
  <>
    <div
      ref={gridRef}
      className="grid gap-3 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
    >
      {visibleAgents.map((agent, i) => (
        <div key={agent.name} style={{ animationDelay: `${i * 40}ms` }}
             className="min-w-0 h-full animate-[fadeSlideUp_0.3s_ease-out_both]">
          <AgentCard ... />
        </div>
      ))}
    </div>

    {canExpand && (
      <button type="button" onClick={() => setExpanded(prev => !prev)} ...>
        {expanded ? '收起 ▲' : `+${hiddenCount} more ▼`}
      </button>
    )}
  </>
)
```

### 不做的事情

- ❌ 不改 AgentCard 的内部布局
- ❌ 不改 DashboardView 的结构
- ❌ 不引入新依赖
- ❌ 不做虚拟滚动（Agent 数量级不需要）
