use super::{Git, Snapshot};
use std::path::Path;

async fn command(root: &Path, args: &[&str]) -> std::process::Output {
    tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Owner",
            "-c",
            "user.email=owner@localhost",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .await
        .unwrap()
}

async fn owner(root: &Path, args: &[&str]) -> String {
    let output = command(root, args).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn registrations_preserve_owner_work_and_keep_their_own_reachable_history() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("plugins/a")).unwrap();
    std::fs::write(root.join(".gitignore"), "/.data/\n").unwrap();
    for path in ["config.toml", "plugins/a/lib.rs", "notes"] {
        std::fs::write(root.join(path), "initial").unwrap();
    }
    let git = Git::new(root);
    git.initialize().await.unwrap();
    assert!(
        !command(root, &["rev-parse", "--verify", "HEAD"])
            .await
            .status
            .success()
    );
    owner(root, &["add", "notes", "config.toml"]).await;
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let head = std::fs::read(root.join(".git/HEAD")).unwrap();
    std::fs::write(root.join("notes"), "owner working").unwrap();
    std::fs::write(root.join("plugins/a/lib.rs"), "captured source").unwrap();
    let source = git.tree("plugins/a").await.unwrap();
    std::fs::write(root.join("plugins/a/lib.rs"), "later source").unwrap();
    std::fs::write(root.join("config.toml"), "later config").unwrap();
    let tree = git
        .compose(
            None,
            &[
                Snapshot::Tree {
                    path: "plugins/a".into(),
                    tree: source,
                },
                Snapshot::File {
                    path: "config.toml".into(),
                    bytes: b"captured config".into(),
                },
            ],
        )
        .await
        .unwrap();
    let first = git.commit(None, &tree, "Registration 1").await.unwrap();
    assert!(git.reference().await.unwrap().is_none());
    git.anchor(&first).await.unwrap();
    let second = git
        .commit(Some(&first), &tree, "Registration 2")
        .await
        .unwrap();
    git.anchor(&second).await.unwrap();
    assert_ne!(first, second);
    assert_eq!(owner(root, &["rev-parse", "enco/registered^"]).await, first);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(root.join(".git/HEAD")).unwrap(), head);
    assert!(
        owner(root, &["for-each-ref", "refs/heads/"])
            .await
            .is_empty()
    );
    for (path, committed, working) in [
        ("plugins/a/lib.rs", "captured source", "later source"),
        ("config.toml", "captured config", "later config"),
    ] {
        assert_eq!(
            owner(root, &["show", &format!("{second}:{path}")]).await,
            committed
        );
        assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(), working);
    }
    assert_eq!(
        std::fs::read_to_string(root.join("notes")).unwrap(),
        "owner working"
    );
    assert_eq!(owner(root, &["show", ":notes"]).await, "initial");
    owner(root, &["commit", "--quiet", "-m", "Owner history"]).await;
    owner(
        root,
        &[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "Rewritten owner history",
        ],
    )
    .await;
    owner(root, &["gc", "--prune=now"]).await;
    assert_eq!(
        owner(root, &["show", &format!("{first}:config.toml")]).await,
        "captured config"
    );
    assert_eq!(
        git.directories(&second)
            .await
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["a"]
    );
}

#[tokio::test]
async fn an_owner_merge_does_not_change_registration_or_get_changed_by_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join(".gitignore"), "/.data/\n").unwrap();
    std::fs::write(root.join("notes"), "base\n").unwrap();
    let git = Git::new(root);
    git.initialize().await.unwrap();
    owner(root, &["add", "."]).await;
    owner(root, &["commit", "--quiet", "-m", "Base"]).await;
    let branch = owner(root, &["symbolic-ref", "--short", "HEAD"]).await;
    owner(root, &["checkout", "--quiet", "-b", "other"]).await;
    std::fs::write(root.join("notes"), "other\n").unwrap();
    owner(root, &["commit", "--quiet", "-am", "Other"]).await;
    owner(root, &["checkout", "--quiet", &branch]).await;
    std::fs::write(root.join("notes"), "owner\n").unwrap();
    owner(root, &["commit", "--quiet", "-am", "Owner"]).await;
    assert!(!command(root, &["merge", "other"]).await.status.success());
    let head = owner(root, &["rev-parse", "HEAD"]).await;
    let index = std::fs::read(root.join(".git/index")).unwrap();
    git.check().await.unwrap();
    let tree = git
        .compose(
            None,
            &[Snapshot::File {
                path: "config.toml".into(),
                bytes: b"accepted".into(),
            }],
        )
        .await
        .unwrap();
    let revision = git.commit(None, &tree, "Registration 1").await.unwrap();
    git.anchor(&revision).await.unwrap();
    assert_eq!(owner(root, &["rev-parse", "HEAD"]).await, head);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert!(root.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        owner(root, &["show", "enco/registered:config.toml"]).await,
        "accepted"
    );
}
