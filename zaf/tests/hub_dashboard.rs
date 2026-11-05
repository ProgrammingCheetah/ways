//! `zaf hub --port <n>` through the built binary: a port that is taken is refused by name.

mod common;

use std::net::TcpListener;

use common::{Project, outcome};

const MANIFEST: &str =
    "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n";

#[test]
fn a_used_port_exits_1_naming_the_port() {
    let held = TcpListener::bind("0.0.0.0:0").expect("binds");
    let port = held.local_addr().expect("address").port().to_string();
    let project = Project::new(MANIFEST);
    let (code, stdout, stderr) = outcome(&project.zaf(&["hub", "--port", &port]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stdout.is_empty(), "nothing is served: {stdout}");
    assert!(
        stderr.contains(&format!("port {port} is already in use")),
        "{stderr}"
    );
}

#[test]
fn a_port_is_not_taken_with_the_hook() {
    let project = Project::new(MANIFEST);
    let (code, _, stderr) = outcome(&project.zaf(&["hub", "report", "--port", "0"]));
    assert_eq!(code, 2, "the report verb has no port: {stderr}");
}
