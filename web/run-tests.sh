#!/bin/bash

# 测试运行脚本

echo "🧪 运行测试..."

# 运行测试并生成覆盖率报告
npm run test:coverage

# 如果覆盖率低于90%，退出码为1
if [ $? -ne 0 ]; then
    echo "❌ 测试失败或覆盖率不足"
    exit 1
fi

echo "✅ 所有测试通过，覆盖率达标！"
