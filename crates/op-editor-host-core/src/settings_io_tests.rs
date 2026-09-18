use super::*;

#[path = "settings_io_checked_tests.rs"]
mod checked_tests;
#[path = "settings_io_save_tests.rs"]
mod save_tests;

#[test]
fn persisted_locale_overrides_the_product_language() {
    assert_eq!(
        resolve_persisted_locale(Locale::Ru, Some("en-US")),
        Locale::EnUs
    );
}

#[test]
fn missing_or_invalid_persisted_locale_preserves_the_product_language() {
    for persisted in [None, Some(""), Some("unknown")] {
        assert_eq!(
            resolve_persisted_locale(Locale::Ru, persisted),
            Locale::Ru,
            "persisted locale {persisted:?} must preserve the caller's seed"
        );
    }
}

#[test]
fn persisted_locale_parsing_uses_shared_bcp47_rules() {
    assert_eq!(
        resolve_persisted_locale(Locale::Ru, Some("EN_us.UTF-8")),
        Locale::EnUs
    );
    assert_eq!(
        resolve_persisted_locale(Locale::Ru, Some("zh-Hant-HK")),
        Locale::ZhTw
    );
    assert_eq!(
        resolve_persisted_locale(Locale::Ru, Some("in-ID")),
        Locale::Id
    );
}

#[test]
fn settings_payload_uses_shared_stable_locale_codes() {
    for locale in Locale::ALL {
        let mut state = EditorState::new();
        state.editor_ui.locale = locale;
        assert_eq!(to_payload(&state).locale.as_deref(), Some(locale.code()));
    }
}

#[test]
fn a_first_run_keeps_the_product_language() {
    // A fresh install has no settings file, so the product's own default is what
    // the first paint uses — the machine's language does not get a vote (see
    // `the_process_environment_does_not_choose_the_product_language`).
    let mut state = EditorState::new();
    let missing = std::env::temp_dir().join("openpencil-settings-that-is-not-there.json");
    let _ = std::fs::remove_file(&missing);

    load_checked_from_path(&mut state, &missing).expect("a missing file is a normal first run");

    assert_eq!(state.editor_ui.locale, Locale::Ru);
}

