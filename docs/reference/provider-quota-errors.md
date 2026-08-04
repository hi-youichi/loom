# Provider 额度耗尽错误参考

> 各 LLM/Search Provider 在额度耗尽或触发速率限制时返回的错误码与处理方式汇总。
>
> Provider 列表基于 [models.dev](https://models.dev/api.json) 的全部 Coding Plan / Token Plan / Step Plan 提供商（共 18 家），加上项目实际使用的通用 API 提供商。

**创建时间**：2025-08-19｜**最后更新**：2025-08-19

---

## 下次可用时间（Reset Time）

触发限制后，仅部分 Provider 在错误响应中直接返回下次可用时间，格式各异：

| Provider | 是否返回 | 字段位置 | 格式 | 示例 |
|----------|---------|---------|------|------|
| 智谱 BigModel / Z.AI | ✅ | error message 内嵌 `{next_flush_time}` | 绝对时间（文本） | `...您的限额将在 2025-08-19 15:00:00 重置` |
| OpenAI | ✅ | HTTP 响应头 `x-ratelimit-reset-*` | Go duration（相对时间） | `x-ratelimit-reset-requests: 1s`<br>`x-ratelimit-reset-tokens: 6m0s` |
| Anthropic | ✅ | HTTP 响应头 `retry-after` | 秒数（相对时间） | `retry-after: 30` |
| 阿里巴巴百炼 | ❌ | — | 需按规则自行计算 | 5h: 每分钟滚动恢复<br>周: 每周一 00:00:00 (UTC+8)<br>月: 下个订阅日 00:00:00 (UTC+8) |
| 腾讯云 TokenHub | ❌ | — | 需按规则自行计算 | 5h: 滑动窗口<br>周: 每周一 00:00:00 (UTC+8)<br>月: 下个订阅月同时刻 |
| Kimi Code | ❌ | — | — | — |
| MiniMax | ❌ | — | — | — |
| 小米 MiMo | ❌ | — | — | — |
| StepFun | ❌ | — | — | — |
| DeepSeek | ❌ | — | — | — |
| Exa Search | ❌ | — | — | — |
| KUAE Cloud | ❌ | — | — | — |
| Umans AI | ❌ | — | — | — |

### 解析建议

- **智谱 / Z.AI**：从 error message 中正则提取 `{next_flush_time}` 对应的时间文本
- **OpenAI**：解析 `x-ratelimit-reset-requests` / `x-ratelimit-reset-tokens` 响应头，将 Go duration 格式（如 `6m0s`）转为绝对时间
- **Anthropic**：读取 `retry-after` 响应头（秒数），加上当前时间即为可用时间
- **阿里 / 腾讯**：无法从错误响应中获取，需根据固定规则计算下次窗口刷新时间

---

## 总览

### Coding Plan / Token Plan 提供商（models.dev 全覆盖）

| Provider | models.dev ID | 额度耗尽标识 | 窗口机制 | 恢复方式 |
|----------|--------------|------------|---------|---------|
| 智谱 BigModel | `zhipuai-coding-plan` | `429 code:1113`（欠费）/ `1309`（过期） | 5h + 周窗口 | 充值 / 续订 |
| Z.AI | `zai-coding-plan` | `429 code:1113` / `1309`（过期） | 5h + 周窗口 | 充值 / 续订 |
| 阿里巴巴百炼 | `alibaba-coding-plan` | `hour/week/month allocated quota exceeded` | 5h + 周 + 月窗口 | 等待窗口刷新 |
| 腾讯云 TokenHub | `tencent-coding-plan` | `429 20097` quota exceeded | 5h + 周 + 月窗口 | 等待窗口刷新 |
| Kimi Code | `kimi-for-coding` | `429 exceeded_current_quota_error` | 周（7天）+ 5h 滚动 | 购买 Extra Usage |
| MiniMax | `minimax-coding-plan` / `minimax-cn-coding-plan` | `1008` 余额不足 / `2056` 超出 Token Plan | 5h + 周窗口 | 积分补充 / 升级 |
| 小米 MiMo | `xiaomi-token-plan-*` | `402` 余额不足 / `429` 速率限制 | 月度 | 平台充值 |
| StepFun | `stepfun-step-plan` / `stepfun-ai-step-plan` | `402` 余额不足 / `429` 超限 | 月度 Credit 池 | 加购加油包 |
| KUAE Cloud | `kuae-cloud-coding-plan` | `429`（推测） | — | — |
| Umans AI | `umans-ai-coding-plan` | `429` 速率限制 | 5h 滚动窗口 | 升级套餐 |

### 通用 API 提供商

| Provider | 额度耗尽标识 | 窗口机制 | 恢复方式 |
|----------|------------|---------|---------|
| OpenAI | `429 insufficient_quota` | 月度 | 购买 credits / 升级 tier |
| Anthropic | `429` + spend limit | 月度 spend limit | 升级 tier / 增加 spend limit |
| DeepSeek | `402` 余额不足 | — | 平台充值 |
| Exa Search | `402 NO_MORE_CREDITS` | — | dashboard.exa.ai 充值 |
| Ollama | 无限制 | — | 本地运行 |

---

## 智谱 BigModel（GLM Coding Plan）

**models.dev ID**：`zhipuai-coding-plan`

**Base URL**：`https://open.bigmodel.cn/api/paas/v4`（通用）/ `https://open.bigmodel.cn/api/coding/paas/v4`（Coding Plan）

**文档**：[错误码](https://docs.bigmodel.cn/cn/api/api-code) | [速率限制](https://docs.bigmodel.cn/cn/api/rate-limit) | [Coding Plan FAQ](https://docs.bigmodel.cn/cn/coding-plan/faq)

### 错误码

| HTTP | 错误码 | 含义 | 解决方法 |
|------|--------|------|---------|
| `429` | `1113` | 账户已欠费 | 充值后重试 |
| `429` | `1302` | 速率限制（并发数超限） | 控制请求频率 |
| `429` | `1305` | 模型访问量过大（平台过载） | 稍后再试 |
| `429` | `1308` | 达到使用上限 | 等待 `next_flush_time` 重置 |
| `429` | `1309` | GLM Coding Plan 套餐已到期 | 前往官方续订 |
| `429` | `1310` | 每周/每月限额达到 | 等待 `next_flush_time` 重置 |
| `429` | `1316` | 5h 上限 + 主账号余额不足 | 充值或等待重置 |
| `429` | `1317` | 7天上限 + 主账号余额不足 | 充值或等待重置 |
| `429` | `1318-1321` | 子账号/企业级月消费上限 | 联系管理员调整 |

### Coding Plan 特性

- **双窗口机制**：每 5 小时限额 + 每周限额
- **额度耗尽后**：不会消耗资源包/账户余额，需等下一周期恢复
- **重要**：必须使用正确的 `base_url`，否则会报 `1113 余额不足`
  - Claude Code：`https://open.bigmodel.cn/api/anthropic`
  - Cherry Studio：`https://open.bigmodel.cn/api/coding/paas/v4/`
  - 其他工具：`https://open.bigmodel.cn/api/coding/paas/v4`

---

## Z.AI Coding Plan

**models.dev ID**：`zai-coding-plan`

**Base URL**：`https://api.z.ai/api/coding/paas/v4`

**文档**：[Error Codes](https://docs.z.ai/api-reference/api-code) | [Usage Policy](https://docs.z.ai/devpack/usage-policy) | [FAQ](https://docs.z.ai/devpack/faq)

Z.AI 是智谱 BigModel 的国际版，错误码体系与 BigModel 完全一致。

### 错误码

| HTTP | 错误码 | 含义 | 解决方法 |
|------|--------|------|---------|
| `429` | `1113` | Insufficient balance or no resource package | 充值 |
| `429` | `1302` | Rate limit reached | 控制请求频率 |
| `429` | `1305` | Service temporarily overloaded | 稍后再试 |
| `429` | `1308` | Usage limit reached for `{unit}`. Resets at `{next_flush_time}` | 等待重置 |
| `429` | `1309` | Coding Plan expired | 续订（https://z.ai/subscribe） |
| `429` | `1310` | Weekly/Monthly limit exhausted. Resets at `{next_flush_time}` | 等待重置 |
| `429` | `1316-1321` | 5h/7d limit + insufficient balance / monthly spend limit | 充值或等待重置 |

### 套餐层级

| 套餐 | 5h Prompt 额度 | 特性 |
|------|---------------|------|
| Lite | ~80 prompts | 单项目开发 |
| Pro | ~400 prompts | 1-2 个并发项目 |
| Max | ~1,600 prompts | 2+ 个并发项目 |

- GLM-5.2 / GLM-5-Turbo 在高峰期消耗 3x 标准速率，非高峰期 2x
- 额度耗尽后不会消耗账户余额，需等待下一 5h 周期刷新

---

## 阿里巴巴百炼（Coding Plan）

**models.dev ID**：`alibaba-coding-plan`（国际）/ `alibaba-coding-plan-cn`（国内）

**Base URL**：`https://coding-intl.dashscope.aliyuncs.com/v1`（国际）/ `https://coding.dashscope.aliyuncs.com/v1`（国内）

**文档**：[Coding Plan 概述](https://help.aliyun.com/zh/model-studio/coding-plan) | [常见问题](https://help.aliyun.com/zh/model-studio/coding-plan-faq) | [错误码](https://help.aliyun.com/zh/model-studio/error-codes) | [限流](https://help.aliyun.com/zh/model-studio/rate-limit) | [Rate Limiting (EN)](https://www.alibabacloud.com/help/en/model-studio/rate-limit)

### 错误码

| HTTP | 错误信息 | 含义 | 解决方法 |
|------|---------|------|---------|
| — | `hour allocated quota exceeded` | 每 5 小时请求额度已用完 | 等待 5h 滚动恢复（每分钟释放 5h 前的额度） |
| — | `week allocated quota exceeded` | 每周请求额度已用完 | 等待每周一 00:00:00（UTC+8）重置 |
| — | `month allocated quota exceeded` | 每月请求额度已用完 | 等待下个订阅日 00:00:00（UTC+8）重置 |
| — | `concurrency allocated quota exceeded` | 并发请求数超出动态上限 | 等待片刻后重试 |
| `429` | `usage allocated quota exceeded` | 模型调用限流（请求过于密集） | 等待一分钟后重试 |
| `429` | `Requests rate limit exceeded` | RPM 限流 | 降低请求频率 |
| `429` | `Allocated quota exceeded` | TPM 限流 | 缩短输入或限制输出长度 |
| `401` | `invalid access token` | 误用通用 API Key 或订阅过期 | 使用套餐专属 Key（`sk-sp-` 开头）+ 专属 Base URL |
| — | `100011` | 无可用额度 | 检查账户可用额度 |

### Pro 套餐示例

| 维度 | 限制 |
|------|------|
| 每 5 小时 | 6,000 次请求 |
| 每周 | 45,000 次请求 |
| 每月 | 90,000 次请求 |

- **5h 滚动恢复**：每分钟自动释放 5 小时前的额度
- **重要**：必须使用 Coding Plan 专属 API Key（`sk-sp-` 开头）和专属 Base URL（含 `coding.dashscope.aliyuncs.com`），否则按按量付费扣费

---

## 腾讯云 TokenHub（Coding Plan）

**models.dev ID**：`tencent-coding-plan`（Coding Plan）/ `tencent-token-plan`（Token Plan）

**Base URL**：`https://api.lkeap.cloud.tencent.com/coding/v3`（Coding Plan）/ `https://api.lkeap.cloud.tencent.com/plan/v3`（Token Plan）

**文档**：[Coding Plan 概述](https://cloud.tencent.com/document/product/1823/130092) | [常见问题](https://cloud.tencent.com/document/product/1823/130103) | [API 错误码](https://cloud.tencent.com/document/product/1823/131595)

### 错误码

| HTTP | 错误码 | 错误信息 | 含义 | 解决方法 |
|------|--------|---------|------|---------|
| `429` | `20097` | `hour allocated quota exceeded` | 每 5h 额度已用完 | 等待 5h 滑动窗口恢复 |
| `429` | `20097` | `week allocated quota exceeded` | 每周额度已用完 | 等待每周一 00:00:00（UTC+8）重置 |
| `429` | `20097` | `month allocated quota exceeded` | 每订阅月额度已用完 | 等待下个订阅月同时刻重置 |
| `429` | — | `tpm rate limit exceeded` | 系统负载高触发限流 | 重试 1-2 次或切换模型 |
| `402` | `401008` | `CodeEndpointFreeQuotaExhausted` | 免费体验额度已耗尽 | 开启后付费 |
| `402` | `403004` | `CodeInsufficientBalance` | 账号已欠费 | 充值后重新启用服务 |
| `429` | `429001` | `CodeRateLimitExceeded` | 请求速率超限 | 降低访问频率 |
| `429` | `429002` | `CodeRPMLimitExceeded` | RPM 超限 | 降低请求频率 |
| `429` | `429003` | `CodeTPMLimitExceeded` | TPM 超限 | 降低访问频率 |
| `429` | `429005` | `CodeConcurrencyLimitExceeded` | 并发超限 | 降低并发 |

### 套餐

| 套餐 | 价格 | 5h 限制 | 周限制 | 月限制 |
|------|------|--------|--------|--------|
| Lite | ¥40/月 | ~1,200 次 | ~9,000 次 | ~18,000 次 |
| Pro | ¥200/月 | ~6,000 次 | ~45,000 次 | ~90,000 次 |

- **额度耗尽后不会转为按量计费**，继续调用将失败报错
- **重要**：仅限编程工具中使用，禁止 API 调用用于自动化脚本

---

## Kimi Code（Moonshot）

**models.dev ID**：`kimi-for-coding`

**Base URL**：`https://api.kimi.com/coding/v1`（OpenAI 兼容）/ `https://api.kimi.com/coding/`（Anthropic 兼容）

**文档**：[Membership Benefits](https://www.kimi.com/code/docs/en/kimi-code/membership.html) | [Membership Guide](https://www.kimi.com/help/kimi-code/membership-guide) | [Error Reference](https://www.kimi.com/code/docs/en/kimi-code/error-reference.html) | [Forum](https://forum.moonshot.ai)

### 错误码

| HTTP | Error Type / Message | 含义 | 解决方法 |
|------|---------------------|------|---------|
| `429` | `exceeded_current_quota_error` | 账户余额不足/配额耗尽 | 购买 Extra Usage 或等待每周刷新 |
| `429` | `rate_limit_reached_error` / `We're receiving too many requests` | 5h 滚动窗口速率限制 | 等待窗口滚动恢复 |
| `429` | `You've reached your usage limit for this period` | 周期使用上限 | 等待刷新 |
| `429` | `You've reached kimi monthly usage limit` | 月度使用上限 | 等待月度刷新 |
| `429` | `The engine is currently overloaded` | 引擎过载 | 稍后重试 |
| `403` | `You've reached your usage limit for this billing cycle` | 计费周期上限 | 升级套餐或等待下一周期 |
| `401` | — | HighSpeed 模型无权限 | 升级至 Allegretto 套餐以上 |

### 配额机制

- **周配额**：每 7 天自动刷新（从订阅日计算），未用完不结转
- **5 小时滚动窗口**：约 300–1,200 请求/5 小时，最大 30 并发
- **Extra Usage**：订阅用户可购买额外用量，配额耗尽时自动切换，不影响会员配额刷新

### 套餐层级

| 套餐 | 速度模式 | 说明 |
|------|---------|------|
| Moderato ($19/月) | Standard | 入门编码套餐 |
| Allegretto | Standard + HighSpeed | 解锁 HighSpeed（~5-6x 速度，~3x 积分消耗） |
| 更高级别 | — | 更大配额池 |

---

## MiniMax（Token Plan）

**models.dev ID**：`minimax-coding-plan`（国际, minimax.io）/ `minimax-cn-coding-plan`（国内, minimaxi.com）

**Base URL**：`https://api.minimax.io/anthropic/v1`（国际）/ `https://api.minimaxi.com/anthropic/v1`（国内）

**文档**：[错误码查询](https://platform.minimaxi.com/docs/api-reference/errorcode) | [Token Plan FAQ](https://platform.minimaxi.com/docs/token-plan/faq) | [Token Plan 概要](https://platform.minimaxi.com/docs/token-plan/intro) | [Error Codes (EN)](https://platform.minimax.io/docs/api-reference/errorcode)

### 错误码

| 错误码 | 含义 | 解决方法 |
|--------|------|---------|
| `1008` | 余额不足（insufficient balance） | 检查账户余额，充值 |
| `2056` | 超出 Token Plan 资源限制（usage limit exceeded） | 等待下一个 5 小时窗口资源释放 |
| `1002` | 请求频率超限（rate limit） | 稍后再试 |
| `2045` | 请求频率增长超限 | 避免请求骤增骤减 |
| `2049` | 无效的 API Key | 检查 API Key |
| `1041` | 连接数限制 | 联系平台 |

### Token Plan 窗口机制

- **5 小时固定窗口** + **周窗口** 双重控制
- 未用完的套餐内额度不结转到下一个计费周期

### 套餐

| 套餐 | 价格 | 适用场景 | Agent 用量 |
|------|------|---------|-----------|
| Plus | ¥49/月 | 轻量个人开发与日常试用 | 3-4 个 Agent |
| Max | ¥119/月 | 高频编程 Agent 与多模态调用 | 4-5 个 Agent |
| Ultra | ¥469/月 | 重度 Agent 工作流 | 6-7 个 Agent |

### 达到限额后的选项

1. 使用已购积分自动补充支付
2. 升级订阅套餐（立即生效）
3. 切换为按量计费 API Key
4. 等待额度窗口重置

---

## 小米 MiMo（Token Plan）

**models.dev ID**：`xiaomi`（按量）/ `xiaomi-token-plan-cn`（中国）/ `xiaomi-token-plan-ams`（欧洲）/ `xiaomi-token-plan-sgp`（新加坡）

**Base URL**：`https://api.xiaomimimo.com/v1`（按量）/ `https://token-plan-cn.xiaomimimo.com/v1`（中国）/ `https://token-plan-ams.xiaomimimo.com/v1`（欧洲）/ `https://token-plan-sgp.xiaomimimo.com/v1`（新加坡）

**文档**：[Token Plan](https://platform.xiaomimimo.com/static/docs/price/tokenplan/subscription.md) | [Model & Rate Limit](https://platform.xiaomimimo.com/static/docs/quick-start/model.md) | [计费公告](https://platform.xiaomimimo.com/docs/zh-CN/news/previous-news/billing)

### 错误码

| HTTP | 含义 | 解决方法 |
|------|------|---------|
| `429` | 速率限制（RPM/TPM 超限） | 合理规划请求频率，使用重试退避策略 |
| `402` | 余额不足 | 平台充值 |

### Token Plan 套餐

| 套餐 | 月费 | 月度 Credit 额度 |
|------|------|-----------------|
| Lite | $6/月（¥39/月） | 4.1B Credits |
| Standard | $16/月（¥99/月） | 11B Credits |
| Pro | $50/月（¥329/月） | 38B Credits |
| Max | $100/月（¥659/月） | 82B Credits |

### 扣费规则

- **Token Plan** 与按量付费余额不互通
- 按量付费：优先消耗赠送金额 → 再消耗充值余额
- 非高峰期（北京时间 0:00-8:00）享 0.8x 消耗系数

---

## StepFun（Step Plan）

**models.dev ID**：`stepfun-step-plan`（国内）/ `stepfun-ai-step-plan`（国际）

**Base URL**：`https://api.stepfun.com/step_plan/v1`（国内）/ `https://api.stepfun.ai/step_plan/v1`（国际）

**文档**：[Step Plan 概述](https://platform.stepfun.com/docs/zh/step-plan/overview) | [错误码](https://platform.stepfun.com/docs/zh/api-reference/error-codes) | [异常处理](https://platform.stepfun.com/docs/zh/guides/developer/exception) | [定价与限速](https://platform.stepfun.com/docs/zh/guides/pricing/details)

### 错误码

| HTTP | 含义 | 解决方法 |
|------|------|---------|
| `402` | 余额不足 | 充值 |
| `429` | 请求频次超限 | 设定 delay 后重试 |
| `451` | 内容未审核通过 | 修改请求信息 |
| `503` | 服务器负载过高 | 稍后重试 |

### Step Plan 特性

- **Credit 月池计费**：额度以 Credit 为单位按月发放，月内灵活消耗
- **月末清零**：Credit 不结转至下一周期
- **超额可加购**：当月额度用尽后可购买加油包，无需等待次月
- **不适用阶梯限速**：Step Plan 用量由月度 Credit 额度管理，不受按量付费的 RPM/TPM 限制

---

## KUAE Cloud（Coding Plan）

**models.dev ID**：`kuae-cloud-coding-plan`

**Base URL**：`https://coding-plan-endpoint.kuaecloud.net/v1`

**文档**：公开文档有限（[LMSpeed 监控](https://lmspeed.net/zh/provider/kuaecloud-coding-plan-endpoint)）

### 错误码

| HTTP | 含义 | 解决方法 |
|------|------|---------|
| `429` | 速率限制（推测，遵循 OpenAI 兼容格式） | 降低请求频率 |

### 已知信息

- 模型：`GLM-4.7`
- API Key 环境变量：`KUAE_API_KEY`
- 公开文档极少，公开根路径返回 404
- 30 天可用性约 0.8%（来源：LMSpeed 监控数据）

---

## Umans AI（Coding Plan）

**models.dev ID**：`umans-ai-coding-plan`

**Base URL**：`https://api.code.umans.ai/v1`

**文档**：[Coding Plan](https://app.umans.ai/offers/code) | [API Docs](https://app.umans.ai/offers/code/docs) | [models.dev](https://models.dev/providers/umans-ai-coding-plan/)

### 错误码

| HTTP | 含义 | 解决方法 |
|------|------|---------|
| `429` | 速率限制（请求窗口超限） | 等待 5h 滚动窗口恢复 |
| `429` | 并发超限 | 降低并发数 |

### 套餐

| 套餐 | 价格 | 限制 | 说明 |
|------|------|------|------|
| Lite | $17/月 | 200 req / 5h 滚动窗口，最多 5 并发 | 入门编码 |
| Pro | $42/月 | 无限 token，最多 4 并发 | 长会话、重度使用 |

- 提供 Anthropic Messages API 兼容端点：`https://api.code.umans.ai/v1/messages`
- 提供 OpenAI Chat Completions 兼容端点：`https://api.code.umans.ai/v1/chat/completions`

---

## OpenAI

**Base URL**：`https://api.openai.com/v1`

**文档**：[Error Codes](https://developers.openai.com/api/docs/guides/error-codes) | [Rate Limits](https://developers.openai.com/api/docs/guides/rate-limits)

### 错误码

| HTTP | 错误码 | 含义 | 解决方法 |
|------|--------|------|---------|
| `429` | `insufficient_quota` | 额度耗尽 — "You exceeded your current quota" | 购买更多 credits 或升级使用层级 |
| `429` | `rate_limit_reached` | 请求过快（TPM/RPM 超限） | 降低请求频率，使用指数退避重试 |

### 响应头

OpenAI 在响应头中返回实时速率限制信息：

| Header | 说明 |
|--------|------|
| `x-ratelimit-remaining-requests` | 剩余请求数 |
| `x-ratelimit-remaining-tokens` | 剩余 token 数 |
| `x-ratelimit-reset-requests` | 请求限制重置时间 |
| `x-ratelimit-reset-tokens` | Token 限制重置时间 |

### 使用层级

| Tier | 资格条件 | 月度额度上限 |
|------|---------|------------|
| Free | 允许地区 | 极低 |
| Tier 1 | $5+ 充值 | $100/月 |
| Tier 2 | $50+ 充值 & 7天以上 | $500/月 |
| Tier 3 | $100+ 充值 & 7天以上 | $1,000/月 |
| Tier 4 | $250+ 充值 & 14天以上 | $5,000/月 |
| Tier 5 | $1,000+ 充值 & 30天以上 | $200,000/月 |

消费达到门槛后自动升级。

---

## Anthropic（Claude）

**Base URL**：`https://api.anthropic.com/v1`

**文档**：[Rate Limits](https://docs.anthropic.com/en/api/rate-limits) | [Usage Limits](https://support.anthropic.com/en/articles/11647753-understanding-usage-and-length-limits)

### 限制类型

| 类型 | 说明 | HTTP |
|------|------|------|
| Rate Limit | RPM / ITPM / OTPM 超限 | `429`（含 `retry-after` 头） |
| Spend Limit | 月度最大消费上限（Settings > Billing） | `429` |
| Acceleration Limit | 使用量急剧增加时触发 | `429` |

### 使用层级

| Tier | 资格条件 | ITPM | OTPM |
|------|---------|------|------|
| Free | 注册即可 | 低 | 低 |
| Tier 1 | $5+ 充值 | 中 | 中 |
| Tier 2 | $40+ 充值 | 较高 | 较高 |
| Tier 3 | $200+ 充值 | 高 | 高 |
| Tier 4 | $400+ 充值 | 最高 | 最高 |

基于已验证信息和累计消费自动升级。

---

## DeepSeek

**Base URL**：`https://api.deepseek.com/v1`

**文档**：[Error Codes](https://api-docs.deepseek.com/zh-cn/quick_start/error_codes) | [Rate Limits](https://api-docs.deepseek.com/zh-cn/quick_start/rate_limit)

### 错误码

| HTTP | 含义 | 解决方法 |
|------|------|---------|
| `402` | 余额不足 | 前往充值页面充值 |
| `429` | 请求速率达到上限（TPM/RPM） | 合理规划请求速率 |
| `500` | 服务器内部故障 | 等待后重试 |
| `503` | 服务器负载过高 | 稍后重试 |

### 并发限制

| 模型 | 并发上限 |
|------|---------|
| deepseek-v4-pro | 500 |
| deepseek-v4-flash | 2,500 |

### 扣费规则

优先扣减赠送余额，再扣充值余额。

---

## Exa Search

**Base URL**：`https://api.exa.ai/search`

**文档**：[Error Codes](https://exa.ai/docs/reference/error-codes) | [Rate Limits](https://exa.ai/docs/reference/rate-limits)

### 错误码

| HTTP | Error Tag | 含义 | 解决方法 |
|------|----------|------|---------|
| `402` | `NO_MORE_CREDITS` | 账户积分耗尽 | 前往 dashboard.exa.ai 充值 |
| `402` | `API_KEY_BUDGET_EXCEEDED` | API Key 超出预算限制 | 联系团队管理员 |
| `402` | `TEAM_BUDGET_EXCEEDED` | 团队超出计费周期预算 | 联系团队管理员 |
| `429` | — | 速率限制 | 指数退避，降低请求率 |

### 速率限制

| Endpoint | 限制 |
|----------|------|
| `/search` | 10 QPS |
| `/contents` | 100 QPS |
| `/answer` | 10 QPS |

---

## Ollama（本地）

**Base URL**：`http://localhost:11434/v1`

无需额度，本地运行无配额限制。仅受本地硬件资源（GPU/CPU/内存）约束。
