use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{
    Answered, Ask, Control, Controls, answer, asked, carry_out, log_name, start, stop, stop_target,
    stop_task, tick, versions, watch_args,
};
use crate::app::chair::{ChairState, ReleaseVersion, WatchView};
use crate::app::hub::controls::Failure;
use crate::cli::chair::keys::Key;
use crate::cli::chair::support::{root, ways};
use crate::domain::{EpicStage, TicketId};

fn version(text: &str) -> ReleaseVersion {
    ReleaseVersion::from(text)
}

/// Ways with no scheduler running.
fn idle() -> ChairState {
    let mut state = ways();
    state.now.watch = Some(WatchView {
        running: false,
        pid: None,
        version: None,
    });
    state
}

#[test]
fn the_version_to_build_is_the_one_whose_epics_are_signed_and_open() {
    let mut state = idle();
    assert_eq!(
        versions(&state),
        [version("0.5.2")],
        "two epics, one version"
    );

    state.epics[1].release = Some(version("0.5.10"));
    state.epics[0].release = Some(version("0.5.9"));
    assert_eq!(
        versions(&state),
        [version("0.5.9"), version("0.5.10")],
        "by number, not by letters"
    );

    state.epics[1].stage = EpicStage::Unsigned;
    state.epics[0].stage = EpicStage::Done;
    assert!(
        versions(&state).is_empty(),
        "unsigned and done epics build nothing"
    );

    state.epics[0].stage = EpicStage::Signed;
    state.epics[0].release = Some(version("--help"));
    assert!(
        versions(&state).is_empty(),
        "a version no command line may take is never offered"
    );
}

#[test]
fn b_asks_to_start_the_one_version_and_enter_says_yes() {
    let state = idle();
    let ask = start(&state).expect("something to ask");
    let control = Control::StartBuild {
        root: root(),
        version: version("0.5.2"),
    };
    assert_eq!(
        ask,
        Ask::Confirm {
            words: "Start the build of 0.5.2? It runs on after zaf closes.".to_owned(),
            control: control.clone(),
            yes: true,
        }
    );
    assert_eq!(answer(&ask, Key::Enter), Answered::Run(control.clone()));
    assert_eq!(answer(&ask, Key::Char('y')), Answered::Run(control));
    assert_eq!(
        answer(&ask, Key::Char('n')),
        Answered::Cancelled("the build was not started".to_owned())
    );
    assert_eq!(answer(&ask, Key::Char('j')), Answered::Waiting);
    assert_eq!(answer(&ask, Key::CtrlC), Answered::Quit);
}

#[test]
fn b_asks_which_version_when_several_are_signed() {
    let mut state = idle();
    state.epics[1].release = Some(version("0.5.3"));

    let ask = start(&state).expect("something to ask");
    assert_eq!(
        ask,
        Ask::Version {
            root: root(),
            versions: vec![version("0.5.2"), version("0.5.3")],
        }
    );
    assert_eq!(
        answer(&ask, Key::Char('3')),
        Answered::Waiting,
        "no third one"
    );
    let Answered::Ask(Ask::Confirm { control, .. }) = answer(&ask, Key::Char('2')) else {
        panic!("the second version is asked about");
    };
    assert_eq!(
        control,
        Control::StartBuild {
            root: root(),
            version: version("0.5.3"),
        }
    );
    assert_eq!(
        answer(&ask, Key::Esc),
        Answered::Cancelled("the build was not started".to_owned())
    );
}

#[test]
fn b_says_why_when_there_is_nothing_to_start() {
    assert_eq!(
        start(&ways()),
        Err("a scheduler already builds 0.5.2; B stops it after its tick".to_owned())
    );
    let mut state = idle();
    for epic in &mut state.epics {
        epic.stage = EpicStage::Unsigned;
    }
    assert_eq!(
        start(&state),
        Err("no signed epic is open: sign one, then b builds its version".to_owned())
    );
    state.now.watch = None;
    assert_eq!(
        start(&state),
        Err("the scheduler could not be read; r reads again".to_owned())
    );
}

#[test]
fn the_sigterm_target_is_the_pid_the_lock_names_while_it_runs() {
    let running = WatchView {
        running: true,
        pid: Some(4242),
        version: Some(version("0.5.2")),
    };
    assert_eq!(stop_target(Some(&running)), Some(4242));
    let gone = WatchView {
        running: false,
        ..running.clone()
    };
    assert_eq!(stop_target(Some(&gone)), None, "a lock nobody holds");
    assert_eq!(stop_target(None), None);
}

