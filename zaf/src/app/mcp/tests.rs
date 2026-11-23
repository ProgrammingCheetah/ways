use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::domain::{META_EXIT_KEY, RoleName};
use crate::fakes::FakeVerbRunner;
use crate::ports::VerbOutcome;

struct Refuses(&'static str);

impl LineChecker for Refuses {
    fn check(&self, _: &[String]) -> Result<CommandPath, String> {
        Err(self.0.to_owned())
    }
}

struct Accepts;

impl LineChecker for Accepts {
    fn check(&self, _: &[String]) -> Result<CommandPath, String> {
        Ok(CommandPath::new(["board", "ready"]))
    }
}

/// A service whose caller is outside and whose manifest forbids nothing.
fn outside_service<'a>(
    specs: Vec<ToolSpec>,
    checker: &'a dyn LineChecker,
    runner: &'a dyn VerbRunner,
    cwd: PathBuf,
    watch: &'a VersionWatch,
) -> ToolService<'a> {
    ToolService::new(
        specs,
        checker,
        runner,
        cwd,
        watch,
        Ok(McpCaller::Outside),
        ForbiddenSection::default(),
    )
}

/// A checker that reads a line's verb and its action as the path.
struct Parses;

impl LineChecker for Parses {
    fn check(&self, argv: &[String]) -> Result<CommandPath, String> {
        let words = argv
            .iter()
            .skip(1)
            .filter(|word| !word.starts_with('-'))
            .take(2);
        Ok(CommandPath::new(words.cloned()))
    }
}

fn verb(tool: &str, args: &[&str]) -> ToolCall {
    ToolCall {
        tool: tool.into(),
        args: args.iter().map(|word| (*word).to_owned()).collect(),
        ..ToolCall::default()
    }
}

fn instance_service<'a>(runner: &'a dyn VerbRunner, caller: CallerAnswer) -> ToolService<'a> {
    ToolService::new(
        vec![],
        &Parses,
        runner,
        PathBuf::from("/p"),
        watch(),
        caller,
        ForbiddenSection::new(["zaf forge merge"]),
    )
}

fn programmer() -> CallerAnswer {
    Ok(McpCaller::Instance(RoleName::from("programmer")))
}

#[test]
fn tool_service_refused_call_starts_no_child() {
    let runner = stamped("v1", "0.5.4");
    let service = instance_service(&runner, programmer());
    let sign = service.call(verb("epic", &["sign", "1"]), &|_| {}, &Cancel::new());
    assert_eq!(
        sign,
        ToolResult {
            text: "zaf epic sign is refused: Sign in zaf's chair.\nexit 1".into(),
            is_error: true,
            exit: 1,
        }
    );
    // A `[forbidden]` line is refused to an instance, in one line and `exit 1`.
    let merge = service.call(verb("forge", &["merge", "3"]), &|_| {}, &Cancel::new());
    assert_eq!((merge.exit, merge.is_error), (1, true));
    assert!(merge.text.starts_with("zaf forge merge is refused: "));
    assert_eq!(merge.text.lines().count(), 2);
    assert!(runner.runs().is_empty());
}

#[test]
fn tool_service_probe_refusal_refuses_every_call() {
    let runner = stamped("v1", "0.5.4");
    let reason = "ZAF_INSTANCE is not set, but x runs above this server";
    let service = instance_service(&runner, Err(reason.into()));
    for tool in ["board", "status", "epic"] {
        let result = service.call(verb(tool, &[]), &|_| {}, &Cancel::new());
        assert_eq!(
            result,
            ToolResult {
                text: format!("zaf's MCP refuses every call: {reason}\nexit 1"),
                is_error: true,
                exit: 1,
            }
        );
    }
    assert!(runner.runs().is_empty());
}

#[test]
fn mcp_begin_refused_with_chair_way_on() {
    let runner = stamped("v1", "0.5.4");
    let service = ToolService::new(
        vec![],
        &Parses,
        &runner,
        PathBuf::from("/p"),
        watch(),
        Ok(McpCaller::Outside),
        ForbiddenSection::default(),
    );
    let result = service.call(
        verb("begin", &["--version", "0.0.1"]),
        &|_| {},
        &Cancel::new(),
    );
    assert_eq!(
        result,
        ToolResult {
            text: format!(
                "zaf begin 0.0.1 is refused: {}\nexit 1",
                crate::app::release::WAY_ON
            ),
            is_error: true,
            exit: 1,
        }
    );
    assert!(result.text.contains(crate::app::release::WAY_ON));
    assert_eq!(result.text.lines().last(), Some("exit 1"));
    assert!(runner.runs().is_empty());
}

