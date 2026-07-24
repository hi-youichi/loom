# Session Summary

本文档梳理 `packages/opencode/src/session/` 中实际存在的三种不同
"summary" 机制及其对外契约。在本文档落地之前,`SessionSummary`、
`SessionCompaction` 和 `summary` agent 三者之间的关系是隐式的、跨文件
不一致的;本文作为新代码、外部 backend 接入、以及任何触及这三者
的重构的权威参考。

## 三种机制一览

| 机制 | 模块 | 用途 | 输出形态 |
|---|---|---|---|
| **SessionSummary** | `src/session/summary.ts` | 归属到单次用户轮次的 git-diff 快照 | `additions` / `deletions` / `files` 计数 + `Snapshot.FileDiff[]` |
| **SessionCompaction** | `src/session/compaction.ts` | 当会话上下文超出窗口时由 LLM 驱动的压缩 | `CompactionPart`(`mode: "compaction"`),内含结构化的滚动摘要 + 序列化后的近期上下文 |
| **`summary` agent** | `src/agent/agent.ts:250` | 预期作为 LLM 摘要器(prompt = `PROMPT_SUMMARY`) | **孤儿 — 仓库内不存在任何调用方** |

`summary` agent 目前是死代码(`git grep 'agents.get("summary")'` 在
生产代码里没有任何结果,只有 `agent.ts` 里的定义)。在这里单独列出,
是为了让后续维护者不要误以为它是活跃机制,也方便后续清理时决定是
删除它,还是让 `compaction` 迁移过去复用它。

## SessionSummary

### 服务接口

`src/session/summary.ts:72`

```ts
export interface Interface {
  readonly summarize: (input: {
    sessionID: SessionID
    messageID: MessageID
  }) => Effect.Effect<void>

  readonly diff: (input: {
    sessionID: SessionID
    messageID?: MessageID
  }) => Effect.Effect<Snapshot.FileDiff[]>

  readonly computeDiff: (input: {
    messages: SessionV1.WithParts[]
  }) => Effect.Effect<Snapshot.FileDiff[]>
}
```

### 语义:git-diff 摘要,而非 LLM 摘要

虽然名字叫 "summary",`SessionSummary.summarize` **并不会调用 LLM**。
它记录的是**文件级 diff** —— 取目标 user message 附近 `step-start` 和
`step-finish` 两个 part 上的 git 快照,计算两者之间的差异。结果用于
驱动 UI 角标(`+12 -3`、`3 files changed`)和每个文件的 diff 面板,
**不是**对话摘要。

真正的 LLM 摘要位于 `SessionCompaction`,并由 `compaction` agent 完成。

### `summarize({ sessionID, messageID })` 分步执行

`src/session/summary.ts:102`

1. **把 session 级的 summary 清零**
   `sessions.setSummary({ sessionID, summary: { additions: 0, deletions: 0, files: 0 } })`,
   避免上一轮的计数残留下渗到当前轮。

2. **广播一个空的 diff 事件**
   `events.publish(Session.Event.Diff, { sessionID, diff: [] })`。
   保证订阅方在真正的计算完成之前先收到一次 "重新开始" 的信号。

3. **短路:`config.snapshot === false` 时直接 return**
   `summary.ts:118` 直接返回。此时 session 仍然拿到清零的计数和空的
   Diff 事件,但不会计算任何 git diff。下游 UI 因此渲染 "无差异",
   而不是陈旧的计数。

4. **加载全部消息,过滤到目标切片**
   过滤出 user message `messageID` 本身,以及所有 `parentID` 等于该
   user message 的 assistant message —— 即紧随目标 user 轮之后的那
   几轮消息。

5. **计算文件 diff**
   `computeDiff({ messages })` 遍历每个 message 的 `parts`,把首个
   `step-start` 快照记作 `from`,把最后一个 `step-finish` 快照记作 `to`,
   然后调用 `snapshot.diffFull(from, to)`。

