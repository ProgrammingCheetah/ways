//! The pack's `bug-report-query` action (design addendum D20, I10): it lists the reports that carry
//! a tag, from a Forgejo place through `curl` or a GitHub one through `gh`. It runs here as a script
//! with the settings in its environment, as `zaf do` passes them, over the fake `curl` and `gh`,
//! which record every call. Outside the ways repository, where the pack is not beside the crate,
//! nothing runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

use common::{fixtures, path_with_zaf};

const TOKEN: &str = "s3cr3t-t0ken-value";

const FORGEJO: [(&str, &str); 4] = [
    ("ZAF_SETTING_KIND", "forgejo-issues"),
    ("ZAF_SETTING_URL", "https://forge.example"),
    ("ZAF_SETTING_REPO", "example/bugs"),
    ("ZAF_SETTING_TOKEN_ENV", "BUG_TOKEN"),
];

const GITHUB: [(&str, &str); 4] = [
    ("ZAF_SETTING_KIND", "github-issues"),
    ("ZAF_SETTING_URL", "https://github.com"),
    ("ZAF_SETTING_REPO", "example/bugs"),
    ("ZAF_SETTING_TOKEN_ENV", "BUG_TOKEN"),
];

/// Forgejo's issues: 1 is labelled `crash`, 2 is unlabelled with a `Tags:` line, 3 has neither (its
/// `Tags:` line holds `crashes`, a longer word), 4 is closed and labelled `crash`.
fn forgejo_issue(number: u32, title: &str, state: &str, body: &str, labels: &[&str]) -> String {
    let labels: Vec<String> = labels
        .iter()
        .enumerate()
        .map(|(id, name)| format!(r#"{{"id":{},"name":"{name}"}}"#, id + 1))
        .collect();
    format!(
        r#"{{"id":{},"number":{number},"title":"{title}","state":"{state}","body":"{body}","url":"https://forge.example/api/v1/repos/example/bugs/issues/{number}","html_url":"https://forge.example/example/bugs/issues/{number}","user":{{"login":"someone","html_url":"https://forge.example/someone"}},"labels":[{}]}}"#,
        number + 100,
        labels.join(",")
    )
}

fn forgejo_open() -> String {
    format!(
        "[{}]",
        [
            forgejo_issue(
                1,
                "Labelled crash",
                "open",
                "It broke\\n\\nzaf: 1",
                &["crash"]
            ),
            forgejo_issue(
                2,
                "Crash with \\\"quotes\\\" &amp; more",
                "open",
                "Tags: crash, update\\n\\nzaf: 1",
                &[],
            ),
            forgejo_issue(3, "Not it", "open", "Tags: crashes\\n\\nzaf: 1", &["other"]),
        ]
        .join(",")
    )
}

fn forgejo_all() -> String {
    format!(
        "[{},{}]",
        &forgejo_open()[1..forgejo_open().len() - 1],
        forgejo_issue(4, "Closed crash", "closed", "Gone", &["crash"])
    )
}

/// GitHub's: the same four, in the shape `gh issue list --json` prints.
fn github_issue(number: u32, title: &str, state: &str, body: &str, labels: &[&str]) -> String {
    let labels: Vec<String> = labels
        .iter()
        .map(|name| format!(r#"{{"id":"LA_{name}","name":"{name}"}}"#))
        .collect();
    format!(
        r#"{{"body":"{body}","labels":[{}],"number":{number},"state":"{state}","title":"{title}","url":"https://github.com/example/bugs/issues/{number}"}}"#,
        labels.join(",")
    )
}

fn github_open() -> String {
    format!(
        "[{}]",
        [
            github_issue(
                1,
                "Labelled crash",
                "OPEN",
                "It broke\\n\\nzaf: 1",
                &["crash"]
            ),
            github_issue(
                2,
                "Crash \\u0026 more",
                "OPEN",
                "Tags: crash, update\\n\\nzaf: 1",
                &[],
            ),
            github_issue(3, "Not it", "OPEN", "Tags: crashes\\n\\nzaf: 1", &["other"]),
        ]
        .join(",")
    )
}

fn github_all() -> String {
    format!(
        "[{},{}]",
        &github_open()[1..github_open().len() - 1],
        github_issue(4, "Closed crash", "CLOSED", "Gone", &["crash"])
    )
}

fn action() -> Option<PathBuf> {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/teams/actions/bug-report-query");
    file.is_file().then_some(file)
}

/// One run of the action.
struct Listed {
    code: i32,
    stdout: String,
    text: String,
    curl: PathBuf,
    gh: PathBuf,
    _temp: tempfile::TempDir,
}

impl Listed {
    /// The argv of every call in `scene`'s `calls` folder, one string each (NULs joined by a space).
    fn calls_of(scene: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(scene.join("calls")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.expect("entry").file_name().into_string().unwrap())
            .filter(|name| !name.ends_with(".config"))
            .collect();
        names.sort();
        names
            .iter()
            .map(|name| {
                let bytes = fs::read(scene.join("calls").join(name)).expect("call");
                String::from_utf8_lossy(&bytes).replace('\0', " ")
            })
            .collect()
    }

    fn curl_calls(&self) -> Vec<String> {
        Self::calls_of(&self.curl)
    }

    fn gh_calls(&self) -> Vec<String> {
        Self::calls_of(&self.gh)
    }

    /// What every `curl` call read on stdin: the config that holds the token's header.
    fn curl_configs(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.curl.join("calls")) else {
            return Vec::new();
        };
        entries
            .map(|entry| entry.expect("entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "config"))
            .map(|path| fs::read_to_string(path).expect("config"))
            .collect()
    }

    fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }
}

/// Runs the action with `args`, the `settings` and `env`, over a fake `curl` and a fake `gh` that
/// answer with the `curl_rules` and `gh_rules` (`name: contents` files in `files`).
fn list(
    args: &[&str],
    settings: &[(&str, &str)],
    env: &[(&str, &str)],
    curl_rules: &str,
    gh_rules: &str,
    files: &[(&str, String)],
) -> Option<Listed> {
    let action = action()?;
    let temp = tempfile::tempdir().expect("temp dir");
    let (curl, gh) = (temp.path().join("curl"), temp.path().join("gh"));
    for (scene, rules) in [(&curl, curl_rules), (&gh, gh_rules)] {
        fs::create_dir_all(scene).expect("scene");
        fs::write(scene.join("rules"), rules).expect("rules");
        for (name, contents) in files {
            fs::write(scene.join(name), contents).expect("file");
        }
    }
    fs::write(gh.join("failure.txt"), "HTTP 502: bad gateway\n").expect("failure");
    let fakes = [fixtures(), fixtures().join("forgejo")];
    let output = Command::new("sh")
        .arg(&action)
        .args(args)
        .env_clear()
        .env("PATH", path_with_zaf(&[&fakes[0], &fakes[1]]))
        .env("FAKE_CURL_DIR", &curl)
        .env("FAKE_GH_DIR", &gh)
        .envs(settings.iter().copied())
        .env("BUG_TOKEN", TOKEN)
        .envs(env.iter().copied())
        .output()
        .expect("spawn sh");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let text = format!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    Some(Listed {
        code: output.status.code().expect("an exit code"),
        stdout,
        text,
        curl,
        gh,
        _temp: temp,
    })
}

/// A Forgejo place that answers an open list and a list of all.
fn forgejo(args: &[&str]) -> Option<Listed> {
    let rules =
        "GET */issues?state=open*\t200\topen.json\nGET */issues?state=all*\t200\tall.json\n";
    list(
        args,
        &FORGEJO,
        &[],
        rules,
        "",
        &[("open.json", forgejo_open()), ("all.json", forgejo_all())],
    )
}

/// A GitHub place that answers an open list and a list of all.
fn github(args: &[&str]) -> Option<Listed> {
    let rules = "issue list*--state open*\t0\topen.json\nissue list*--state all*\t0\tall.json\n";
    list(
        args,
        &GITHUB,
        &[],
        "",
        rules,
        &[("open.json", github_open()), ("all.json", github_all())],
    )
}

const LINKS: &str = "https://forge.example/example/bugs/issues";
const GH_LINKS: &str = "https://github.com/example/bugs/issues";

#[test]
fn forgejo_lists_reports_by_tag() {
    let Some(listed) = forgejo(&["--tag", "crash"]) else {
        return;
    };
    assert_eq!(listed.code, 0, "{}", listed.text);
    assert_eq!(
        listed.lines(),
        [
            format!("#1 Labelled crash {LINKS}/1"),
            format!("#2 Crash with \"quotes\" &amp; more {LINKS}/2"),
        ],
        "{}",
        listed.text
    );
    // The open reports alone are asked for, from the place's issues, with the token as a header.
    let calls = listed.curl_calls();
    assert!(!calls.is_empty(), "the place was asked");
    for call in &calls {
        assert!(
            call.contains("https://forge.example/api/v1/repos/example/bugs/issues?state=open"),
            "{call}"
        );
    }
    assert!(listed.gh_calls().is_empty(), "a Forgejo place calls no gh");
    assert!(
        listed
            .curl_configs()
            .iter()
            .all(|config| config.contains(&format!("Authorization: token {TOKEN}"))),
        "{:?}",
        listed.curl_configs()
    );
}

#[test]
fn github_lists_reports_by_tag() {
    let Some(listed) = github(&["--tag", "crash"]) else {
        return;
    };
    assert_eq!(listed.code, 0, "{}", listed.text);
    assert_eq!(
        listed.lines(),
        [
            format!("#1 Labelled crash {GH_LINKS}/1"),
            format!("#2 Crash & more {GH_LINKS}/2"),
        ],
        "{}",
        listed.text
    );
    let calls = listed.gh_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        calls[0].starts_with("issue list -R example/bugs --state open"),
        "{calls:?}"
    );
    assert!(
        listed.curl_calls().is_empty(),
        "a GitHub place calls no curl"
    );
}

#[test]
fn all_adds_closed_reports() {
    for place in [forgejo, github] {
        let Some(open) = place(&["--tag", "crash"]) else {
            return;
        };
        assert_eq!(open.code, 0, "{}", open.text);
        assert_eq!(open.lines().len(), 2, "{}", open.text);
        assert!(!open.stdout.contains("Closed crash"), "{}", open.text);

        let all = place(&["--tag", "crash", "--all"]).expect("the action");
        assert_eq!(all.code, 0, "{}", all.text);
        assert_eq!(all.lines().len(), 3, "{}", all.text);
        assert!(
            all.lines()[2].starts_with("#4 Closed crash "),
            "{}",
            all.text
        );
        // The flag may come first.
        let first = place(&["--all", "--tag", "crash"]).expect("the action");
        assert_eq!(first.stdout, all.stdout);
    }

    // A place that lists the closed reports whatever it is asked for does not show them either.
    let rules = "GET */issues*\t200\tall.json\n";
    let listed = list(
        &["--tag", "crash"],
        &FORGEJO,
        &[],
        rules,
        "",
        &[("all.json", forgejo_all())],
    )
    .expect("the action");
    assert_eq!(listed.code, 0, "{}", listed.text);
    assert_eq!(listed.lines().len(), 2, "{}", listed.text);
    assert!(!listed.stdout.contains("Closed crash"), "{}", listed.text);
}

#[test]
fn tag_matches_whole_words() {
    for place in [forgejo, github] {
        // A part of a word, of a label and of a `Tags:` word matches nothing.
        for tag in ["cras", "crashe", "update,crash", "CRASH"] {
            let Some(listed) = place(&["--tag", tag]) else {
                return;
            };
            assert_eq!(listed.code, 0, "{tag}: {}", listed.text);
            assert!(listed.stdout.is_empty(), "{tag}: {}", listed.text);
        }
        // The longer word is a tag of its own.
        let listed = place(&["--tag", "crashes"]).expect("the action");
        assert_eq!(listed.lines().len(), 1, "{}", listed.text);
        assert!(listed.stdout.starts_with("#3 "), "{}", listed.text);
        // A word in the middle of the list matches; a `Tags:` line past the first does not.
        let listed = place(&["--tag", "update"]).expect("the action");
        assert_eq!(listed.lines().len(), 1, "{}", listed.text);
        assert!(listed.stdout.starts_with("#2 "), "{}", listed.text);
    }
}

#[test]
fn tags_line_counts_on_the_first_line_alone() {
    let later = forgejo_issue(9, "Later line", "open", "Intro\\nTags: crash", &[]);
    let crlf = forgejo_issue(
        8,
        "Windows ends",
        "open",
        "Tags:crash,update\\r\\nmore",
        &[],
    );
    let json = format!("[{later},{crlf}]");
    let rules = "GET */issues*\t200\topen.json\n";
    let Some(listed) = list(
        &["--tag", "crash"],
        &FORGEJO,
        &[],
        rules,
        "",
        &[("open.json", json)],
    ) else {
        return;
    };
    assert_eq!(listed.code, 0, "{}", listed.text);
    assert_eq!(
        listed.lines(),
        [format!("#8 Windows ends {LINKS}/8")],
        "{}",
        listed.text
    );
}

#[test]
fn forgejo_reads_every_page() {
    // A full first page asks for a second; a second that holds nothing new ends the reading.
    let page = |first: u32| {
        let issues: Vec<String> = (first..first + 50)
            .map(|n| forgejo_issue(n, "Crash", "open", "x", &["crash"]))
            .collect();
        format!("[{}]", issues.join(","))
    };
    let rules =
        "GET *page=1\t200\tone.json\nGET *page=2\t200\ttwo.json\nGET *page=3\t200\tnone.json\n";
    let Some(listed) = list(
        &["--tag", "crash"],
        &FORGEJO,
        &[],
        rules,
        "",
        &[
            ("one.json", page(1)),
            ("two.json", page(51)),
            ("none.json", "[]".to_owned()),
        ],
    ) else {
        return;
    };
    assert_eq!(listed.code, 0, "{}", listed.text);
    assert_eq!(listed.lines().len(), 100, "{}", listed.text);
    assert_eq!(listed.curl_calls().len(), 3, "{:?}", listed.curl_calls());
}

#[test]
fn no_match_prints_nothing() {
    for place in [forgejo, github] {
        let Some(listed) = place(&["--tag", "nothing"]) else {
            return;
        };
        assert_eq!(listed.code, 0, "{}", listed.text);
        assert!(listed.text.is_empty(), "{}", listed.text);
    }
}

#[test]
fn no_tag_is_refused() {
    for args in [
        &[][..],
        &["--all"],
        &["--tag"],
        &["--tag", ""],
        &["--tag", "crash", "--nope"],
    ] {
        for settings in [FORGEJO, GITHUB] {
            let Some(listed) = list(args, &settings, &[], "", "", &[]) else {
                return;
            };
            assert_eq!(listed.code, 1, "{args:?}: {}", listed.text);
            assert!(listed.text.contains("--tag"), "{args:?}: {}", listed.text);
            assert!(listed.stdout.is_empty(), "{args:?}: {}", listed.text);
            assert!(listed.curl_calls().is_empty() && listed.gh_calls().is_empty());
        }
    }
}

#[test]
fn no_settings_names_the_table() {
    let Some(listed) = list(&["--tag", "crash"], &[], &[], "", "", &[]) else {
        return;
    };
    assert_eq!(listed.code, 1, "{}", listed.text);
    for key in ["kind", "url", "repo", "token_env"] {
        assert!(listed.text.contains(key), "{key}: {}", listed.text);
    }
    assert!(
        listed.text.contains("[actions.bug-report-query]"),
        "{}",
        listed.text
    );
    assert!(
        listed.text.contains("~/.zaf/config.toml"),
        "{}",
        listed.text
    );
    assert!(listed.curl_calls().is_empty() && listed.gh_calls().is_empty());

    // Only the settings that are unset are named as missing.
    let settings = [
        ("ZAF_SETTING_KIND", "forgejo-issues"),
        ("ZAF_SETTING_URL", "https://forge.example"),
    ];
    let listed = list(&["--tag", "crash"], &settings, &[], "", "", &[]).expect("the action");
    assert_eq!(listed.code, 1, "{}", listed.text);
    let first = listed.text.lines().next().expect("a line");
    assert!(
        first.contains("repo") && first.contains("token_env"),
        "{}",
        listed.text
    );
    assert!(
        !first.contains("kind") && !first.contains("url"),
        "{}",
        listed.text
    );

    // A kind that is neither is refused too.
    let settings = [
        ("ZAF_SETTING_KIND", "gitlab"),
        ("ZAF_SETTING_URL", "https://forge.example"),
        ("ZAF_SETTING_REPO", "example/bugs"),
        ("ZAF_SETTING_TOKEN_ENV", "BUG_TOKEN"),
    ];
    let listed = list(&["--tag", "crash"], &settings, &[], "", "", &[]).expect("the action");
    assert_eq!(listed.code, 1, "{}", listed.text);
    assert!(listed.text.contains("gitlab"), "{}", listed.text);
    assert!(listed.curl_calls().is_empty() && listed.gh_calls().is_empty());
}

#[test]
fn missing_token_is_refused() {
    for settings in [FORGEJO, GITHUB] {
        let mut named = settings.to_vec();
        named.retain(|(key, _)| *key != "ZAF_SETTING_TOKEN_ENV");
        named.push(("ZAF_SETTING_TOKEN_ENV", "NO_SUCH_TOKEN"));
        let Some(listed) = list(
            &["--tag", "crash"],
            &named,
            &[("GH_TOKEN", "machine")],
            "",
            "",
            &[],
        ) else {
            return;
        };
        assert_eq!(listed.code, 1, "{}", listed.text);
        assert!(listed.text.contains("NO_SUCH_TOKEN"), "{}", listed.text);
        assert!(listed.curl_calls().is_empty() && listed.gh_calls().is_empty());

        let listed = list(
            &["--tag", "crash"],
            &settings,
            &[("BUG_TOKEN", "")],
            "",
            "",
            &[],
        )
        .expect("the action");
        assert_eq!(listed.code, 1, "{}", listed.text);
        assert!(listed.curl_calls().is_empty() && listed.gh_calls().is_empty());
    }
}

#[test]
fn token_stays_out_of_arguments_and_log() {
    let Some(listed) = forgejo(&["--tag", "crash"]) else {
        return;
    };
    assert_eq!(listed.code, 0, "{}", listed.text);
    for call in listed.curl_calls() {
        assert!(!call.contains(TOKEN), "{call}");
    }
    assert!(!listed.text.contains(TOKEN), "{}", listed.text);

    let listed = github(&["--tag", "crash"]).expect("the action");
    assert_eq!(listed.code, 0, "{}", listed.text);
    for call in listed.gh_calls() {
        assert!(!call.contains(TOKEN), "{call}");
    }
    assert!(!listed.text.contains(TOKEN), "{}", listed.text);
    // It reaches gh through the environment alone, as GH_TOKEN on github.com.
    let env = fs::read_to_string(listed.gh.join("env/00000")).expect("the env record");
    assert!(env.starts_with(&format!("GH_TOKEN={TOKEN}\n")), "{env}");

    // A failed listing keeps it out of the log too.
    let down = "GET */issues*\tcurl:7\tfailure.txt\n";
    let failed = list(
        &["--tag", "crash"],
        &FORGEJO,
        &[],
        down,
        "",
        &[("failure.txt", "could not connect\n".to_owned())],
    )
    .expect("the action");
    assert_eq!(failed.code, 2, "{}", failed.text);
    assert!(!failed.text.contains(TOKEN), "{}", failed.text);
    for call in failed.curl_calls() {
        assert!(!call.contains(TOKEN), "{call}");
    }
}

#[test]
fn backend_failure_is_exit_2() {
    // The place is down, answers with an error, answers with no list, or has no rule at all.
    for rules in [
        "GET */issues*\tcurl:7\tfailure.txt\n",
        "GET */issues*\t500\tfailure.txt\n",
        "GET */issues*\t200\tfailure.txt\n",
        "",
    ] {
        let Some(listed) = list(
            &["--tag", "crash"],
            &FORGEJO,
            &[],
            rules,
            "",
            &[("failure.txt", "nope\n".to_owned())],
        ) else {
            return;
        };
        assert_eq!(listed.code, 2, "{rules}: {}", listed.text);
        assert!(listed.stdout.is_empty(), "{rules}: {}", listed.text);
    }

    // gh fails, or answers with no list.
    for rules in [
        "issue list*\t1\tfailure.txt\n",
        "issue list*\t0\tfailure.txt\n",
    ] {
        let listed = list(
            &["--tag", "crash"],
            &GITHUB,
            &[],
            "",
            rules,
            &[("failure.txt", "HTTP 502: bad gateway\n".to_owned())],
        )
        .expect("the action");
        assert_eq!(listed.code, 2, "{rules}: {}", listed.text);
        assert!(listed.stdout.is_empty(), "{rules}: {}", listed.text);
    }
}
