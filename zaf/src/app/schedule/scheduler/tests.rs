use std::collections::BTreeMap;

use super::*;

/// A world in memory: a clock that moves a second each pause, requests that arrive at given
/// turns, a binary that changes when told, signals at given turns, and a log of every call.
struct Fake {
    started: Instant,
    elapsed: Duration,
    /// The turn, counted by the looks at the requests.
    turn: usize,
    /// Who asks at each turn.
    asks: BTreeMap<usize, Vec<String>>,
    /// The turns at whose top a stop signal has arrived (the first that holds ends the loop).
    stop_at: usize,
    /// A signal that arrives during this tick number.
    stop_during_tick: Option<u64>,
    stopped: bool,
    /// What each look at the binary finds, in turn; nothing once used up.
    binary: Vec<Found>,
    /// The binary becomes new during this tick number.
    new_binary_during_tick: Option<u64>,
    /// How many looks find `spawn.lock` held.
    held: usize,
    /// The exit code of each tick, by number; 0 otherwise.
    codes: BTreeMap<u64, u8>,
    /// How long each tick takes.
    tick_takes: Duration,
    in_tick: bool,
    /// The stop signals are caught.
    caught: bool,
    /// What the request file holds when the loop begins, or why it cannot be used.
    left: Result<Vec<String>, String>,
    /// The stop signals are held back.
    holding: bool,
    /// A signal arrives during the restart (which fails here), after the last look for one.
    stop_during_restart: bool,
    /// A signal held back, pending until it is let through.
    pending_signal: bool,
    /// A signal arrives while the binary is looked at.
    stop_at_binary: bool,
    /// The calls made before the first turn.
    startup: Vec<&'static str>,
    /// Every epic of the version is done.
    built: bool,
    /// What each run of the fake action `release-open` ends in, in turn; the last one repeats.
    release_open: Vec<Result<Opened, String>>,
    log: Vec<String>,
}

impl Fake {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            elapsed: Duration::ZERO,
            turn: 0,
            asks: BTreeMap::new(),
            stop_at: 50,
            stop_during_tick: None,
            stopped: false,
            binary: Vec::new(),
            new_binary_during_tick: None,
            held: 0,
            codes: BTreeMap::new(),
            tick_takes: Duration::ZERO,
            in_tick: false,
            caught: false,
            left: Ok(Vec::new()),
            holding: false,
            stop_during_restart: false,
            pending_signal: false,
            stop_at_binary: false,
            startup: Vec::new(),
            built: false,
            release_open: Vec::new(),
            log: Vec::new(),
        }
    }

    fn ask_at(mut self, turn: usize, who: &[&str]) -> Self {
        self.asks
            .insert(turn, who.iter().map(|who| (*who).to_owned()).collect());
        self
    }

    fn ticks(&self) -> Vec<&str> {
        self.log
            .iter()
            .filter(|line| line.starts_with("tick "))
            .map(String::as_str)
            .collect()
    }
}

impl Host for Fake {
    fn catch_stop(&mut self) {
        self.startup.push("catch stop");
        self.caught = true;
    }

    fn left(&mut self) -> Result<Vec<String>, String> {
        assert!(self.caught, "requests are taken before a stop is caught");
        self.startup.push("left");
        self.left.clone()
    }

    fn up(&mut self) {
        self.startup.push("up");
    }

    fn now(&mut self) -> Instant {
        self.started + self.elapsed
    }

    fn stop(&mut self) -> Option<String> {
        assert!(!self.in_tick, "the signal is read during a tick");
        assert!(self.caught, "a stop is looked for before it is caught");
        (self.stopped || self.turn >= self.stop_at).then(|| "SIGTERM".to_owned())
    }

    fn hold_stop(&mut self) {
        self.log.push("hold stop".to_owned());
        self.holding = true;
    }

    fn let_stop_through(&mut self) {
        self.log.push("let stop through".to_owned());
        self.holding = false;
        if self.pending_signal {
            self.pending_signal = false;
            self.stopped = true;
        }
    }

    fn binary(&mut self) -> Found {
        assert!(!self.in_tick, "the binary is looked at during a tick");
        self.log.push("binary".to_owned());
        if self.stop_at_binary {
            self.stopped = true;
        }
        if self.binary.is_empty() {
            Found::Nothing
        } else {
            self.binary.remove(0)
        }
    }

