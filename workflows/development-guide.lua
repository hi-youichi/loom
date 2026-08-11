--------------------------------------------
-- Goal:  Write and adversarially verify a Loom developer guide
-- Arch:
--   discover ==> documents[] ==> write ==> challenge[2] ==> revise ==> verify
--   documents[] ==> audit ==> report
-- Flow:  discover -> write -> challenge -> revise -> verify -> audit -> report
--------------------------------------------

meta = {
  reasoning = "Decompose the developer guide by contributor task, assign one owner per document, and verify every claim against source code",
  phases = {
    { label = "discover", description = "Map Loom architecture and define developer-guide documents", agents = 1 },
    { label = "write", description = "Write one developer-guide document per owner", dynamic = true },
    { label = "challenge", description = "Challenge implementation facts and contributor usability", dynamic = true },
    { label = "revise", description = "Fix critical and major findings", dynamic = true },
    { label = "verify", description = "Perform final source-grounded verification", dynamic = true },
    { label = "audit", description = "Audit cross-document consistency", agents = 1 },
    { label = "report", description = "Report verified documents and unresolved issues", agents = 1 }
  }
}

local PLAN = {
  type = "object",
  properties = { documents = { type = "array", items = {
    type = "object",
    properties = {
      name = { type = "string" }, path = { type = "string" }, purpose = { type = "string" },
      audience = { type = "string" }, source_paths = { type = "array", items = { type = "string" } },
      required_topics = { type = "array", items = { type = "string" } },
      excluded_topics = { type = "array", items = { type = "string" } }
    },
    required = { "name", "path", "purpose", "audience", "source_paths", "required_topics", "excluded_topics" }
  } } },
  required = { "documents" }
}

local CHALLENGE = {
  type = "object",
  properties = {
    document = { type = "string" }, passed = { type = "boolean" },
    findings = { type = "array", items = { type = "object", properties = {
      claim = { type = "string" }, verdict = { type = "string" }, severity = { type = "string" },
      evidence = { type = "array", items = { type = "string" } }, correction = { type = "string" }
    }, required = { "claim", "verdict", "severity", "evidence", "correction" } } }
  },
  required = { "document", "passed", "findings" }
}

local VERIFY = {
  type = "object",
  properties = {
    document = { type = "string" }, passed = { type = "boolean" },
    unresolved = { type = "array", items = { type = "string" } }, details = { type = "string" }
  },
  required = { "document", "passed", "unresolved", "details" }
}

local AUDIT = {
  type = "object",
  properties = { passed = { type = "boolean" }, findings = { type = "array", items = { type = "string" } } },
  required = { "passed", "findings" }
}

local function target_path(repo, path)
  if string.match(path, "^%a:[/\\]") or string.match(path, "^/") then return path end
  return repo .. "/" .. string.gsub(path, "\\\\", "/")
end

local function output_of(r)
  if r and r.ok and r.output then return r.output end
  return { passed = false, findings = {} }
end

local function major(findings)
  local n = 0
  for _, f in ipairs(findings or {}) do
    if f.severity == "critical" or f.severity == "major" then n = n + 1 end
  end
  return n
end

