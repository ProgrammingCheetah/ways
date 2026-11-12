use super::*;

#[test]
fn memory_converts_to_units_without_rounding() {
    let sum: Units = [512, 256, 1]
        .map(|mib| Units::of(Mebibytes::new(mib)))
        .into_iter()
        .sum();
    assert_eq!(
        sum.mebibytes(),
        Mebibytes::new(769),
        "mebibytes add exactly"
    );
    assert!((sum.as_f64() - 769.0 / 1024.0).abs() < f64::EPSILON);
}

#[test]
fn one_unit_is_1024_mebibytes() {
    assert_eq!(Units::whole(1).mebibytes(), Mebibytes::new(1024));
    assert_eq!(Units::of(Mebibytes::new(1024)), Units::whole(1));
    assert!((Units::whole(6).as_f64() - 6.0).abs() < f64::EPSILON);
}

#[test]
fn a_unit_count_prints_with_one_decimal() {
    assert_eq!(Units::of(Mebibytes::new(512)).to_string(), "0.5");
    assert_eq!(Units::default().to_string(), "0.0");
    assert_eq!(Units::whole(3).to_string(), "3.0");
    assert_eq!(Units::of(Mebibytes::new(1536)).to_string(), "1.5");
    assert_eq!(Units::of(Mebibytes::new(1000)).to_string(), "1.0");
}

#[test]
fn kibibytes_round_to_the_nearest_mebibyte() {
    assert_eq!(Mebibytes::from_kibibytes(524_288), Mebibytes::new(512));
    assert_eq!(Mebibytes::from_kibibytes(511), Mebibytes::new(0));
    assert_eq!(Mebibytes::from_kibibytes(512), Mebibytes::new(1));
    assert_eq!(
        Mebibytes::from_kibibytes(u64::MAX),
        Mebibytes::new(u64::MAX / 1024)
    );
}