#[test]
fn the_process_environment_does_not_choose_the_product_language() {
    // The operator's decision, made explicit: the interface is Russian from the
    // very first paint — not because the machine says so. The startup load used
    // to seed the locale from LC_ALL / LC_MESSAGES / LANG before reading the
    // settings file, which made the first run of an English machine English, a
    // Japanese machine Japanese, and so on.
    //
    // A subprocess, because the environment is process-wide and the loader reads
    // it through `dirs::config_dir()`: HOME (and XDG_CONFIG_HOME) point at an
    // empty directory, so the child really is a first run.
    let home = std::env::temp_dir().join("openpencil-locale-probe-home");
    std::fs::create_dir_all(&home).expect("probe home directory");

    for (case, lc_all, lc_messages, lang) in [
        ("lang-only", None, None, Some("ja_JP.UTF-8")),
        (
            "lc-all-wins",
            Some("de_DE.UTF-8"),
            Some("fr_FR"),
            Some("ja_JP"),
        ),
        ("posix-c", Some("C"), None, None),
        ("nothing-set", None, None, None),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .arg("--exact")
            .arg("settings_io::settings_io_tests::system_locale_probe")
            .arg("--ignored")
            .env_remove("LC_ALL")
            .env_remove("LC_MESSAGES")
            .env_remove("LANG")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"));
        if let Some(value) = lc_all {
            command.env("LC_ALL", value);
        }
        if let Some(value) = lc_messages {
            command.env("LC_MESSAGES", value);
        }
        if let Some(value) = lang {
            command.env("LANG", value);
        }

        let output = command.output().expect("locale probe process starts");
        assert!(
            output.status.success(),
            "case={case}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
#[ignore = "executed in an isolated subprocess by the_process_environment_does_not_choose_the_product_language"]
fn system_locale_probe() {
    // The real startup load, on a machine with no settings file at all.
    let mut state = EditorState::new();
    load(&mut state);
    assert_eq!(
        state.editor_ui.locale,
        Locale::Ru,
        "the product's language is its own, whatever LANG says"
    );
}

#[test]
fn apply_payload_persisted_locale_overrides_the_product_language() {
    let payload: SettingsPayload =
        serde_json::from_str(r#"{"version":1,"locale":"en-US"}"#).unwrap();
    let mut state = EditorState::new();
    state.editor_ui.locale = Locale::Ru;

    apply_payload(&mut state, payload);

    assert_eq!(state.editor_ui.locale, Locale::EnUs);
}

#[test]
fn apply_payload_missing_or_invalid_locale_preserves_the_product_language() {
    for json in [
        r#"{"version":1}"#,
        r#"{"version":1,"locale":""}"#,
        r#"{"version":1,"locale":"unknown"}"#,
    ] {
        let payload: SettingsPayload = serde_json::from_str(json).unwrap();
        let mut state = EditorState::new();
        state.editor_ui.locale = Locale::Ru;

        apply_payload(&mut state, payload);

        assert_eq!(
            state.editor_ui.locale,
            Locale::Ru,
            "payload {json} must preserve the product language"
        );
    }
}

#[test]
fn locale_change_updates_settings_fingerprint() {
    let mut state = EditorState::new();
    state.editor_ui.locale = Locale::Ru;
    let before = fingerprint(&state);

    state.editor_ui.locale = Locale::Ja;

    assert_ne!(before, fingerprint(&state));
}

#[test]
fn imported_agents_are_excluded_from_persistence() {
    // A user-entered agent must persist; an auto-imported (e.g. Zode)
    // agent must NOT, so its API key never lands in settings.json.
    let mut state = EditorState::new();
    let manual = state
        .editor_ui
        .agent_settings
        .add_builtin_agent_with_defaults("Manual", "manual-key", "m1");
    let imported = state
        .editor_ui
        .agent_settings
        .add_builtin_agent_with_defaults("Imported", "zode-key", "m2");
    state
        .editor_ui
        .agent_settings
        .imported_agent_ids
        .insert(imported.clone());

    let payload = to_payload(&state);
    let persisted = payload.builtin_agents.unwrap();
    let ids: Vec<_> = persisted.iter().map(|a| a.id.clone()).collect();
    assert!(ids.contains(&manual), "user-entered agent should persist");
    assert!(
        !ids.contains(&imported),
        "imported agent (and its key) must not be persisted"
    );
    assert!(
        persisted.iter().all(|a| a.api_key != "zode-key"),
        "imported API key must never reach settings.json"
    );
}

#[test]
fn connected_state_round_trips_through_payload() {
    // Connect Claude (0) + Antigravity (4) + DeepSeek Harness (6,
    // the append-only tail slot), leave the rest off.
    let mut src = EditorState::new();
    src.editor_ui.agent_settings.connected = [true, false, false, false, true, false, true];
    // Serialize → JSON → deserialize, the real on-disk path.
    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);
    assert_eq!(
        dst.editor_ui.agent_settings.connected,
        [true, false, false, false, true, false, true]
    );
}

#[test]
fn six_cli_mcp_payload_migrates_to_new_cli_count() {
    let payload: SettingsPayload = serde_json::from_str(
        r#"{"version":1,"mcp_cli_enabled":[true,false,true,false,true,true]}"#,
    )
    .unwrap();
    let mut dst = EditorState::new();
    dst.editor_ui.agent_settings.mcp_cli_enabled = [true; 13];

    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.mcp_cli_enabled,
        [true, false, false, true, true, false, false, false, false, false, false, false, false]
    );
}

#[test]
fn seven_cli_mcp_payload_keeps_its_toggles_and_leaves_the_new_clis_off() {
    let payload: SettingsPayload = serde_json::from_str(
        r#"{"version":1,"mcp_cli_enabled":[true,false,true,false,true,false,true]}"#,
    )
    .unwrap();
    let mut dst = EditorState::new();
    dst.editor_ui.agent_settings.mcp_cli_enabled = [true; 13];

    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.mcp_cli_enabled,
        [true, false, true, false, true, false, true, false, false, false, false, false, false]
    );
}

#[test]
fn eleven_cli_mcp_payload_keeps_its_toggles_and_leaves_zcode_off() {
    let payload: SettingsPayload = serde_json::from_str(
        r#"{"version":1,"mcp_cli_enabled":[true,false,true,false,true,false,true,true,false,true,false]}"#,
    )
    .unwrap();
    let mut dst = EditorState::new();
    dst.editor_ui.agent_settings.mcp_cli_enabled = [true; 13];

    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.mcp_cli_enabled,
        [true, false, true, false, true, false, true, true, false, true, false, false, false]
    );
}

