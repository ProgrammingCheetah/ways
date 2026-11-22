use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc;

use super::{close, hand_on, on_update, start_control, start_fix, start_writer};
use crate::app::chair::{
    ChairAction, ChairActions, Fix, FixVerb, Outcome, ReleaseVersion, Ruling, Signing,
};
use crate::app::hub::controls::Failure;
use crate::cli::chair::build::{Control, Controls};
use crate::cli::chair::model::Command;
use crate::cli::chair::source::Update;
use crate::cli::chair::support::chair;
use crate::domain::TicketId;

/// Writes that record what they were asked and write nothing.
#[derive(Default)]
struct Recorder {
    ruled: Mutex<Vec<TicketId>>,
}

impl ChairActions for Recorder {
    fn rule(&self, ruling: &Ruling) -> Result<Option<TicketId>, Failure> {
        self.ruled
            .lock()
            .expect("not poisoned")
            .push(ruling.question);
        Ok(Some(TicketId::from(234)))
    }

    fn fix(&self, _fix: &Fix) -> Result<String, Failure> {
        Err(Failure::Refused("no fixes in a test".to_owned()))
    }

    fn sign(&self, _signing: &Signing) -> Result<bool, Failure> {
        Ok(true)
    }
}

fn ruling(question: u64) -> ChairAction {
    ChairAction::Rule(
        Ruling::new(PathBuf::from("/srv/ways"), TicketId::from(question), "Yes").expect("words"),
    )
}

#[test]
fn closing_writes_what_waits_and_says_so() {
    let writes = std::sync::Arc::new(Recorder::default());
    let (updates, heard) = mpsc::channel();
    let (to_write, writer) = start_writer(writes.clone(), updates);
    to_write.send(ruling(278)).expect("the writer listens");

    let said = close(to_write, writer, &heard, &[ruling(285)], writes.as_ref());

    assert_eq!(
        *writes.ruled.lock().expect("not poisoned"),
        [TicketId::from(278), TicketId::from(285)],
        "the write under way ends first, then what waited"
    );
    assert_eq!(
        said,
        [
            "#278 ruled; #234 ready".to_owned(),
            "#285 ruled; #234 ready (written as zaf closed)".to_owned(),
        ]
    );
}

#[test]
fn a_write_that_ends_is_handed_to_the_chair() {
    let mut chair = chair((80, 24));
    let outcome = Outcome::Ruled {
        question: TicketId::from(285),
        ready: None,
    };

    let commands = on_update(&mut chair, Update::Written(ruling(285), outcome));

    assert_eq!(commands, [Command::ReadNow(0)]);
    assert_eq!(
        chair.note.as_ref().map(|note| note.words.as_str()),
        Some("#285 ruled")
    );
}

/// Controls that refuse every stop and tick, and start every build.
struct Starter;

impl Controls for Starter {
    fn start(&self, _root: &std::path::Path, version: &ReleaseVersion) -> Result<String, Failure> {
        Ok(format!("the build of {version} started (pid 5150)"))
    }

    fn stop(&self, _root: &std::path::Path, _pid: u32) -> Result<String, Failure> {
        Err(Failure::Refused("no scheduler runs now".to_owned()))
    }

    fn stop_task(&self, _root: &std::path::Path, _task: TicketId) -> Result<String, Failure> {
        Err(Failure::Refused("no instance is running it".to_owned()))
    }

    fn tick(&self, _root: &std::path::Path) -> Result<String, Failure> {
        Err(Failure::Refused("no scheduler runs now".to_owned()))
    }
}

#[test]
fn a_control_runs_on_its_own_thread_and_its_outcome_reaches_the_chair() {
    let (updates, heard) = mpsc::channel();
    let control = Control::StartBuild {
        root: PathBuf::from("/srv/ways"),
        version: ReleaseVersion::from("0.5.2"),
    };

    start_control(std::sync::Arc::new(Starter), control.clone(), updates);
    let update = heard
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the control ends");

    let Update::Controlled(ran, outcome) = &update else {
        panic!("a control's outcome: {update:?}");
    };
    assert_eq!(*ran, control);
    assert_eq!(
        *outcome,
        Ok("the build of 0.5.2 started (pid 5150)".to_owned())
    );
    let mut chair = chair((80, 24));
    assert_eq!(on_update(&mut chair, update), [Command::ReadNow(0)]);
}

