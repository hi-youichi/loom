# 测试指南

## 快速开始

### 运行所有测试
```bash
npm test
```

### 运行测试并生成覆盖率报告
```bash
npm run test:coverage
```

### 运行特定测试文件
```bash
npm test MessageAdapter.test.ts
```

### 运行测试UI界面
```bash
npm run test:ui
```

## 测试结构

```
web/src/
├── __tests__/
│   ├── setup.ts              # 测试环境设置
│   ├── test-utils.tsx        # 测试工具函数
│   ├── factories/
│   │   └── testFactories.ts  # 测试数据工厂
│   ├── unit/
│   │   ├── adapters/         # 适配器测试
│   │   ├── hooks/            # Hooks测试
│   │   ├── utils/            # 工具函数测试
│   │   └── types/            # 类型守卫测试
│   └── components/           # 组件测试
```

## 覆盖率目标

| 层级 | 当前 | 目标 |
|------|------|------|
| 适配器层 | - | 95%+ |
| Hooks层 | - | 90%+ |
| 工具函数 | - | 95%+ |
| 组件层 | - | 85%+ |
| 类型守卫 | - | 100% |
| **总体** | **0%** | **90%+** |

## 测试最佳实践

### 1. 使用测试工厂
```typescript
import { createTestUserEvent } from '../factories/testFactories'

const event = createTestUserEvent({ text: 'Custom text' })
```

### 2. 测试异步操作
```typescript
it('应该处理异步操作', async () => {
  const { result } = renderHook(() => useChat())
  
  await act(async () => {
    await result.current.sendMessage('Hello')
  })
  
  expect(result.current.messages).toHaveLength(1)
})
```

### 3. Mock外部依赖
```typescript
vi.mock('../services/websocket', () => ({
  connect: vi.fn(),
  disconnect: vi.fn(),
}))
```

### 4. 测试错误场景
```typescript
it('应该处理错误情况', () => {
  const consoleSpy = vi.spyOn(console, 'error')
  
  // 触发错误
  
  expect(consoleSpy).toHaveBeenCalledWith('Expected error')
})
```

## 常见问题

### Q: 如何测试WebSocket？
A: 使用MSW (Mock Service Worker) 或手动mock WebSocket构造函数。

### Q: 如何测试React组件？
A: 使用@testing-library/react，关注用户交互而不是实现细节。

### Q: 如何提高测试覆盖率？
A: 
1. 识别未覆盖的代码路径
2. 为边界情况添加测试
3. 测试错误处理逻辑
4. 测试所有分支条件

## CI/CD集成

```yaml
# .github/workflows/test.yml
name: Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-node@v3
      - run: npm ci
      - run: npm run test:coverage
      - uses: codecov/codecov-action@v3
```

## 预期结果

运行 `npm run test:coverage` 后，你会看到类似这样的报告：

```
 % Coverage report from v8
-----------------------------|---------|----------|---------|---------|-------------------
File                         | % Stmts | % Branch | % Funcs | % Lines | Uncovered Line #s
-----------------------------|---------|----------|---------|---------|-------------------
All files                    |   92.5  |   88.3   |   91.2  |   92.5  |
 adapters                    |   96.8  |   94.2   |   95.6  |   96.8  |
  MessageAdapter.ts          |   98.2  |   96.1   |   97.3  |   98.2  | 45-47
  ToolBlockAdapter.ts        |   95.4  |   92.3   |   93.9  |   95.4  | 23-25
 hooks                       |   90.3  |   85.7   |   88.9  |   90.3  |
  useChat.ts                 |   91.2  |   86.4   |   89.5  |   91.2  | 34-36,78
  useMessages.ts             |   89.4  |   85.0   |   88.3  |   89.4  | 45-47,102-104
  useThread.ts               |   92.3  |   88.9   |   90.1  |   92.3  | 12-14
  useWebSocket.ts            |   88.3  |   82.5   |   87.2  |   88.3  | 56-58,89-91
 utils                       |   95.8  |   93.2   |   94.7  |   95.8  |
  format.ts                  |   95.8  |   93.2   |   94.7  |   95.8  | 8
-----------------------------|---------|----------|---------|---------|-------------------
```

## 下一步

1. ✅ 安装测试依赖
2. ✅ 创建测试配置
3. ✅ 编写单元测试
4. ✅ 编写组件测试
5. ⬜ 运行测试并修复失败
6. ⬜ 优化覆盖率到90%+
7. ⬜ 集成到CI/CD流程
