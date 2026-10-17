use super::*;

#[tokio::test]
async fn source_values_follow_only_local_gitignore_rules_and_include_dotfiles() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("plugin");
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(home.path().join(".gitignore"), "plugin/\n").unwrap();
    std::fs::write(root.join(".ignore"), "source.rs\n").unwrap();
    std::fs::write(root.join(".gitignore"), "/target/\nprivate\n").unwrap();
    std::fs::write(root.join("nested/.gitignore"), "*.tmp\n").unwrap();
    std::fs::write(root.join("nested/local.tmp"), "ignored").unwrap();
    std::fs::write(root.join("source.rs"), "source").unwrap();
    std::fs::write(root.join(".cargo/config.toml"), "target = 'one'").unwrap();
    std::fs::write(root.join(".git"), "gitdir: elsewhere").unwrap();
    std::fs::write(root.join("private"), "ignored").unwrap();
    let before = FileTree::read(&root).await.unwrap();
    assert!(before.files.contains_key("source.rs"));
    assert!(before.files.contains_key(".cargo/config.toml"));
    assert!(!before.files.contains_key("nested/local.tmp"));
    assert!(!before.files.contains_key(".git"));
    std::fs::write(root.join("private"), "changed").unwrap();
    assert_eq!(before.hash(), FileTree::read(&root).await.unwrap().hash());
    std::fs::write(root.join(".cargo/config.toml"), "target = 'two'").unwrap();
    assert_ne!(before.hash(), FileTree::read(&root).await.unwrap().hash());
    std::os::unix::fs::symlink("source.rs", root.join("alias.rs")).unwrap();
    assert!(matches!(
        FileTree::read(&root).await,
        Err(DevelopmentError::Entry(EntryError {
            path,
            problem: EntryProblem::NotRegular,
        })) if path == root.join("alias.rs")
    ));
}

#[tokio::test]
async fn plans_handle_file_directory_replacements_in_both_directions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("a")).unwrap();
    std::fs::write(root.path().join("a/file"), b"old").unwrap();
    std::fs::write(root.path().join("b"), b"old").unwrap();
    let target =
        FileTree::from_files([("a", b"file".to_vec()), ("b/file", b"nested".to_vec())]).unwrap();
    Directory::read(root.path(), Ownership::Source)
        .await
        .unwrap()
        .plan_write(target.clone())
        .unwrap()
        .execute(&mut Vec::new())
        .await
        .unwrap();
    assert_eq!(
        FileTree::read(root.path()).await.unwrap().hash(),
        target.hash()
    );
}

#[tokio::test]
async fn manifest_and_file_blobs_recover_the_same_portable_value() {
    let home = tempfile::tempdir().unwrap();
    let store = crate::SqliteStore::open(crate::StorePaths {
        db: home.path().join("enco.db"),
        blobs: home.path().join("blobs"),
        artifacts: home.path().join("artifacts"),
    })
    .await
    .unwrap();
    let tree = FileTree {
        files: [
            ("z".parse().unwrap(), b"last".to_vec()),
            ("a".parse().unwrap(), b"first".to_vec()),
        ]
        .into(),
    };
    let source = tree.store(&store).await.unwrap();
    assert_eq!(source, tree.hash());
    assert_eq!(
        FileTree::load(&store, source).await.unwrap().files,
        tree.files
    );
    let manifest = String::from_utf8(store.get_blob(&source).await.unwrap()).unwrap();
    assert_eq!(
        manifest,
        format!(
            "{{\"a\":\"{}\",\"z\":\"{}\"}}",
            ContentHash::of(b"first"),
            ContentHash::of(b"last")
        )
    );
}

