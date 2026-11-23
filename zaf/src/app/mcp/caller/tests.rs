use std::ffi::OsString;

use super::*;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{Instance, InstanceNumber, RoleName, TicketId};

const AT: &str = "2026-11-22T08:00:00Z";

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

/// A probe over a fake environment (`instance`) and a fake process tree (`ancestors`, this
/// process first), with every recorded process alive, over the run folders in `dir`.
fn probe(instance: Option<&str>, ancestors: &[Holder], dir: &tempfile::TempDir) -> ProcessProbe {
    ProcessProbe::new(
        instance.map(OsString::from),
        ancestors.to_vec(),
        dir.path().to_owned(),
        |_| true,
    )
}

/// Records a running launch of `team instance 3 on #10` held by process 4242.
fn launch_above(dir: &tempfile::TempDir) {
    let launch = PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(10),
            number: InstanceNumber::from(3),
        },
    };
    runs::reserve(dir.path(), &launch, &holder(4242), AT).expect("reserves");
}

fn tree() -> Vec<Holder> {
    vec![holder(5000), holder(4242), holder(1)]
}

#[test]
fn caller_probe_instance_from_env() {
    let dir = tempfile::tempdir().expect("temp dir");
    // The variable decides, with or without a launch above.
    let named = probe(Some("programmer instance 7 on #1"), &tree(), &dir);
    assert_eq!(
        named.caller(),
        Ok(McpCaller::Instance(RoleName::from("programmer")))
    );
    launch_above(&dir);
    assert_eq!(
        named.caller(),
        Ok(McpCaller::Instance(RoleName::from("programmer")))
    );
}

#[test]
fn caller_probe_malformed_instance_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    for bad in ["", "   ", "programmer", "programmer instance x on #1"] {
        let refused = probe(Some(bad), &tree(), &dir)
            .caller()
            .expect_err("a bad value is refused, never outside");
        assert!(
            matches!(&refused, Refusal::UnparsedInstance(_)),
            "{bad:?}: {refused:?}"
        );
        let text = refused.to_string();
        assert!(text.contains(INSTANCE_VAR), "{text}");
        assert!(text.contains(&format!("{bad:?}")), "{text}");
    }
    // Not even valid text.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let bytes = ProcessProbe::new(
            Some(OsString::from_vec(vec![0xff, 0xfe])),
            tree(),
            dir.path().to_owned(),
            |_| true,
        );
        assert!(matches!(bytes.caller(), Err(Refusal::UnparsedInstance(_))));
    }
}

#[test]
fn caller_probe_nameless_instance_above_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    launch_above(&dir);
    let refused = probe(None, &tree(), &dir)
        .caller()
        .expect_err("an instance above has lost its name");
    assert_eq!(
        refused,
        Refusal::NamelessInstance("team instance 3 on #10".to_owned())
    );
    assert!(refused.to_string().contains("team instance 3 on #10"));
}

#[test]
fn caller_probe_outside() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(probe(None, &tree(), &dir).caller(), Ok(McpCaller::Outside));
    // A launch that is not above, or no longer runs, does not name the caller.
    launch_above(&dir);
    assert_eq!(
        probe(None, &[holder(5000), holder(1)], &dir).caller(),
        Ok(McpCaller::Outside)
    );
    let ended = ProcessProbe::new(None, tree(), dir.path().to_owned(), |_| false);
    assert_eq!(ended.caller(), Ok(McpCaller::Outside));
}

#[test]
fn caller_probe_unreadable_tree_is_refused_not_outside() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(
        probe(None, &[], &dir).caller(),
        Err(Refusal::TreeUnreadable)
    );
}