    fn restart(&mut self) -> String {
        self.log.push("restart".to_owned());
        if self.stop_during_restart {
            // Held back, it waits; let through, it would reach a handler the exec wipes.
            assert!(self.holding, "a stop during the restart is not held back");
            self.pending_signal = true;
        }
        "cannot exec: a test".to_owned()
    }

    fn requests(&mut self) -> Vec<String> {
        self.turn += 1;
        self.asks.remove(&self.turn).unwrap_or_default()
    }

    fn held(&mut self) -> bool {
        if self.held > 0 {
            self.held -= 1;
            return true;
        }
        false
    }

    fn tick(&mut self, number: u64, asked: &Asked, memory: &mut Memory) -> u8 {
        self.in_tick = true;
        self.log.push(format!("tick {number}: {}", asked.line()));
        self.elapsed += self.tick_takes;
        if self.new_binary_during_tick == Some(number) {
            self.binary.push(Found::Restart("zaf 9.9.9".to_owned()));
        }
        if self.stop_during_tick == Some(number) {
            self.stopped = true;
        }
        memory.keep(Placement::default(), SystemTime::UNIX_EPOCH);
        self.in_tick = false;
        self.codes.get(&number).copied().unwrap_or(0)
    }

    fn version_built(&mut self) -> bool {
        self.built
    }

    fn open_release(&mut self) -> Result<Opened, String> {
        self.log.push("release-open".to_owned());
        if self.release_open.len() > 1 {
            self.release_open.remove(0)
        } else {
            self.release_open.first().cloned().expect("a fake result")
        }
    }

    fn built(&mut self, version: &Version, number: PrNumber, link: Option<&str>) {
        self.log.push(built_line(version, number, link));
    }

    fn next_tick(&mut self, interval: Duration) {
        self.log
            .push(format!("next tick in {}s", interval.as_secs()));
    }

    fn waits_for_lock(&mut self) {
        self.log.push("waits for spawn.lock".to_owned());
    }

    fn pause(&mut self) {
        self.elapsed += Duration::from_secs(1);
    }

    fn say(&mut self, line: &str) {
        self.log.push(line.to_owned());
    }

    fn warn(&mut self, line: &str) {
        self.log.push(format!("warn: {line}"));
    }
}

const MINUTE: Duration = Duration::from_secs(60);

fn start() -> Vec<String> {
    vec!["start".to_owned()]
}

#[test]
fn the_first_tick_runs_at_once_for_who_asked_first() {
    let mut fake = Fake::new();
    fake.stop_at = 3;

    let ended = run(
        &mut fake,
        MINUTE,
        vec!["restart".to_owned(), "ruling #45".to_owned()],
    );

    assert_eq!(
        ended,
        Ended::Stopped {
            signal: "SIGTERM".to_owned(),
            unserved: Vec::new()
        }
    );
    assert_eq!(fake.ticks(), ["tick 1: asked by: restart, ruling #45"]);
    assert_eq!(
        fake.log[0], "tick 1: asked by: restart, ruling #45",
        "who asked first is served before the binary is looked at"
    );
    assert_eq!(fake.log[2], "binary", "then the binary is");
}

#[test]
fn an_event_asks_and_the_tick_does_not_wait_for_the_timer() {
    // The timer is an hour away; a request comes at the fifth turn.
    let mut fake = Fake::new().ask_at(5, &["phase-end implement #7"]);
    fake.stop_at = 8;

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(
        fake.ticks(),
        [
            "tick 1: asked by: start",
            "tick 2: asked by: phase-end implement #7"
        ]
    );
    // Asked at the fifth look, after four pauses: about four seconds, not an hour.
    assert!(fake.elapsed < Duration::from_secs(10), "{:?}", fake.elapsed);
}

#[test]
fn several_requests_make_one_tick_that_names_them() {
    // Two requests in one look and one more, a repeat, in the same look: one tick for all.
    let mut fake = Fake::new().ask_at(
        3,
        &[
            "phase-end implement #7",
            "ruling #45",
            "phase-end implement #7",
        ],
    );
    fake.stop_at = 6;

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(
        fake.ticks(),
        [
            "tick 1: asked by: start",
            "tick 2: asked by: phase-end implement #7, ruling #45"
        ]
    );
}

