//! The pack's `bug-report` action (design addendum D20, I10): called without its parameters or
//! its settings it says what is missing, exits 1 and files nothing. It runs here as a script with
//! the settings in its environment, as `zaf do` passes them, over the fake `curl` and `gh`, which
//! record every call. Outside the ways repository, where the pack is not beside the crate, nothing
//! runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

use common::{fixtures, path_with_zaf};

fn action() -> Option<PathBuf> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/teams/actions/bug-report");
    file.is_file().then_some(file)
}

/// Runs the action with `args` and the `ZAF_SETTING_*` of `settings`, giving the exit code, what it
/// printed and whether the fake `curl` or `gh` was called.
fn run(args: &[&str], settings: &[(&str, &str)]) -> Option<(i32, String, bool)> {
    let file = action()?;
    let temp = tempfile::tempdir().expect("temp dir");
    let (curl, gh) = (temp.path().join("curl"), temp.path().join("gh"));
    for scene in [&curl, &gh] {
        fs::create_dir_all(scene).expect("scene");
        fs::write(scene.join("rules"), "").expect("rules");
    }
    let fakes = [fixtures(), fixtures().join("forgejo")];
    let output = Command::new("sh")
        .arg(&file)
        .args(args)
        .env_clear()
        .env("PATH", path_with_zaf(&[&fakes[0], &fakes[1]]))
        .env("FAKE_CURL_DIR", &curl)
        .env("FAKE_GH_DIR", &gh)
        .envs(settings.iter().copied())
        .output()
        .expect("spawn sh");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let called = curl.join("calls").exists() || gh.join("calls").exists();
    Some((output.status.code().expect("an exit code"), text, called))
}

#[test]
fn no_parameters_lists_them() {
    let Some((code, text, called)) = run(&[], &[]) else {
        return;
    };
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("Please use the following parameters"),
        "{text}"
    );
    for parameter in ["--title", "--body", "--tags", "--verb", "--exit"] {
        assert!(text.contains(parameter), "{parameter}: {text}");
    }
    assert!(!called, "nothing was called");

    // A missing title or body, an unknown flag and a flag with no value are refused the same way.
    for args in [
        &["--title", "t"][..],
        &["--body", "b"],
        &["--title", "t", "--body", "b", "--nope", "x"],
        &["--title", "t", "--body"],
        &["--title", "t", "--body", "b", "--exit", "many"],
    ] {
        let (code, text, called) = run(args, &[]).expect("the action");
        assert_eq!(code, 1, "{args:?}: {text}");
        assert!(
            text.contains("Please use the following parameters"),
            "{args:?}: {text}"
        );
        assert!(!called, "{args:?}: nothing was called");
    }
}

#[test]
fn no_settings_names_the_table() {
    let Some((code, text, called)) = run(&["--title", "t", "--body", "b"], &[]) else {
        return;
    };
    assert_eq!(code, 1, "{text}");
    for key in ["kind", "url", "repo", "token_env"] {
        assert!(text.contains(key), "{key}: {text}");
    }
    assert!(text.contains("[actions.bug-report]"), "{text}");
    assert!(text.contains("~/.zaf/config.toml"), "{text}");
    assert!(!called, "nothing was called");

    // Only the settings that are unset are named as missing.
    let settings = [
        ("ZAF_SETTING_KIND", "forgejo-issues"),
        ("ZAF_SETTING_URL", "https://forge.example"),
    ];
    let (code, text, called) =
        run(&["--title", "t", "--body", "b"], &settings).expect("the action");
    assert_eq!(code, 1, "{text}");
    let first = text.lines().next().expect("a line");
    assert!(
        first.contains("repo") && first.contains("token_env"),
        "{text}"
    );
    assert!(!first.contains("kind") && !first.contains("url"), "{text}");
    assert!(!called, "nothing was called");
}

#[test]
fn unknown_kind_is_refused() {
    let settings = [
        ("ZAF_SETTING_KIND", "gitlab"),
        ("ZAF_SETTING_URL", "https://forge.example"),
        ("ZAF_SETTING_REPO", "example/bugs"),
        ("ZAF_SETTING_TOKEN_ENV", "BUG_TOKEN"),
    ];
    let Some((code, text, called)) = run(&["--title", "t", "--body", "b"], &settings) else {
        return;
    };
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("kind is wrong"), "{text}");
    assert!(text.contains("gitlab"), "{text}");
    assert!(text.contains("[actions.bug-report]"), "{text}");
    assert!(!called, "nothing was called");
}