#[test]
fn shift_b_asks_to_stop_with_no_as_the_default() {
    let ask = stop(&ways()).expect("something to ask");
    let control = Control::StopBuild {
        root: root(),
        pid: 4242,
    };
    assert_eq!(
        ask,
        Ask::Confirm {
            words: "Stop the scheduler of 0.5.2 (pid 4242) after its tick?".to_owned(),
            control: control.clone(),
            yes: false,
        }
    );
    assert_eq!(
        answer(&ask, Key::Enter),
        Answered::Cancelled("the scheduler was not stopped".to_owned()),
        "Enter keeps it running"
    );
    assert_eq!(answer(&ask, Key::Char('Y')), Answered::Run(control));
    assert_eq!(
        stop(&idle()),
        Err("no scheduler runs; b starts one".to_owned())
    );
}

#[test]
fn t_asks_the_running_scheduler_for_a_tick() {
    let ask = tick(&ways()).expect("something to ask");
    assert_eq!(
        ask,
        Ask::Confirm {
            words: "Ask the scheduler of 0.5.2 for a tick now?".to_owned(),
            control: Control::Tick { root: root() },
            yes: true,
        }
    );
    assert_eq!(
        tick(&idle()),
        Err("no scheduler runs to tick; b starts one, and it ticks at once".to_owned())
    );
}

#[test]
fn x_stops_only_a_ticket_that_runs() {
    let state = ways();
    let ask = stop_task(&state, Some(TicketId::from(243))).expect("#243 runs");
    assert_eq!(
        ask,
        Ask::Confirm {
            words: "Stop #243? Its instance ends and its claim is freed.".to_owned(),
            control: Control::StopTask {
                root: root(),
                task: TicketId::from(243),
            },
            yes: false,
        }
    );
    assert_eq!(
        stop_task(&state, Some(TicketId::from(233))),
        Err("#233 is not running (waiting): nothing to stop".to_owned())
    );
    assert_eq!(
        stop_task(&state, None),
        Err("no ticket is selected".to_owned())
    );
    assert_eq!(asked('x', &state, Some(TicketId::from(243))), Some(Ok(ask)));
    assert_eq!(asked('q', &state, None), None);
}

/// Controls that record what they ran.
#[derive(Default)]
struct Recorder {
    ran: Mutex<Vec<String>>,
}

impl Recorder {
    fn note(&self, what: String) {
        self.ran.lock().expect("not poisoned").push(what);
    }
}

impl Controls for Recorder {
    fn start(&self, root: &Path, version: &ReleaseVersion) -> Result<String, Failure> {
        self.note(format!("start {} {version}", root.display()));
        Ok("started".to_owned())
    }

    fn stop(&self, _root: &Path, pid: u32) -> Result<String, Failure> {
        self.note(format!("stop {pid}"));
        Err(Failure::Refused("no scheduler runs now".to_owned()))
    }

    fn stop_task(&self, _root: &Path, task: TicketId) -> Result<String, Failure> {
        self.note(format!("stop {task}"));
        Err(Failure::Failed("the board did not answer".to_owned()))
    }

    fn tick(&self, _root: &Path) -> Result<String, Failure> {
        self.note("tick".to_owned());
        Ok("asked".to_owned())
    }
}

#[test]
fn a_control_runs_through_the_port_and_says_why_it_did_not() {
    let controls = Recorder::default();
    let root = PathBuf::from("/srv/ways");

    let started = carry_out(
        &controls,
        &Control::StartBuild {
            root: root.clone(),
            version: version("0.5.2"),
        },
    );
    assert_eq!(started, Ok("started".to_owned()));
    let stopped = carry_out(
        &controls,
        &Control::StopBuild {
            root: root.clone(),
            pid: 7,
        },
    );
    assert_eq!(
        stopped,
        Err("the scheduler was not stopped: no scheduler runs now".to_owned())
    );
    let task = carry_out(
        &controls,
        &Control::StopTask {
            root: root.clone(),
            task: TicketId::from(243),
        },
    );
    assert_eq!(
        task,
        Err("#243 was not stopped: the board did not answer".to_owned())
    );
    assert_eq!(
        carry_out(&controls, &Control::Tick { root }),
        Ok("asked".to_owned())
    );
    assert_eq!(
        *controls.ran.lock().expect("not poisoned"),
        ["start /srv/ways 0.5.2", "stop 7", "stop #243", "tick"]
    );
}

#[test]
fn the_scheduler_runs_as_the_verb_and_logs_under_a_stamped_name() {
    assert_eq!(
        watch_args(&version("0.5.2")),
        ["spawn", "--watch", "--version", "0.5.2"]
    );
    let at = humantime::parse_rfc3339("2026-11-19T03:15:00Z").expect("time");
    assert_eq!(
        log_name(at, &version("0.5.2")),
        "20261119T031500Z_spawn-watch-0.5.2.log"
    );
}
