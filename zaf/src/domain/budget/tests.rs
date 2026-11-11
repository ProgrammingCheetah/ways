use super::*;
use crate::domain::Mebibytes;

fn budget(units: u32) -> Budget {
    Budget {
        units: Some(units),
        ..Budget::default()
    }
}

#[test]
fn a_launch_inside_the_budget_fits() {
    let budget = budget(38);
    let in_use = Units::whole(34);
    assert_eq!(budget.fits(in_use, false, TaskSize::S, 0, 4), Fit::Fits);
    assert_eq!(budget.fits(in_use, false, TaskSize::M, 0, 4), Fit::Fits);
}

#[test]
fn a_launch_over_the_budget_does_not() {
    let budget = budget(38);
    assert_eq!(
        budget.fits(Units::whole(35), false, TaskSize::M, 0, 4),
        Fit::NoBudget
    );
    // A part of a unit in use counts too: 34.5 plus 4 passes 38.
    let in_use = Units::of(Mebibytes::new(34 * 1024 + 512));
    assert_eq!(budget.fits(in_use, false, TaskSize::M, 0, 4), Fit::NoBudget);
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
fn e_leaves_the_reserve() {
    // No reserve set: the default of 6 units, so an E costs 38 - 6 = 32.
    let budget = budget(38);
    assert_eq!(budget.reserve, DEFAULT_RESERVE);
    assert_eq!(budget.cost(TaskSize::E), Units::whole(32));
    assert_eq!(
        budget.fits(Units::default(), false, TaskSize::E, 0, 4),
        Fit::Fits
    );
    assert_eq!(
        budget.fits(Units::whole(1), false, TaskSize::E, 0, 4),
        Fit::Fits
    );
    assert_eq!(
        budget.fits(Units::whole(7), false, TaskSize::E, 0, 4),
        Fit::NoBudget
    );
    // A set reserve moves the cost.
    let wide = Budget {
        reserve: 10,
        ..budget
    };
    assert_eq!(wide.cost(TaskSize::E), Units::whole(28));
}

#[test]
fn small_work_runs_beside_e() {
    // A running E uses 32 of 38; an S of 2 units still fits in the reserve, an M of 4 as well,
    // and an XL of 16 does not.
    let budget = budget(38);
    let in_use = Units::whole(32);
    assert_eq!(budget.fits(in_use, true, TaskSize::S, 0, 4), Fit::Fits);
    assert_eq!(budget.fits(in_use, true, TaskSize::M, 0, 4), Fit::Fits);
    assert_eq!(budget.fits(in_use, true, TaskSize::XL, 0, 4), Fit::NoBudget);
}

#[test]
fn one_e_at_a_time() {
    let budget = budget(38);
    // Nothing else is in use, yet a second E is refused while one runs.
    assert_eq!(
        budget.fits(Units::default(), true, TaskSize::E, 0, 4),
        Fit::NoBudget
    );
    // The same E with none live fits.
    assert_eq!(
        budget.fits(Units::default(), false, TaskSize::E, 0, 4),
        Fit::Fits
    );
}

#[test]
fn an_overridden_size_charges_its_own_cost() {
    let budget = Budget {
        units: Some(38),
        sizes: SizeTable::default().with(TaskSize::M, 6),
        ..Budget::default()
    };
    assert_eq!(budget.cost(TaskSize::M), Units::whole(6));
    assert_eq!(budget.cost(TaskSize::S), Units::whole(2));
    assert_eq!(
        budget.fits(Units::whole(33), false, TaskSize::M, 0, 4),
        Fit::NoBudget
    );
}

#[test]
fn the_project_slots_stay_a_ceiling() {
    let budget = budget(1000);
    assert_eq!(
        budget.fits(Units::default(), false, TaskSize::XS, 4, 4),
        Fit::NoFreeSlot
    );
    assert_eq!(
        Budget::default().fits(Units::default(), false, TaskSize::XS, 4, 4),
        Fit::NoFreeSlot
    );
    assert_eq!(
        budget.fits(Units::default(), false, TaskSize::XS, 3, 4),
        Fit::Fits
    );
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
        assert_eq!(budget.fits(in_use, false, size, 3, 4), Fit::Fits, "{size}");
    }
}
