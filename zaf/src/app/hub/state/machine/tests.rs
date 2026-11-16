use super::*;

#[test]
fn budget_comes_from_the_homes_config() {
    let home = tempfile::tempdir().expect("home");
    std::fs::write(
        home.path().join(HOME_FILE),
        "[spawn]\nbudget = 24\nreserve = 4\n",
    )
    .expect("config");
    let budget = budget_of(home.path());
    assert_eq!(budget.units, Some(24));
    assert_eq!(budget.reserve, 4);
}

#[test]
fn no_config_is_no_machine_cap() {
    let home = tempfile::tempdir().expect("home");
    assert_eq!(budget_of(home.path()), Budget::default());
    std::fs::write(home.path().join(HOME_FILE), "[spawn]\nbudget = \"many\"\n").expect("config");
    assert_eq!(budget_of(home.path()), Budget::default());
}

#[test]
fn the_machine_line_serializes_with_its_names() {
    let line = MachineLine {
        instances: 3,
        projects: 2,
        in_use: Units::default(),
        budget: Budget::default(),
    };
    let json = serde_json::to_value(&line).expect("serializes");
    assert_eq!(json["instances"], 3);
    assert_eq!(json["projects"], 2);
    assert_eq!(json["in_use_units"], 0.0);
    assert!(json["budget"].get("units").is_some());
}
