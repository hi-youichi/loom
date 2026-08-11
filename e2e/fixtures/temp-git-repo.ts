/**
 * e2e/fixtures/temp-git-repo.ts
 *
 * Playwright test fixture for creating temporary git repositories.
 *
 * 基于 temp-project fixture，添加 git init 和基本配置。
 *
 * @module fixtures/temp-git-repo
 */

import { execSync } from "node:child_process";
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { TestFixture } from "@playwright/test";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface TempGitRepoOptions {
  /** 是否创建初始 commit */
  initialCommit?: boolean;
  /** 初始分支名（默认 main） */
  initialBranch?: string;
  /** 是否创建 remote（本地 bare repo） */
  withRemote?: boolean;
}

export interface TempGitRepoFixture {
  /** 临时 git 仓库 */
  tempGitRepo: {
    projectDir: string;
    remoteUrl?: string;
  };
}

// ─── Helper Functions ────────────────────────────────────────────────────────

/**
 * 执行 git 命令。
 */
function git(repoDir: string, command: string): string {
  return execSync(command, {
    cwd: repoDir,
    encoding: "utf-8",
    stdio: ["pipe", "pipe", "pipe"],
  });
}

// ─── Fixture Implementation ─────────────────────────────────────────────────

/**
 * tempGitRepo fixture。
 *
 * 在系统临时目录创建一个临时 git 仓库，用于 E2E 测试。
 *
 * 创建的仓库结构：
 * ```
 * <temp-dir>/
 * ├── README.md
 * ├── src/
 * │   └── index.ts
 * └── .git/
 * ```
 *
 * 可选配置：
 * - initialCommit: 创建初始 commit
 * - initialBranch: 指定初始分支名
 * - withRemote: 创建本地 bare repo 作为 remote
 *
 * 测试结束后自动清理。
 */
export const tempGitRepo: Record<string, TestFixture> = {
  tempGitRepo: async ({}, use) => {
    // 创建临时目录
    const projectDir = mkdtempSync(join(tmpdir(), "oc-e2e-git-"));

    // 创建基本项目结构
    writeFileSync(join(projectDir, "README.md"), "# Test Project\n");
    mkdirSync(join(projectDir, "src"), { recursive: true });
    writeFileSync(join(projectDir, "src", "index.ts"), "// placeholder\n");

    // 初始化 git 仓库
    git(projectDir, "git init --initial-branch=main");

    let remoteUrl: string | undefined;

    await use({ projectDir, remoteUrl });

    // 清理临时目录
    try {
      rmSync(projectDir, { recursive: true, force: true });
      console.log("[temp-git-repo] Cleaned up:", projectDir);
    } catch (err) {
      console.warn("[temp-git-repo] Cleanup failed:", err);
    }
  },
};
