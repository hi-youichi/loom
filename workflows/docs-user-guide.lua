--------------------------------------------
-- Goal:  Write and adversarially verify Loom user-guide documents
-- Arch:
--   discover ==> documents[] ==> write ==> challenge[2] ==> revise ==> verify
--   documents[] ==> global audit ==> report
-- Flow: discover -> write -> challenge -> revise -> verify -> audit -> report
--------------------------------------------

meta = {
  reasoning = "Decompose by document; assign one owner and two independent challengers to each document",
  phases = {
    { label = "discover", description = "Define document units from Loom source", agents = 1 },
    { label = "write", description = "Write one document per owner", dynamic = true },
    { label = "challenge", description = "Challenge source facts and usability", dynamic = true },
    { label = "revise", description = "Fix critical and major findings", dynamic = true },
    { label = "verify", description = "Verify commands, links, and terminology", dynamic = true },
    { label = "audit", description = "Audit cross-document consistency", agents = 1 },
    { label = "report", description = "Report verified documents and open issues", agents = 1 }
  }
}

local PLAN = {
  type = "object",
  properties = {
    documents = { type = "array", items = {
      type = "object",
      properties = {
        name = { type = "string" }, path = { type = "string" },
        purpose = { type = "string" }, audience = { type = "string" },
        source_paths = { type = "array", items = { type = "string" } },
        required_topics = { type = "array", items = { type = "string" } },
        excluded_topics = { type = "array", items = { type = "string" } }
      },
      required = { "name", "path", "purpose", "audience", "source_paths", "required_topics", "excluded_topics" }
    } }
  },
  required = { "documents" }
}

local CHALLENGE = {
  type = "object",
  properties = {
    document = { type = "string" }, passed = { type = "boolean" },
    findings = { type = "array", items = {
      type = "object",
      properties = {
        claim = { type = "string" }, verdict = { type = "string" },
        severity = { type = "string" },
        evidence = { type = "array", items = { type = "string" } },
        correction = { type = "string" }
      },
      required = { "claim", "verdict", "severity", "evidence", "correction" }
    } }
  },
  required = { "document", "passed", "findings" }
}

local VERIFY = {
  type = "object",
  properties = {
    document = { type = "string" }, passed = { type = "boolean" },
    unresolved = { type = "array", items = { type = "string" } },
    details = { type = "string" }
  },
  required = { "document", "passed", "unresolved", "details" }
}

local AUDIT = {
  type = "object",
  properties = {
    passed = { type = "boolean" }, findings = { type = "array", items = { type = "string" } }
  },
  required = { "passed", "findings" }
}

local function out(r)
  if r and r.ok and r.output then return r.output end
  return { passed = false, findings = { { claim = "agent execution", verdict = "unclear", severity = "major", evidence = { r and r.status or "missing" }, correction = "retry stage" } } }
end

local function target_path(repo, path)
  if string.match(path, "^%a:[/\\]") or string.match(path, "^/") then
    return path
  end
  return repo .. "/" .. string.gsub(path, "\\\\", "/")
end

local function major(findings)
  local n = 0
  for _, f in ipairs(findings or {}) do
    if f.severity == "critical" or f.severity == "major" then n = n + 1 end
  end
  return n
end

