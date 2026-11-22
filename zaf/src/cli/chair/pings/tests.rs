use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::{MOST, Notice, Notifier, Ping, SCRIPT, desktop_on, pings};
use crate::app::chair::{ChairState, InboxItem};
use crate::app::hub::next::NextKind;
use crate::cli::chair::support::ways;
use crate::domain::{EpicStage, TicketId};

fn question(number: u64, title: &str) -> InboxItem {
    InboxItem {
        kind: NextKind::Question,
        title: title.to_owned(),
        blocks: 1,
        how: "Rule it.".to_owned(),
        command: None,
        link: None,
        number: Some(TicketId::from(number)),
        question: None,
        begins: None,
    }
}

fn notice(body: &str) -> Ping {
    Ping::Notify(Notice {
        title: "zaf · ways".to_owned(),
        body: body.to_owned(),
    })
}

#[test]
fn a_new_question_rings_the_bell_and_notifies_its_first_line() {
    let before = ways();
    let mut after = ways();
    after.inbox.insert(
        0,
        question(
            293,
            "Question #293: Should install.sh expand ~?\nMore words below.",
        ),
    );

    assert_eq!(
        pings(Some(&before), &after),
        [
            Ping::Bell,
            notice("Question #293: Should install.sh expand ~?")
        ]
    );
}

#[test]
fn nothing_new_rings_nothing() {
    let before = ways();
    let mut after = ways();
    after.inbox.remove(0);
    after.inbox.reverse();
    assert_eq!(
        pings(Some(&before), &after),
        [],
        "gone and moved are not new"
    );
    assert_eq!(pings(None, &after), [], "the first read pings nothing");
}

#[test]
fn an_item_is_new_by_its_id_not_its_words() {
    let mut before = ways();
    let mut after = ways();
    after.inbox[0].title = "Question #285: reworded".to_owned();
    assert_eq!(pings(Some(&before), &after), [], "the same question");

    let unticked = |title: &str| InboxItem {
        kind: NextKind::Unticked,
        title: title.to_owned(),
        blocks: 1,
        how: String::new(),
        command: None,
        link: None,
        number: None,
        question: None,
        begins: None,
    };
    before
        .inbox
        .push(unticked("1 ready task in ways: no scheduler ticks"));
    after
        .inbox
        .push(unticked("2 ready tasks in ways: no scheduler ticks"));
    assert_eq!(
        pings(Some(&before), &after),
        [],
        "a project has one such item, whatever it counts"
    );
}

#[test]
fn a_read_without_the_board_pings_nothing_nor_does_the_one_after() {
    let before = ways();
    let mut unread = ways();
    unread.reason = Some("the board cannot be reached".to_owned());
    unread.inbox.clear();
    let mut back = ways();
    back.inbox.push(question(300, "Question #300: new"));

    assert_eq!(pings(Some(&before), &unread), []);
    assert_eq!(pings(Some(&unread), &back), []);
}

#[test]
fn many_new_items_ring_once_and_notify_a_few() {
    let before = ways();
    let mut after = ways();
    for number in 300..306 {
        after
            .inbox
            .push(question(number, &format!("Question #{number}: new")));
    }
    let given = pings(Some(&before), &after);
    assert_eq!(given.first(), Some(&Ping::Bell));
    assert_eq!(given.iter().filter(|ping| **ping == Ping::Bell).count(), 1);
    assert_eq!(given.len(), 1 + MOST);
}

fn all_done(state: &mut ChairState) {
    for epic in &mut state.epics {
        epic.stage = EpicStage::Done;
        epic.done = epic.total;
    }
}

#[test]
fn a_version_whose_epics_all_become_done_notifies() {
    let before = ways();
    let mut after = ways();
    all_done(&mut after);
    assert_eq!(
        pings(Some(&before), &after),
        [Ping::Bell, notice("Release 0.5.2: every epic is done")]
    );
    let mut again = ways();
    all_done(&mut again);
    assert_eq!(pings(Some(&after), &again), [], "once, as it finishes");
}

#[test]
fn notify_send_gets_the_text_as_an_argument_after_its_options() {
    let notifier = Notifier::NotifySend(PathBuf::from("/usr/bin/notify-send"));
    let text = Notice {
        title: "zaf · ways".to_owned(),
        body: "-u critical \"$(rm -rf ~)\"".to_owned(),
    };
    let (program, args) = notifier.command_line(&text);
    assert_eq!(program, PathBuf::from("/usr/bin/notify-send"));
    assert_eq!(
        args,
        [
            "--app-name=zaf",
            "--",
            "zaf · ways",
            "-u critical \"$(rm -rf ~)\""
        ]
        .map(OsString::from)
    );
}