// ---- Filing on a Forgejo place (I10, D20): the fake `curl` serves labels and an issue reply.

const TOKEN: &str = "s3cr3t-t0ken-value";
const LABELS: &str = r#"[{"id":7,"name":"crash","color":"ee0000"},{"id":8,"name":"type/task"},{"id":9,"name":"state/ready"}]"#;
const ISSUE: &str = r#"{"id":1,"url":"https://forge.example/api/v1/repos/example/bugs/issues/5","html_url":"https://forge.example/example/bugs/issues/5","number":5,"user":{"html_url":"https://forge.example/someone"}}"#;
const LINK: &str = "https://forge.example/example/bugs/issues/5";
const PLACE: &str = "https://forge.example/api/v1/repos/example/bugs";

const FORGEJO: [(&str, &str); 4] = [
    ("ZAF_SETTING_KIND", "forgejo-issues"),
    ("ZAF_SETTING_URL", "https://forge.example"),
    ("ZAF_SETTING_REPO", "example/bugs"),
    ("ZAF_SETTING_TOKEN_ENV", "BUG_TOKEN"),
];

/// One run of the action over a fake `curl` scene.
struct Filed {
    code: i32,
    stdout: String,
    text: String,
    scene: PathBuf,
    _temp: tempfile::TempDir,
}

impl Filed {
    /// The argv of every call the fake `curl` took, one string each (NULs joined by a space).
    fn arguments(&self) -> Vec<String> {
        self.calls(false)
    }

    /// What every call read on stdin: the config that holds the token's header and the body.
    fn configs(&self) -> Vec<String> {
        self.calls(true)
    }

    fn calls(&self, configs: bool) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.scene.join("calls")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.expect("entry").file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".config") == configs)
            .collect();
        names.sort();
        names
            .iter()
            .map(|name| {
                let bytes = fs::read(self.scene.join("calls").join(name)).expect("call");
                String::from_utf8_lossy(&bytes).replace('\0', " ")
            })
            .collect()
    }

    /// The JSON the action posted, read back out of the config the fake `curl` was handed.
    fn posts(&self) -> Vec<serde_json::Value> {
        let mut posts = Vec::new();
        for (call, config) in self.arguments().iter().zip(&self.configs()) {
            if !call.contains("--request POST") {
                continue;
            }
            let line = config
                .lines()
                .find_map(|line| line.strip_prefix("data-raw = \""))
                .expect("a data-raw line");
            let line = line.strip_suffix('"').expect("a closing quote");
            let mut json = String::new();
            let mut chars = line.chars();
            while let Some(ch) = chars.next() {
                if ch != '\\' {
                    json.push(ch);
                    continue;
                }
                match chars.next().expect("an escape") {
                    'n' => json.push('\n'),
                    'r' => json.push('\r'),
                    't' => json.push('\t'),
                    other => json.push(other),
                }
            }
            posts.push(serde_json::from_str(&json).expect("the posted JSON"));
        }
        posts
    }

    fn post(&self) -> serde_json::Value {
        let mut posts = self.posts();
        assert_eq!(posts.len(), 1, "one issue posted: {}", self.text);
        posts.remove(0)
    }
}

/// Files with `args` and `env` over a place that serves `LABELS` and `ISSUE`, or as `rules` say.
fn file(args: &[&str], env: &[(&str, &str)], rules: Option<&str>) -> Option<Filed> {
    let action = action()?;
    let temp = tempfile::tempdir().expect("temp dir");
    let scene = temp.path().join("curl");
    fs::create_dir_all(&scene).expect("scene");
    fs::write(scene.join("labels.json"), LABELS).expect("labels");
    fs::write(scene.join("issue.json"), ISSUE).expect("issue");
    let rules = rules.map_or_else(
        || format!("GET {PLACE}/labels*\t200\tlabels.json\nPOST {PLACE}/issues\t201\tissue.json\n"),
        str::to_owned,
    );
    fs::write(scene.join("rules"), rules).expect("rules");
    let gh = temp.path().join("gh");
    fs::create_dir_all(&gh).expect("gh scene");
    fs::write(gh.join("rules"), "").expect("rules");
    let fakes = [fixtures(), fixtures().join("forgejo")];
    let output = Command::new("sh")
        .arg(&action)
        .args(args)
        .env_clear()
        .env("PATH", path_with_zaf(&[&fakes[0], &fakes[1]]))
        .env("FAKE_CURL_DIR", &scene)
        .env("FAKE_GH_DIR", &gh)
        .envs(FORGEJO)
        .env("BUG_TOKEN", TOKEN)
        .envs(env.iter().copied())
        .output()
        .expect("spawn sh");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let text = format!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    Some(Filed {
        code: output.status.code().expect("an exit code"),
        stdout,
        text,
        scene,
        _temp: temp,
    })
}

