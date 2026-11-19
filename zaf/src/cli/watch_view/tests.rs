use super::*;

fn plain() -> Screen {
    Screen {
        colour: false,
        live: true,
        rows: 24,
        columns: 80,
    }
}

fn line(text: &str) -> Line {
    vec![(Meaning::Bold, text.into())]
}

#[test]
fn the_first_frame_clears_below_and_goes_up_over_nothing() {
    let (text, height) = redraw(&plain(), &[line("one"), line("two")], 0);

    assert_eq!(text, "\r\x1b[Jone\ntwo\n");
    assert_eq!(height, 2);
}

#[test]
fn the_next_frame_goes_up_over_the_last() {
    let (text, height) = redraw(&plain(), &[line("one")], 3);

    assert_eq!(text, "\x1b[3A\r\x1b[Jone\n");
    assert_eq!(height, 1);
}

#[test]
fn colour_paints_each_piece_of_a_line() {
    let screen = Screen {
        colour: true,
        ..plain()
    };
    let (text, _) = redraw(&screen, &[line("one")], 0);

    assert_eq!(text, "\r\x1b[J\x1b[1mone\x1b[0m\n");
}

#[test]
fn the_waiting_line_names_the_version() {
    let version = Version::new(0, 3, 0);
    let text: String = reading(&version, 1)
        .iter()
        .map(|(_, text)| text.as_str())
        .collect();

    assert_eq!(text, "zaf is building 0.3.0  ⠙  reading the board");
}

fn question(number: u64, text: &str) -> QuestionState {
    QuestionState {
        number,
        raised_on: Some(6),
        text: text.to_owned(),
        door: false,
        url: None,
    }
}

#[test]
fn an_answer_is_the_line_as_typed_without_its_ending() {
    assert_eq!(
        answer_of("  two  spaces `` \r\n"),
        Some("  two  spaces `` ")
    );
    assert_eq!(answer_of("yes"), Some("yes"));
}

#[test]
fn a_line_with_no_words_skips() {
    assert_eq!(answer_of("\n"), None);
    assert_eq!(answer_of(""), None);
    assert_eq!(answer_of("  \t \r\n"), None);
}

#[test]
fn the_block_shows_the_question_in_full_and_how_to_answer_elsewhere() {
    let block = question_block(
        &plain(),
        &question(23, "Which one?\nThe email, or the name?"),
    );

    assert_eq!(
        block,
        "\n  #23 asks: Which one?\n    The email, or the name?\n  \
         The rest keeps building while you think. Answer here, or from any terminal:\n  \
         zaf board ruling 23 'your answer'\n\n  Your answer: "
    );
}

#[test]
fn flat_still_cleans_questions() {
    assert_eq!(flat("A\x1b[2Jb\u{7}c\u{9b}d"), "A [2Jb c d");
    assert_eq!(flat("plain words"), "plain words");
}

#[test]
fn the_block_leaves_the_boards_control_characters_out() {
    let block = question_block(&plain(), &question(4, "A\x1b[2Jb\u{7}c"));

    assert!(!block.contains('\x1b'), "{block:?}");
    assert!(!block.contains('\u{7}'), "{block:?}");
    assert!(block.contains("A [2Jb c"), "{block:?}");
}

#[test]
fn the_lowest_open_question_is_asked_first_and_a_settled_one_never() {
    let state = ProjectState {
        project: "p".to_owned(),
        root: PathBuf::new(),
        missing: false,
        gathered_at: String::new(),
        reason: None,
        epics: Vec::new(),
        queue: Vec::new(),
        questions: vec![question(9, "b"), question(7, "a")],
        problems: Vec::new(),
        instances: Vec::new(),
        watch: None,
        last_tick: None,
    };
    let mut settled = BTreeSet::new();

    assert_eq!(next_question(&state, &settled).map(|q| q.number), Some(7));
    settled.insert(7);
    assert_eq!(next_question(&state, &settled).map(|q| q.number), Some(9));
    settled.insert(9);
    assert!(next_question(&state, &settled).is_none());
}

