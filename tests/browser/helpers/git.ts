import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const gitEnv = {
  ...process.env,
  GIT_CONFIG_GLOBAL: "/dev/null",
  GIT_CONFIG_SYSTEM: "/dev/null",
};

export function git(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    env: gitEnv,
  }).trim();
}

/** A real repository with one commit. No user checkout is touched. */
export function tempRepository(
  prefix = "jabot-browser-repo-",
): { dir: string; name: string } {
  const dir = mkdtempSync(path.join(tmpdir(), prefix));
  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.email", "test@example.com");
  git(dir, "config", "user.name", "Test");
  writeFileSync(path.join(dir, ".gitignore"), ".env\n");
  writeFileSync(path.join(dir, "README.md"), "# project\n");
  git(dir, "add", "-A");
  git(dir, "commit", "-m", "first");
  writeFileSync(path.join(dir, ".env"), "TOKEN=secret\n");
  return { dir, name: path.basename(dir) };
}

/** Same as the host's unsaveable-worktree fixture: `git add` of `*.bin` fails. */
export function breakCommitting(repo: string): void {
  git(repo, "config", "filter.brokenlfs.clean", "false");
  git(repo, "config", "filter.brokenlfs.required", "true");
  writeFileSync(path.join(repo, ".gitattributes"), "*.bin filter=brokenlfs\n");
  git(repo, "add", "-A");
  git(repo, "commit", "-m", "lfs-tracked binaries");
}

export function addOrigin(repo: string, url: string): void {
  git(repo, "remote", "add", "origin", url);
}