#[test]
fn eight_cli_mcp_payload_drops_gemini_without_shifting_later_clis() {
    let payload: SettingsPayload = serde_json::from_str(
        r#"{"version":1,"mcp_cli_enabled":[true,false,true,false,true,false,true,true]}"#,
    )
    .unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.mcp_cli_enabled,
        [true, false, false, true, false, true, true, false, false, false, false, false, false]
    );
}

#[test]
fn seven_provider_connected_payload_is_the_current_layout_and_round_trips() {
    // The current layout has seven slots (DeepSeek Harness appended at
    // the tail). A 7-entry file must round-trip VERBATIM — treating it
    // as the legacy Gemini-era layout would silently shift every saved
    // connection on every load.
    let payload: SettingsPayload = serde_json::from_str(
        r#"{"version":1,"connected":[true,false,true,false,true,true,false]}"#,
    )
    .unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.connected,
        [true, false, true, false, true, true, false]
    );
}

#[test]
fn six_provider_connected_payload_keeps_flags_and_leaves_dsh_off() {
    // Files written since the Gemini retirement have six slots; the
    // appended DeepSeek Harness slot starts disconnected.
    let payload: SettingsPayload =
        serde_json::from_str(r#"{"version":1,"connected":[true,false,true,false,true,true]}"#)
            .unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.connected,
        [true, false, true, false, true, true, false]
    );
}

#[test]
fn five_provider_connected_payload_drops_retired_gemini() {
    let payload: SettingsPayload =
        serde_json::from_str(r#"{"version":1,"connected":[true,false,false,false,true]}"#).unwrap();
    let mut dst = EditorState::new();
    dst.editor_ui.agent_settings.connected = [false, false, false, false, true, true, true];
    apply_payload(&mut dst, payload);
    assert_eq!(
        dst.editor_ui.agent_settings.connected,
        [true, false, false, false, false, false, false]
    );
}

#[test]
fn legacy_settings_without_connected_field_default_to_disconnected() {
    // A settings.json written before the `connected` field
    // existed must still load — the missing field defaults to
    // all-disconnected rather than failing the parse.
    let legacy = r#"{"version":1,"theme":"dark","locale":"en-US"}"#;
    let payload: SettingsPayload = serde_json::from_str(legacy).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);
    assert_eq!(dst.editor_ui.agent_settings.connected, [false; 7]);
}

#[test]
fn builtin_agents_round_trip_through_payload() {
    let mut src = EditorState::new();
    src.editor_ui.agent_settings.add_builtin_agent_config(
        "MiniMax",
        "sk-test",
        "MiniMax-M2.7",
        BuiltinAgentKind::Anthropic,
        "https://api.minimaxi.com/anthropic",
    );

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.builtin_agents.len(), 1);
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].display_name,
        "MiniMax"
    );
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].api_key,
        "sk-test"
    );
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].preset,
        BuiltinAgentPresetKey::MiniMax
    );
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].models,
        ["MiniMax-M2.7"]
    );
}