#[test]
fn mcp_begin_ready_not_refused() {
    let runner = FakeVerbRunner::default();
    runner.record(VerbOutcome {
        stdout: "ready written\n".into(),
        stderr: String::new(),
        exit: 0,
    });
    let service = ToolService::new(
        vec![],
        &Parses,
        &runner,
        PathBuf::from("/p"),
        watch(),
        Ok(McpCaller::Outside),
        ForbiddenSection::default(),
    );
    let result = service.call(verb("begin", &["ready"]), &|_| {}, &Cancel::new());
    assert_eq!(
        result,
        ToolResult {
            text: "ready written\n".into(),
            is_error: false,
            exit: 0,
        }
    );
    let runs = runner.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].argv, ["zaf", "begin", "ready"]);
}

#[test]
fn tool_service_allowed_call_runs() {
    let runner = FakeVerbRunner::default();
    runner.record(VerbOutcome {
        stdout: "ready\n".into(),
        stderr: String::new(),
        exit: 0,
    });
    let service = instance_service(&runner, programmer());
    let result = service.call(verb("board", &["ready"]), &|_| {}, &Cancel::new());
    assert_eq!(
        result,
        ToolResult {
            text: "ready\n".into(),
            is_error: false,
            exit: 0,
        }
    );
    let runs = runner.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].argv, ["zaf", "board", "ready"]);
}

fn spec(name: &str) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("the {name} verb"),
        input_schema: serde_json::json!({}),
    }
}

fn call() -> ToolCall {
    ToolCall {
        tool: "board".into(),
        args: vec!["ready".into()],
        stdin: Some("input".into()),
        json: true,
        more: true,
    }
}

#[test]
fn usage_error_runs_nothing() {
    let runner = FakeVerbRunner::default();
    let checker = Refuses("error: unrecognized subcommand");
    let service = outside_service(
        vec![],
        &checker,
        &runner,
        PathBuf::from("/project"),
        watch(),
    );
    let result = service.call(call(), &|_| {}, &Cancel::new());
    assert_eq!(
        result,
        ToolResult {
            text: "error: unrecognized subcommand\nexit 2".into(),
            is_error: true,
            exit: 2,
        }
    );
    assert!(runner.runs().is_empty());
}

#[test]
fn call_passes_argv_and_stdin() {
    let runner = FakeVerbRunner::default();
    runner.record(VerbOutcome {
        stdout: "out".into(),
        stderr: "err".into(),
        exit: 1,
    });
    let service = outside_service(
        vec![],
        &Accepts,
        &runner,
        PathBuf::from("/project"),
        watch(),
    );
    let result = service.call(call(), &|_| {}, &Cancel::new());
    assert_eq!(
        result,
        ToolResult {
            text: "out\nerr\nexit 1".into(),
            is_error: true,
            exit: 1,
        }
    );
    let runs = runner.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].argv, ["zaf", "--json", "--more", "board", "ready"]);
    assert_eq!(runs[0].stdin.as_deref(), Some("input"));
    assert_eq!(runs[0].cwd, PathBuf::from("/project"));
}

#[test]
fn flags_are_left_off_when_not_asked() {
    let runner = FakeVerbRunner::default();
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), watch());
    let plain = ToolCall {
        tool: "status".into(),
        ..ToolCall::default()
    };
    service.call(plain, &|_| {}, &Cancel::new());
    assert_eq!(runner.runs()[0].argv, ["zaf", "status"]);
}

#[test]
fn ticks_reach_the_caller() {
    let runner = FakeVerbRunner::default();
    runner.tick_with(Tick {
        elapsed: Duration::from_secs(60),
        last_stderr: Some("working".into()),
    });
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), watch());
    let seen = Mutex::new(Vec::new());
    service.call(
        call(),
        &|tick| seen.lock().expect("lock").push(tick),
        &Cancel::new(),
    );
    assert_eq!(seen.lock().expect("lock").len(), 1);
}

#[test]
fn list_returns_the_specs() {
    let runner = FakeVerbRunner::default();
    let specs = vec![spec("board"), spec("status")];
    let service = outside_service(
        specs.clone(),
        &Accepts,
        &runner,
        PathBuf::from("/p"),
        watch(),
    );
    assert_eq!(service.list(), specs);
}

fn shaped(stdout: &str, stderr: &str, exit: i32) -> ToolResult {
    let runner = FakeVerbRunner::default();
    runner.record(VerbOutcome {
        stdout: stdout.into(),
        stderr: stderr.into(),
        exit,
    });
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), watch());
    service.call(call(), &|_| {}, &Cancel::new())
}

#[test]
fn exit_zero_no_trailer() {
    let result = shaped("done\n", "", 0);
    assert_eq!(result.text, "done\n");
    assert!(!result.is_error);
}