#[test]
fn the_end_screen_names_the_pull_request_and_the_next_release() {
    let end = End {
        version: Version::new(0, 1, 0),
        number: "30".to_owned(),
        next: Version::new(0, 2, 0),
    };
    let text: Vec<String> = end_lines(&end, 9, 23)
        .iter()
        .map(|line| line.iter().map(|(_, text)| text.as_str()).collect())
        .collect();

    assert_eq!(
        text,
        [
            "",
            "✓ zaf built 0.1.0: 9 tickets merged in 23 min.",
            "",
            "Next: merge pull request #30 on the forge to ship 0.1.0.",
            "Then start the next release: zaf begin --version 0.2.0",
            "",
        ]
    );
}

type Calls = Arc<std::sync::Mutex<Vec<String>>>;

/// A terminal stand-in that writes down what the restore path does to it, in order.
struct Recording(Calls);

impl Terminal for Recording {
    fn flush_output(&self) {
        self.0.lock().unwrap().push("flush_output".to_owned());
    }

    fn flush_typed(&self) {
        self.0.lock().unwrap().push("flush_typed".to_owned());
    }

    fn replace_fd(&self, fd: i32, _file: &File) {
        self.0.lock().unwrap().push(format!("replace_fd {fd}"));
    }

    fn write_terminal(&self, _to: &File, text: &str) {
        self.0.lock().unwrap().push(format!("write {text:?}"));
    }

    fn settle(&self, settings: &str) {
        self.0.lock().unwrap().push(format!("settle {settings}"));
    }
}

fn restore_on(calls: &Calls) -> Arc<Restore> {
    let null = || File::open("/dev/null").unwrap();
    Arc::new(Restore {
        terminal: Box::new(Recording(Arc::clone(calls))),
        stdout: null(),
        stderr: null(),
        settings: "saved".to_owned(),
        done: AtomicBool::new(false),
    })
}

#[test]
fn put_back_flushes_typed_input() {
    let calls = Calls::default();
    let restore = restore_on(&calls);

    restore.put_back();
    // Every way out may call it; the terminal is put back once.
    restore.put_back();

    let calls = calls.lock().unwrap();
    let at = |what: &str| calls.iter().position(|call| call == what).unwrap();
    assert_eq!(
        calls.iter().filter(|call| *call == "flush_typed").count(),
        1
    );
    assert!(at("flush_typed") < at("replace_fd 1"));
    assert!(at("flush_typed") < at("settle saved"));
    assert_eq!(calls.last().unwrap(), "settle saved");
}

#[test]
fn put_back_turns_bracketed_paste_off_once() {
    let calls = Calls::default();
    let restore = restore_on(&calls);

    restore.put_back();
    restore.put_back();

    let calls = calls.lock().unwrap();
    let off = format!("write {PASTE_OFF:?}");
    assert_eq!(calls.iter().filter(|call| **call == off).count(), 1);
    let at = |what: &str| calls.iter().position(|call| call == what).unwrap();
    assert!(at(&off) < at("settle saved"));
}

fn paste_of(bytes: &[u8]) -> (Typed, bool) {
    let cleared = std::cell::Cell::new(false);
    let mut input = std::io::Cursor::new(bytes.to_vec());
    let typed = read_typed(&mut input, || false, || cleared.set(true));
    (typed, cleared.get())
}

#[test]
fn a_paste_that_ends_inside_a_newline_is_refused() {
    // No final newline: the line discipline hands over the first line alone.
    let (typed, cleared) = paste_of(b"\x1b[200~line one\n");
    assert_eq!(typed, Typed::Many);
    assert!(cleared);
    // A final newline inside the markers.
    let (typed, cleared) = paste_of(b"\x1b[200~line one\nline two\n\x1b[201~\n");
    assert_eq!(typed, Typed::Many);
    assert!(cleared);
    // A carriage return stands for a newline too.
    let (typed, _) = paste_of(b"\x1b[200~a\rb\x1b[201~\n");
    assert_eq!(typed, Typed::Many);
}

