// ---------------------------------------------------------------------------
// #33: the document tiers ask who is editing, not only what the token may do.
// ---------------------------------------------------------------------------

/// A verifier that can say what `StaticVerifier` cannot: which roles an account
/// holds.
///
/// An env-injected token table has no hub behind it, so it answers "no roles"
/// for everyone — which is enough to prove the refusal half of the roles model,
/// and useless for the half that has to keep working (a visitor the hub made an
/// editor). Tokens here read `user@role|role`, with the roles optional and the
/// session cookie spelled the same way.
struct RoleVerifier;

impl IdentityVerifier for RoleVerifier {
    fn resolve(
        &self,
        presented: &PresentedCredentials,
    ) -> std::result::Result<ResolvedIdentity, OnlineAuthError> {
        use crate::web_canvas_server::tenant_auth::IdentityVia;
        let (credential, via) = match (&presented.bearer, &presented.session_cookie) {
            (Some(token), _) => (token.as_str(), IdentityVia::ApiToken),
            (None, Some(cookie)) => (cookie.as_str(), IdentityVia::SessionCookie),
            (None, None) => return Err(OnlineAuthError::MissingCredential),
        };
        let (user, roles) = credential
            .split_once('@')
            .ok_or(OnlineAuthError::UnknownCredential)?;
        let roles: Vec<&str> = roles.split('|').filter(|role| !role.is_empty()).collect();
        Ok(ResolvedIdentity {
            user_id: user.to_string(),
            username: user.to_string(),
            display_name: user.to_string(),
            roles: op_editor_core::access::RoleSet::from_wire(&roles),
            via,
            scopes: crate::mcp_serve::tool_profile::McpScopes::FULL,
        })
    }
}

/// Drive one request through the online loop under [`RoleVerifier`].
fn serve_roles(registry: &TenantRegistry, request: Request) -> String {
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.wire().into_bytes()),
        output: Vec::new(),
    };
    let barrier = crate::web_canvas_server::tenant::WriteBarrier::default();
    serve_one_online(
        &mut stream,
        registry,
        &RoleVerifier,
        None,
        &barrier,
        TEST_PEER,
    )
    .expect("serve_one_online");
    String::from_utf8_lossy(&stream.output).into_owned()
}

/// Address a request at another account's tenant.
fn as_tenant(mut request: Request, owner: &'static str) -> Request {
    request.tenant = Some(owner);
    // A request addressed at another account's tenant must also say WHICH
    // document: admission is a property of that document's access list
    // (issue #127).
    request.file = Some(SHARED_DOC);
    request
}

/// Put `target` on `owner_token`'s access list, through the real share route.
fn grant(registry: &TenantRegistry, owner_token: &'static str, target: &str) {
    grant_at(registry, owner_token, target, "editor");
}

/// The same at a named level.
///
/// Editing rather than the default, because a level is a CEILING over the
/// caller's roles (#56): a grant of viewing leaves an editor's roles capped, so
/// a test about what a ROLE may do has to hand out the level that role could
/// actually use. Tests about the cap itself pass the level they mean.
fn grant_at(registry: &TenantRegistry, owner_token: &'static str, target: &str, level: &str) {
    let granted = serve_roles(
        registry,
        Request::json(
            "POST",
            op_editor_core::share_routes::GRANT,
            &serde_json::json!({
                "userId": target,
                "level": level,
                "file": SHARED_DOC,
            })
            .to_string(),
        )
        .with_bearer(owner_token),
    );
    assert_eq!(status_line(&granted), "HTTP/1.1 200 OK", "{granted}");
}

/// A `tools/call` message for one tool.
fn tool_call(tool: &str, arguments: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
    )
}

#[test]
fn an_account_writes_its_own_document_whatever_roles_the_hub_sends() {
    // Ownership grants edit on your own document — the operator's decision, and
    // the reason a deployment whose hub sends no roles is not read-only for the
    // people the documents belong to.
    let registry = registry();
    let pushed = serve_roles(
        &registry,
        Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("userA@"),
    );
    assert_eq!(status_line(&pushed), "HTTP/1.1 200 OK", "{pushed}");

    let read = serve_roles(
        &registry,
        Request::new("GET", "/api/mcp/document").with_bearer("userA@"),
    );
    assert!(read.contains("Tenant Rect"), "{read}");
}