#[test]
fn a_paste_event_reaches_the_chair_as_text() {
    let mut chair = chair((80, 24));
    let now = std::time::Instant::now();
    chair.key(crate::cli::chair::keys::Key::Enter, now);
    chair.key(crate::cli::chair::keys::Key::CtrlU, now);

    let commands = super::on_event(
        &mut chair,
        &crossterm::event::Event::Paste("Yes\rq".to_owned()),
    );

    assert!(commands.is_empty(), "{commands:?}");
    assert!(chair.pending.is_empty());
    assert!(
        matches!(&chair.mode, crate::cli::chair::model::Mode::Answer(answer) if answer.text == "Yes\nq"),
        "{:?}",
        chair.mode
    );
}

/// Writes that panic on question 13, hang on every fix until told, and rule the rest.
struct Troubled {
    ruled: Mutex<Vec<TicketId>>,
    fixes: Mutex<mpsc::Receiver<()>>,
}

impl ChairActions for Troubled {
    fn rule(&self, ruling: &Ruling) -> Result<Option<TicketId>, Failure> {
        assert_ne!(ruling.question, TicketId::from(13), "a bug in a write");
        self.ruled
            .lock()
            .expect("not poisoned")
            .push(ruling.question);
        Ok(None)
    }

    fn fix(&self, _fix: &Fix) -> Result<String, Failure> {
        let _ = self.fixes.lock().expect("not poisoned").recv();
        Ok("done".to_owned())
    }

    fn sign(&self, _signing: &Signing) -> Result<bool, Failure> {
        Ok(true)
    }
}

fn troubled() -> (std::sync::Arc<Troubled>, mpsc::Sender<()>) {
    let (release, fixes) = mpsc::channel();
    let writes = Troubled {
        ruled: Mutex::new(Vec::new()),
        fixes: Mutex::new(fixes),
    };
    (std::sync::Arc::new(writes), release)
}

fn fix_action() -> ChairAction {
    ChairAction::Fix(Fix::new(
        PathBuf::from("/srv/ways"),
        FixVerb::Doctor,
        "zaf doctor",
    ))
}

#[test]
fn a_write_that_panics_is_said_as_failed_and_the_writer_goes_on() {
    let (writes, _release) = troubled();
    let (updates, heard) = mpsc::channel();
    let (to_write, writer) = start_writer(writes.clone(), updates);
    to_write.send(ruling(13)).expect("listens");
    to_write.send(ruling(285)).expect("still listens");

    let first = heard
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("said");
    let second = heard
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("said");

    let Update::Written(_, outcome) = first else {
        panic!("{first:?}");
    };
    assert!(!outcome.written(), "{outcome:?}");
    assert!(
        outcome.line().starts_with("#13 was not ruled: zaf failed"),
        "{}",
        outcome.line()
    );
    assert!(
        matches!(second, Update::Written(_, Outcome::Ruled { .. })),
        "{second:?}"
    );
    drop(to_write);
    writer
        .expect("a writer")
        .join()
        .expect("it ended without a panic");
}

#[test]
fn a_write_the_writer_cannot_take_is_said_and_its_question_comes_back() {
    let mut chair = chair((80, 24));
    let now = std::time::Instant::now();
    chair.key(crate::cli::chair::keys::Key::Enter, now);
    chair.key(crate::cli::chair::keys::Key::Enter, now);
    let action = chair.pending.drain().pop().expect("a ruling waits");
    let (to_write, gone) = mpsc::channel::<ChairAction>();
    drop(gone);

    let commands = hand_on(&mut chair, &to_write, action);

    assert_eq!(commands, [Command::ReadNow(0)]);
    let words = chair.note.as_ref().map(|note| note.words.clone());
    assert_eq!(
        words.as_deref(),
        Some(
            "#285 was not ruled: the chair's writer has stopped: nothing was written; the \
             chair's log says why"
        )
    );
    assert!(chair.hidden.is_empty(), "the question is back in the inbox");
}

#[test]
fn a_hung_fix_holds_neither_the_writer_nor_the_close() {
    let (writes, release) = troubled();
    let (updates, heard) = mpsc::channel();
    let (to_write, writer) = start_writer(writes.clone(), updates.clone());

    start_fix(writes.clone(), fix_action(), updates);
    to_write.send(ruling(285)).expect("listens");
    let ruled = heard
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the ruling is written while the fix hangs");
    assert!(
        matches!(ruled, Update::Written(ChairAction::Rule(_), _)),
        "{ruled:?}"
    );

    let started = std::time::Instant::now();
    let said = close(to_write, writer, &heard, &[], writes.as_ref());
    assert!(said.is_empty(), "{said:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "closing does not wait for the fix"
    );
    let _ = release.send(());
}
