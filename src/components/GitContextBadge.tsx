import { GitBranch } from "lucide-react";
import type { WorkspaceGitContext } from "../types";

export function GitContextBadge({
  gitContext,
  className,
}: {
  gitContext: WorkspaceGitContext;
  className?: string;
}) {
  const label = gitContext.branch
    ? gitContext.branch
    : gitContext.revision
      ? `detached · ${gitContext.revision}`
      : "unborn";
  const title = `${gitContext.branch ? `Branch: ${gitContext.branch}` : `Detached at: ${gitContext.revision ?? "no commit"}`}\n${gitContext.isWorktree ? "Git worktree" : "Working tree"}: ${gitContext.root}`;

  return (
    <span
      className={`topbar-git-context${className ? ` ${className}` : ""}`}
      data-testid="topbar-git-context"
      title={title}
    >
      <GitBranch size={13} strokeWidth={1.6} aria-hidden="true" />
      <span className="topbar-git-branch">{label}</span>
      {gitContext.isWorktree && <span className="topbar-git-kind">worktree</span>}
    </span>
  );
}
