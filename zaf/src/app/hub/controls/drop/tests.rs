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
