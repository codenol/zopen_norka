/// The deploy lane builds two things and nothing else, and publishes them only
/// where publishing is wanted.
///
/// Issue #90: the "do not simplify this into a workspace build" rule lived in a
/// COMMENT, and a comment fails no build. The workspace also holds the desktop
/// host (winit, accesskit, a GL-linked skia), the mobile hosts and the
/// FFI/JNI crates — none of them is deployed to a web server, and building them
/// here costs tens of minutes per tag (the exact cost #74 exists to remove from
/// the production box).
fn web_deploy_workflow() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/web-deploy-build.yml"
    ))
    .expect("web-deploy-build workflow is readable")
}

/// The shell a step actually runs, so a rule about COMMANDS is not tripped by a
/// comment that explains the rule.
///
/// The workflow's own header says "DO NOT simplify either job into a workspace
/// build", which contains the words this test forbids — in a comment.
fn run_scripts(workflow: &str) -> Vec<String> {
    let mut scripts: Vec<String> = Vec::new();
    let mut block_indent: Option<usize> = None;
    let mut body: Vec<&str> = Vec::new();
    for line in workflow.lines() {
        let indent = line.len() - line.trim_start().len();
        match block_indent {
            Some(opened) if indent > opened => body.push(line),
            Some(_) => {
                scripts.push(body.join("\n"));
                body.clear();
                block_indent = None;
            }
            None => {}
        }
        if block_indent.is_none() && line.trim_start().starts_with("run: |") {
            block_indent = Some(indent);
        }
    }
    if block_indent.is_some() {
        scripts.push(body.join("\n"));
    }
    scripts
}

#[test]
fn the_deploy_lane_never_builds_the_workspace() {
    let workflow = web_deploy_workflow();
    let scripts = run_scripts(&workflow);
    assert!(!scripts.is_empty(), "the workflow runs something");

    for script in &scripts {
        for forbidden in ["--workspace", "--all "] {
            assert!(
                !script.contains(forbidden),
                "web-deploy-build must select its crates with -p, not `{forbidden}`: \
                 the workspace carries hosts this lane does not deploy\n{script}"
            );
        }
    }
    assert!(
        workflow.contains("cargo build -p op-host-web-server"),
        "the daemon half is built by its own -p selection"
    );
    assert!(
        workflow.contains("tools/check-wasm-bundle.sh"),
        "the bundle half goes through the one recipe every lane shares, \
         rather than a second copy of it"
    );
}

#[test]
fn the_deploy_lane_publishes_nothing_on_a_pull_request() {
    let workflow = web_deploy_workflow();

    // The guard sits in the same STEP as the `uses:`, so it is looked for there
    // rather than counted across the file: a job-level guard is not a
    // step-level one, and a third guard elsewhere does not cover an unguarded
    // upload.
    let lines: Vec<&str> = workflow.lines().collect();
    let mut uploads = 0;
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("uses: actions/upload-artifact") {
            continue;
        }
        uploads += 1;
        let guarded = lines[index.saturating_sub(4)..index]
            .iter()
            .any(|line| line.contains("github.event_name != 'pull_request'"));
        assert!(
            guarded,
            "the upload at line {} must stand down on a pull request: a PR run \
             proves the recipe, it does not publish",
            index + 1
        );
    }
    assert!(
        uploads >= 2,
        "expected both halves to be uploaded, found {uploads}"
    );
}

#[test]
fn the_deploy_lane_pins_wasm_bindgen_to_the_locked_version() {
    let workflow = web_deploy_workflow();

    assert!(
        workflow.contains("name = \"wasm-bindgen\""),
        "the CLI version must be READ from Cargo.lock, not written down twice"
    );
    assert!(
        workflow.contains("Cargo.lock"),
        "and Cargo.lock is where it is read from"
    );
    assert!(
        workflow.contains("tools/pinned-release-tools.sh cargo-cli wasm-bindgen-cli"),
        "the installer must be the pinned one; a bare `cargo install` ignores \
         the version it just resolved"
    );
}

#[test]
fn the_deploy_lane_keeps_the_js_glue_assertion() {
    let workflow = web_deploy_workflow();

    // The bundle shipped without wasm-bindgen's `snippets/` once, and the page
    // 404s on the JS modules when it does.
    assert!(
        workflow.contains("snippets/"),
        "the assembly step must still assert the wasm-bindgen JS glue is present"
    );
}