#[test]
fn each_look_that_finds_a_request_makes_one_tick() {
    // One request at the first look joins the first tick; one at the next look makes the second.
    let mut fake = Fake::new()
        .ask_at(1, &["sign #1"])
        .ask_at(2, &["claimed #7"]);
    fake.stop_at = 4;

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(
        fake.ticks(),
        [
            "tick 1: asked by: start, sign #1",
            "tick 2: asked by: claimed #7"
        ]
    );
}

#[test]
fn the_timer_asks_once_the_interval_has_passed_since_the_last_tick() {
    let mut fake = Fake::new();
    fake.stop_at = 130;

    run(&mut fake, MINUTE, start());

    assert_eq!(
        fake.ticks(),
        [
            "tick 1: asked by: start",
            "tick 2: asked by: timer",
            "tick 3: asked by: timer"
        ]
    );
    assert!(fake.log.contains(&"next tick in 60s".to_owned()));
}

#[test]
fn the_timer_counts_from_the_end_of_the_last_tick() {
    // A request at the tenth turn moves the timer: the next timer tick is a minute after it.
    let mut fake = Fake::new().ask_at(10, &["ruling #45"]);
    fake.stop_at = 69;

    run(&mut fake, MINUTE, start());

    assert_eq!(
        fake.ticks(),
        ["tick 1: asked by: start", "tick 2: asked by: ruling #45"]
    );
}

#[test]
fn a_failed_tick_is_named_and_the_loop_goes_on() {
    let mut fake = Fake::new().ask_at(3, &["ruling #45"]);
    fake.codes.insert(1, 2);
    fake.stop_at = 5;

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(fake.ticks().len(), 2);
    assert!(
        fake.log
            .contains(&"warn: tick 1 failed with exit 2; the watch goes on".to_owned()),
        "{:?}",
        fake.log
    );
}

#[test]
fn the_loop_restarts_only_between_ticks() {
    // The binary changes while the first tick runs, which takes a while: the restart waits for
    // the tick to end, and comes at the next top of the loop, before anything else.
    let mut fake = Fake::new();
    fake.tick_takes = Duration::from_secs(30);
    fake.new_binary_during_tick = Some(1);
    fake.stop_at = 3;

    run(&mut fake, MINUTE * 60, start());

    let at = |line: &str| {
        fake.log
            .iter()
            .position(|logged| logged == line)
            .unwrap_or_else(|| panic!("no {line}: {:?}", fake.log))
    };
    let ended = at("next tick in 3600s");
    assert!(at("tick 1: asked by: start") < ended);
    assert!(at("restarting as zaf 9.9.9") > ended, "{:?}", fake.log);
    assert_eq!(at("restart"), at("restarting as zaf 9.9.9") + 1);
    // The stop signals are held back from before the last look for one until the exec failed.
    assert_eq!(at("hold stop") + 1, at("restarting as zaf 9.9.9"));
    assert_eq!(at("let stop through"), at("restart") + 1);
    // The exec failed here, so the old image goes on and says why.
    assert!(
        fake.log
            .contains(&"warn: cannot exec: a test; this image goes on".to_owned()),
        "{:?}",
        fake.log
    );
}

#[test]
fn a_binary_that_does_not_run_is_named_and_the_loop_goes_on() {
    let mut fake = Fake::new().ask_at(4, &["ruling #45"]);
    fake.binary = vec![
        Found::Nothing,
        Found::Refused("the binary at /x does not run".to_owned()),
    ];
    fake.stop_at = 6;

    run(&mut fake, MINUTE * 60, start());

    assert!(!fake.log.contains(&"restart".to_owned()), "{:?}", fake.log);
    assert!(
        fake.log
            .contains(&"warn: the binary at /x does not run".to_owned())
    );
    assert_eq!(fake.ticks().len(), 2);
}

#[test]
fn a_signal_during_a_tick_ends_the_loop_once_the_tick_has_ended() {
    let mut fake = Fake::new().ask_at(2, &["ruling #45"]);
    fake.stop_during_tick = Some(1);

    let ended = run(&mut fake, MINUTE * 60, start());

    assert_eq!(
        ended,
        Ended::Stopped {
            signal: "SIGTERM".to_owned(),
            unserved: Vec::new()
        }
    );
    assert_eq!(fake.ticks(), ["tick 1: asked by: start"]);
    assert_eq!(
        fake.log.last().map(String::as_str),
        Some("next tick in 3600s")
    );
}

