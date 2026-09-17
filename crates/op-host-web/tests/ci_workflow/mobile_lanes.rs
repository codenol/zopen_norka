#[test]
fn ios_app_store_workflow_is_reusable_and_supports_exact_source_retries() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workflow = std::fs::read_to_string(repo.join(".github/workflows/ios-app-store.yml"))
        .expect("iOS App Store workflow is readable");
    let remote_ref_gate = std::fs::read_to_string(repo.join("tools/check-op-auth-remote-ref.sh"))
        .expect("remote release ref gate is readable");
    let build_number = std::fs::read_to_string(repo.join("tools/ios-build-number.sh"))
        .expect("iOS App Store build number helper is readable");
    let publisher = std::fs::read_to_string(repo.join("scripts/publish-ios-testflight.sh"))
        .expect("iOS App Store publisher is readable");

    assert!(
        workflow.contains("name: iOS App Store / TestFlight"),
        "the iOS publishing lane should be named for its App Store destination"
    );
    assert!(
        workflow.contains("workflow_call:\n    inputs:\n      release_sha:"),
        "the formal release should call the App Store lane as a reusable workflow"
    );
    assert!(
        workflow.contains("workflow_dispatch:\n    inputs:\n      release_sha:"),
        "App Store publication should also be independently dispatchable"
    );
    assert!(
        !workflow.contains("push:\n    tags: ['v*']"),
        "the reusable App Store lane must not duplicate the formal release tag trigger"
    );
    assert!(
        workflow
            .matches("release_sha:\n        description:")
            .count()
            == 2
            && workflow
                .matches("release_ref:\n        description:")
                .count()
                == 2
            && workflow
                .matches("required: true\n        type: string")
                .count()
                >= 4,
        "reusable and manual publication should require an exact source SHA and release ref"
    );
    assert!(
        workflow.contains(
            "      APPLE_TEAM_ID:\n        description: Apple Developer team identifier\n        required: true",
        ) && workflow.contains("      OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_CN:")
            && workflow.contains("      OPENPENCIL_BUILD_COLLAB_BOOTSTRAP_URL_GLOBAL:")
            && workflow.contains(
                "      IOS_DISTRIBUTION_CERTIFICATE_BASE64:\n        description: Apple Distribution PKCS12 certificate\n        required: false",
            )
            && workflow.contains("      IOS_PROVISIONING_PROFILE_BASE64:"),
        "the reusable lane should declare repository secrets and defer signing secrets to testflight"
    );
    assert!(
        workflow.contains("ref: ${{ inputs.release_sha }}")
            && workflow.contains("OPENPENCIL_RELEASE_SHA: ${{ inputs.release_sha }}")
            && workflow.contains("OPENPENCIL_RELEASE_REF: ${{ inputs.release_ref }}")
            && !workflow.contains("auth_artifact_sha")
            && !workflow.contains(".op-auth-artifact"),
        "called and manual runs must build the explicit exact source without an Auth overlay"
    );
    let preflight = workflow
        .find("Bind requested source to the trusted trigger ref before checkout")
        .expect("the iOS workflow has a pre-checkout trust gate");
    let checkout = workflow
        .find("Checkout the exact release source without credentials")
        .expect("the iOS workflow checks out the selected source");
    assert!(
        preflight < checkout
            && workflow.contains("[[ \"$REQUESTED_RELEASE_SHA\" == \"$GITHUB_SHA\" ]]")
            && workflow.contains("[[ \"$REQUESTED_RELEASE_REF\" == \"$GITHUB_REF\" ]]")
            && workflow.contains("^refs/(heads|tags)/v[0-9]+\\.[0-9]+\\.[0-9]+$"),
        "manual source inputs must be bound inline to the trigger before checkout"
    );
    assert!(
        workflow
            .contains("OPENPENCIL_CANONICAL_REMOTE: https://github.com/ZSeven-W/openpencil.git")
            && workflow.contains("tools/check-op-auth-remote-ref.sh --self-test")
            && remote_ref_gate.contains("$release_ref^{}")
            && remote_ref_gate
                .contains("canonical annotated release tag does not peel to the source commit")
            && remote_ref_gate.contains(
                "canonical lightweight release tag does not point directly at the source commit",
            ),
        "publication must safely resolve lightweight and annotated refs to the exact canonical source"
    );
    assert!(
        workflow.contains("tools/ios-build-number.sh --self-test")
            && workflow.contains("build_number=$(tools/ios-build-number.sh)")
            && build_number.contains("epoch_minutes=$((10#$epoch_seconds / 60))")
            && build_number.contains("10000000 1000.0.0")
            && build_number.contains("99999999 9999.99.99")
            && workflow.contains("^[1-9][0-9]{0,3}\\.([0-9]|[1-9][0-9])\\.([0-9]|[1-9][0-9])$",)
            && publisher.contains("^[1-9][0-9]{0,3}\\.([0-9]|[1-9][0-9])\\.([0-9]|[1-9][0-9])$",)
            && publisher
                .contains("iOS build number must use conservative 4.2.2 numeric components")
            && workflow.contains("group: ios-app-store-tech-zseven-openpencil")
            && workflow.contains("cancel-in-progress: false")
            && !workflow.contains("GITHUB_RUN_NUMBER")
            && !workflow.contains("GITHUB_RUN_ID"),
        "independent and called workflows need one monotonic Apple-compatible build number space"
    );
    assert!(
        workflow.contains("tools/check-op-auth-release-matrix.test.sh")
            && workflow.contains("tools/check-op-auth-release-matrix.sh")
            && workflow.contains("tools/check-op-auth-prebuilt.sh --require-hardened")
            && !workflow.contains("tools/check-op-auth-artifact-commit.sh")
            && !workflow.contains("OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION")
            && !workflow.contains("OP_AUTH_RELEASE_WORKSPACE_VERSION")
            && workflow
                .contains("IOS_MARKETING_VERSION: ${{ needs.verify.outputs.version }}"),
        "the iOS lane must validate the adopted matrix independently of the app version and source SHA"
    );
    assert!(
        workflow.contains("environment: testflight")
            && workflow.contains("run: scripts/publish-ios-testflight.sh"),
        "the protected App Store job should reuse the existing testflight environment and publisher"
    );
    assert!(
        workflow.contains("ITSAppUsesNonExemptEncryption=NO")
            && workflow.contains("deliberately carries no compliance code")
            && !workflow.contains("IOS_USES_NON_EXEMPT_ENCRYPTION")
            && !workflow.contains("IOS_ENCRYPTION_EXPORT_COMPLIANCE_CODE")
            && publisher.contains("INFOPLIST_KEY_ITSAppUsesNonExemptEncryption=NO")
            && publisher.contains("== false ]]")
            && publisher.contains("Print :ITSEncryptionExportComplianceCode")
            && !publisher.contains("INFOPLIST_KEY_ITSEncryptionExportComplianceCode")
            && !publisher.contains("IOS_USES_NON_EXEMPT_ENCRYPTION")
            && !publisher.contains("IOS_ENCRYPTION_EXPORT_COMPLIANCE_CODE"),
        "the reviewed exempt-encryption decision must stay source-controlled and omit a compliance code"
    );
}

