use super::*;
use crate::app::hub::guard::{Guard, LINK_PARAMETER};
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{Instance, InstanceNumber, RoleName, TicketId};

const AT: &str = "2026-11-13T08:00:00Z";

fn outside(_: u32) -> Standing {
    Standing::Outside
}

/// A zaf home with one project on its list whose run folder holds a live instance: this very
/// process, so the test process is inside it.
fn home_with_instance_above() -> (tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join(".zaf.toml"), "").expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    let launch = PlannedLaunch {
        instance: Instance {
            role: RoleName::from("programmer"),
            epic: TicketId::from(1),
            number: InstanceNumber::from(7),
        },
    };
    let me = process::this().expect("this process");
    runs::reserve(&project.path().join(STATE_DIR), &launch, &me, AT).expect("reserves");
    (home, project)
}

fn key_of(link: &str) -> &str {
    link.split_once(&format!("#{LINK_PARAMETER}="))
        .expect("a key in the fragment")
        .1
}

#[test]
fn refused_inside_instance() {
    let (home, _project) = home_with_instance_above();
    let judge = judge_over(home.path().to_owned());
    let me = judge(std::process::id());
    assert_eq!(
        me,
        Standing::Inside("programmer instance 7 on #1".to_owned())
    );

    // The client refuses before it connects, and says nothing of a key.
    let guard = Guard::start(false).expect("a key");
    let link = guard.link("http://127.0.0.1:7777").expect("a link");
    let socket =
        LinkSocket::serve(home.path(), Some(link.clone()), Arc::clone(&judge)).expect("serves");
    let refused = fetch(home.path(), &me).expect_err("refused");
    assert!(matches!(refused, LinkError::Inside(_)), "got {refused:?}");
    assert!(!refused.to_string().contains(key_of(&link)));

    // A client that skipped its own check is refused by the hub, which sees who is on the line.
    let asked = fetch(home.path(), &Standing::Outside).expect_err("refused by the hub");
    assert!(
        matches!(&asked, LinkError::Refused(why) if why.contains("inside an instance")),
        "got {asked:?}"
    );
    assert!(!asked.to_string().contains(key_of(&link)));
    socket.release();
}

#[test]
fn prints_working_link() {
    let home = tempfile::tempdir().expect("home");
    let guard = Guard::start(false).expect("a key");
    let link = guard.link("http://127.0.0.1:7777").expect("a link");
    let socket =
        LinkSocket::serve(home.path(), Some(link.clone()), Arc::new(outside)).expect("serves");

    let printed = fetch(home.path(), &Standing::Outside).expect("a link");
    assert_eq!(printed, link);
    // The key in it is the one the hub admits for a control, and no other.
    assert_eq!(guard.admit(Some(key_of(&printed))), Ok(()));
    assert!(guard.admit(Some("not-the-key")).is_err());

    let mode = std::fs::metadata(socket_path(home.path()))
        .expect("the socket")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, SOCKET_MODE);
    socket.release();
}

#[test]
fn a_read_only_hub_gives_no_link() {
    let home = tempfile::tempdir().expect("home");
    let socket = LinkSocket::serve(home.path(), None, Arc::new(outside)).expect("serves");
    let asked = fetch(home.path(), &Standing::Outside).expect_err("no link");
    assert!(
        matches!(&asked, LinkError::Refused(why) if why.contains("read-only")),
        "got {asked:?}"
    );
    socket.release();
}

#[test]
fn no_hub_running() {
    let home = tempfile::tempdir().expect("home");
    let none = fetch(home.path(), &Standing::Outside).expect_err("none runs");
    assert!(matches!(none, LinkError::NoHub), "got {none:?}");
    assert!(none.to_string().contains("no hub runs"));

    // A socket file a killed hub left behind has nobody on it.
    LinkSocket::serve(home.path(), None, Arc::new(outside)).expect("serves");
    drop(UnixListener::bind(home.path().join("other.sock")).expect("binds"));
    std::fs::rename(home.path().join("other.sock"), socket_path(home.path())).expect("replaced");
    let stale = fetch(home.path(), &Standing::Outside).expect_err("none runs");
    assert!(matches!(stale, LinkError::NoHub), "got {stale:?}");
}

#[test]
fn the_hub_refuses_whom_it_cannot_tell() {
    let me = PeerCredentials { pid: 5, uid: 1000 };
    let link = Some("http://h/#key=k");
    assert_eq!(
        answer(link, Some(me), 1000, &outside),
        Reply::Link("http://h/#key=k".to_owned())
    );
    assert!(matches!(
        answer(link, None, 1000, &outside),
        Reply::Refused(_)
    ));
    assert!(matches!(
        answer(link, Some(me), 1001, &outside),
        Reply::Refused(_)
    ));
    assert!(matches!(
        answer(link, Some(me), 1000, &|_| Standing::Unknown),
        Reply::Refused(_)
    ));
}

#[test]
fn a_reply_reads_back_as_written() {
    for reply in [
        Reply::Link("http://h:1/#key=ab".to_owned()),
        Reply::Refused("no".to_owned()),
    ] {
        assert_eq!(Reply::parse(&reply.render()), Some(reply));
    }
    assert_eq!(Reply::parse("garbage"), None);
}
