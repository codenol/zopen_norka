//! Tests for the online identity boundary.

use super::*;

fn request_with(authorization: Option<&str>, cookie: Option<&str>) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        path: "/api/mcp/document".into(),
        body: String::new(),
        host: None,
        origin: None,
        token: None,
        content_type: None,
        authorization: authorization.map(str::to_string),
        cookie: cookie.map(str::to_string),
        query: None,
    }
}

#[test]
fn a_bearer_header_yields_the_token_without_its_scheme() {
    let presented = PresentedCredentials::from_request(&request_with(Some("Bearer tokA"), None));
    assert_eq!(presented.bearer.as_deref(), Some("tokA"));
    assert_eq!(presented.session_cookie, None);
}

#[test]
fn the_bearer_scheme_match_is_case_insensitive_but_the_token_is_not_touched() {
    for header in ["bearer TokA", "BEARER TokA", "Bearer   TokA  "] {
        let presented = PresentedCredentials::from_request(&request_with(Some(header), None));
        assert_eq!(presented.bearer.as_deref(), Some("TokA"), "{header}");
    }
}

#[test]
fn a_non_bearer_authorization_header_presents_no_credential() {
    for header in ["Basic dXNlcjpwdw==", "tokA", "Bearer", "Bearer   "] {
        let presented = PresentedCredentials::from_request(&request_with(Some(header), None));
        assert_eq!(presented.bearer, None, "{header}");
    }
}

#[test]
fn the_session_cookie_is_picked_out_of_a_multi_cookie_header() {
    let presented = PresentedCredentials::from_request(&request_with(
        None,
        Some("theme=dark; op_hub_session=sessA; other=1"),
    ));
    assert_eq!(presented.session_cookie.as_deref(), Some("sessA"));
}

#[test]
fn a_cookie_whose_name_merely_contains_the_session_name_is_not_the_session() {
    let presented = PresentedCredentials::from_request(&request_with(
        None,
        Some("not_op_hub_session=evil; op_hub_session_backup=evil2"),
    ));
    assert_eq!(presented.session_cookie, None);
}

#[test]
fn a_request_with_no_headers_presents_nothing() {
    let presented = PresentedCredentials::from_request(&request_with(None, None));
    assert!(presented.is_empty());
}

#[test]
fn the_static_verifier_maps_a_known_token_to_its_account() {
    let verifier = StaticVerifier::parse("tokA=userA,tokB=userB");
    let identity = verifier
        .resolve(&PresentedCredentials {
            bearer: Some("tokB".into()),
            session_cookie: None,
        })
        .expect("known token resolves");
    assert_eq!(identity.user_id, "userB");
    assert_eq!(identity.via, IdentityVia::ApiToken);
}

#[test]
fn the_static_verifier_tolerates_whitespace_and_skips_malformed_pairs() {
    let verifier = StaticVerifier::parse(" tokA = userA , garbage , =nouser , tokC= ,tokB=userB ");
    assert!(verifier
        .resolve(&PresentedCredentials {
            bearer: Some("tokA".into()),
            session_cookie: None,
        })
        .is_ok());
    assert!(verifier
        .resolve(&PresentedCredentials {
            bearer: Some("tokC".into()),
            session_cookie: None,
        })
        .is_err());
}

#[test]
fn an_unknown_token_and_a_missing_one_both_answer_401() {
    let verifier = StaticVerifier::parse("tokA=userA");
    assert_eq!(
        verifier
            .resolve(&PresentedCredentials {
                bearer: Some("nope".into()),
                session_cookie: None,
            })
            .unwrap_err()
            .http_status(),
        "401 Unauthorized"
    );
    assert_eq!(
        verifier
            .resolve(&PresentedCredentials::default())
            .unwrap_err(),
        OnlineAuthError::MissingCredential
    );
    // Same code, so the answer cannot be used to probe which tokens exist.
    assert_eq!(
        OnlineAuthError::UnknownCredential.code(),
        OnlineAuthError::MissingCredential.code()
    );
}

#[test]
fn an_empty_table_reports_a_misconfigured_deployment_rather_than_401() {
    let verifier = StaticVerifier::parse("");
    assert!(verifier.is_empty());
    assert_eq!(
        verifier
            .resolve(&PresentedCredentials {
                bearer: Some("tokA".into()),
                session_cookie: None,
            })
            .unwrap_err(),
        OnlineAuthError::VerifierUnavailable
    );
}

#[test]
fn a_bearer_token_wins_over_a_cookie_presented_on_the_same_request() {
    let verifier = StaticVerifier::parse("tokA=userA,sessB=userB");
    let identity = verifier
        .resolve(&PresentedCredentials {
            bearer: Some("tokA".into()),
            session_cookie: Some("sessB".into()),
        })
        .expect("resolves");
    assert_eq!(identity.user_id, "userA");
    assert_eq!(identity.via, IdentityVia::ApiToken);
}

#[test]
fn a_cookie_alone_resolves_and_records_how_it_was_established() {
    let verifier = StaticVerifier::parse("sessB=userB");
    let identity = verifier
        .resolve(&PresentedCredentials {
            bearer: None,
            session_cookie: Some("sessB".into()),
        })
        .expect("resolves");
    assert_eq!(identity.user_id, "userB");
    assert_eq!(identity.via, IdentityVia::SessionCookie);
}

#[test]
fn the_static_verifier_admits_without_roles_rather_than_denying() {
    // The env table knows tokens, not people: it has no roles to give. An
    // empty role set therefore means "no roles", and the account keeps the
    // baseline every verified account has — it is NOT a denial. A dev
    // deployment that suddenly lost access to its own documents would be a
    // regression, and `OPENPENCIL_ONLINE_STATIC_IDENTITIES` predates roles.
    let verifier = StaticVerifier::parse("tokA=userA");
    let identity = verifier
        .resolve(&PresentedCredentials {
            bearer: Some("tokA".into()),
            session_cookie: None,
        })
        .expect("resolves");
    assert!(identity.roles.is_empty());
    assert!(identity.roles.unrecognized().is_empty());
    assert_eq!(
        identity.roles.rights(),
        op_editor_core::access::Rights::VIEW_ONLY
    );
    assert!(identity.roles.rights().can_view());
    // Everything that changes state or widens the circle stays closed.
    assert!(!identity.roles.rights().can_edit());
    assert!(!identity.roles.rights().can_comment());
    assert!(!identity.roles.rights().can_invite());
    assert!(!identity.roles.rights().can_manage_users());
}
