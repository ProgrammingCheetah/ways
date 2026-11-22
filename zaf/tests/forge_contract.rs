//! The Forgejo forge against a real instance: one pull request from open to merged, through the
//! [`Forge`] port.
//!
//! It runs when `ZAF_CONTRACT_FORGEJO_URL` names an instance, `ZAF_CONTRACT_FORGEJO_REPO` a
//! scratch repository on it and `ZAF_CONTRACT_FORGEJO_TOKEN_ENV` the variable holding a token
//! that can write there (the token itself is never in a test's variables); without all three it
//! passes without running. It makes a branch with one new file on the repository's default
//! branch, opens a pull request for it, posts a pending and then a passing commit status, reads
//! the checks, the head and the pull request, sets and reads the hand-off, merges, and reads the
//! merge back. It deletes the branch when it ends, and closes the pull request when the merge
//! never happened; a merged pull request stays.

use std::cell::Cell;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use zaf::adapters::forgejo::{ForgejoApi, ForgejoCiFilter, ForgejoForge, Method};
use zaf::domain::{PullRequestFilter, RepoPath};
use zaf::fakes::{FakeCiFilter, FakeForge};
use zaf::manifest::{EnvVarName, ForgeUrl, RepoName};
use zaf::ports::{
    BranchName, ChangedFiles, CheckBucket, CheckLink, CheckLog, CheckState, CiFilter, CommitId,
    Forge, MergeOutcome, MergeStrategy, NewPullRequest, Opened, PortError, PrBody, PrTitle,
};

/// The environment variable naming the Forgejo instance.
const URL_VAR: &str = "ZAF_CONTRACT_FORGEJO_URL";

/// The environment variable naming the scratch repository (`owner/name`) on that instance.
const REPO_VAR: &str = "ZAF_CONTRACT_FORGEJO_REPO";

/// The environment variable naming the variable that holds the token.
const TOKEN_ENV_VAR: &str = "ZAF_CONTRACT_FORGEJO_TOKEN_ENV";

/// The folder the contract's files land in on the default branch.
const FOLDER: &str = "zaf-forge-contract";

/// The commit status the contract posts.
const CONTEXT: &str = "zaf-contract / check";

/// How many times a merge Forgejo cannot make yet is tried, and the wait between two tries.
const MERGE_TRIES: u32 = 10;
const MERGE_WAIT: Duration = Duration::from_secs(2);

/// The door to the scratch repository; none unless all three variables are set.
fn api_from_env() -> Option<ForgejoApi> {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let url = var(URL_VAR)?;
    let repo = var(REPO_VAR)?;
    let token_env = var(TOKEN_ENV_VAR)?;
    let api = ForgejoApi::from_env(
        ForgeUrl::from(url.as_str()),
        RepoName::from(repo.as_str()),
        EnvVarName::from(token_env.as_str()),
    );
    assert!(
        api.has_token(),
        "{TOKEN_ENV_VAR} names `{token_env}`, which holds no token"
    );
    Some(api)
}

/// What the contract made, removed when it ends, pass or fail: the branch, and the pull request
/// when it is still open.
struct Cleanup {
    api: ForgejoApi,
    branch: String,
    pr: Cell<Option<u64>>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(number) = self.pr.get() {
            let open = self
                .api
                .get::<Value>(&format!("pulls/{number}"))
                .is_ok_and(|pull| pull["state"] == "open");
            if open {
                let _ = self.api.probe(
                    Method::Patch,
                    &format!("pulls/{number}"),
                    Some(&json!({ "state": "closed" })),
                );
            }
        }
        let _ = self
            .api
            .probe(Method::Delete, &format!("branches/{}", self.branch), None);
    }
}