#[test]
fn builtin_agent_multiple_models_round_trip_with_legacy_first_mirror() {
    let mut src = EditorState::new();
    src.editor_ui.agent_settings.add_builtin_agent_configs(
        "Private",
        "sk-test",
        ["model-a", "model-b", "model-c"],
        BuiltinAgentKind::OpenAiCompat,
        "https://example.com/v1",
    );

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(raw["builtin_agents"][0]["model"], "model-a");
    assert_eq!(
        raw["builtin_agents"][0]["models"],
        serde_json::json!(["model-a", "model-b", "model-c"])
    );

    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].models,
        ["model-a", "model-b", "model-c"]
    );
}

#[test]
fn preferred_agent_team_size_round_trips_and_seeds_chat_on_load() {
    let mut src = EditorState::new();
    src.editor_ui.preferred_agent_team_size = 3;

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.preferred_agent_team_size, 3);
    // `load`'s whole point: tab 0's live chat setting is seeded from the
    // persisted preference, not left at `ChatState::default()`'s 1.
    assert_eq!(
        dst.chat.agent_team_size, 3,
        "tab 0 must be seeded from the persisted preference"
    );
}

#[test]
fn legacy_settings_without_preferred_agent_team_size_default_to_one() {
    // A settings.json written before this field existed must still load —
    // `SettingsPayload::preferred_agent_team_size` deserializes to `None`
    // (`#[serde(default)]`), which is a no-op on the fresh `EditorState`'s
    // already-default value, landing on the same `1` a defaulted
    // `ChatState` starts with — never a parse failure, never some OTHER
    // fallback number.
    let legacy = r#"{"version":1,"theme":"dark","locale":"en-US"}"#;
    let payload: SettingsPayload = serde_json::from_str(legacy).unwrap();
    assert_eq!(payload.preferred_agent_team_size, None);

    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.preferred_agent_team_size, 1);
    assert_eq!(dst.chat.agent_team_size, 1);
}

#[test]
fn duplicate_builtin_agents_are_deduped_on_load() {
    let settings = r#"{
        "version": 1,
        "builtin_agents": [
            {
                "id": "builtin-1",
                "display_name": "MINIMAX",
                "kind": "openai-compat",
                "api_key": "sk-test",
                "model": "MiniMax-M2.7",
                "base_url": "https://api.minimaxi.com/v1",
                "enabled": true
            },
            {
                "id": "builtin-2",
                "display_name": "MINIMAX",
                "kind": "openai-compat",
                "api_key": "sk-test",
                "model": "MiniMax-M2.7",
                "base_url": "https://api.minimaxi.com/v1",
                "enabled": true
            }
        ]
    }"#;
    let payload: SettingsPayload = serde_json::from_str(settings).unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.builtin_agents.len(), 1);
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].id,
        "builtin-1"
    );
    assert_eq!(dst.editor_ui.agent_settings.next_builtin_agent_id, 2);
}

#[test]
fn best_effort_load_keeps_operator_and_browser_owned_provider_cards_separate() {
    let settings = r#"{
        "version": 1,
        "builtin_agents": [
            {
                "id": "builtin-1",
                "preset": "openai",
                "display_name": "Operator",
                "kind": "openai-compat",
                "api_key": "same-key",
                "model": "model-a",
                "base_url": "https://api.openai.com/v1",
                "enabled": true
            },
            {
                "id": "web-credential:builtin:browser-1",
                "preset": "openai",
                "display_name": "Browser",
                "kind": "openai-compat",
                "api_key": "same-key",
                "model": "model-b",
                "base_url": "https://api.openai.com/v1",
                "enabled": true
            }
        ]
    }"#;
    let payload: SettingsPayload = serde_json::from_str(settings).unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    let agents = &dst.editor_ui.agent_settings.builtin_agents;
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].id, "builtin-1");
    assert_eq!(agents[0].models, ["model-a"]);
    assert_eq!(agents[1].id, "web-credential:builtin:browser-1");
    assert_eq!(agents[1].models, ["model-b"]);
}

