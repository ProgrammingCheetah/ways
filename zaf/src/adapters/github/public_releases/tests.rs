use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::*;

const REPO: &str = "ProgrammingCheetah/ways";
const LIST: &str = "GET https://api.github.com/repos/ProgrammingCheetah/ways/releases?per_page=100";
const DOWNLOAD: &str = "https://github.com/ProgrammingCheetah/ways/releases/download";
const TOKEN: &str = "secret-xyz";

/// A scene for the fake `curl` in `tests/fixtures/forgejo/` (it speaks any HTTP API): its rules,
/// responses and the calls it saw.
struct FakeCurl {
    scene: tempfile::TempDir,
    rules: std::cell::Cell<usize>,
}

impl FakeCurl {
    fn new() -> Self {
        let scene = tempfile::tempdir().expect("scene directory");
        fs::write(scene.path().join("rules"), "").expect("rules");
        Self {
            scene,
            rules: std::cell::Cell::new(0),
        }
    }

    /// Answers calls matching the glob `pattern` (`METHOD URL`) with `status` and `body`.
    fn on(&self, pattern: &str, status: u16, body: impl AsRef<[u8]>) -> &Self {
        self.answer(pattern, &status.to_string(), body.as_ref())
    }

    /// Fails calls matching `pattern` as curl fails when nothing answers.
    fn unreachable(&self, pattern: &str) -> &Self {
        self.answer(
            pattern,
            "curl:6",
            b"curl: (6) Could not resolve host: api.github.com",
        )
    }

    fn answer(&self, pattern: &str, answer: &str, body: &[u8]) -> &Self {
        let index = self.rules.get();
        self.rules.set(index + 1);
        let response = format!("response-{index}");
        fs::write(self.scene.path().join(&response), body).expect("response");
        let mut rules = fs::read_to_string(self.scene.path().join("rules")).expect("rules");
        rules.push_str(&format!("{pattern}\t{answer}\t{response}\n"));
        fs::write(self.scene.path().join("rules"), rules).expect("rules");
        self
    }