#[test]
fn a_one_line_paste_is_the_line_without_its_markers() {
    let (typed, cleared) = paste_of(b"\x1b[200~one line\x1b[201~\n");
    assert_eq!(typed, Typed::Line("one line\n".to_owned()));
    assert!(!cleared);
    // Typed text after the paste stays; a marker in it does not.
    let (typed, _) = paste_of(b"\x1b[200~one\x1b[201~ two\x1b[201~\n");
    assert_eq!(typed, Typed::Line("one two\n".to_owned()));
}

#[test]
fn a_paste_with_input_queued_behind_it_is_refused() {
    let cleared = std::cell::Cell::new(false);
    let mut input = std::io::Cursor::new(b"\x1b[200~one\x1b[201~\n".to_vec());
    let typed = read_typed(&mut input, || true, || cleared.set(true));
    assert_eq!(typed, Typed::Many);
    assert!(cleared.get());
}

/// A view on a stand-in terminal, drawing to nowhere, over a folder with no project in it.
fn view_on(calls: &Calls) -> (WatchView, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let view = View {
        screen: plain(),
        drawing: File::create("/dev/null").unwrap(),
        root: dir.path().to_owned(),
        state_dir: dir.path().join(".zaf"),
        version: Version::new(0, 5, 2),
        started: Instant::now(),
    };
    let view = WatchView::spawn(restore_on(calls), view).unwrap();
    (view, dir)
}

/// One way the view is left, given the view and the end screen's text.
type Leave = fn(WatchView, End);

#[test]
fn every_way_out_restores_through_put_back() {
    let end = End {
        version: Version::new(0, 5, 2),
        number: "1".to_owned(),
        next: Version::new(0, 5, 3),
    };
    let ways: [(&str, Leave); 3] = [
        // No next question, and a Ctrl-C that ended the loop: the host ends the view.
        ("end", |view, _| {
            view.end();
        }),
        // The restart into a new zaf drops the view before the exec.
        ("drop", |view, _| drop(view)),
        // The version is built.
        ("finish", |view, end| view.finish(end)),
    ];
    for (way, leave) in ways {
        let calls = Calls::default();
        let (view, _dir) = view_on(&calls);

        leave(view, end.clone());

        let calls = calls.lock().unwrap();
        let typed = calls.iter().position(|call| call == "flush_typed");
        let settled = calls.iter().position(|call| call == "settle saved");
        assert!(
            typed.is_some() && settled.is_some() && typed < settled,
            "{way}: {calls:?}"
        );
    }
}

#[test]
fn multi_line_answer_is_refused_and_asked_again() {
    let cleared = std::cell::Cell::new(false);
    let mut input = std::io::Cursor::new(b"first\nsecond\n".to_vec());

    let typed = read_typed(&mut input, || true, || cleared.set(true));

    // Nothing is taken as an answer, and what was queued is thrown away.
    assert_eq!(typed, Typed::Many);
    assert!(cleared.get());
    let block = question_block(&plain(), &question(5, "Which one?"));
    let again = refused(&plain(), &block);
    assert!(
        again.contains("one line only; answer longer ones with `zaf board ruling`"),
        "{again:?}"
    );
    assert!(!again.contains("recorded"), "{again:?}");
    // The same question is shown again, ending on the prompt.
    assert!(again.ends_with(&block), "{again:?}");
}

#[test]
fn single_line_answer_is_recorded() {
    let cleared = std::cell::Cell::new(false);
    let mut input = std::io::Cursor::new(b"yes\n".to_vec());

    let typed = read_typed(&mut input, || false, || cleared.set(true));

    assert_eq!(typed, Typed::Line("yes\n".to_owned()));
    assert!(!cleared.get());
    let Typed::Line(line) = typed else { return };
    assert_eq!(answer_of(&line), Some("yes"));
}