#[test]
fn duplicate_auto_named_builtin_agents_are_deduped_on_load() {
    let settings = r#"{
        "version": 1,
        "builtin_agents": [
            {
                "id": "builtin-5",
                "display_name": "Built-in Agent 5",
                "kind": "anthropic",
                "api_key": "sk-test",
                "model": "claude-sonnet-4-5",
                "base_url": "https://api.anthropic.com",
                "enabled": true
            },
            {
                "id": "builtin-6",
                "display_name": "Built-in Agent 6",
                "kind": "anthropic",
                "api_key": "sk-test",
                "model": "claude-sonnet-4-5",
                "base_url": "https://api.anthropic.com",
                "enabled": true
            }
        ]
    }"#;
    let payload: SettingsPayload = serde_json::from_str(settings).unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.builtin_agents.len(), 1);
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].display_name,
        "Built-in Agent 5"
    );
    assert_eq!(dst.editor_ui.agent_settings.next_builtin_agent_id, 6);
}

#[test]
fn builtin_agent_payload_without_api_key_loads_as_empty_key() {
    let settings = r#"{
        "version": 1,
        "builtin_agents": [
            {
                "id": "builtin-2",
                "preset": "bailian-coding",
                "display_name": "百炼CP",
                "kind": "openai-compat",
                "model": "qwen3-coder-plus",
                "base_url": "https://coding.dashscope.aliyuncs.com/v1",
                "enabled": false
            }
        ]
    }"#;
    let payload: SettingsPayload = serde_json::from_str(settings).unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.builtin_agents.len(), 1);
    let agent = &dst.editor_ui.agent_settings.builtin_agents[0];
    assert_eq!(agent.display_name, "百炼CP");
    assert!(agent.api_key.is_empty());
    assert!(!agent.enabled);
}

#[test]
fn acp_agents_round_trip_through_payload() {
    let mut src = EditorState::new();
    let mut env = std::collections::BTreeMap::new();
    env.insert("ACP_TOKEN".into(), "secret".into());
    src.editor_ui.agent_settings.add_acp_agent_config(
        "Design Agent",
        AcpConnectionType::Local,
        "/usr/local/bin/design-agent",
        vec!["--stdio".into()],
        env,
        None,
        true,
    );
    src.editor_ui.agent_settings.add_acp_agent_config(
        "Remote Agent",
        AcpConnectionType::Remote,
        "",
        Vec::new(),
        std::collections::BTreeMap::new(),
        Some("ws://localhost:8100".into()),
        false,
    );

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.acp_agents.len(), 2);
    let local = &dst.editor_ui.agent_settings.acp_agents[0];
    assert_eq!(local.display_name, "Design Agent");
    assert_eq!(local.connection_type, AcpConnectionType::Local);
    assert_eq!(local.command, "/usr/local/bin/design-agent");
    assert!(!local.connected);
    assert_eq!(local.args, vec!["--stdio"]);
    assert_eq!(
        local.env.get("ACP_TOKEN").map(String::as_str),
        Some("secret")
    );
    let remote = &dst.editor_ui.agent_settings.acp_agents[1];
    assert_eq!(remote.connection_type, AcpConnectionType::Remote);
    assert_eq!(remote.url.as_deref(), Some("ws://localhost:8100"));
    assert!(!remote.enabled);
    assert_eq!(dst.editor_ui.agent_settings.next_acp_agent_id, 3);
}

#[test]
fn image_generation_profiles_round_trip_through_payload() {
    let mut src = EditorState::new();
    let first = src.editor_ui.agent_settings.add_image_gen_profile();
    let second = src.editor_ui.agent_settings.add_image_gen_profile();
    let second_profile = &mut src.editor_ui.agent_settings.image_gen_profiles[1];
    second_profile.name = "Gemini Image".into();
    second_profile.provider = ImageGenProvider::Gemini;
    second_profile.api_key = "image-key".into();
    second_profile.model = "gemini-image".into();
    second_profile.base_url = Some("https://images.example/v1".into());
    assert!(src
        .editor_ui
        .agent_settings
        .set_active_image_gen_profile(&second));

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.image_gen_profiles.len(), 2);
    assert_eq!(
        dst.editor_ui
            .agent_settings
            .active_image_gen_profile_id
            .as_deref(),
        Some(second.as_str())
    );
    assert_eq!(dst.editor_ui.agent_settings.image_gen_profiles[0].id, first);
    assert_eq!(
        dst.editor_ui.agent_settings.image_gen_profiles[1].provider,
        ImageGenProvider::Gemini
    );
    assert_eq!(
        dst.editor_ui.agent_settings.image_gen_profiles[1].base_url,
        Some("https://images.example/v1".into())
    );
}

