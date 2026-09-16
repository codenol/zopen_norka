//! The one setting two stores name twice.
//!
//! `accounts` (`op-accounts`) and the tenant store (`web_canvas_server`) each
//! name `OPENPENCIL_ONLINE_DATA_DIR` themselves rather than one importing the
//! other: the account store must not depend on the web layer, and after issue
//! #75 it must not depend on this crate at all. The price of naming it twice is
//! exactly this test — a deployment that sets the variable has to find both
//! stores in the same place, and nothing else checks that the two spellings
//! stayed one setting. Only a crate that can see both can ask it.

/// The account store's copy and the tenant store's copy are one variable.
#[test]
fn the_data_directory_variable_is_the_one_the_deployment_already_uses() {
    assert_eq!(
        op_accounts::accounts::DATA_DIR_ENV,
        crate::web_canvas_server::tenant_store::DATA_DIR_ENV
    );
}