#[tokio::test]
async fn owned_projection_removes_stale_inputs_without_following_symlinks() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("kept"), "owner data").unwrap();
    let old = FileTree {
        files: [
            ("src/lib.rs".parse().unwrap(), b"old".to_vec()),
            ("build.rs".parse().unwrap(), b"stale build".to_vec()),
        ]
        .into(),
    };
    old.project(&home.path().join("sdk")).await.unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("sdk/extra")).unwrap();
    let current = FileTree {
        files: [("src/lib.rs".parse().unwrap(), b"current".to_vec())].into(),
    };
    current.project(&home.path().join("sdk")).await.unwrap();
    assert_eq!(
        std::fs::read(home.path().join("sdk/src/lib.rs")).unwrap(),
        b"current"
    );
    assert!(!home.path().join("sdk/build.rs").exists());
    assert!(!home.path().join("sdk/extra").exists());
    assert_eq!(
        std::fs::read(outside.path().join("kept")).unwrap(),
        b"owner data"
    );
}

#[tokio::test]
async fn planning_finds_ignored_collisions_before_execution() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".gitignore"), "b\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("b")).unwrap();
    let target =
        FileTree::from_files([(".gitignore", Vec::new()), ("b/file", b"target".to_vec())]).unwrap();
    let conflicts = Directory::read(root.path(), Ownership::Source)
        .await
        .unwrap()
        .plan_write(target)
        .err()
        .unwrap();
    assert!(
        matches!(&conflicts[..], [WriteConflict::Occupied { path }] if path == &root.path().join("b"))
    );
}

#[tokio::test]
async fn an_interrupted_plan_reports_progress_and_retries_only_the_remaining_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a"), b"old").unwrap();
    let target =
        FileTree::from_files([("a", b"target".to_vec()), ("b/file", b"target".to_vec())]).unwrap();
    let plan = Directory::read(root.path(), Ownership::Source)
        .await
        .unwrap()
        .plan_write(target.clone())
        .unwrap();
    std::fs::write(root.path().join("b"), b"owner edit after planning").unwrap();
    let mut written = Vec::new();
    assert!(plan.execute(&mut written).await.is_err());
    assert_eq!(written, [root.path().join("a")]);
    std::fs::remove_file(root.path().join("b")).unwrap();
    written.clear();
    Directory::read(root.path(), Ownership::Source)
        .await
        .unwrap()
        .plan_write(target.clone())
        .unwrap()
        .execute(&mut written)
        .await
        .unwrap();
    assert_eq!(written, [root.path().join("b/file")]);
    assert_eq!(
        FileTree::read(root.path()).await.unwrap().hash(),
        target.hash()
    );
}

#[tokio::test]
async fn removing_a_source_directory_never_deletes_entries_created_after_planning() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("plugin");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("source"), b"source").unwrap();
    let plan = Directory::read(&directory, Ownership::Source)
        .await
        .unwrap()
        .plan_removal()
        .unwrap();
    std::fs::write(directory.join("new"), b"owner work").unwrap();
    assert!(plan.execute(&mut Vec::new()).await.is_err());
    assert_eq!(std::fs::read(directory.join("new")).unwrap(), b"owner work");
}

#[test]
fn source_paths_are_canonical_at_every_deserialization_boundary() {
    for path in [
        "",
        "/absolute",
        "../escape",
        "a/../escape",
        "./a",
        "a//b",
        "a/",
        "a\\b",
    ] {
        assert!(
            serde_json::from_value::<SourcePath>(serde_json::json!(path)).is_err(),
            "{path}"
        );
    }
    assert!(
        matches!(serde_json::from_value::<SourcePath>(serde_json::json!(".cargo/config.toml")), Ok(path) if path.as_str() == ".cargo/config.toml")
    );
}

#[tokio::test]
async fn an_empty_directory_is_distinct_from_an_absent_target() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("plugin");
    for ownership in [Ownership::Source, Ownership::Complete] {
        Directory::read(&path, ownership)
            .await
            .unwrap()
            .plan_write(FileTree::default())
            .unwrap()
            .execute(&mut Vec::new())
            .await
            .unwrap();
        assert!(path.is_dir());
        Directory::read(&path, ownership)
            .await
            .unwrap()
            .plan_removal()
            .unwrap()
            .execute(&mut Vec::new())
            .await
            .unwrap();
        assert!(!path.exists());
    }
}
