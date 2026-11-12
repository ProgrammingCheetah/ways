use super::*;
use crate::domain::{TaskSize, Units};

fn table(text: &str) -> Table {
    text.parse().expect("toml")
}

#[test]
fn local_toml_sets_the_gate_slot() {
    // `[gate] slot` is one of the two manifest settings the local file may lay over the project.
    let layer = local_layer(table("[gate]\nslot = \"flock -x /tmp/gate\"\n")).expect("allowed");
    assert_eq!(layer, table("[gate]\nslot = \"flock -x /tmp/gate\"\n"));
    // Beside the project's gate, the merged table has the project's commands and the slot.
    let mut merged = table("[gate]\nfull = \"just gate\"\nlight = \"just light\"\n");
    merge(&mut merged, layer);
    assert_eq!(
        merged,
        table(
            "[gate]\nfull = \"just gate\"\nlight = \"just light\"\nslot = \"flock -x /tmp/gate\"\n"
        )
    );
    // The other gate keys stay the project's.
    let refused = local_layer(table("[gate]\nfull = \"true\"\n")).expect_err("not a machine fact");
    assert!(refused.to_string().ends_with("it sets [gate] full"));
}

#[test]
fn the_local_layer_drops_its_own_tables() {
    let layer = local_layer(table(
        "[budget]\nweekly_usd = 5\n[auth]\ntoken_env = \"T\"\n[report.prices]\nm = [1.0, 2.0, 3.0, 4.0]\n[spawn]\nslots = 3\n",
    ))
    .expect("allowed");
    assert_eq!(layer, table("[spawn]\nslots = 3\n"));
}

const PROJECT: &str =
    "[project]\nname = \"p\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

/// Loads a project holding the `(file, text)` pairs, with `machine` as the zaf home's
/// `config.toml` when given.
fn load_project(files: &[(&str, &str)], machine: Option<&str>) -> Result<Manifest, ManifestError> {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("repo");
    let home = temp.path().join("zaf-home");
    std::fs::create_dir_all(&home).expect("home");
    for (name, text) in files {
        let file = root.join(name);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(file, text).expect("write");
    }
    if let Some(text) = machine {
        std::fs::write(home.join(HOME_FILE), text).expect("machine config");
    }
    load(&root, Some(&home))
}

#[test]
fn the_machine_budget_reads_from_the_zaf_home() {
    let manifest =
        load_project(&[(ROOT_FILE, PROJECT)], Some("[spawn]\nbudget = 38\n")).expect("loads");
    assert_eq!(manifest.spawn.budget().units, Some(38));
    // Unset, there is no machine cap.
    let manifest = load_project(&[(ROOT_FILE, PROJECT)], None).expect("loads");
    assert_eq!(manifest.spawn.budget().units, None);
    // A budget is a positive integer.
    let zero = load_project(&[(ROOT_FILE, PROJECT)], Some("[spawn]\nbudget = 0\n"));
    assert!(zero.is_err(), "a budget of 0 is refused");
}

#[test]
fn a_project_cannot_set_the_machine_budget() {
    for project_file in [ROOT_FILE, FOLDER_FILE] {
        let text = format!("{PROJECT}[spawn]\nbudget = 38\n");
        let error = load_project(&[(project_file, &text)], None).expect_err("the machine's");
        let message = error.to_string();
        assert!(message.contains(project_file), "{message}");
        assert!(message.contains("budget"), "{message}");
    }
    // The local file may not set it either.
    let error = load_project(
        &[(ROOT_FILE, PROJECT), (LOCAL_FILE, "[spawn]\nbudget = 38\n")],
        None,
    )
    .expect_err("not a local setting");
    assert!(error.to_string().contains("[spawn] budget"), "{error}");
}