fn labels_of(post: &serde_json::Value) -> Vec<i64> {
    post["labels"]
        .as_array()
        .expect("labels")
        .iter()
        .map(|id| id.as_i64().expect("an id"))
        .collect()
}

#[test]
fn forgejo_files_with_existing_labels() {
    let args = [
        "--title",
        "It broke",
        "--body",
        "Details",
        "--tags",
        "crash,update",
    ];
    let Some(filed) = file(&args, &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    assert_eq!(filed.stdout.trim(), LINK, "{}", filed.text);
    let post = filed.post();
    assert_eq!(labels_of(&post), [7], "only the tag the place has");
    assert_eq!(post["title"], "It broke");
    assert!(post["body"].as_str().unwrap().ends_with("Details"));
}

#[test]
fn body_carries_tags_and_context() {
    let version = Command::new(env!("CARGO_BIN_EXE_zaf"))
        .arg("--version")
        .output()
        .expect("zaf --version");
    let version = String::from_utf8_lossy(&version.stdout)
        .lines()
        .next()
        .expect("a version line")
        .to_owned();
    let uname = |flag: &str| {
        let out = Command::new("uname").arg(flag).output().expect("uname");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let args = [
        "--title",
        "t",
        "--body",
        "What happened",
        "--tags",
        "crash,update",
        "--verb",
        "zaf check",
        "--exit",
        "2",
    ];
    let Some(filed) = file(&args, &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    let post = filed.post();
    let body = post["body"].as_str().expect("a body");
    assert_eq!(body.lines().next(), Some("Tags: crash, update"), "{body}");
    for part in [
        version.as_str(),
        uname("-s").as_str(),
        uname("-m").as_str(),
        "zaf check",
        "Exit: 2",
        "Reported by: outside zaf",
        "What happened",
    ] {
        assert!(body.contains(part), "{part}: {body}");
    }

    // With ZAF_INSTANCE it is the role; with no tags the first line is `Tags:` alone.
    let args = ["--title", "t", "--body", "b"];
    let env = [("ZAF_INSTANCE", "programmer instance 3 on #1009")];
    let filed = file(&args, &env, None).expect("the action");
    assert_eq!(filed.code, 0, "{}", filed.text);
    let post = filed.post();
    let body = post["body"].as_str().expect("a body");
    assert_eq!(body.lines().next(), Some("Tags:"), "{body}");
    assert!(body.contains("Reported by: programmer\n"), "{body}");
    assert!(
        !body.contains("outside zaf") && !body.contains("Exit:"),
        "{body}"
    );
    assert_eq!(labels_of(&post), Vec::<i64>::new());
}

#[test]
fn home_folder_is_scrubbed() {
    let args = [
        "--title",
        "fails in /home/someone/project",
        "--body",
        "see /home/someone/x and /home/someone",
    ];
    let Some(filed) = file(&args, &[("HOME", "/home/someone")], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    let post = filed.post();
    assert_eq!(post["title"], "fails in ~/project");
    let body = post["body"].as_str().expect("a body");
    assert!(body.ends_with("see ~/x and ~"), "{body}");
    assert!(!body.contains("/home/someone"), "{body}");
}

#[test]
fn body_is_capped() {
    let long = "y".repeat(20_000);
    let Some(filed) = file(&["--title", "t", "--body", &long], &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    let post = filed.post();
    let count = post["body"].as_str().expect("a body").chars().count();
    assert!(count <= 8_000 && count > 7_000, "{count} characters");
}

#[test]
fn zaf_labels_are_never_set() {
    let args = [
        "--title",
        "t",
        "--body",
        "b",
        "--tags",
        "type/task,state/ready,crash",
    ];
    let Some(filed) = file(&args, &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    assert_eq!(labels_of(&filed.post()), [7], "{}", filed.text);
}

#[test]
fn token_stays_out_of_arguments_and_log() {
    let args = ["--title", "t", "--body", "b", "--tags", "crash"];
    let Some(filed) = file(&args, &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    let arguments = filed.arguments();
    assert!(arguments.len() >= 2, "labels and issue: {arguments:?}");
    for call in &arguments {
        assert!(!call.contains(TOKEN), "{call}");
    }
    assert!(!filed.text.contains(TOKEN), "{}", filed.text);
    // It travels as a header on curl's stdin.
    let configs = filed.configs();
    assert!(
        configs
            .iter()
            .all(|config| config.contains("Authorization")),
        "{configs:?}"
    );

    // A failed filing keeps it out of the log too.
    let rules = "GET */labels*\tcurl:7\tlabels.json\nPOST */issues\tcurl:7\tlabels.json\n";
    let failed = file(&["--title", "t", "--body", "b"], &[], Some(rules)).expect("the action");
    assert_eq!(failed.code, 2, "{}", failed.text);
    assert!(!failed.text.contains(TOKEN), "{}", failed.text);
    for call in failed.arguments() {
        assert!(!call.contains(TOKEN), "{call}");
    }
}

#[test]
fn missing_token_is_refused() {
    // The variable the settings name here is one that is unset.
    let env = [("ZAF_SETTING_TOKEN_ENV", "NO_SUCH_TOKEN")];
    let Some(filed) = file(&["--title", "t", "--body", "b"], &env, None) else {
        return;
    };
    assert_eq!(filed.code, 1, "{}", filed.text);
    assert!(filed.text.contains("NO_SUCH_TOKEN"), "{}", filed.text);
    assert!(filed.arguments().is_empty(), "nothing was called");

    // An empty variable is as good as none.
    let env = [("BUG_TOKEN", "")];
    let filed = file(&["--title", "t", "--body", "b"], &env, None).expect("the action");
    assert_eq!(filed.code, 1, "{}", filed.text);
    assert!(filed.text.contains("BUG_TOKEN"), "{}", filed.text);
    assert!(filed.arguments().is_empty(), "nothing was called");
}

#[test]
fn forge_failure_is_exit_2() {
    // The place is down.
    let down = "GET */labels*\tcurl:7\tlabels.json\nPOST */issues\tcurl:7\tlabels.json\n";
    let Some(filed) = file(&["--title", "t", "--body", "b"], &[], Some(down)) else {
        return;
    };
    assert_eq!(filed.code, 2, "{}", filed.text);
    assert!(filed.stdout.is_empty(), "{}", filed.text);

    // The place answers with an error, and with a reply that holds no link.
    for status in ["500", "201"] {
        let rules =
            format!("GET */labels*\t200\tlabels.json\nPOST */issues\t{status}\tlabels.json\n");
        let filed = file(&["--title", "t", "--body", "b"], &[], Some(&rules)).expect("the action");
        assert_eq!(filed.code, 2, "{status}: {}", filed.text);
        assert!(filed.text.contains(status), "{status}: {}", filed.text);
        assert!(filed.stdout.is_empty(), "{}", filed.text);
    }

    // A fake with no rule at all answers 599.
    let filed = file(&["--title", "t", "--body", "b"], &[], Some("")).expect("the action");
    assert_eq!(filed.code, 2, "{}", filed.text);
}

// ---- Filing on a GitHub place (I9, D20): the fake `gh` lists labels and creates the issue.

const GH_LINK: &str = "https://github.com/example/bugs/issues/9";

const GITHUB: [(&str, &str); 4] = [
    ("ZAF_SETTING_KIND", "github-issues"),
    ("ZAF_SETTING_URL", "https://github.com"),
    ("ZAF_SETTING_REPO", "example/bugs"),
    ("ZAF_SETTING_TOKEN_ENV", "BUG_TOKEN"),
];

/// One run of the action over a fake `gh` scene.
struct GhFiled {
    code: i32,
    stdout: String,
    text: String,
    scene: PathBuf,
    _temp: tempfile::TempDir,
}

impl GhFiled {
    /// The argv of every call, one string each (NULs joined by a space).
    fn calls(&self) -> Vec<String> {
        self.read("calls")
    }

    fn envs(&self) -> Vec<String> {
        self.read("env")
    }

    fn stdins(&self) -> Vec<String> {
        self.read("stdin")
    }

    fn read(&self, folder: &str) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.scene.join(folder)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.expect("entry").file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
            .iter()
            .map(|name| {
                let bytes = fs::read(self.scene.join(folder).join(name)).expect("record");
                String::from_utf8_lossy(&bytes).replace('\0', " ")
            })
            .collect()
    }

    /// The call whose argv holds `issue create`, as (argv, env, stdin).
    fn creation(&self) -> (String, String, String) {
        let at = self
            .calls()
            .iter()
            .position(|call| call.contains("issue create"))
            .unwrap_or_else(|| panic!("no issue created: {}", self.text));
        (
            self.calls()[at].clone(),
            self.envs()[at].clone(),
            self.stdins()[at].clone(),
        )
    }
}

/// Files with `args` on a GitHub place, `env` over the settings, with `rules` for the fake `gh` or
/// one that has the labels `crash`, `type/task` and `state/ready` and answers with `GH_LINK`.
fn file_gh(args: &[&str], env: &[(&str, &str)], rules: Option<&str>) -> Option<GhFiled> {
    let action = action()?;
    let temp = tempfile::tempdir().expect("temp dir");
    let scene = temp.path().join("gh");
    fs::create_dir_all(&scene).expect("scene");
    fs::write(scene.join("labels.txt"), "crash\ntype/task\nstate/ready\n").expect("labels");
    fs::write(scene.join("link.txt"), format!("{GH_LINK}\n")).expect("link");
    fs::write(scene.join("failure.txt"), "HTTP 502: bad gateway\n").expect("failure");
    let rules = rules.map_or_else(
        || "label list*\t0\tlabels.txt\nissue create*\t0\tlink.txt\n".to_owned(),
        str::to_owned,
    );
    fs::write(scene.join("rules"), rules).expect("rules");
    let curl = temp.path().join("curl");
    fs::create_dir_all(&curl).expect("curl scene");
    fs::write(curl.join("rules"), "").expect("rules");
    let fakes = [fixtures(), fixtures().join("forgejo")];
    let output = Command::new("sh")
        .arg(&action)
        .args(args)
        .env_clear()
        .env("PATH", path_with_zaf(&[&fakes[0], &fakes[1]]))
        .env("FAKE_CURL_DIR", &curl)
        .env("FAKE_GH_DIR", &scene)
        .env("FAKE_GH_STDIN", "1")
        .envs(GITHUB)
        .env("BUG_TOKEN", TOKEN)
        .envs(env.iter().copied())
        .output()
        .expect("spawn sh");
    assert!(!curl.join("calls").exists(), "a GitHub place calls no curl");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let text = format!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    Some(GhFiled {
        code: output.status.code().expect("an exit code"),
        stdout,
        text,
        scene,
        _temp: temp,
    })
}

#[test]
fn github_files_with_gh_and_labels() {
    let args = [
        "--title",
        "It broke",
        "--body",
        "Details",
        "--tags",
        "crash,update",
    ];
    let Some(filed) = file_gh(&args, &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    assert_eq!(filed.stdout.trim(), GH_LINK, "{}", filed.text);
    let calls = filed.calls();
    assert_eq!(calls.len(), 2, "labels, then the issue: {calls:?}");
    assert!(
        calls[0].starts_with("label list -R example/bugs"),
        "{calls:?}"
    );
    let (argv, _, stdin) = filed.creation();
    assert!(
        argv.starts_with("issue create -R example/bugs --label crash --title It broke"),
        "{argv}"
    );
    assert!(argv.contains("--body-file -"), "{argv}");
    assert_eq!(
        argv.matches("--label").count(),
        1,
        "only the tag the place has: {argv}"
    );
    // The body goes on stdin, composed as the Forgejo filing composes it, and not in the arguments.
    assert!(stdin.starts_with("Tags: crash, update\n"), "{stdin}");
    assert!(stdin.ends_with("Details"), "{stdin}");
    assert!(!argv.contains("Details"), "{argv}");
}

#[test]
fn gh_token_comes_from_token_env() {
    let args = ["--title", "t", "--body", "b"];
    // On github.com GH_TOKEN is the configured token, over every token the machine holds.
    let machine = [
        ("GH_TOKEN", "machine-login"),
        ("GITHUB_TOKEN", "machine-github"),
        ("GH_ENTERPRISE_TOKEN", "machine-enterprise"),
        ("GITHUB_ENTERPRISE_TOKEN", "machine-github-enterprise"),
        ("GH_HOST", "elsewhere.example"),
    ];
    let Some(filed) = file_gh(&args, &machine, None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    let (argv, env, _) = filed.creation();
    assert_eq!(
        env,
        format!(
            "GH_TOKEN={TOKEN}\nGITHUB_TOKEN unset\nGH_ENTERPRISE_TOKEN unset\nGITHUB_ENTERPRISE_TOKEN unset\nGH_HOST unset\n"
        )
    );
    assert!(!argv.contains(TOKEN), "{argv}");
    assert!(!filed.text.contains(TOKEN), "{}", filed.text);

    // On another host GH_HOST names it and GH_ENTERPRISE_TOKEN is the configured token.
    let mut env = machine.to_vec();
    env.push(("ZAF_SETTING_URL", "https://ghe.example.com:8443/some/path"));
    let filed = file_gh(&args, &env, None).expect("the action");
    assert_eq!(filed.code, 0, "{}", filed.text);
    assert!(!filed.envs().is_empty());
    for env in filed.envs() {
        assert_eq!(
            env,
            format!(
                "GH_TOKEN unset\nGITHUB_TOKEN unset\nGH_ENTERPRISE_TOKEN={TOKEN}\nGITHUB_ENTERPRISE_TOKEN unset\nGH_HOST=ghe.example.com:8443\n"
            )
        );
    }
    assert!(!filed.text.contains(TOKEN), "{}", filed.text);
}

#[test]
fn github_never_sets_zaf_labels() {
    let args = [
        "--title",
        "t",
        "--body",
        "b",
        "--tags",
        "type/task,state/ready,crash,type/bug",
    ];
    let Some(filed) = file_gh(&args, &[], None) else {
        return;
    };
    assert_eq!(filed.code, 0, "{}", filed.text);
    let (argv, _, _) = filed.creation();
    assert_eq!(argv.matches("--label").count(), 1, "{argv}");
    assert!(argv.contains("--label crash"), "{argv}");
    assert!(
        !argv.contains("type/") && !argv.contains("state/"),
        "{argv}"
    );
}

#[test]
fn github_missing_token_calls_nothing() {
    let args = ["--title", "t", "--body", "b", "--tags", "crash"];
    let env = [
        ("ZAF_SETTING_TOKEN_ENV", "NO_SUCH_TOKEN"),
        ("GH_TOKEN", "machine"),
    ];
    let Some(filed) = file_gh(&args, &env, None) else {
        return;
    };
    assert_eq!(filed.code, 1, "{}", filed.text);
    assert!(filed.text.contains("NO_SUCH_TOKEN"), "{}", filed.text);
    assert!(filed.calls().is_empty(), "nothing was called");

    let filed = file_gh(&args, &[("BUG_TOKEN", "")], None).expect("the action");
    assert_eq!(filed.code, 1, "{}", filed.text);
    assert!(filed.text.contains("BUG_TOKEN"), "{}", filed.text);
    assert!(filed.calls().is_empty(), "nothing was called");
}

#[test]
fn gh_failure_is_exit_2() {
    let args = ["--title", "t", "--body", "b", "--tags", "crash"];
    let rules = "label list*\t0\tlabels.txt\nissue create*\t1\tfailure.txt\n";
    let Some(filed) = file_gh(&args, &[], Some(rules)) else {
        return;
    };
    assert_eq!(filed.code, 2, "{}", filed.text);
    assert!(filed.stdout.is_empty(), "{}", filed.text);
    assert!(filed.text.contains("502"), "{}", filed.text);

    // A label list that fails files without labels; a reply with no link is a failure too.
    let rules = "label list*\t1\tfailure.txt\nissue create*\t0\tlink.txt\n";
    let filed = file_gh(&args, &[], Some(rules)).expect("the action");
    assert_eq!(filed.code, 0, "{}", filed.text);
    assert!(!filed.creation().0.contains("--label"), "{}", filed.text);
    let rules = "label list*\t0\tlabels.txt\nissue create*\t0\tlabels.txt\n";
    let filed = file_gh(&args, &[], Some(rules)).expect("the action");
    assert_eq!(filed.code, 2, "{}", filed.text);
}
