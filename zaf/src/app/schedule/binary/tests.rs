use super::*;

fn identity(inode: u64) -> Identity {
    Identity {
        device: 1,
        inode,
        size: 10,
        modified: None,
    }
}

#[test]
fn the_running_image_or_no_file_is_nothing_to_do() {
    let running = identity(1);

    assert_eq!(next(running, None, None, Some(running)), Look::Same);
    assert_eq!(next(running, None, Some(identity(2)), None), Look::Same);
}

#[test]
fn a_new_file_waits_one_look_and_is_then_ready() {
    let running = identity(1);
    let new = identity(2);

    assert_eq!(next(running, None, None, Some(new)), Look::Settling);
    assert_eq!(next(running, None, Some(new), Some(new)), Look::Ready(new));
    // Still being written: it changed again since the last look.
    let grown = Identity { size: 20, ..new };
    assert_eq!(next(running, None, Some(new), Some(grown)), Look::Settling);
}

#[test]
fn a_refused_file_is_tried_again_only_once_it_changes() {
    let running = identity(1);
    let refused = identity(2);

    assert_eq!(
        next(running, Some(refused), Some(refused), Some(refused)),
        Look::Same
    );
    let fixed = identity(3);
    assert_eq!(
        next(running, Some(refused), Some(refused), Some(fixed)),
        Look::Settling
    );
    assert_eq!(
        next(running, Some(refused), Some(fixed), Some(fixed)),
        Look::Ready(fixed)
    );
}

#[test]
fn a_binary_looks_at_its_path() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("zaf");
    std::fs::write(&path, "old").expect("the old file");
    let running = Identity::at(&path).expect("its identity");
    let mut binary = Binary::new(path.clone(), running);
    assert_eq!(binary.look(), Look::Same);

    // Replaced by a rename, as `zaf update` replaces it: a new inode.
    let staged = dir.path().join("zaf.new");
    std::fs::write(&staged, "new!").expect("the new file");
    std::fs::rename(&staged, &path).expect("renamed over");
    assert_eq!(binary.look(), Look::Settling);
    let Look::Ready(new) = binary.look() else {
        panic!("not ready at the second look");
    };

    binary.refuse(new);
    assert_eq!(binary.look(), Look::Same, "refused until it changes");
}

#[test]
fn a_binary_that_answers_its_version_gives_the_first_line() {
    let Some(true_) = ["/bin/true", "/usr/bin/true"]
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
    else {
        return;
    };
    // GNU `true` answers `--version`; another `true` prints nothing, which is refused.
    match version_of(true_) {
        Ok(version) => assert!(!version.contains('\n'), "{version}"),
        Err(VersionError::Refused(why)) => assert!(why.contains("printed nothing"), "{why}"),
        Err(VersionError::Busy) => panic!("busy"),
    }
}

#[test]
fn a_binary_that_fails_or_cannot_run_is_refused() {
    if let Some(false_) = ["/bin/false", "/usr/bin/false"]
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
    {
        assert_eq!(
            version_of(false_),
            Err(VersionError::Refused(
                "its --version ended with exit 1".to_owned()
            ))
        );
    }
    let dir = tempfile::tempdir().expect("a temporary directory");
    let missing = version_of(&dir.path().join("no-such-zaf"));
    assert!(
        matches!(&missing, Err(VersionError::Refused(why)) if why.starts_with("it cannot be run")),
        "{missing:?}"
    );
}

#[test]
fn the_path_to_start_drops_what_the_system_adds_to_a_replaced_binary() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let zaf = dir.path().join("zaf");
    std::fs::write(&zaf, "").expect("a file");
    let deleted = PathBuf::from(format!("{} (deleted)", zaf.display()));

    // Started by name, found on the search path: the image's path, without the suffix.
    assert_eq!(
        start_path(Some("zaf".into()), None, Ok(deleted.clone())),
        Ok(zaf.clone())
    );
    // Started from a path: that path, the file there now.
    let link = dir.path().join("bin-zaf");
    std::os::unix::fs::symlink(&zaf, &link).expect("a link");
    assert_eq!(
        start_path(Some(link.clone().into()), None, Ok(deleted.clone())),
        Ok(link)
    );
    // A relative path is read against the folder it was started in.
    assert_eq!(
        start_path(
            Some("./zaf".into()),
            Some(dir.path().to_owned()),
            Ok(deleted.clone())
        ),
        Ok(dir.path().join("./zaf"))
    );
    // A path that names no file is passed over for the image's.
    assert_eq!(
        start_path(Some("/nowhere/zaf".into()), None, Ok(deleted)),
        Ok(zaf)
    );
    assert!(start_path(None, None, Err(std::io::Error::other("a test"))).is_err());
}
