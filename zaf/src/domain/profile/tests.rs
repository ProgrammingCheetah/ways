use strum::VariantArray;

use super::*;

#[test]
fn profile_tokens_are_pinned() {
    let tokens: Vec<String> = Profile::VARIANTS.iter().map(ToString::to_string).collect();
    assert_eq!(tokens, ["lean", "build", "design", "review"]);
    for profile in Profile::VARIANTS {
        assert_eq!(profile.to_string().parse::<Profile>(), Ok(*profile));
    }
}