#[test]
fn a_held_spawn_lock_holds_the_tick_and_keeps_who_asked() {
    let mut fake = Fake::new().ask_at(2, &["ruling #45"]);
    fake.held = 3;
    fake.stop_at = 8;

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(fake.ticks(), ["tick 1: asked by: start, ruling #45"]);
    let waits = fake
        .log
        .iter()
        .filter(|line| *line == "waits for spawn.lock")
        .count();
    assert_eq!(waits, 1, "said once: {:?}", fake.log);
}

#[test]
fn the_asked_line_names_each_once_and_counts_the_rest() {
    let asked = Asked::new(
        ["a", "b", "a", "c", "d", "e", "f", "g", "h", "i", "j"]
            .into_iter()
            .map(str::to_owned),
    );

    assert_eq!(asked.who().len(), 10);
    assert_eq!(asked.line(), "asked by: a, b, c, d, e, f, g, h, and 2 more");
    assert_eq!(Asked::new(["timer".to_owned()]).line(), "asked by: timer");
}

#[test]
fn memory_keeps_the_last_placement_that_was_read() {
    let mut memory = Memory::default();
    assert_eq!(memory.placement(), None);
    assert_eq!(memory.gathered_at(), None);

    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
    memory.keep(Placement::default(), at);

    assert_eq!(memory.placement(), Some(&Placement::default()));
    assert_eq!(memory.gathered_at(), Some(at));
}

#[test]
fn a_binary_is_not_restarted_into_while_a_request_waits_for_its_tick() {
    // A request is taken while `spawn.lock` is held for three looks, and a new binary arrives
    // meanwhile: the restart waits for that request's tick.
    let mut fake = Fake::new().ask_at(1, &["ruling #45"]);
    fake.binary = vec![Found::Nothing, Found::Restart("zaf 9.9.9".to_owned())];
    fake.held = 3;
    fake.stop_at = 8;

    run(&mut fake, MINUTE * 60, Vec::new());

    let at = |line: &str| {
        fake.log
            .iter()
            .position(|logged| logged.starts_with(line))
            .unwrap_or_else(|| panic!("no {line}: {:?}", fake.log))
    };
    assert!(
        at("tick 1: asked by: ruling #45") < at("restarting as zaf 9.9.9"),
        "{:?}",
        fake.log
    );
}

#[test]
fn a_stop_while_the_lock_is_held_hands_back_who_was_not_served() {
    let mut fake = Fake::new().ask_at(2, &["ruling #45"]);
    fake.held = 100;
    fake.stop_at = 4;

    let ended = run(&mut fake, MINUTE * 60, Vec::new());

    assert_eq!(
        ended,
        Ended::Stopped {
            signal: "SIGTERM".to_owned(),
            unserved: vec!["ruling #45".to_owned()]
        }
    );
    assert!(fake.ticks().is_empty());
}

#[test]
fn a_stop_at_startup_hands_back_the_requests_left_in_the_file() {
    let mut fake = Fake::new();
    fake.left = Ok(vec!["ruling #45".to_owned()]);
    // The signal is there at the first look.
    fake.stop_at = 0;

    let ended = run(&mut fake, MINUTE * 60, vec!["restart".to_owned()]);

    // Caught before anything was taken from the file, so what was taken is handed back.
    assert_eq!(fake.startup, ["catch stop", "left", "up"]);
    assert_eq!(
        ended,
        Ended::Stopped {
            signal: "SIGTERM".to_owned(),
            unserved: vec!["restart".to_owned(), "ruling #45".to_owned()]
        }
    );
    assert!(fake.ticks().is_empty());
}

#[test]
fn a_request_file_that_cannot_be_used_at_the_start_ends_the_loop_before_any_tick() {
    let mut fake = Fake::new();
    fake.left = Err("cannot read .zaf/tick.request: a test".to_owned());

    let ended = run(&mut fake, MINUTE * 60, start());

    assert_eq!(
        ended,
        Ended::Failed("cannot read .zaf/tick.request: a test".to_owned())
    );
    assert_eq!(fake.startup, ["catch stop", "left"]);
    assert!(fake.log.is_empty(), "{:?}", fake.log);
}

