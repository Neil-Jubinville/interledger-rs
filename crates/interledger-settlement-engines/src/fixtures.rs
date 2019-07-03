use super::test_helpers::TestAccount;

lazy_static! {
    pub static ref TEST_ACCOUNT_0: TestAccount =
        TestAccount::new(0, "3cdb3d9e1b74692bb1e3bb5fc81938151ca64b02");
    // pub static ref MESSAGES_API: Matcher = Matcher::Regex(r"^/accounts/\d*/messages$".to_string());
    // pub static ref SETTLEMENT_API: Matcher =
    //     Matcher::Regex(r"^/accounts/\d*/settlement$".to_string());
}
