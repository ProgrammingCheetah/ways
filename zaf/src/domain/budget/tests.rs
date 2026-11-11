use super::*;
use crate::domain::Mebibytes;

fn budget(units: u32) -> Budget {
    Budget {
        units: Some(units),
        sizes: SizeTable::default(),
    }
}

#[test]
fn a_launch_inside_the_budget_fits() {
    let budget = budget(38);
    let in_use = Units::whole(34);
    assert_eq!(budget.fits(in_use, TaskSize::S, 0, 4), Fit::Fits);
    assert_eq!(budget.fits(in_use, TaskSize::M, 0, 4), Fit::Fits);
}

#[test]
fn a_launch_over_the_budget_does_not() {
    let budget = budget(38);
    assert_eq!(
        budget.fits(Units::whole(35), TaskSize::M, 0, 4),
        Fit::NoBudget
    );
    // A part of a unit in use counts too: 34.5 plus 4 passes 38.
    let in_use = Units::of(Mebibytes::new(34 * 1024 + 512));
    assert_eq!(budget.fits(in_use, TaskSize::M, 0, 4), Fit::NoBudget);
}

#[test]
fn the_default_sizes_cost_one_to_sixteen() {
    let budget = budget(38);
    let costs = [
        (TaskSize::XS, 1),
        (TaskSize::S, 2),
        (TaskSize::M, 4),
        (TaskSize::L, 8),
        (TaskSize::XL, 16),
    ];
    for (size, units) in costs {
        assert_eq!(budget.cost(size), Units::whole(units), "{size}");
    }
}

#[test]
fn an_e_costs_the_whole_budget() {
    let budget = budget(38);
    assert_eq!(budget.cost(TaskSize::E), Units::whole(38));
    assert_eq!(budget.fits(Units::default(), TaskSize::E, 0, 4), Fit::Fits);
    assert_eq!(
        budget.fits(Units::whole(1), TaskSize::E, 0, 4),
        Fit::NoBudget
    );
}

#[test]
fn an_overridden_size_charges_its_own_cost() {
    let budget = Budget {
        units: Some(38),
        sizes: SizeTable::default().with(TaskSize::M, 6),
    };
    assert_eq!(budget.cost(TaskSize::M), Units::whole(6));
    assert_eq!(budget.cost(TaskSize::S), Units::whole(2));
    assert_eq!(
        budget.fits(Units::whole(33), TaskSize::M, 0, 4),
        Fit::NoBudget
    );
}

#[test]
fn the_project_slots_stay_a_ceiling() {
    let budget = budget(1000);
    assert_eq!(
        budget.fits(Units::default(), TaskSize::XS, 4, 4),
        Fit::NoFreeSlot
    );
    assert_eq!(
        Budget::default().fits(Units::default(), TaskSize::XS, 4, 4),
        Fit::NoFreeSlot
    );
    assert_eq!(budget.fits(Units::default(), TaskSize::XS, 3, 4), Fit::Fits);
}

#[test]
fn no_budget_means_no_cap() {
    let budget = Budget::default();
    assert_eq!(budget.units, None);
    let in_use = Units::whole(10_000);
    for size in [
        TaskSize::XS,
        TaskSize::S,
        TaskSize::M,
        TaskSize::L,
        TaskSize::XL,
        TaskSize::E,
    ] {
        assert_eq!(budget.fits(in_use, size, 3, 4), Fit::Fits, "{size}");
    }
}
