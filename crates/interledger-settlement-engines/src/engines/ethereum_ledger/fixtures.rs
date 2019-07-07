use super::test_helpers::TestAccount;

lazy_static! {
    pub static ref ALICE: TestAccount =
        TestAccount::new(1, "3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
    pub static ref BOB: TestAccount =
        TestAccount::new(0, "2fcd07047c209c46a767f8338cb0b14955826826", "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
    // pub static ref MESSAGES_API: Matcher = Matcher::Regex(r"^/accounts/\d*/messages$".to_string());
    // pub static ref SETTLEMENT_API: Matcher =
    //     Matcher::Regex(r"^/accounts/\d*/settlement$".to_string());
}
