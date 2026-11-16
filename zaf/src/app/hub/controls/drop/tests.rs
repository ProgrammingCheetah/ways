use super::*;

#[test]
fn a_body_that_is_not_a_drop_is_a_bad_request() {
    for body in ["", "[]", "{}", "{\"project\": 3}", "not json"] {
        let answer = run(Some(Path::new("/nowhere")), body.as_bytes());
        assert_eq!(answer.status, "400 Bad Request", "{body}");
    }
}

#[test]
fn no_zaf_home_is_unavailable() {
    let answer = run(None, br#"{"project": "/some/folder"}"#);
    assert_eq!(answer.status, "503 Service Unavailable");
}

#[test]
fn a_folder_not_on_the_list_is_not_found() {
    let home = tempfile::tempdir().expect("home");
    let answer = run(Some(home.path()), br#"{"project": "/some/folder"}"#);
    assert_eq!(answer.status, "404 Not Found");
}

#[test]
fn a_present_project_named_through_a_symlink_is_found_and_stays() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).expect("folder");
    std::fs::write(real.join(".zaf.toml"), "").expect("manifest");
    Projects::add(&home, &real).expect("listed");
    // The same folder by a path that is not the one the list keeps, as a temp folder is on macOS.
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    let body = serde_json::json!({ "project": link }).to_string();
    let answer = run(Some(&home), body.as_bytes());
    assert_eq!(answer.status, "409 Conflict", "{}", answer.body);
    assert_eq!(Projects::read(&home).len(), 1, "it stays");
}