function main()
  -- The workflow is executed from an external Luft runtime, so use explicit
  -- paths rather than relying on the runtime working directory or args table.
  local repo = "C:/Users/heycj/dev/loom"
  local dir = "docs/user-guide"

  phase("discover", 1)
  local p = agent({
    name = "document-planner",
    description = "Plan Loom user-guide documents",
    prompt = "In repository `" .. repo .. "`, read README.md, docs/, Cargo.toml, apps/cli/src/args.rs, "
      .. "CLI handlers, workflow sources, ACP sources, config sources, and examples. "
      .. "Plan a task-oriented first-party user guide under `" .. dir .. "` for developers using Loom. "
      .. "Return independent document units. For each give name, path, audience, purpose, exact source_paths, "
      .. "required_topics, and excluded_topics. Cover stable user paths first and mark experimental features.",
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
      {
        label = "write",
        handler = function(d)
          phase("write " .. d.name)
          local w = agent({
            name = "writer-" .. d.name,
            description = "Write " .. d.name,
            prompt = "Own exactly one document: `" .. target_path(repo, d.path) .. "` in repository `" .. repo .. "`. "
              .. "Read these source paths first: " .. json.encode(d.source_paths) .. ". "
              .. "Write or replace only that file using the file editing tool. Purpose: " .. d.purpose .. ". "
              .. "Audience: " .. d.audience .. ". Required topics: " .. json.encode(d.required_topics) .. ". "
              .. "Excluded topics: " .. json.encode(d.excluded_topics) .. ". "
              .. "Use Chinese prose with English commands and technical terms. Ground every command, path, "
              .. "option, and behavior in current source. Include prerequisites, runnable examples, and explicit "
              .. "experimental labels. Do not edit other files. After writing, verify that the target file exists "
              .. "at the exact path and report failure if it does not."
            , timeout_ms = 240000
          })
          return { document = d, writer = w }
        end
      },
      {
        label = "challenge",
        handler = function(x)
          phase("challenge " .. x.document.name)
          if not x.writer.ok then return { document = x.document, writer = x.writer, challenges = {} } end
          local c = parallel({ "source", "usability" }, function(kind)
            if kind == "source" then
              return { name = "source-challenger-" .. x.document.name, description = "Audit source facts", prompt =
                "Adversarially audit `" .. target_path(repo, x.document.path) .. "` in `" .. repo .. "`. Read its referenced source paths "
                .. json.encode(x.document.source_paths) .. ". Try to refute commands, flags, defaults, config precedence, "
                .. "paths, lifecycle behavior, stable/experimental labels, and internal links. Return every issue with "
                .. "claim, verdict, severity (critical/major/minor), evidence, correction. passed=true only with no "
                .. "critical or major issue. If the target file is missing, return one critical finding and stop. "
                .. "Return evidence as an array of strings.", schema = CHALLENGE, timeout_ms = 180000 }
            end
            return { name = "usability-challenger-" .. x.document.name, description = "Test user path", prompt =
              "Adversarially test `" .. target_path(repo, x.document.path) .. "` as a new Loom user. Check prerequisites, command order, "
              .. "copy-paste examples, unexplained concepts, unsafe ambiguity, and whether the main task can be completed. "
              .. "Return every issue with claim, verdict, severity, evidence, correction. passed=true only with no critical "
              .. "or major blocker. If the target file is missing, return one critical finding and stop. "
              .. "Return evidence as an array of strings.", schema = CHALLENGE, timeout_ms = 180000 }
          end)
          return { document = x.document, writer = x.writer, challenges = c }
        end
      },
      {
        label = "revise",
        handler = function(x)
          phase("revise " .. x.document.name)
          local findings = {}
          for _, c in ipairs(x.challenges or {}) do
            for _, f in ipairs(out(c).findings or {}) do table.insert(findings, f) end
          end
          if major(findings) == 0 and x.writer.ok then return { document = x.document, findings = findings, revision = { ok = true, status = "skipped" } } end
          local r = agent({
            name = "reviser-" .. x.document.name,
            description = "Revise " .. x.document.name,
            prompt = "Revise only `" .. target_path(repo, x.document.path) .. "` in repository `" .. repo .. "`. Read the current file and "
              .. "verify corrections against source. Fix all critical and major findings below, and fix minor findings "
              .. "when in scope. Preserve task-oriented structure and label uncertainty. If the file is missing, create it "
              .. "from the document plan. Findings: " .. json.encode(findings), timeout_ms = 240000
          })
          return { document = x.document, findings = findings, revision = r }
        end
      },
      {
        label = "verify",
        handler = function(x)
          phase("verify " .. x.document.name)
          local v = agent({
            name = "verifier-" .. x.document.name,
            description = "Verify " .. x.document.name,
            prompt = "Final-check `" .. target_path(repo, x.document.path) .. "` in repository `" .. repo .. "`. Verify every command and "
              .. "option against CLI source, paths and config statements against implementation, internal links, terms "
              .. "(Session, Workflow Instance, Memory, Skill, Working Folder, Model, Provider, Tier), and experimental "
              .. "labels. Do not edit. Return unresolved issues and details.", schema = VERIFY, timeout_ms = 180000
          })
          return { document = x.document, findings = x.findings, verification = v }
        end
      }
    }
  }

  phase("audit", 1)
  local audit = agent({
    name = "global-editor",
    description = "Audit cross-document consistency",
    prompt = "Audit all documents under `" .. repo .. "/" .. dir .. "`. Check duplicated or missing "
      .. "responsibilities, inconsistent definitions, conflicting commands, broken links, README navigation, and "
      .. "stable/experimental scope. Do not edit individual documents. Return concrete findings.",
    schema = AUDIT,
    timeout_ms = 180000
  })

  local summary = {}
  for _, item in ipairs(run.items or {}) do
    local v = out(item.output.verification)
    table.insert(summary, {
      document = item.output.document.name,
      path = item.output.document.path,
      passed = v.passed == true,
      unresolved = v.unresolved or {},
      findings = #(item.output.findings or {})
    })
  end
  report({ status = "completed", documents = summary, global_audit = out(audit), next_step = "Resolve major findings and rerun for convergence" })
end