function main()
  local repo = "C:/Users/heycj/dev/loom"
  local dir = "docs/development-guide"

  phase("discover", 1)
  local p = agent({
    name = "development-guide-planner",
    description = "Plan Loom developer-guide documents",
    prompt = "在仓库 `" .. repo .. "` 中阅读 README.md、现有 docs、Cargo workspace、主要 crate 的 lib/main、CLI、workflow、ACP、MCP、backend、config、storage、test 和 examples。为 Loom 贡献者设计一份开发指南，输出互相独立的文档单元，每个单元必须包含 name、path、purpose、audience、exact source_paths、required_topics、excluded_topics。优先覆盖真实贡献路径：环境准备、架构、执行模型、CLI、Workflow/Luft、ACP/backend、tool/MCP/skill、配置与持久化、测试调试、完整功能开发示例、贡献流程。不要把用户操作指南内容重复进来；明确实验性或内部 API。",
    schema = PLAN,
    timeout_ms = 240000
  })
  if not p.ok then report({ status = "failed", stage = "discover", error = p.status }); return end
  local docs = p.output.documents or {}
  if #docs == 0 then report({ status = "failed", stage = "discover", error = "no documents" }); return end

  local run = pipeline {
    items = docs,
    max_inflight = 4,
    stages = {
      { label = "write", handler = function(d)
        phase("write " .. d.name)
        local w = agent({
          name = "developer-guide-writer-" .. d.name,
          description = "Write " .. d.name,
          prompt = "你负责唯一一篇文档：`" .. target_path(repo, d.path) .. "`。先阅读计划中的源码：" .. json.encode(d.source_paths) .. "。使用文件编辑工具只创建或修改这一篇文档，不改其他文件。目标读者是 Loom 贡献者；用中文叙述，保留 Rust/Lua/CLI/API 技术名词和命令。必须解释源码证据、模块边界、调用流程、扩展点、测试方式和常见坑；所有版本、路径、命令、配置项、行为都必须以当前源码为准。不要臆测未实现的 API；实验性内容显式标注。写完后确认目标文件确实存在。"
        })
        return { document = d, writer = w }
      end },
      { label = "challenge", handler = function(x)
        phase("challenge " .. x.document.name)
        local c = parallel({ "source", "contributor" }, function(kind)
          if kind == "source" then
            return { name = "developer-source-challenger-" .. x.document.name, description = "Refute source claims", prompt =
              "对文档 `" .. target_path(repo, x.document.path) .. "` 做源码对抗审查。阅读文档引用的源码和相关调用链，尝试反驳模块职责、类型、生命周期、默认值、错误处理、并发、配置优先级、命令和测试说明。若文件不存在，直接返回一个 critical finding。每条问题返回 claim、verdict、severity（critical/major/minor）、evidence（字符串数组）、correction；只有没有 critical/major 才能 passed=true。", schema = CHALLENGE, timeout_ms = 180000 }
          end
          return { name = "developer-usability-challenger-" .. x.document.name, description = "Test contributor usability", prompt =
            "以第一次参与 Loom 的 Rust 贡献者身份审阅 `" .. target_path(repo, x.document.path) .. "`。检查读者能否按文档定位源码、理解数据流、完成一次安全的小修改并运行验证；检查前置知识、命令可复制性、术语、跨文档链接、边界条件和危险操作。若文件不存在，直接返回一个 critical finding。每条问题返回 claim、verdict、severity（critical/major/minor）、evidence（字符串数组）、correction；只有没有 critical/major 才能 passed=true。", schema = CHALLENGE, timeout_ms = 180000 }
        end)
        return { document = x.document, writer = x.writer, challenges = c }
      end },
      { label = "revise", handler = function(x)
        phase("revise " .. x.document.name)
        local findings = {}
        for _, c in ipairs(x.challenges or {}) do
          for _, f in ipairs(output_of(c).findings or {}) do table.insert(findings, f) end
        end
        if major(findings) == 0 and x.writer.ok then return { document = x.document, findings = findings, revision = { ok = true, status = "skipped" } } end
        local r = agent({
          name = "developer-guide-reviser-" .. x.document.name,
          description = "Revise " .. x.document.name,
          prompt = "只修改 `" .. target_path(repo, x.document.path) .. "`。重新阅读相关源码，修复以下所有 critical/major 问题，并在合理范围内修复 minor 问题。若文件缺失则按文档计划创建。保留源码证据、贡献者导向结构和实验性标记；不要修改其他文件。问题列表：" .. json.encode(findings),
          timeout_ms = 240000
        })
        return { document = x.document, findings = findings, revision = r }
      end },
      { label = "verify", handler = function(x)
        phase("verify " .. x.document.name)
        local v = agent({
          name = "developer-guide-verifier-" .. x.document.name,
          description = "Verify " .. x.document.name,
          prompt = "最终核验 `" .. target_path(repo, x.document.path) .. "`。对照当前源码检查每个关键类型、模块路径、调用关系、命令、配置、测试命令和内部链接；确认文档没有把用户指南、设计提案或未实现功能写成事实。不要编辑文件。返回 unresolved 数组和 details；存在任何未解决的事实错误或主要阻塞时 passed=false。",
          schema = VERIFY,
          timeout_ms = 180000
        })
        return { document = x.document, findings = x.findings, verification = v }
      end }
    }
  }

  phase("audit", 1)
  local audit = agent({
    name = "developer-guide-editor",
    description = "Audit developer-guide consistency",
    prompt = "审阅 `" .. repo .. "/" .. dir .. "` 下全部开发指南文档以及现有用户指南。检查目录职责是否重叠或缺失，术语和 crate 名称是否一致，调用链是否冲突，链接和命令是否有效，是否混淆 Loom 与 Luft，是否把实验性 API 写成稳定承诺。不要编辑文件，返回具体 findings；无 critical/major 问题才 passed=true。",
    schema = AUDIT,
    timeout_ms = 180000
  })

  local summary = {}
  for _, item in ipairs(run.items or {}) do
    local v = output_of(item.output.verification)
    table.insert(summary, { document = item.output.document.name, path = item.output.document.path, passed = v.passed == true, unresolved = v.unresolved or {}, findings = #(item.output.findings or {}) })
  end
  report({ status = "completed", documents = summary, global_audit = output_of(audit), next_step = "Review unresolved findings before publishing the guide" })
end