#[test]
fn openverse_oauth_round_trips_through_payload() {
    let mut src = EditorState::new();
    src.editor_ui.agent_settings.openverse_client_id = "client-id".into();
    src.editor_ui.agent_settings.openverse_client_secret = "client-secret".into();
    src.editor_ui.agent_settings.openverse_credential_owner = Some("browser".into());

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert_eq!(
        dst.editor_ui.agent_settings.openverse_client_id,
        "client-id"
    );
    assert_eq!(
        dst.editor_ui.agent_settings.openverse_client_secret,
        "client-secret"
    );
    assert_eq!(
        dst.editor_ui
            .agent_settings
            .openverse_credential_owner
            .as_deref(),
        Some("browser")
    );
}

#[test]
fn auto_update_preference_round_trips_through_payload() {
    let mut src = EditorState::new();
    src.editor_ui.agent_settings.auto_update_enabled = false;

    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    let payload: SettingsPayload = serde_json::from_str(&json).unwrap();
    let mut dst = EditorState::new();
    apply_payload(&mut dst, payload);

    assert!(!dst.editor_ui.agent_settings.auto_update_enabled);
}

#[test]
fn explicit_saved_builtin_preset_is_preserved_during_load() {
    let settings = r#"{
        "version": 1,
        "builtin_agents": [
            {
                "id": "builtin-3",
                "preset": "doubao",
                "display_name": "方舟CP",
                "kind": "anthropic",
                "api_key": "sk-test",
                "model": "ark-code-latest",
                "base_url": "https://ark.cn-beijing.volces.com/api/coding",
                "enabled": true
            }
        ]
    }"#;
    let payload: SettingsPayload = serde_json::from_str(settings).unwrap();
    let mut dst = EditorState::new();

    apply_payload(&mut dst, payload);

    assert_eq!(dst.editor_ui.agent_settings.builtin_agents.len(), 1);
    assert_eq!(
        dst.editor_ui.agent_settings.builtin_agents[0].preset,
        BuiltinAgentPresetKey::Doubao
    );
}

#[test]
fn the_model_roles_survive_a_settings_round_trip() {
    // Issues #249/#250: the roles have to live in the settings FILE. Measured:
    // writing them by hand made the daemon print "unknown settings field in
    // root" and offer no shared model at all — the format did not know them.
    let mut state = op_editor_core::EditorState::default();
    state.editor_ui.agent_settings.builder_model = Some("builtin:builtin-1:flash".into());
    state.editor_ui.agent_settings.verifier_model = Some("builtin:builtin-2:vision".into());

    let payload = to_payload(&state);
    let mut restored = op_editor_core::EditorState::default();
    apply_payload(&mut restored, payload);

    assert_eq!(
        restored.editor_ui.agent_settings.builder_model.as_deref(),
        Some("builtin:builtin-1:flash")
    );
    assert_eq!(
        restored.editor_ui.agent_settings.verifier_model.as_deref(),
        Some("builtin:builtin-2:vision")
    );

    // A file written before the roles existed still loads: absent means "the
    // shared model does both", which is the documented fallback.
    let legacy = serde_json::json!({ "version": 1, "theme": "dark" });
    let parsed: SettingsPayload = serde_json::from_value(legacy).expect("a legacy file loads");
    let mut old = op_editor_core::EditorState::default();
    apply_payload(&mut old, parsed);
    assert_eq!(old.editor_ui.agent_settings.builder_model, None);
    assert_eq!(old.editor_ui.agent_settings.verifier_model, None);
}
