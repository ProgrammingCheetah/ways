use super::*;

fn instance() -> Instance {
    Instance {
        role: RoleName::from("programmer"),
        epic: IssueNumber::from(12),
        number: InstanceNumber::from(3),
    }
}

#[test]
fn an_instance_reads_back_from_its_display_form() {
    let shown = instance().to_string();
    assert_eq!(shown, "programmer instance 3 on #12");
    assert_eq!(shown.parse::<Instance>(), Ok(instance()));
    assert_eq!(
        "programmer instance 3 on 12".parse::<Instance>(),
        Ok(instance())
    );
}

#[test]
fn text_that_is_not_an_instance_is_rejected() {
    for text in [
        "",
        "programmer",
        "programmer instance three on #12",
        "programmer instance 3 on #x",
        "programmer instance 3",
        " instance 3 on #12",
    ] {
        let rejected = IdError::Instance(text.to_owned());
        assert_eq!(text.parse::<Instance>(), Err(rejected), "`{text}`");
    }
}

#[test]
fn the_slug_names_an_instance_as_one_folder() {
    let team = Instance {
        role: RoleName::from("team"),
        epic: IssueNumber::from(16),
        number: InstanceNumber::from(12),
    };
    assert_eq!(team.slug(), "team-instance-12-on-16");
    assert_eq!(instance().slug(), "programmer-instance-3-on-12");
}

#[test]
fn text_that_is_not_an_instance_slugs_the_same_way() {
    assert_eq!(
        Instance::slug_of("Team instance 12 on #16"),
        "team-instance-12-on-16"
    );
    assert_eq!(Instance::slug_of("local"), "local");
    assert_eq!(Instance::slug_of("programmer-3"), "programmer-3");
    assert_eq!(
        Instance::slug_of("a/b  c"),
        "a-b-c",
        "a path separator never survives"
    );
    assert_eq!(Instance::slug_of(".."), "unnamed", "no dot folders");
    assert_eq!(Instance::slug_of(" # "), "unnamed");
    assert_eq!(Instance::slug_of(""), "unnamed");
}
