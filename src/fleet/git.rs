use std::path::Path;

use tokio::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitInfo {
    pub repo: Option<String>,
    pub branch: Option<String>,
}

/// Repo name is the git toplevel's directory name, or the cwd's own name outside a repo.
pub async fn lookup(cwd: &str) -> GitInfo {
    let toplevel = git(cwd, &["rev-parse", "--show-toplevel"]).await;
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    let repo_path = toplevel.as_deref().unwrap_or(cwd);
    let repo = Path::new(repo_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    GitInfo { repo, branch }
}

async fn git(cwd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = StdCommand::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[tokio::test]
    async fn finds_repo_name_and_branch() {
        let dir = tempfile::Builder::new()
            .prefix("shiprepo")
            .tempdir()
            .unwrap();
        git(dir.path(), &["init", "-q", "-b", "feature-x"]);
        git(
            dir.path(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let sub = dir.path().join("src");
        std::fs::create_dir(&sub).unwrap();

        let info = lookup(sub.to_str().unwrap()).await;

        assert!(
            info.repo.as_deref().unwrap().starts_with("shiprepo"),
            "{info:?}"
        );
        assert_eq!(info.branch.as_deref(), Some("feature-x"));
    }

    #[tokio::test]
    async fn falls_back_to_directory_name_outside_git() {
        let dir = tempfile::Builder::new()
            .prefix("plaindir")
            .tempdir()
            .unwrap();
        let info = lookup(dir.path().to_str().unwrap()).await;
        assert!(
            info.repo.as_deref().unwrap().starts_with("plaindir"),
            "{info:?}"
        );
        assert_eq!(info.branch, None);
    }
}