6. **把 diff 挂到 user message 上**
   `target.info.summary = { ...target.info.summary, diffs: msgDiffs }`
   接着 `sessions.updateMessage(target.info)`。diffs 挂在 user message
   自己身上,而不是 session 记录上。

### 调用点 —— 两处都通过 `Effect.forkIn(scope)` 异步派发

目前生产环境里只有两处调用方,**都用 `Effect.forkIn(scope)`**,
保证主流程不会被 summary 阻塞。

#### 1. prompt 循环,step 1

`src/session/prompt.ts:1253`

```ts
if (step === 1)
  yield* summary
    .summarize({ sessionID, messageID: lastUser.id })
    .pipe(Effect.ignore, Effect.forkIn(scope))
```

每条新的 user message 进入时触发一次,在循环进入 step 1 之后立即执行。
负责清空上一轮的残留 diff,同时把本轮的 diff 计算丢到后台。

#### 2. processor,patch 完成后

`src/session/processor.ts:472`

```ts
yield* summary
  .summarize({
    sessionID: ctx.sessionID,
    messageID: ctx.assistantMessage.parentID,
  })
  .pipe(Effect.ignore, Effect.forkIn(scope))
```

assistant 完成一个 `patch` part 后触发。diff 归属到触发该 assistant 轮的
user message(即 `assistantMessage.parentID`),而不是 assistant 自己。

### 为什么是 `Effect.forkIn(scope)` 而不是 inline `yield*`

两处调用都用 `Effect.ignore, Effect.forkIn(scope)` 包了一层。两条理由
在实践中不可妥协:

1. **Git 快照捕获在大仓库上是同步 I/O。** `Snapshot.diffFull(from, to)`
   在大 worktree 上的冷启动 `git diff` 可能卡几十到几百毫秒。让 provider
   turn 主循环阻塞在这个上面是不可接受的。
2. **两次相邻的调用不会相互竞争。** prompt 循环和 processor 都对同一
   个 scope 上 fire-and-forget `summarize`。`Effect.ignore` 丢弃返回值,
   `forkIn(scope)` 把 fiber 挂到调用方的生命周期上,session 结束时一起
   清理,不会跨请求边界泄漏。

如果未来有调用方需要同步拿到 diff(比如 TUI 的某个热路径必须等渲染),
应该改为调用 `summary.diff({ sessionID, messageID })` —— 该方法只读取
已经挂到 `target.info.summary.diffs` 上的结果,不会再算一遍。

## 配置

`config.snapshot` 控制 `summary.ts:118` 的短路。设为 `false` 时:

- `setSummary(0)` 仍然执行(session 计数清零)
- `publish(Diff, { diff: [] })` 仍然执行(UI 收到空事件)
- 不计算 git diff,不挂任何 per-message diffs
- UI 渲染 "snapshot 不可用" / 零计数

没有 per-call 的开关 —— 这个开关是全局的。如果将来某个特性需要为某
一次调用跳过 summary,应该在调用 `summarize` 之前自行分支,不要依赖
这个 config 闸口。

## 与 `SessionCompaction` 的对比

| 维度 | `SessionSummary` | `SessionCompaction` |
|---|---|---|
| 触发时机 | 每个 prompt、每个 patch | 仅溢出时(`/summarize` 端点或自动) |
| 触发点 | `prompt.ts:1253`、`processor.ts:472` | `handlers/session.ts:282`(`POST /session/:id/summarize`)、prompt 循环里的溢出检测 |
| 实现机制 | Git 快照 diff(不调 LLM) | `compaction` agent(`agent.ts:220`,prompt `PROMPT_COMPACTION`) |
| 输出 | `additions/deletions/files` + per-file diffs | 结构化滚动摘要 + 序列化后的近期上下文,挂成 `compaction` part |
| 同步 / 异步 | 永远 `forkIn(scope)` | 内联 `yield* compaction.process(...)` |
| 用户可见标签 | "3 files changed"、per-file diff 面板 | 替换掉旧上下文交给模型;UI 上看不到,只能从 token 数变小感知 |

