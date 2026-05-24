# 进化相关设计决策

> 通用设计决策见 [design/decisions.md](../design/decisions.md)。

## D3: 文件系统而非数据库

**决策**：记忆和技能用 Markdown 文件存储，仅会话搜索用 SQLite FTS5。

**备选方案**：
1. ✅ 文件系统（选择）
2. ❌ 全 SQLite
3. ❌ JSON 文件

**理由**：
- Markdown 文件人可读可编辑，用户可以手动查看/修改记忆
- SQLite 仅用于搜索场景（FTS5），搜索不需要人直接读
- 全 SQLite 会引入"必须用 levol CLI 才能查看记忆"的问题
- JSON 文件不如 Markdown 可读

## D6: 进化模块用纯 Rust 实现

**决策**：GEPA 优化引擎作为独立 Rust crate `loom-evolution`，通过 trait 抽象接入。

**备选方案**：
1. ✅ 纯 Rust crate + trait 抽象（选择）
2. ❌ Python 子进程（DSPy 依赖，需要额外运行时）
3. ❌ 内嵌 loom 模块（耦合度高，不可复用）

**理由**：
- 消除 Python 运行时依赖，保持单二进制分发
- trait 抽象使 `loom-evolution` 可独立测试和复用
- `loom` 只需写一层薄适配器（`EvolutionLlm` 桥接 `loom::llm::LlmClient`）
- 参考：[hermes-agent-self-evolution](https://github.com/NousResearch/hermes-agent-self-evolution) 的 Python 实现，用 Rust 等价重写

## D7: Review 异步执行

**决策**：后台审查在会话结束后异步运行，不阻塞用户。

**备选方案**：
1. ✅ 异步（选择）
2. ❌ 同步阻塞

**理由**：
- Review 需要调用 LLM，耗时较长（10-30 秒）
- 用户不应该等待 Review 完成才能继续工作
- Review 结果写文件，用户下次会话自然能看到

**风险**：Review 可能写入低质量内容。缓解：大小限制 + 用户可 review/edit。
