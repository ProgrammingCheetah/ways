use super::*;

#[test]
fn issue_number_parses_with_and_without_hash() {
    let expected = TicketId::from(412);
    assert_eq!("#412".parse::<TicketId>(), Ok(expected));
    assert_eq!("412".parse::<TicketId>(), Ok(expected));
    assert_eq!(expected.to_string(), "#412");
}

#[test]
fn issue_number_rejects_non_digits() {
    let not_a_number_error = IdError::TicketId("#4a".to_owned());
    assert_eq!("#4a".parse::<TicketId>(), Err(not_a_number_error));
    assert!("#".parse::<TicketId>().is_err());
    assert!("-3".parse::<TicketId>().is_err());
}

#[test]
fn page_id_is_one_token() {
    let page = "page-7".parse::<PageId>();
    assert_eq!(page.map(|id| id.to_string()), Ok("page-7".to_owned()));
    assert!("two words".parse::<PageId>().is_err());
    assert!("a,b".parse::<PageId>().is_err());
    assert!("".parse::<PageId>().is_err());
}
