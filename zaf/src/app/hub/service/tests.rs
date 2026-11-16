use super::*;

#[test]
fn the_unit_folder_is_under_the_config_home_else_dot_config() {
    let os = |text: &str| Some(OsString::from(text));
    assert_eq!(
        unit_dir(os("/cfg"), os("/home/me")),
        Some(PathBuf::from("/cfg/systemd/user"))
    );
    assert_eq!(
        unit_dir(os(""), os("/home/me")),
        Some(PathBuf::from("/home/me/.config/systemd/user"))
    );
    assert_eq!(unit_dir(None, None), None);
    assert_eq!(
        unit_path(Path::new("/cfg/systemd/user")),
        PathBuf::from("/cfg/systemd/user/zaf-hub.service")
    );
}

#[test]
fn a_path_with_a_space_is_quoted_and_the_zaf_home_is_passed_on() {
    let unit = Unit {
        exe: PathBuf::from("/opt/my zaf/zaf"),
        port: 7777,
        project: PathBuf::from("/work/a%b"),
        zaf_home: Some(PathBuf::from("/var/zaf home")),
    };
    let text = unit.render();
    assert!(
        text.contains("ExecStart=\"/opt/my zaf/zaf\" hub --port 7777\n"),
        "{text}"
    );
    assert!(text.contains("WorkingDirectory=/work/a%%b\n"), "{text}");
    assert!(
        text.contains("Environment=\"ZAF_HOME=/var/zaf home\"\n"),
        "{text}"
    );
}

#[test]
fn the_working_directory_is_never_quoted() {
    let unit = Unit {
        exe: PathBuf::from("/opt/zaf/zaf"),
        port: 7777,
        project: PathBuf::from("/work/my $project/a%b"),
        zaf_home: None,
    };
    let text = unit.render();
    assert!(
        text.contains("WorkingDirectory=/work/my $project/a%%b\n"),
        "{text}"
    );
}
