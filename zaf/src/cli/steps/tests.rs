use super::*;

/// A writer whose bytes the test reads back.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn screen(live: bool, colour: bool) -> Screen {
    Screen {
        colour,
        live,
        rows: 24,
        columns: 80,
    }
}

fn rows() -> Vec<PlanRow> {
    vec![PlanRow {
        verb: "Write".into(),
        noun: ".zaf.toml".into(),
        why: "so zaf remembers".into(),
    }]
}

fn run(screen: Screen, end: StepEnd) -> String {
    let out = Shared::default();
    let mut steps = ScreenSteps::new(screen, rows(), out.clone());
    steps.started(0);
    steps.ended(0, &end);
    drop(steps);
    out.text()
}

#[test]
fn a_plain_screen_prints_only_the_final_line() {
    let text = run(screen(false, false), StepEnd::Done(".zaf.toml".into()));
    assert_eq!(text, "  ✓ Write .zaf.toml\n");
    assert!(!text.contains('\r') && !text.contains('\x1b'));
}

#[test]
fn a_failed_step_ends_with_a_cross() {
    let text = run(screen(false, false), StepEnd::Failed("no folder".into()));
    assert_eq!(text, "  ✗ Write no folder\n");
    assert!(!text.contains('\r') && !text.contains('\x1b'));
}

#[test]
fn a_live_screen_spins_then_redraws_in_place() {
    let text = run(screen(true, false), StepEnd::Done(".zaf.toml".into()));
    assert!(FRAMES.iter().any(|frame| text.contains(frame)), "{text:?}");
    assert!(text.starts_with("\r\x1b[K  "), "{text:?}");
    assert!(text.ends_with("\r\x1b[K  ✓ Write .zaf.toml\n"), "{text:?}");
}

#[test]
fn dropping_mid_step_stops_the_spinner() {
    let out = Shared::default();
    let mut steps = ScreenSteps::new(screen(true, false), rows(), out.clone());
    steps.started(0);
    drop(steps);
    // The thread was joined: nothing writes after the drop.
    let after = out.text();
    thread::sleep(TICK * 3);
    assert_eq!(out.text(), after);
    assert!(after.ends_with(REDRAW), "{after:?}");
}