#[test]
fn a_stop_that_lands_during_a_failed_restart_ends_the_loop_at_its_next_look() {
    let mut fake = Fake::new();
    fake.binary = vec![Found::Restart("zaf 9.9.9".to_owned())];
    fake.stop_during_restart = true;
    // A request at the very next look, which the stop must come before: it is not taken.
    fake = fake.ask_at(1, &["ruling #45"]);

    let ended = run(&mut fake, MINUTE * 60, Vec::new());

    assert_eq!(
        ended,
        Ended::Stopped {
            signal: "SIGTERM".to_owned(),
            unserved: Vec::new()
        }
    );
    assert!(fake.ticks().is_empty(), "{:?}", fake.log);
    assert_eq!(fake.turn, 0, "no request is taken after the stop");
    assert_eq!(
        fake.log,
        [
            "binary",
            "hold stop",
            "restarting as zaf 9.9.9",
            "restart",
            "let stop through",
            "warn: cannot exec: a test; this image goes on"
        ]
    );
}

#[test]
fn a_stop_before_the_last_look_stops_rather_than_restarts() {
    let mut fake = Fake::new();
    fake.binary = vec![Found::Restart("zaf 9.9.9".to_owned())];
    // It comes while the new binary answers `--version`: there at the last look.
    fake.stop_at_binary = true;

    let ended = run(&mut fake, MINUTE * 60, Vec::new());

    assert_eq!(
        ended,
        Ended::Stopped {
            signal: "SIGTERM".to_owned(),
            unserved: Vec::new()
        }
    );
    assert_eq!(fake.log, ["binary", "hold stop", "let stop through"]);
}

fn pull_request() -> Opened {
    Opened {
        version: Version::new(0, 5, 2),
        number: PrNumber::from(12),
        link: Some("https://forge.example/zuri/ways/pulls/12".to_owned()),
    }
}

/// A fake with three ticks (the first, and two asked for) and a stop after them.
fn three_ticks() -> Fake {
    let mut fake = Fake::new()
        .ask_at(3, &["ruling #1"])
        .ask_at(5, &["ruling #2"]);
    fake.stop_at = 8;
    fake
}

fn count(fake: &Fake, line: &str) -> usize {
    fake.log.iter().filter(|each| each.as_str() == line).count()
}

#[test]
fn built_version_opens_release_once() {
    let mut fake = three_ticks();
    fake.built = true;
    fake.release_open = vec![Ok(pull_request())];

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(fake.ticks().len(), 3);
    assert_eq!(count(&fake, "release-open"), 1, "{:?}", fake.log);
    let says =
        "zaf built 0.5.2: release pull request #12, https://forge.example/zuri/ways/pulls/12";
    assert_eq!(count(&fake, says), 1, "said once: {:?}", fake.log);
    let opened = fake.log.iter().position(|line| line == "release-open");
    let first_next = fake
        .log
        .iter()
        .position(|line| line.starts_with("next tick"));
    assert!(opened < first_next, "after the first tick, before the wait");
}

#[test]
fn open_epic_opens_nothing() {
    let mut fake = three_ticks();
    fake.built = false;
    fake.release_open = vec![Ok(pull_request())];

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(fake.ticks().len(), 3);
    assert_eq!(count(&fake, "release-open"), 0, "{:?}", fake.log);
    assert!(
        fake.log.iter().all(|line| !line.starts_with("zaf built")),
        "{:?}",
        fake.log
    );
}

#[test]
fn release_open_failure_retries_next_tick() {
    let mut fake = three_ticks();
    fake.built = true;
    fake.release_open = vec![
        Err("the branch is not on the remote".to_owned()),
        Err("FAIL release-open".to_owned()),
        Ok(pull_request()),
    ];

    run(&mut fake, MINUTE * 60, start());

    assert_eq!(fake.ticks().len(), 3);
    assert_eq!(
        count(&fake, "release-open"),
        3,
        "tried on each tick, then done"
    );
    let warnings: Vec<&String> = fake
        .log
        .iter()
        .filter(|line| line.starts_with("warn: the version is built"))
        .collect();
    assert_eq!(warnings.len(), 2, "one for each failure: {:?}", fake.log);
    assert!(warnings[0].contains("the branch is not on the remote"));
    assert!(warnings[1].contains("FAIL release-open"));
    let said = fake
        .log
        .iter()
        .filter(|line| line.starts_with("zaf built"))
        .count();
    assert_eq!(said, 1);
}