两套机制是**互补**的,不是替代关系。一个长 session 里 `SessionSummary`
会更新很多次(每轮一次),`SessionCompaction` 只会更新寥寥数次
(每次溢出一次)。

## 孤儿:`summary` agent

`src/agent/agent.ts:250` 注册了一个名为 `summary` 的 agent:

```ts
summary: {
  name: "summary",
  mode: "primary",
  options: {},
  native: true,
  hidden: true,
  permission: Permission.merge(
    defaults,
    Permission.fromConfig({ "*": "deny" }),
    user,
  ),
  prompt: PROMPT_SUMMARY,
}
```

它的 prompt(`src/agent/prompt/summary.txt`)指示模型产出 coding 会话的
叙述性摘要 —— 即 **LLM 摘要**。这跟 `SessionCompaction` 的职责完全重叠
(后者用的是 `agent.ts:220` 的 `compaction` agent)。

`git grep 'agents.get("summary")'` 在生产代码里返回**零**调用方。它既
没被 `SessionSummary` 调用(那是 git-diff),也没被 `SessionCompaction`
调用(后者用的是 `agents.get("compaction")`),也没被任何 CLI/TUI/SDK
入口调用。

**清理 action items(不在本文档范围内):**

1. 决定是删除 `agent.ts:250` 的 `summary` agent 条目并移除
   `PROMPT_SUMMARY`,还是让 `SessionCompaction` 改用 `summary`、
   并把 `compaction` 干掉。
2. 如果保留,至少接上一个调用方 —— 不然它就是每次 agent 注册时的
   额外开销,而且是新代码可能误调 `agents.get("summary")` 期望它跟
   其他 hidden agent 一样能用的陷阱。

## 对外部 backend(例如 Loom)的影响

`/session/:sessionID/summarize` 这个 HTTP 端点
(`src/server/routes/instance/httpapi/groups/session.ts:303`)映射到的是
`SessionCompaction.create`,**不是** `SessionSummary.summarize`。希望
跟 opencode 协议兼容的外部 backend(Loom、hermes-port 等)必须实现两
个不同的面:

1. **`POST /session/:id/summarize`** —— LLM compaction。应当匹配
   `compaction` agent 契约:hidden primary agent、`permission: deny`、
   prompt `PROMPT_COMPACTION`。输出是挂到父 user message 上的
   `CompactionPart`。

2. **`SessionSummary.summarize` 语义** —— 目前没有 HTTP 入口。如果
   将来某个端点(`/session/:id/diff` 目前只读)需要支持写入,契约是:
   "接受 `{sessionID, messageID}`,按上面描述的 git-diff 流水线执行。"
   opencode 这边的派发模型是异步的,所以调用方不应当阻塞等返回;如果
   真的需要同步契约,应该改成暴露 `diff()`。

外部 backend 作者**不应**把 `summary.summarize()` 当作文本摘要原语。
这样做会跟 opencode 的 UI 期望悄悄错位 —— opencode 的 UI 全程都是
diff 形态的。

## 遗留问题

- 是否要把 `SessionSummary` 重命名为 `SessionDiffSummary`,以消除跟
  LLM 版的 `summary`/`compaction` agent 之间的歧义?重命名同时也能让
  孤儿 `summary` agent 这个名字不再那么误导。
- 是否要把 `Effect.ignore` 换成结构化日志,让 diff 计算的失败能被观测到?
  目前 `forkIn` 出来的 fiber 失败会被静默吞掉。
- prompt 循环 + processor 的双重调用是有意为之,还是某次重构遗留的
  痕迹?两次调用在常见场景下指向同一个 `userMessage.parentID`,后者
  会覆盖前者的结果。