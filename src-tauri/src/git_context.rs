use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGitContext {
    pub branch: Option<String>,
    pub revision: Option<String>,
    pub root: String,
    pub is_worktree: bool,
}

pub fn inspect_workspace(path: &Path) -> Result<Option<WorkspaceGitContext>, String> {
    let Some(metadata) = run_git(
        path,
        &[
            "rev-parse",
            "--show-toplevel",
            "--absolute-git-dir",
            "--git-common-dir",
        ],
    )?
    else {
        return Ok(None);
    };

    let mut lines = metadata.lines();
    let root = lines
        .next()
        .ok_or_else(|| "git did not return a worktree root".to_string())?;
    let git_dir = lines
        .next()
        .ok_or_else(|| "git did not return its git directory".to_string())?;
    let common_dir = lines
        .next()
        .ok_or_else(|| "git did not return its common directory".to_string())?;
    let root_path = std::fs::canonicalize(root).unwrap_or_else(|_| PathBuf::from(root));

    let branch =
        run_git(&root_path, &["branch", "--show-current"])?.filter(|branch| !branch.is_empty());
    let revision = if branch.is_none() {
        run_git(&root_path, &["rev-parse", "--short", "HEAD"])?
            .filter(|revision| !revision.is_empty())
    } else {
        None
    };

    Ok(Some(WorkspaceGitContext {
        branch,
        revision,
        root: root_path.to_string_lossy().into_owned(),
        is_worktree: resolve_git_dir(git_dir, path) != resolve_git_dir(common_dir, path),
    }))
}

fn run_git(path: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn resolve_git_dir(path: &str, working_dir: &Path) -> PathBuf {
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        working_dir.join(path)
    };
    std::fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::inspect_workspace;
    use std::{path::Path, process::Command};
    use tempfile::TempDir;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git must be installed for this test");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn reports_branch_and_linked_worktree_context() {
        let temp = TempDir::new().unwrap();
        let repository = temp.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "--initial-branch=main"]);
        git(
            &repository,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );

        let main = inspect_workspace(&repository).unwrap().unwrap();
        assert_eq!(main.branch.as_deref(), Some("main"));
        assert!(!main.is_worktree);

        let linked_path = temp.path().join("feature-worktree");
        git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                "feature/context",
                linked_path.to_str().unwrap(),
            ],
        );
        let linked = inspect_workspace(&linked_path).unwrap().unwrap();
        assert_eq!(linked.branch.as_deref(), Some("feature/context"));
        assert!(linked.is_worktree);
        assert_eq!(
            Path::new(&linked.root),
            std::fs::canonicalize(&linked_path).unwrap()
        );

        let linked_subdirectory = linked_path.join("src").join("nested");
        std::fs::create_dir_all(&linked_subdirectory).unwrap();
        let nested_linked = inspect_workspace(&linked_subdirectory).unwrap().unwrap();
        assert_eq!(nested_linked.branch, linked.branch);
        assert_eq!(nested_linked.root, linked.root);
        assert!(nested_linked.is_worktree);

        git(&linked_path, &["checkout", "--detach"]);
        let detached = inspect_workspace(&linked_path).unwrap().unwrap();
        assert_eq!(detached.branch, None);
        assert!(detached.revision.is_some());
    }

    #[test]
    fn does_not_report_repository_subdirectories_as_linked_worktrees() {
        let temp = TempDir::new().unwrap();
        git(temp.path(), &["init", "--initial-branch=main"]);
        let subdirectory = temp.path().join("src").join("nested");
        std::fs::create_dir_all(&subdirectory).unwrap();

        let context = inspect_workspace(&subdirectory).unwrap().unwrap();
        assert_eq!(context.branch.as_deref(), Some("main"));
        assert_eq!(
            Path::new(&context.root),
            std::fs::canonicalize(temp.path()).unwrap()
        );
        assert!(!context.is_worktree);
    }

    #[test]
    fn returns_none_for_a_non_git_directory() {
        let temp = TempDir::new().unwrap();
        assert!(inspect_workspace(temp.path()).unwrap().is_none());
    }
}