/// Standard base64, as the contents API takes a file's bytes.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, byte)| {
            word | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                let index = (word >> (18 - 6 * at)) & 0x3f;
                text.push(char::from(ALPHABET[index as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// Posts a commit status at `sha`.
fn post_status(api: &ForgejoApi, sha: &str, state: &str, link: &str) {
    let reply = api
        .probe(
            Method::Post,
            &format!("statuses/{sha}"),
            Some(&json!({
                "state": state,
                "context": CONTEXT,
                "target_url": link,
                "description": "the zaf forge contract",
            })),
        )
        .expect("the status posts");
    assert!(reply.success(), "posting a status: {}", reply.message());
}

#[test]
fn a_forgejo_pull_request_opens_reports_its_checks_takes_a_handoff_and_merges() {
    let Some(api) = api_from_env() else {
        return;
    };
    let forge = ForgejoForge::from(api.clone());

    // The token may read the repository and write its pull requests and issues.
    for check in forge.access() {
        assert!(check.ok, "{} failed: {}", check.what, check.detail);
    }

    let base = api
        .get::<Value>("")
        .expect("the repository reads")
        .get("default_branch")
        .and_then(Value::as_str)
        .expect("a default branch")
        .to_owned();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_nanos();
    let branch = format!("zaf-forge-contract-{}-{stamp}", std::process::id());
    let path = format!("{FOLDER}/{}-{stamp}.txt", std::process::id());
    let cleanup = Cleanup {
        api: api.clone(),
        branch: branch.clone(),
        pr: Cell::new(None),
    };

    // 1. A branch with one new file.
    let created = api
        .probe(
            Method::Post,
            &format!("contents/{path}"),
            Some(&json!({
                "content": base64(format!("zaf forge contract {stamp}\n").as_bytes()),
                "message": "zaf forge contract: one new file",
                "branch": base,
                "new_branch": branch,
            })),
        )
        .expect("the file posts");
    assert_eq!(
        created.status,
        201,
        "creating the branch: {}",
        created.message()
    );
    let file: Value = serde_json::from_str(&created.body).expect("the file's commit");
    let sha = file["commit"]["sha"]
        .as_str()
        .expect("the commit's id")
        .to_owned();

    // 2. A pull request, opened once: a second open hands the same one back.
    let request = NewPullRequest {
        head: BranchName::from(branch.as_str()),
        base: BranchName::from(base.as_str()),
        title: PrTitle::from("zaf forge contract"),
        body: PrBody::from("Opened and merged by the zaf forge contract test."),
    };
    let opened = forge
        .open_pull_request(&request)
        .expect("the pull request opens");
    let Opened::New(opened_pr) = opened else {
        panic!("a fresh branch had a pull request open already: {opened:?}");
    };
    let pr = opened_pr.number;
    assert!(
        opened_pr.link.as_str().starts_with("http"),
        "its link: {opened_pr:?}"
    );
    cleanup.pr.set(Some(pr.get()));
    assert_eq!(
        forge.open_pull_request(&request),
        Ok(Opened::AlreadyOpen(opened_pr))
    );
    assert_eq!(forge.default_branch(), Ok(BranchName::from(base.as_str())));

    // 3 and 4. The head, then a pending and a passing status, each read back as the check.
    assert_eq!(forge.head(pr), Ok(Some(sha.clone())));
    assert_eq!(forge.check_runs(pr), Ok(Vec::new()), "no status yet");
    let link = format!("{}/{}", api.url(), api.repo());
    post_status(&api, &sha, "pending", &link);
    let runs = forge.check_runs(pr).expect("the checks read");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].bucket, CheckBucket::Pending);
    assert_eq!(runs[0].name.as_str(), CONTEXT);
    assert_eq!(runs[0].link, Some(CheckLink::from(link.as_str())));
    let checks = forge.checks(pr).expect("the checks read");
    assert_eq!(checks[0].state, CheckState::Pending);
    assert_eq!(checks[0].finished_at, None);

    post_status(&api, &sha, "success", &link);
    let runs = forge.check_runs(pr).expect("the checks read");
    assert_eq!(runs.len(), 1, "the latest status stands: {runs:?}");
    assert_eq!(runs[0].bucket, CheckBucket::Pass);
    let checks = forge.checks(pr).expect("the checks read");
    assert_eq!(checks[0].state, CheckState::Passed);
    assert!(checks[0].finished_at.is_some());
    assert_eq!(
        forge.check_log(&CheckLink::from(link.as_str())),
        Ok(CheckLog::AtLink)
    );

    let open = forge.pull_request(pr).expect("the pull request reads");
    assert_eq!(open.merged_at, None);
    assert_eq!(open.head, Some(sha.clone()), "the one read gives the head");
    assert!(!open.merged);
    assert_eq!(forge.checks_at(pr, open.head.as_deref()), forge.checks(pr));
    assert_eq!(forge.merged(pr), Ok(false));
    assert_eq!(
        forge.changed_files(pr),
        Ok(ChangedFiles::complete(vec![RepoPath::from(path.as_str())]))
    );
    assert_eq!(forge.review_comments(pr), Ok(Vec::new()));
    assert_eq!(forge.scanning_alerts(pr), Ok(Vec::new()));

    // 5. The hand-off: set, read, replaced in place.
    assert_eq!(forge.handoff(pr), Ok(None));
    forge
        .set_handoff(pr, "first hand-off")
        .expect("the hand-off posts");
    assert_eq!(forge.handoff(pr), Ok(Some("first hand-off".to_owned())));
    forge
        .set_handoff(pr, "second hand-off")
        .expect("the hand-off is edited");
    assert_eq!(forge.handoff(pr), Ok(Some("second hand-off".to_owned())));
    let comments: Vec<Value> = api
        .get(&format!("issues/{}/comments", pr.get()))
        .expect("the comments read");
    assert_eq!(
        comments.len(),
        1,
        "one hand-off comment, edited: {comments:?}"
    );

    // 6. A merge at another head is refused before anything is sent; at the head it merges.
    let elsewhere = CommitId::from("0000000000000000000000000000000000000000");
    assert!(matches!(
        forge.merge_pull_request(pr, MergeStrategy::Merge, Some(&elsewhere)),
        Err(PortError::Rejected(_))
    ));
    let head = CommitId::from(sha.as_str());
    let mut tries = 0;
    let outcome = loop {
        tries += 1;
        match forge.merge_pull_request(pr, MergeStrategy::Merge, Some(&head)) {
            Err(PortError::Conflict(why)) if tries < MERGE_TRIES => {
                eprintln!("try {tries}: {why}");
                thread::sleep(MERGE_WAIT);
            }
            outcome => break outcome,
        }
    };
    assert_eq!(outcome, Ok(MergeOutcome::Merged));
    assert_eq!(
        forge.merge_pull_request(pr, MergeStrategy::Merge, None),
        Ok(MergeOutcome::AlreadyMerged)
    );
    assert_eq!(forge.merged(pr), Ok(true));
    let merged = forge.pull_request(pr).expect("the pull request reads");
    assert!(merged.merged);
    assert!(merged.merged_at.is_some());
    assert!(merged.merged_at >= Some(merged.opened_at));

    // 7. The branch goes when `cleanup` drops; the merged pull request stays.
    drop(cleanup);
    let gone = api.get::<Value>(&format!("branches/{branch}"));
    assert!(matches!(gone, Err(PortError::NotFound(_))), "{gone:?}");
}

/// A pull request on the fake forge.
fn fake_pull(forge: &FakeForge) -> zaf::ports::PrNumber {
    let request = NewPullRequest {
        head: BranchName::from("feature/v1-1-2"),
        base: BranchName::from("main"),
        title: PrTitle::from("a change"),
        body: PrBody::from("a body"),
    };
    match forge.open_pull_request(&request).expect("it opens") {
        Opened::New(pr) | Opened::AlreadyOpen(pr) => pr.number,
    }
}

#[test]
fn changed_files_lists_a_rename_under_both_names() {
    let forge = FakeForge::default();
    let pr = fake_pull(&forge);
    forge
        .set_changed_files(pr, &["README.md", "zaf/x.rs", "docs/x.rs"])
        .expect("files");
    let listed = forge.changed_files(pr).expect("the files read");
    let names: Vec<&str> = listed.paths.iter().map(RepoPath::as_str).collect();
    assert_eq!(names, ["README.md", "zaf/x.rs", "docs/x.rs"]);
    assert!(!listed.cut_short);
    assert_eq!(listed.last().map(RepoPath::as_str), Some("docs/x.rs"));
}

#[test]
fn changed_files_reads_past_one_page() {
    let forge = FakeForge::default();
    let pr = fake_pull(&forge);
    let names: Vec<String> = (0..130).map(|at| format!("src/file{at}.rs")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    forge.set_changed_files(pr, &refs).expect("files");
    let listed = forge.changed_files(pr).expect("the files read");
    assert_eq!(listed.paths.len(), 130);
    assert!(!listed.cut_short);
    forge.set_changed_files_cut_short(pr, true).expect("cut");
    assert!(forge.changed_files(pr).expect("read").cut_short);
}

#[test]
fn base64_is_the_standard_alphabet_with_padding() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

/// The folder Forgejo Actions reads workflows from.
const WORKFLOWS: &str = ".forgejo/workflows";

/// Two branches the contract made on the scratch repository, removed when it ends: a base with a
/// `.yml` and a `.yaml` workflow, and a head off it that adds a third file the base does not have.
struct Scene {
    api: ForgejoApi,
    base: String,
    head: String,
    base_paths: [String; 2],
    head_path: String,
}

impl Drop for Scene {
    fn drop(&mut self) {
        for branch in [&self.head, &self.base] {
            let _ = self
                .api
                .probe(Method::Delete, &format!("branches/{branch}"), None);
        }
    }
}

/// Writes `path` on `from`, on a new branch named `new_branch` when one is given.
fn write_file(api: &ForgejoApi, path: &str, text: &str, from: &str, new_branch: Option<&str>) {
    let mut body = json!({
        "content": base64(text.as_bytes()),
        "message": "zaf forge contract: a workflow",
        "branch": from,
    });
    if let Some(name) = new_branch {
        body["new_branch"] = json!(name);
    }
    let reply = api
        .probe(Method::Post, &format!("contents/{path}"), Some(&body))
        .expect("the file posts");
    assert_eq!(reply.status, 201, "writing {path}: {}", reply.message());
}

fn forgejo_scene(api: &ForgejoApi) -> Scene {
    let default = api
        .get::<Value>("")
        .expect("the repository reads")
        .get("default_branch")
        .and_then(Value::as_str)
        .expect("a default branch")
        .to_owned();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_nanos();
    let id = format!("{}-{stamp}", std::process::id());
    let scene = Scene {
        api: api.clone(),
        base: format!("zaf-ci-contract-base-{id}"),
        head: format!("zaf-ci-contract-head-{id}"),
        base_paths: [
            format!("{WORKFLOWS}/zaf-contract-{id}.yml"),
            format!("{WORKFLOWS}/zaf-contract-{id}.yaml"),
        ],
        head_path: format!("{WORKFLOWS}/zaf-contract-head-{id}.yml"),
    };
    write_file(
        api,
        &scene.base_paths[0],
        "on: pull_request\njobs: {}\n",
        &default,
        Some(&scene.base),
    );
    write_file(
        api,
        &scene.base_paths[1],
        "on: push\njobs: {}\n",
        &scene.base,
        None,
    );
    write_file(
        api,
        &scene.head_path,
        "on: pull_request\njobs: {}\n",
        &scene.base,
        Some(&scene.head),
    );
    scene
}

/// The workflow paths `filter` finds at `branch`.
fn paths_at(filter: &dyn CiFilter, branch: &str) -> Vec<String> {
    filter
        .workflows(&BranchName::from(branch))
        .expect("the workflows read")
        .into_iter()
        .map(|workflow| workflow.path.as_str().to_owned())
        .collect()
}

#[test]
fn ci_filter_reads_the_base_not_the_head() {
    // The fake: a workflow at the base and a different one added on the head.
    let fake = FakeCiFilter::default();
    fake.set_workflow(
        "main",
        ".forgejo/workflows/base.yml",
        PullRequestFilter::AnyChange,
    );
    fake.set_workflow(
        "feature/x",
        ".forgejo/workflows/base.yml",
        PullRequestFilter::AnyChange,
    );
    fake.set_workflow(
        "feature/x",
        ".forgejo/workflows/head.yml",
        PullRequestFilter::NoTrigger,
    );
    assert_eq!(paths_at(&fake, "main"), [".forgejo/workflows/base.yml"]);

    // Forgejo, against a real instance when one is named. The GitHub adapter has no scratch
    // repository to run against: its unit test holds that it reads the base's ref alone.
    let Some(api) = api_from_env() else {
        return;
    };
    let scene = forgejo_scene(&api);
    let filter = ForgejoCiFilter::from(api);
    let at_base = paths_at(&filter, &scene.base);
    assert!(
        !at_base.contains(&scene.head_path),
        "the head's workflow came through: {at_base:?}"
    );
    assert!(
        at_base.contains(&scene.base_paths[0]),
        "the base's own is missing: {at_base:?}"
    );
    let at_head = paths_at(&filter, &scene.head);
    assert!(at_head.contains(&scene.head_path), "{at_head:?}");
}

#[test]
fn ci_filter_reads_yml_and_yaml() {
    let fake = FakeCiFilter::default();
    fake.set_workflow(
        "main",
        ".forgejo/workflows/a.yml",
        PullRequestFilter::AnyChange,
    );
    fake.set_workflow(
        "main",
        ".forgejo/workflows/b.yaml",
        PullRequestFilter::NoTrigger,
    );
    assert_eq!(
        paths_at(&fake, "main"),
        [".forgejo/workflows/a.yml", ".forgejo/workflows/b.yaml"]
    );

    let Some(api) = api_from_env() else {
        return;
    };
    let scene = forgejo_scene(&api);
    let filter = ForgejoCiFilter::from(api);
    let found = filter
        .workflows(&BranchName::from(scene.base.as_str()))
        .expect("the workflows read");
    let filter_of = |path: &str| {
        found
            .iter()
            .find(|workflow| workflow.path.as_str() == path)
            .map(|workflow| workflow.filter.clone())
    };
    assert_eq!(
        filter_of(&scene.base_paths[0]),
        Some(PullRequestFilter::AnyChange)
    );
    assert_eq!(
        filter_of(&scene.base_paths[1]),
        Some(PullRequestFilter::NoTrigger)
    );
}
