use super::*;

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