#[test]
fn a_role_less_visitor_reads_the_shared_document_but_no_write_of_its_lands() {
    let registry = registry();
    grant(&registry, "userA@", "userB");

    // Reading is what sharing is for, and it still works.
    let read = serve_roles(
        &registry,
        as_tenant(
            Request::new("GET", "/api/mcp/document").with_bearer("userB@"),
            "userA",
        ),
    );
    assert_eq!(status_line(&read), "HTTP/1.1 200 OK", "{read}");
    assert_eq!(body(&read)["version"], 0, "{read}");

    // Every route that carries a document refuses — including the AI design
    // turn and JSON-RPC, which are dispatched ahead of the REST handler.
    let add_page = tool_call("add_page", r#"{"name":"Nope"}"#);
    for (method, path, payload) in [
        ("POST", "/api/mcp/document", SYNC_BODY),
        ("POST", "/api/mcp/sync-reset", ""),
        ("POST", "/api/mcp/selection", r#"{"selectedIds":["n9"]}"#),
        ("POST", "/api/ai/standard", "{}"),
        ("POST", "/mcp", add_page.as_str()),
    ] {
        let response = serve_roles(
            &registry,
            as_tenant(
                Request::json(method, path, payload).with_bearer("userB@"),
                "userA",
            ),
        );
        assert_eq!(
            status_line(&response),
            "HTTP/1.1 403 Forbidden",
            "{method} {path}: {response}"
        );
        assert_eq!(
            body(&response)["error"],
            "read-only-role",
            "{method} {path}: {response}"
        );
    }

    // The reads it may still make: the catalog and a read tool.
    let listed = serve_roles(
        &registry,
        as_tenant(
            Request::json("POST", "/mcp", TOOLS_LIST).with_bearer("userB@"),
            "userA",
        ),
    );
    assert_eq!(status_line(&listed), "HTTP/1.1 200 OK", "{listed}");
    assert!(listed.contains("add_page"), "{listed}");
    let read_tool = serve_roles(
        &registry,
        as_tenant(
            Request::json("POST", "/mcp", &tool_call("get_document_info", "{}"))
                .with_bearer("userB@"),
            "userA",
        ),
    );
    assert_ne!(body(&read_tool)["result"]["isError"], true, "{read_tool}");

    // And not one of the refusals changed the owner's document: no version
    // bump, no node, and not the selection the visitor tried to set.
    let owner = serve_roles(
        &registry,
        Request::new("GET", "/api/mcp/document").with_bearer("userA@"),
    );
    assert_eq!(body(&owner)["version"], 0, "{owner}");
    assert!(!owner.contains("Tenant Rect"), "{owner}");
    let selection = serve_roles(
        &registry,
        Request::new("GET", "/api/mcp/selection").with_bearer("userA@"),
    );
    assert_eq!(
        body(&selection)["selectedIds"].as_array().map(Vec::len),
        Some(0),
        "{selection}"
    );
}

#[test]
fn a_visitor_whose_roles_grant_an_edit_writes_the_shared_document() {
    let registry = registry();
    grant(&registry, "userA@", "userB");
    let pushed = serve_roles(
        &registry,
        as_tenant(
            Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("userB@ux_ui"),
            "userA",
        ),
    );
    assert_eq!(status_line(&pushed), "HTTP/1.1 200 OK", "{pushed}");

    let owner = serve_roles(
        &registry,
        Request::new("GET", "/api/mcp/document").with_bearer("userA@"),
    );
    assert!(owner.contains("Tenant Rect"), "{owner}");
    assert_eq!(body(&owner)["version"], 1, "{owner}");
}

#[test]
fn a_visitor_is_refused_a_document_that_is_not_shared_with_them_before_its_roles() {
    // An editing role on someone else's document: the answer is about the
    // document, and it is the same code the tenant lease answers with.
    let registry = registry();
    let response = serve_roles(
        &registry,
        as_tenant(
            Request::json("POST", "/api/mcp/document", SYNC_BODY).with_bearer("userB@ux_ui"),
            "userA",
        ),
    );
    assert_eq!(
        status_line(&response),
        "HTTP/1.1 403 Forbidden",
        "{response}"
    );
    assert_eq!(body(&response)["error"], "tenant-not-shared", "{response}");
}

#[test]
fn a_browser_session_is_held_to_the_same_answer_as_a_token() {
    // The hole #33 named: a session IS the account and carries every scope, so
    // the scope gate never refused one — a role-less visitor could replace the
    // owner's document with its own cookie.
    let registry = registry();
    grant(&registry, "userA@", "userB");
    let refused = serve_roles(
        &registry,
        as_tenant(
            Request::json("POST", "/api/mcp/document", SYNC_BODY)
                .with_session("userB@")
                .with_origin(PUBLIC_ORIGIN),
            "userA",
        ),
    );
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");
    assert_eq!(body(&refused)["error"], "read-only-role", "{refused}");

    // The browser's own document is still its own to write.
    let own = serve_roles(
        &registry,
        Request::json("POST", "/api/mcp/document", SYNC_BODY)
            .with_session("userA@")
            .with_origin(PUBLIC_ORIGIN),
    );
    assert_eq!(status_line(&own), "HTTP/1.1 200 OK", "{own}");
}

// ---------------------------------------------------------------------------
// #41: the account's own configuration is not part of a shared document.
// ---------------------------------------------------------------------------

/// The settings modal's two writes, as the shell sends them.
fn configuration_writes() -> [(&'static str, &'static str); 2] {
    [
        ("/api/settings/credentials", "{}"),
        ("/api/mcp/server", r#"{"action":"start","port":15000}"#),
    ]
}

#[test]
fn an_editing_role_on_a_shared_document_does_not_reach_the_owners_workspace() {
    // UX/UI may change the document it was given; the account's provider keys
    // and its MCP switch are not part of that document.
    let registry = registry();
    grant(&registry, "userA@", "userB");
    for (path, payload) in configuration_writes() {
        let refused = serve_roles(
            &registry,
            as_tenant(
                Request::json("POST", path, payload).with_bearer("userB@ux_ui"),
                "userA",
            ),
        );
        assert_eq!(
            status_line(&refused),
            "HTTP/1.1 403 Forbidden",
            "{path}: {refused}"
        );
        assert_eq!(
            body(&refused)["error"],
            "read-only-role",
            "{path}: {refused}"
        );
    }
}

#[test]
fn a_refused_configuration_write_leaves_the_workspace_alone() {
    let registry = registry();
    grant(&registry, "userA@", "userB");

    // The owner picks a port, so the value the visitor tries to write has
    // something to overwrite.
    let started = serve_roles(
        &registry,
        Request::json(
            "POST",
            "/api/mcp/server",
            r#"{"action":"start","port":3102}"#,
        )
        .with_bearer("userA@"),
    );
    assert_eq!(status_line(&started), "HTTP/1.1 200 OK", "{started}");
    assert_eq!(body(&started)["port"], 3102, "{started}");

    let refused = serve_roles(
        &registry,
        as_tenant(
            Request::json(
                "POST",
                "/api/mcp/server",
                r#"{"action":"start","port":15000}"#,
            )
            .with_bearer("userB@ux_ui"),
            "userA",
        ),
    );
    assert_eq!(status_line(&refused), "HTTP/1.1 403 Forbidden", "{refused}");

    // A stop carries no port, so the port in the answer is whichever write
    // landed: the visitor's 15000, or the owner's 3102.
    let stopped = serve_roles(
        &registry,
        Request::json("POST", "/api/mcp/server", r#"{"action":"stop"}"#).with_bearer("userA@"),
    );
    assert_eq!(status_line(&stopped), "HTTP/1.1 200 OK", "{stopped}");
    assert_eq!(body(&stopped)["port"], 3102, "{stopped}");
    assert_eq!(body(&stopped)["running"], false, "{stopped}");
}

#[test]
fn an_admin_may_configure_a_workspace_shared_with_them() {
    let registry = registry();
    grant(&registry, "userA@", "userB");
    let admin = serve_roles(
        &registry,
        as_tenant(
            Request::json(
                "POST",
                "/api/mcp/server",
                r#"{"action":"start","port":3210}"#,
            )
            .with_bearer("userB@admin"),
            "userA",
        ),
    );
    assert_eq!(status_line(&admin), "HTTP/1.1 200 OK", "{admin}");
    assert_eq!(body(&admin)["port"], 3210, "{admin}");

    // The credential route answers an admin exactly as it answers the owner:
    // online never persists them, so the refusal is the deployment's, not this
    // gate's.
    let credentials = serve_roles(
        &registry,
        as_tenant(
            Request::json("POST", "/api/settings/credentials", "{}").with_bearer("userB@admin"),
            "userA",
        ),
    );
    assert_ne!(
        body(&credentials)["error"],
        "read-only-role",
        "{credentials}"
    );
}

// ---------------------------------------------------------------------------
// #42: the collaboration panel is not a way round the document gate.
// ---------------------------------------------------------------------------

#[test]
fn a_visitor_without_an_editing_role_cannot_drive_a_session_on_the_document() {
    // Undo applies an editor command to this document and the other actions
    // feed the session that carries the peers' commands — including the two
    // that admit a peer. A caller that may watch a document is not a caller
    // that may drive a session on it; a caller that may edit it, is.
    //
    // Online the relay is unavailable and the panel is a projection, so this
    // refuses a reach rather than a use.
    let visitor_registry = registry();
    grant(&visitor_registry, "userA@", "userB");
    for action in [
        r#"{"type":"requestUndo"}"#,
        r#"{"type":"openCreate"}"#,
        r#"{"type":"approveAdmissionViewer","requestKey":"k"}"#,
    ] {
        let refused = serve_roles(
            &visitor_registry,
            as_tenant(
                Request::json("POST", op_editor_core::collab_routes::ACTION, action)
                    .with_bearer("userB@qa"),
                "userA",
            ),
        );
        assert_eq!(
            status_line(&refused),
            "HTTP/1.1 403 Forbidden",
            "{action}: {refused}"
        );
        assert_eq!(
            body(&refused)["error"],
            "read-only-role",
            "{action}: {refused}"
        );
    }

    // An editing role on the same document reaches the panel, exactly as it
    // reaches every other write on it.
    //
    // Its own registry, because the queue below holds one action for the whole
    // document: a second action is answered `409 collab-busy`, not `202`.
    let editor_registry = registry();
    grant(&editor_registry, "userA@", "userB");
    let editor = serve_roles(
        &editor_registry,
        as_tenant(
            Request::json(
                "POST",
                op_editor_core::collab_routes::ACTION,
                r#"{"type":"openCreate"}"#,
            )
            .with_bearer("userB@ux_ui"),
            "userA",
        ),
    );
    assert_eq!(status_line(&editor), "HTTP/1.1 202 Accepted", "{editor}");

    // The owner's own panel is unaffected.
    let owner_registry = registry();
    let owner = serve_roles(
        &owner_registry,
        Request::json(
            "POST",
            op_editor_core::collab_routes::ACTION,
            r#"{"type":"openCreate"}"#,
        )
        .with_bearer("userA@"),
    );
    assert_eq!(status_line(&owner), "HTTP/1.1 202 Accepted", "{owner}");

    // And the projection the visitor may read is still theirs to read.
    let state = serve_roles(
        &visitor_registry,
        as_tenant(
            Request::new("GET", op_editor_core::collab_routes::STATE).with_bearer("userB@qa"),
            "userA",
        ),
    );
    assert_eq!(status_line(&state), "HTTP/1.1 200 OK", "{state}");
}