#[test]
fn osascript_reads_the_text_from_argv_never_from_the_script() {
    let notifier = Notifier::Osascript(PathBuf::from("/usr/bin/osascript"));
    let text = Notice {
        title: "zaf · ways".to_owned(),
        body: "\" & (do shell script \"rm -rf ~\") & \"".to_owned(),
    };
    let (program, args) = notifier.command_line(&text);
    assert_eq!(program, PathBuf::from("/usr/bin/osascript"));
    let script: Vec<&OsStr> = args.iter().take(6).map(OsString::as_os_str).collect();
    assert_eq!(
        script,
        ["-e", SCRIPT[0], "-e", SCRIPT[1], "-e", SCRIPT[2]].map(OsStr::new)
    );
    assert!(
        SCRIPT.iter().all(|line| !line.contains("rm -rf")),
        "the script is fixed"
    );
    assert_eq!(args[6], OsString::from("zaf · ways"));
    assert_eq!(args[7], OsString::from(text.body));
    assert_eq!(args.len(), 8);
}

#[test]
fn the_notifier_is_the_systems_own_when_it_is_on_path() {
    let folder = tempfile::tempdir().expect("a folder");
    for name in ["notify-send", "osascript"] {
        std::fs::write(folder.path().join(name), "").expect("a stand-in");
    }
    let path = std::env::join_paths([PathBuf::from("/nowhere"), folder.path().to_owned()])
        .expect("a PATH");

    assert_eq!(
        Notifier::find("linux", Some(&path)),
        Some(Notifier::NotifySend(folder.path().join("notify-send")))
    );
    assert_eq!(
        Notifier::find("macos", Some(&path)),
        Some(Notifier::Osascript(folder.path().join("osascript")))
    );
    assert_eq!(Notifier::find("linux", Some(OsStr::new("/nowhere"))), None);
    assert_eq!(Notifier::find("linux", None), None);
}

#[test]
fn zaf_notify_0_turns_the_desktop_off() {
    assert!(desktop_on(None));
    assert!(desktop_on(Some(OsStr::new("1"))));
    assert!(!desktop_on(Some(OsStr::new("0"))));
}

#[test]
fn the_offer_to_begin_the_next_version_rings_nothing() {
    let before = ways();
    let mut after = ways();
    after.inbox.push(InboxItem {
        kind: NextKind::Phase,
        title: "Begin 0.5.4: its objectives come first".to_owned(),
        blocks: 0,
        how: String::new(),
        command: Some("zaf begin --version 0.5.4".to_owned()),
        link: None,
        number: None,
        question: None,
        begins: Some(crate::app::chair::ReleaseVersion::from("0.5.4")),
    });

    assert_eq!(pings(Some(&before), &after), []);
}

#[test]
fn notify_send_reads_no_markup_from_the_text_and_long_text_is_cut() {
    let notifier = Notifier::NotifySend(PathBuf::from("/usr/bin/notify-send"));
    let notice = Notice {
        title: format!("zaf · {}", "w".repeat(200)),
        body: format!("<a href=\"x\">Q&A</a> {}", "b".repeat(400)),
    };

    let (_, args) = notifier.command_line(&notice);

    let title = args[2].to_string_lossy().into_owned();
    let body = args[3].to_string_lossy().into_owned();
    assert_eq!(title.chars().count(), super::TITLE_MOST);
    assert!(title.ends_with('…'));
    assert!(
        body.starts_with("&lt;a href=\"x\"&gt;Q&amp;A&lt;/a&gt; bbb"),
        "{body}"
    );
    assert!(!body.contains('<') && !body.contains('>'));
    assert!(body.ends_with('…'));

    let osascript = Notifier::Osascript(PathBuf::from("/usr/bin/osascript"));
    let (_, args) = osascript.command_line(&notice);
    let body = args
        .last()
        .expect("the text")
        .to_string_lossy()
        .into_owned();
    assert_eq!(body.chars().count(), super::BODY_MOST, "cut, never escaped");
    assert!(body.starts_with("<a href"));
}