    fn releases(&self, token: Option<&str>) -> PublicReleases {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forgejo");
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search_path = std::env::join_paths(
            std::iter::once(fixtures).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        PublicReleases::with_token(RepoName::from(REPO), token)
            .env("PATH", search_path)
            .env("FAKE_CURL_DIR", self.scene.path())
    }

    /// The argv and the stdin config of each call, in order.
    fn calls(&self) -> Vec<(Vec<String>, String)> {
        let Ok(entries) = fs::read_dir(self.scene.path().join("calls")) else {
            return Vec::new();
        };
        let mut names: Vec<_> = entries
            .map(|entry| entry.expect("entry").path())
            .filter(|path| path.extension().is_none())
            .collect();
        names.sort();
        names
            .into_iter()
            .map(|path| {
                let argv = fs::read_to_string(&path).expect("argv");
                let argv = argv
                    .split('\0')
                    .filter(|arg| !arg.is_empty())
                    .map(str::to_owned)
                    .collect();
                let config = fs::read_to_string(path.with_extension("config")).expect("config");
                (argv, config)
            })
            .collect()
    }
}

fn release(tag: &str, draft: bool, prerelease: bool, body: Value, assets: &[&str]) -> Value {
    json!({
        "tag_name": tag, "draft": draft, "prerelease": prerelease, "body": body,
        "assets": assets.iter().map(|name| json!({
            "name": name,
            "browser_download_url": format!("{DOWNLOAD}/{tag}/{name}"),
        })).collect::<Vec<_>>(),
    })
}

fn listing(rows: &[Value]) -> String {
    Value::Array(rows.to_vec()).to_string()
}

#[test]
fn public_releases_lists_published_tags_only() {
    let fake = FakeCurl::new();
    fake.on(
        LIST,
        200,
        listing(&[
            release("zaf-v0.2.1", false, false, json!("x"), &[]),
            release("zaf-v0.3.0", true, false, json!("x"), &[]),
            release("zaf-v0.3.0-rc.1", false, true, json!("x"), &[]),
            release("zaf-v0.2.0", false, false, json!("x"), &[]),
        ]),
    );
    assert_eq!(
        fake.releases(None).tags().expect("lists"),
        ["zaf-v0.2.1", "zaf-v0.2.0"]
    );
}

#[test]
fn public_releases_reads_notes() {
    let fake = FakeCurl::new();
    fake.on(
        LIST,
        200,
        listing(&[
            release(
                "zaf-v0.5.2",
                false,
                false,
                json!("- one\r\n- two \u{1b}[31mred\u{7}"),
                &[],
            ),
            release("zaf-v0.5.1", false, false, json!(""), &[]),
            release("zaf-v0.5.0", false, false, Value::Null, &[]),
            release("zaf-v0.6.0", true, false, json!("a draft"), &[]),
        ]),
    );
    let releases = fake.releases(None);

    // Control characters are dropped: the network cannot drive the terminal.
    assert_eq!(
        releases.notes("zaf-v0.5.2").expect("reads"),
        Some("- one\n- two [31mred".to_owned())
    );
    assert_eq!(releases.notes("zaf-v0.5.1").expect("reads"), None);
    assert_eq!(releases.notes("zaf-v0.5.0").expect("reads"), None);
    assert!(matches!(
        releases.notes("zaf-v0.6.0"),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        releases.notes("zaf-v9.9.9"),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn public_releases_downloads_named_assets() {
    let fake = FakeCurl::new();
    fake.on(
        LIST,
        200,
        listing(&[release(
            "zaf-v0.2.1",
            false,
            false,
            json!("x"),
            &["a.tar.gz", "a.tar.gz.sha256"],
        )]),
    )
    .on(
        &format!("GET {DOWNLOAD}/zaf-v0.2.1/a.tar.gz"),
        200,
        [0u8, 1, 2, b'\n', 255, b'\n', 7],
    )
    .on(
        &format!("GET {DOWNLOAD}/zaf-v0.2.1/a.tar.gz.sha256"),
        200,
        "abc  a.tar.gz\n",
    );
    let dir = tempfile::tempdir().expect("dir");
    fs::write(dir.path().join("a.tar.gz"), "old").expect("old file");
    let releases = fake.releases(Some(TOKEN));

    releases
        .download(
            "zaf-v0.2.1",
            &["a.tar.gz".to_owned(), "a.tar.gz.sha256".to_owned()],
            dir.path(),
        )
        .expect("downloads");

    assert_eq!(
        fs::read(dir.path().join("a.tar.gz")).expect("archive"),
        [0u8, 1, 2, b'\n', 255, b'\n', 7]
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("a.tar.gz.sha256")).expect("sum"),
        "abc  a.tar.gz\n"
    );
    // The token goes to the API alone, never to a download.
    let calls = fake.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls[0].1.contains(TOKEN));
    assert!(!calls[1].1.contains(TOKEN) && !calls[2].1.contains(TOKEN));
    assert!(calls[1].0.contains(&"--location".to_owned()));

    assert!(matches!(
        releases.download("zaf-v0.2.1", &["b.tar.gz".to_owned()], dir.path()),
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        releases.download("zaf-v0.2.1", &["../a.tar.gz".to_owned()], dir.path()),
        Err(PortError::Backend(_))
    ));
}

#[test]
fn a_download_link_outside_the_repository_is_not_followed() {
    let fake = FakeCurl::new();
    let mut row = release("zaf-v0.2.1", false, false, json!("x"), &["a.tar.gz"]);
    row["assets"][0]["browser_download_url"] = json!("https://evil.test/a.tar.gz");
    fake.on(LIST, 200, listing(&[row]));
    let dir = tempfile::tempdir().expect("dir");

    let result = fake
        .releases(None)
        .download("zaf-v0.2.1", &["a.tar.gz".to_owned()], dir.path());

    assert!(matches!(result, Err(PortError::Backend(_))), "{result:?}");
    assert_eq!(fake.calls().len(), 1, "only the listing was asked for");
    assert!(!dir.path().join("a.tar.gz").exists());
}

#[test]
fn the_token_never_reaches_the_command_line() {
    let fake = FakeCurl::new();
    fake.on(LIST, 200, listing(&[]));
    fake.releases(Some(TOKEN)).tags().expect("lists");

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let (argv, config) = &calls[0];
    assert!(argv.iter().all(|arg| !arg.contains(TOKEN)), "{argv:?}");
    assert!(argv.contains(&"--disable".to_owned()));
    assert_eq!(
        config,
        &format!("header = \"Authorization: Bearer {TOKEN}\"\n")
    );
    assert!(!format!("{:?}", fake.releases(Some(TOKEN))).contains(TOKEN));

    // No token, no header.
    let anonymous = FakeCurl::new();
    anonymous.on(LIST, 200, listing(&[]));
    anonymous.releases(None).tags().expect("lists");
    assert_eq!(anonymous.calls()[0].1, "");
}

#[test]
fn a_rate_limit_is_named_and_changes_nothing() {
    let fake = FakeCurl::new();
    fake.on(
        LIST,
        403,
        json!({"message": "API rate limit exceeded for 203.0.113.9."}).to_string(),
    );
    let dir = tempfile::tempdir().expect("dir");

    let error = fake
        .releases(None)
        .download("zaf-v0.2.1", &["a.tar.gz".to_owned()], dir.path())
        .expect_err("refused");
    let PortError::Backend(text) = error else {
        panic!("a refusal is a backend error: {error:?}");
    };
    assert!(text.contains("rate limit"), "{text}");
    assert!(text.contains("GITHUB_TOKEN"), "{text}");
    assert_eq!(fs::read_dir(dir.path()).expect("dir").count(), 0);

    let limited = FakeCurl::new();
    limited.on(LIST, 429, "");
    let error = limited.releases(Some(TOKEN)).tags().expect_err("refused");
    let PortError::Backend(text) = error else {
        panic!("a refusal is a backend error");
    };
    assert!(
        text.contains("429") && text.contains("rate limit"),
        "{text}"
    );
    assert!(!text.contains(TOKEN));
}

#[test]
fn no_network_is_unavailable_and_says_so() {
    let fake = FakeCurl::new();
    fake.unreachable(LIST);
    let error = fake.releases(Some(TOKEN)).tags().expect_err("no network");
    let PortError::Unavailable(text) = error else {
        panic!("no network is unavailable: {error:?}");
    };
    assert!(text.contains("no network"), "{text}");
    assert!(!text.contains(TOKEN));

    // No curl at all is unavailable too.
    let none = PublicReleases::with_token(RepoName::from(REPO), None)
        .env("PATH", "/nonexistent-zaf-test-path");
    assert!(matches!(none.tags(), Err(PortError::Unavailable(_))));
}

const PLUGIN: &str = "GET https://raw.githubusercontent.com/ProgrammingCheetah/ways/main/plugins/teams/.claude-plugin/plugin.json";

#[test]
fn plugin_version_reads_main() {
    let fake = FakeCurl::new();
    fake.on(PLUGIN, 200, r#"{"name":"teams","version":"2.1.5"}"#);
    assert_eq!(
        fake.releases(None).plugin_version().expect("reads"),
        semver::Version::new(2, 1, 5)
    );
}

#[test]
fn plugin_version_without_version_is_an_error() {
    for body in [
        r#"{"name":"teams"}"#,
        r#"{"version":7}"#,
        r#"{"version":"two"}"#,
        "<html>captive portal</html>",
    ] {
        let fake = FakeCurl::new();
        fake.on(PLUGIN, 200, body);
        let error = fake.releases(None).plugin_version().expect_err(body);
        let PortError::Backend(text) = error else {
            panic!("{body}: {error:?}");
        };
        assert!(text.contains("plugin.json"), "{text}");
    }
}

#[test]
fn plugin_version_touches_no_marketplace() {
    use std::os::unix::fs::PermissionsExt;

    let fake = FakeCurl::new();
    fake.on(PLUGIN, 200, r#"{"name":"teams","version":"2.1.5"}"#);
    // A `claude` that records every call it gets, first on the PATH of the call.
    let bin = tempfile::tempdir().expect("bin");
    let record = bin.path().join("claude-calls");
    let claude = bin.path().join("claude");
    fs::write(
        &claude,
        format!("#!/bin/sh\necho \"$@\" >> {}\n", record.display()),
    )
    .expect("claude");
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).expect("chmod");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forgejo");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [bin.path().to_owned(), fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    let releases = PublicReleases::with_token(RepoName::from(REPO), Some(TOKEN))
        .env("PATH", search_path)
        .env("FAKE_CURL_DIR", fake.scene.path());

    releases.plugin_version().expect("reads");

    assert!(!record.exists(), "claude was called");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    // The token goes to GitHub's API alone: not on the command line, not to the raw host.
    assert!(calls[0].0.iter().all(|arg| !arg.contains(TOKEN)));
    assert!(!calls[0].1.contains(TOKEN));
}

#[test]
fn one_listing_per_run() {
    let fake = FakeCurl::new();
    fake.on(
        LIST,
        200,
        listing(&[
            release("zaf-v0.5.3", false, false, json!("new"), &["a.tar.gz"]),
            release("zaf-v0.5.2", false, false, json!("two"), &[]),
            release("zaf-v0.5.1", false, false, json!("one"), &[]),
            release("zaf-v0.5.0", false, false, json!("zero"), &[]),
        ]),
    )
    .on(&format!("GET {DOWNLOAD}/zaf-v0.5.3/a.tar.gz"), 200, "bytes");
    let releases = fake.releases(None);
    let dir = tempfile::tempdir().expect("dir");

    assert_eq!(releases.tags().expect("lists").len(), 4);
    for tag in ["zaf-v0.5.2", "zaf-v0.5.1", "zaf-v0.5.0"] {
        assert!(releases.notes(tag).expect("reads").is_some());
    }
    releases
        .download("zaf-v0.5.3", &["a.tar.gz".to_owned()], dir.path())
        .expect("downloads");

    let calls = fake.calls();
    let listings = calls
        .iter()
        .filter(|(argv, _)| {
            argv.iter()
                .any(|arg| arg.ends_with("/releases?per_page=100"))
        })
        .count();
    assert_eq!(listings, 1);
    assert_eq!(calls.len(), 2, "the listing and the download");
}

#[test]
fn a_failed_listing_is_not_kept() {
    let fake = FakeCurl::new();
    // The first rule answers one call only (`!`), the next rule the later ones.
    fake.answer(LIST, "403!", b"").on(
        LIST,
        200,
        listing(&[release("zaf-v0.5.2", false, false, json!("x"), &[])]),
    );
    let releases = fake.releases(None);

    assert!(matches!(releases.tags(), Err(PortError::Backend(_))));
    assert_eq!(releases.tags().expect("lists again"), ["zaf-v0.5.2"]);
    assert_eq!(releases.tags().expect("kept"), ["zaf-v0.5.2"]);
    assert_eq!(fake.calls().len(), 2, "the failed listing and the next");
}

#[test]
fn a_reply_that_is_not_a_release_list_is_a_backend_error() {
    let fake = FakeCurl::new();
    fake.on(LIST, 200, "<html>captive portal</html>");
    assert!(matches!(
        fake.releases(None).tags(),
        Err(PortError::Backend(_))
    ));
}