#[test]
fn the_reserve_reads_from_the_zaf_home() {
    let manifest =
        load_project(&[(ROOT_FILE, PROJECT)], Some("[spawn]\nbudget = 38\n")).expect("loads");
    assert_eq!(manifest.spawn.budget().reserve, 6, "the default");
    assert_eq!(manifest.spawn.budget().cost(TaskSize::E), Units::whole(32));
    let machine = "[spawn]\nbudget = 38\nreserve = 10\n";
    let manifest = load_project(&[(ROOT_FILE, PROJECT)], Some(machine)).expect("loads");
    assert_eq!(manifest.spawn.budget().reserve, 10);
    assert_eq!(manifest.spawn.budget().cost(TaskSize::E), Units::whole(28));
}

#[test]
fn a_reserve_over_the_budget_fails() {
    for reserve in [38, 40] {
        let machine = format!("[spawn]\nbudget = 38\nreserve = {reserve}\n");
        let error = load_project(&[(ROOT_FILE, PROJECT)], Some(&machine)).expect_err("too big");
        let message = error.to_string();
        assert!(message.contains("reserve"), "{message}");
        assert!(message.contains(HOME_FILE), "{message}");
    }
    // The default reserve of 6 is over a budget of 6 too.
    let small = "[spawn]\nbudget = 6\n";
    assert!(load_project(&[(ROOT_FILE, PROJECT)], Some(small)).is_err());
    // With no budget there is nothing for a reserve to exceed.
    let no_cap = "[spawn]\nreserve = 40\n";
    assert!(load_project(&[(ROOT_FILE, PROJECT)], Some(no_cap)).is_ok());
}

#[test]
fn a_project_cannot_set_the_reserve() {
    for project_file in [ROOT_FILE, FOLDER_FILE] {
        let text = format!("{PROJECT}[spawn]\nreserve = 4\n");
        let error = load_project(&[(project_file, &text)], None).expect_err("the machine's");
        let message = error.to_string();
        assert!(message.contains(project_file), "{message}");
        assert!(message.contains("reserve"), "{message}");
    }
    // The local file may not set it either.
    let error = load_project(
        &[(ROOT_FILE, PROJECT), (LOCAL_FILE, "[spawn]\nreserve = 4\n")],
        None,
    )
    .expect_err("not a local setting");
    assert!(error.to_string().contains("[spawn] reserve"), "{error}");
}

#[test]
fn a_project_overrides_a_size() {
    let text = format!("{PROJECT}[spawn.sizes]\nM = 6\n");
    let manifest =
        load_project(&[(ROOT_FILE, &text)], Some("[spawn]\nbudget = 38\n")).expect("loads");
    let budget = manifest.spawn.budget();
    assert_eq!(budget.cost(TaskSize::M), Units::whole(6));
    assert_eq!(budget.cost(TaskSize::S), Units::whole(2));
    // The zaf home may set a size too, below the project.
    let machine = "[spawn]\nbudget = 38\n[spawn.sizes]\nM = 5\nL = 9\n";
    let manifest = load_project(&[(ROOT_FILE, &text)], Some(machine)).expect("loads");
    let budget = manifest.spawn.budget();
    assert_eq!(
        budget.cost(TaskSize::M),
        Units::whole(6),
        "the project wins"
    );
    assert_eq!(
        budget.cost(TaskSize::L),
        Units::whole(9),
        "the machine fills"
    );
}

#[test]
fn the_size_table_refuses_e() {
    let text = format!("{PROJECT}[spawn.sizes]\nE = 10\n");
    let error = load_project(&[(ROOT_FILE, &text)], None).expect_err("no E entry");
    let message = format!("{error:?} {error}");
    assert!(message.contains("`E`"), "{message}");
    let text = format!("{PROJECT}[spawn.sizes]\nXXL = 32\n");
    let error = load_project(&[(ROOT_FILE, &text)], None).expect_err("no such size");
    let message = format!("{error:?} {error}");
    assert!(message.contains("`XXL`"), "{message}");
    let text = format!("{PROJECT}[spawn.sizes]\nS = 0\n");
    assert!(
        load_project(&[(ROOT_FILE, &text)], None).is_err(),
        "a cost is positive"
    );
}