#[test]
fn exit_one_trailer() {
    let result = shaped("not ready\n", "", 1);
    assert_eq!(result.text, "not ready\nexit 1");
    assert!(result.is_error);
}

#[test]
fn exit_two_trailer() {
    let result = shaped("", "backend missing\n", 2);
    assert_eq!(result.text, "backend missing\nexit 2");
    assert!(result.is_error);
}

#[test]
fn do_exit_125_trailer() {
    let result = shaped("", "NOT_RUN gate: no such action", 125);
    assert_eq!(result.text, "NOT_RUN gate: no such action\nexit 125");
    assert!(result.is_error);
}

#[test]
fn both_streams_in_order() {
    assert_eq!(shaped("out\n", "err\n", 0).text, "out\nerr\n");
    assert_eq!(shaped("out", "err", 0).text, "out\nerr");
}

#[test]
fn meta_exit_always() {
    assert_eq!(META_EXIT_KEY, "zaf/exit");
    for exit in [0, 1, 2, 64, 125, 3] {
        assert_eq!(shaped("x", "", exit).exit, exit);
    }
    let refused = outside_service(
        vec![],
        &Refuses("usage"),
        &FakeVerbRunner::default(),
        PathBuf::from("/p"),
        watch(),
    )
    .call(call(), &|_| {}, &Cancel::new());
    assert_eq!(refused.exit, USAGE_EXIT);
}

/// A watch of a server whose version is 0.5.4, leaked so a test's service can borrow it
/// without naming it.
fn watch() -> &'static VersionWatch {
    Box::leak(Box::new(VersionWatch::new("0.5.4")))
}

/// A runner whose binary has `stamp` and answers its first run (`--version`) with `reports`;
/// `answer` queues what the calls after it get.
fn stamped(stamp: &str, reports: &str) -> FakeVerbRunner {
    let runner = FakeVerbRunner::default();
    runner.stamp_with(Some(stamp));
    runner.record(VerbOutcome {
        stdout: format!("zaf {reports}\n"),
        stderr: String::new(),
        exit: 0,
    });
    runner
}

fn answer(runner: &FakeVerbRunner, stdout: &str, exit: i32) {
    runner.record(VerbOutcome {
        stdout: stdout.into(),
        stderr: String::new(),
        exit,
    });
}

const LINE: &str = "zaf was updated to 0.6.0; restart zaf's MCP (/mcp) for the new tool list";

#[test]
fn version_line_when_different() {
    let runner = stamped("a", "0.6.0");
    answer(&runner, "done\n", 0);
    let watch = watch();
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), watch);
    let result = service.call(call(), &|_| {}, &Cancel::new());
    assert_eq!(result.text, format!("done\n{LINE}"));
    assert!(!result.is_error);
    assert_eq!(runner.runs()[0].argv, ["zaf", "--version"]);

    // With a failing exit the line goes just before the trailer.
    let runner = stamped("a", "0.6.0");
    answer(&runner, "not ready\n", 1);
    let fresh = self::watch();
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), fresh);
    let result = service.call(call(), &|_| {}, &Cancel::new());
    assert_eq!(result.text, format!("not ready\n{LINE}\nexit 1"));
}

#[test]
fn no_line_when_same() {
    let runner = stamped("a", "0.5.4");
    answer(&runner, "done\n", 0);
    let watch = watch();
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), watch);
    let result = service.call(call(), &|_| {}, &Cancel::new());
    assert_eq!(result.text, "done\n");
}

#[test]
fn version_read_once_per_binary() {
    let runner = stamped("a", "0.6.0");
    answer(&runner, "one\n", 0);
    answer(&runner, "two\n", 0);
    let watch = watch();
    let service = outside_service(vec![], &Accepts, &runner, PathBuf::from("/p"), watch);
    assert!(
        service
            .call(call(), &|_| {}, &Cancel::new())
            .text
            .ends_with(LINE)
    );
    assert!(
        service
            .call(call(), &|_| {}, &Cancel::new())
            .text
            .ends_with(LINE)
    );
    let versions = |runner: &FakeVerbRunner| {
        runner
            .runs()
            .iter()
            .filter(|run| run.argv == ["zaf", "--version"])
            .count()
    };
    assert_eq!(versions(&runner), 1);

    // A changed binary is read again, once.
    runner.stamp_with(Some("b"));
    answer(&runner, "zaf 0.5.4\n", 0);
    answer(&runner, "three\n", 0);
    answer(&runner, "four\n", 0);
    assert_eq!(
        service.call(call(), &|_| {}, &Cancel::new()).text,
        "three\n"
    );
    assert_eq!(service.call(call(), &|_| {}, &Cancel::new()).text, "four\n");
    assert_eq!(versions(&runner), 2);
}