#[test]
fn formal_release_calls_mobile_distribution_lanes_without_coupling_ios_failure() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let release = std::fs::read_to_string(repo.join(".github/workflows/rust-release.yml"))
        .expect("Rust release workflow is readable");
    let android = std::fs::read_to_string(repo.join(".github/workflows/android-release.yml"))
        .expect("Android release workflow is readable");
    let flattener = std::fs::read_to_string(repo.join("tools/flatten-release-artifacts.sh"))
        .expect("release artifact flattener is readable");

    assert!(
        release.contains("uses: ./.github/workflows/android-release.yml")
            && release.contains("environment: testflight")
            && release.contains("run: scripts/publish-ios-testflight.sh")
            && release.contains(
                "IOS_DISTRIBUTION_CERTIFICATE_BASE64: ${{ secrets.IOS_DISTRIBUTION_CERTIFICATE_BASE64 }}",
            )
            && !release.contains("auth_artifact_ref")
            && !release.contains("tools/check-op-auth-artifact-commit.sh"),
        "the formal tag release should build Android assets and publish iOS from the protected environment"
    );
    assert!(
        release.contains(
            "needs: [version, android-release, build, web-docker, sdk-packages, vsix]",
        ) && !release.contains(
            "needs: [version, android-release, ios-app-store, build, web-docker, sdk-packages, vsix]",
        ),
        "signed Android assets must join the GitHub Release while App Store failure stays isolated"
    );
    assert!(
        release
            .contains("run: tools/flatten-release-artifacts.sh dist release-files \"$OP_VERSION\"",)
            && flattener.contains("expected_apk=\"OpenPencil-${version}-android.apk\"")
            && flattener.contains("expected_aab=\"OpenPencil-${version}-android.aab\"")
            && flattener.contains("sha256sum --check SHA256SUMS.android.txt"),
        "GitHub Release must flatten and verify the exact signed Android APK/AAB handoff"
    );
    assert!(
        android.contains("\"on\":\n  workflow_call:")
            && !android.contains("workflow_dispatch:")
            && !android.contains("push:\n    tags:")
            && android.contains("environment: release-production")
            && android.contains("name: openpencil-android-${{ needs.verify.outputs.version }}")
            && android.contains("*.apk")
            && android.contains("*.aab")
            && android.contains("does not upload to an app store")
            && android.contains("ref: ${{ needs.verify.outputs.source_sha }}")
            && !android.contains(".op-auth-artifact")
            && !android.contains("OP_AUTH_RELEASE_EXPECTED_OPENPENCIL_REVISION"),
        "Android must be a protected reusable asset lane, not an independent Play publisher"
    );
}
