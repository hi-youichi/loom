# Web Workspace E2E 测试文档

## 概述
本文档描述了 Web 端工作区（Workspace）功能的端到端测试覆盖范围、运行方式和扩展指南。

## 测试覆盖范围
当前测试覆盖以下核心功能：

### 1. 基础展示
- ✅ 工作区选择器组件正常显示

### 2. 工作区管理
- ✅ 创建新工作区
- ✅ 切换不同工作区
- ✅ 删除工作区

### 3. 数据隔离
- ✅ 不同工作区间的会话线程相互隔离
- ✅ 切换工作区时保留对应工作区的会话历史

## 环境要求
- Node.js >= 18
- Playwright 支持的浏览器（自动安装）
- 前端开发服务运行在 http://localhost:5173

## 运行测试

### 1. 安装依赖
```bash
cd web
npm install
```

### 2. 安装 Playwright 浏览器
```bash
npx playwright install
```

### 3. 启动前端服务
```bash
npm run dev
```

### 4. 运行 Workspace E2E 测试
```bash
# 仅运行 workspace 相关测试
npx playwright test workspace-management.spec.ts

# 运行所有 E2E 测试
npx playwright test
```

### 5. 查看测试报告
```bash
npx playwright show-report
```

## 测试文件结构
```
web/e2e/
├── workspace-management.spec.ts    # 工作区管理测试
├── workspace-thread-list.spec.ts   # 工作区线程列表测试
└── ...其他测试文件
```

## 测试用例说明

### 1. should display workspace selector
验证工作区选择器组件在页面加载后正常显示。

### 2. should create new workspace
测试工作区创建流程：
- 打开工作区下拉菜单
- 点击创建按钮
- 输入工作区名称
- 提交创建
- 验证新工作区被自动选中

### 3. should switch between workspaces
测试工作区切换功能：
- 创建两个不同的工作区
- 在两个工作区之间切换
- 验证每次切换后选中的工作区名称正确显示

### 4. should isolate threads between workspaces
验证工作区间数据隔离：
- 在工作区A中发送一条消息
- 切换到工作区B，验证没有消息
- 切换回工作区A，验证消息仍然存在

### 5. should delete workspace
测试工作区删除功能：
- 创建测试工作区
- 删除该工作区
- 验证工作区已从列表中移除

## 测试约定
1. 所有测试元素使用 `data-testid` 属性定位，避免依赖类名或文本内容
2. 每个测试用例保持独立，不依赖其他测试的执行结果
3. 测试前自动清理 localStorage，避免数据污染
4. 异步操作添加合理的超时时间，防止测试挂起

## 添加新测试
1. 在 `web/e2e/` 目录下创建新的测试文件，命名格式为 `[feature-name].spec.ts`
2. 参考现有测试结构编写测试用例
3. 给需要定位的元素添加对应的 `data-testid` 属性
4. 运行测试验证通过后提交代码

## 相关组件
- `web/src/components/workspace/WorkspaceSelector.tsx` - 工作区选择器组件
