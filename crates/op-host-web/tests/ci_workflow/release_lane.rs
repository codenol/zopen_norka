#[test]
fn release_workflow_documents_current_canvaskit_bundle_path() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    assert!(
        workflow.contains("wasm-bundle-build.yml"),
        "release workflow notes should point at the current CanvasKit bundle workflow"
    );
    assert!(
        !workflow.contains("--features skia"),
        "release workflow must not describe the retired skia wasm build path"
    );
    assert!(
        !workflow.contains("EMSDK"),
        "release workflow must not describe the retired EMSDK wasm build path"
    );
}

#[test]
fn release_workflow_resolves_macos_signing_identity_from_keychain() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    assert!(
        workflow.contains("Resolve macOS signing identity"),
        "release workflow should resolve the imported Developer ID identity before signing"
    );
    assert!(
        workflow.contains("security find-identity -v -p codesigning"),
        "release workflow should query the imported keychain identity instead of guessing it"
    );
    assert!(
        !workflow.contains("Developer ID Application ($APPLE_TEAM_ID)"),
        "release workflow must not hard-code an incomplete Developer ID identity"
    );
}

#[test]
fn release_workflow_notarizes_macos_app_before_packaging_dmg() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    let notarize = workflow
        .find("Notarize macOS app")
        .expect("release workflow should notarize the .app before packaging");
    let package = workflow
        .find("Package DMG (macos)")
        .expect("release workflow should package a DMG after app notarization");
    let notarize_dmg = workflow
        .find("Notarize DMG (macos)")
        .expect("release workflow should notarize the final DMG artifact");
    assert!(
        notarize < package,
        "release workflow should notarize and staple OpenPencil.app before creating the DMG"
    );
    assert!(
        package < notarize_dmg,
        "release workflow should notarize the final DMG after packaging the stapled app"
    );
    assert!(
        workflow.contains("ditto -c -k --keepParent \"$APP\""),
        "release workflow should submit a zipped .app bundle to Apple notarytool"
    );
    assert!(
        workflow.contains("ditto \"$APP\" \"$STAGE/OpenPencil.app\""),
        "release workflow should preserve stapled app metadata when staging the DMG"
    );
    assert!(
        workflow.contains("xcrun stapler validate \"$STAGE/OpenPencil.app\""),
        "release workflow should validate the staged app before creating the DMG"
    );
    assert!(
        workflow.contains("xcrun stapler staple \"$APP\""),
        "release workflow should staple the notarization ticket to the .app before DMG packaging"
    );
    assert!(
        workflow.contains("xcrun stapler staple \"$DMG\""),
        "release workflow should staple the final DMG so Gatekeeper can validate the downloaded artifact"
    );
    assert!(
        workflow.contains("xcrun stapler validate \"$DMG\""),
        "release workflow should validate the final stapled DMG"
    );
    assert!(
        workflow.contains("xcrun notarytool log"),
        "release workflow should fetch the Apple notary log when notarization fails"
    );
    assert!(
        workflow.contains("::error::macOS app notarization failed"),
        "macOS app notarization failures should block the release"
    );
    assert!(
        workflow.contains("echo \"::error::macOS app notarization failed\"\n            exit 1"),
        "invalid notarization status can be reported with a zero notarytool exit code, so the workflow must exit non-zero explicitly"
    );
    assert!(
        !workflow.contains("exit \"$notary_status\""),
        "workflow must not reuse notarytool's exit code after parsing an Invalid status"
    );
    assert!(
        workflow.contains("::error::macOS DMG notarization failed"),
        "final DMG notarization failures should block the release"
    );
    assert!(
        !workflow.contains("cp -R \"$APP\" \"$STAGE/OpenPencil.app\""),
        "workflow should not use cp -R for the stapled app bundle because it can lose notarization metadata"
    );
    assert!(
        !workflow.contains("signed but not notarized"),
        "release workflow must not upload macOS artifacts after notarization fails"
    );
}

#[test]
fn release_workflow_does_not_publish_unsigned_macos_desktop_tarball() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    assert!(
        workflow.contains("if [ \"$RUNNER_OS\" != \"macOS\" ]; then\n            tar czf ../../../openpencil-desktop-${{ matrix.label }}.tar.gz openpencil-desktop"),
        "macOS release artifacts should not include the raw desktop binary tarball because the signed/notarized desktop artifact is the DMG"
    );
    assert!(
        workflow.contains("tar czf ../../../op-cli-${{ matrix.label }}.tar.gz op"),
        "macOS and Linux release artifacts should keep publishing standalone op CLI archives"
    );

    let macos_block_start = workflow
        .find("#   macOS")
        .expect("workflow should document macOS artifacts");
    let windows_block_start = workflow
        .find("#   Windows")
        .expect("workflow should document Windows artifacts");
    let macos_block = &workflow[macos_block_start..windows_block_start];
    assert!(
        !macos_block.contains("openpencil-desktop-<label>.tar.gz"),
        "release workflow comments should not document a macOS raw desktop tarball"
    );
    assert!(
        macos_block.contains("op-cli-<label>.tar.gz"),
        "release workflow comments should still document the macOS CLI archive"
    );
}

#[test]
fn macos_bundle_signing_uses_hardened_runtime_for_developer_id() {
    let script = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/bundle-macos.sh"
    ))
    .expect("bundle-macos script is readable");

    assert!(
        script.contains("--timestamp --options runtime"),
        "Developer ID macOS signing should enable hardened runtime and secure timestamp"
    );
    assert!(
        script.contains("sign_macos_code \"$APP/Contents/MacOS/openpencil-desktop\""),
        "bundle script should explicitly sign the main app executable before signing the bundle"
    );
    assert!(
        script.contains("sign_macos_code \"$APP/Contents/MacOS/op\""),
        "bundle script should sign the embedded op CLI before signing the bundle"
    );

    let sign_cli = script
        .find("sign_macos_code \"$APP/Contents/MacOS/op\"")
        .expect("bundle script signs embedded op CLI");
    let sign_main = script
        .find("sign_macos_code \"$APP/Contents/MacOS/openpencil-desktop\"")
        .expect("bundle script signs the main app executable");
    assert!(
        sign_cli < sign_main,
        "bundle script should sign embedded CLI executables before the main app executable"
    );
}

#[test]
fn release_workflow_installs_nsis_on_windows_runner() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    let install_step_start = workflow
        .find("- name: Install NSIS (windows)")
        .expect("release workflow should install NSIS before invoking makensis");
    let following_step_offset = workflow[install_step_start..]
        .find("- name: Validate Windows signing input policy")
        .expect("NSIS installation should remain a dedicated release step");
    let install_step = &workflow[install_step_start..install_step_start + following_step_offset];
    let self_test = install_step
        .find("& tools/install-pinned-nsis.ps1 -SelfTest")
        .expect("the digest-pinned NSIS installer should reject checksum mismatches first");
    let install = install_step
        .find("\n          & tools/install-pinned-nsis.ps1\n")
        .expect("the verified NSIS installer should run after its self-test");

    assert!(
        self_test < install,
        "the checksum self-test must pass before the digest-pinned NSIS install"
    );
    assert!(
        !workflow.contains("choco install"),
        "release workflow must not regress to an unpinned Chocolatey install"
    );
}

#[test]
fn release_workflow_services_pinned_vc_runtime_for_nsis_and_scoop() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workflow = std::fs::read_to_string(repo.join(".github/workflows/rust-release.yml"))
        .expect("rust-release workflow is readable");
    assert!(
        workflow
            .find("& tools/stage-pinned-vcredist.ps1 -ValidateBuildToolset")
            .expect("release workflow should run the MSVC compatibility gate")
            < workflow
                .find("- name: Build (host)")
                .expect("release workflow should build the host"),
        "the Windows MSVC compatibility gate must run before the host build"
    );
    let stage_start = workflow
        .find("- name: Stage pinned Microsoft Visual C++ Redistributable (windows)")
        .expect("release workflow should stage the pinned VC++ Redistributable");
    let package_start = workflow
        .find("- name: Package NSIS installer (windows)")
        .expect("release workflow should package the NSIS installer");
    assert!(
        stage_start < package_start,
        "the verified VC++ Redistributable must be staged before makensis runs"
    );
    let stage_step = &workflow[stage_start..package_start];
    assert!(
        stage_step.contains("& tools/stage-pinned-vcredist.ps1 -SelfTest")
            && stage_step.contains("& tools/stage-pinned-vcredist.ps1 -Destination $vcRedist"),
        "the Windows release must run the network-free stager self-test before staging its payload"
    );
    assert!(
        stage_step.contains("VC_REDIST_FILE=$vcRedist") && stage_step.contains("$env:GITHUB_ENV"),
        "the staged VC++ Redistributable path must cross the PowerShell step boundary explicitly"
    );
    let package_end = workflow[package_start..]
        .find("- name: Sign NSIS installer (windows)")
        .map(|offset| package_start + offset)
        .expect("NSIS packaging should be followed by its signing step");
    let package_step = &workflow[package_start..package_end];
    assert!(
        package_step.contains("makensis")
            && package_step.contains(r#"/DVC_REDIST_FILE=$env:VC_REDIST_FILE"#),
        "makensis must receive the staged VC++ Redistributable as a mandatory input"
    );
    let scoop_start = workflow
        .find("- name: Update Scoop manifests")
        .expect("release workflow should update Scoop manifests");
    let scoop_end = workflow[scoop_start..]
        .find("- name: Commit Scoop updates")
        .map(|offset| scoop_start + offset)
        .expect("Scoop manifest generation should precede its commit step");
    let scoop_step = &workflow[scoop_start..scoop_end];
    assert!(
        scoop_step.matches("write_scoop_manifest \\").count() == 2,
        "desktop and CLI Scoop manifests must both use the reviewed shared generator"
    );
    assert!(
        !scoop_step.contains("url: [$x64_url")
            && !scoop_step.contains("hash: [$x64_hash")
            && scoop_step.contains("--arg redist_url \"$VC_REDIST_URL\"")
            && scoop_step.contains("--arg redist_sha256 \"$VC_REDIST_SHA256\"")
            && scoop_step.contains("installer: $installer"),
        "desktop and CLI Scoop manifests must retain app-only assets plus the conditional Runtime hook"
    );
    let runtime_precheck = scoop_step
        .find("$installedVersion = Get-InstalledVcRuntimeVersion")
        .expect("Scoop should check the installed Runtime");
    let runtime_download = scoop_step
        .find("Invoke-WebRequest -Uri $redistUrl")
        .expect("Scoop should conditionally download the Runtime");
    assert!(
        runtime_precheck < runtime_download
            && scoop_step.contains("ReparsePoint")
            && scoop_step.contains("Get-FileHash")
            && scoop_step.contains("$downloadedFileVersion")
            && scoop_step.contains("$downloadedProductVersion")
            && scoop_step.contains("Get-AuthenticodeSignature")
            && scoop_step.contains("X509NameType]::SimpleName")
            && scoop_step.matches("Get-InstalledVcRuntimeVersion").count() == 3,
        "Scoop must verify the Runtime before elevated servicing and post-check it afterward"
    );
}

#[test]
fn windows_nsis_installer_services_vc_runtime_fail_closed() {
    let installer = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/package-windows.nsi"
    ))
    .expect("Windows NSIS installer is readable");
    assert!(
        installer.contains("!ifndef VC_REDIST_FILE")
            && installer.contains(r#"File "/oname=${VC_REDIST_EXE}" "${VC_REDIST_FILE}""#)
            && !installer.contains(r#"File /nonfatal "/oname=${VC_REDIST_EXE}""#),
        "the VC++ Redistributable must be a required, embedded NSIS input"
    );
    assert!(
        installer.contains(
            "!getdllversion /packed /productversion \"${VC_REDIST_FILE}\" VC_REDIST_VERSION_",
        ) && !installer.contains("!getdllversion /noerrors")
            && installer.contains("StrCpy $VCRuntimeRequiredHigh \"${VC_REDIST_VERSION_HIGH}\"")
            && installer.contains("StrCpy $VCRuntimeRequiredLow \"${VC_REDIST_VERSION_LOW}\"")
            && installer.contains("SetRegView 64")
            && installer.contains(r#"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\${ARCH}"#)
            && installer.contains("Installed"),
        "NSIS must compare the embedded Runtime version with the installed machine Runtime"
    );
    assert!(
        installer.contains("ExecWait")
            && installer.contains("/install /passive /norestart")
            && installer.contains("OpenPencil-vc-redist-${ARCH}.log"),
        "NSIS must invoke the Microsoft Runtime installer non-interactively and retain its log"
    );
    assert!(
        installer.contains("SetRebootFlag true")
            && installer.contains("Function .onInstSuccess")
            && installer.contains("SetErrorLevel 3010"),
        "NSIS must preserve reboot-required state and re-check already-installed status"
    );
    assert!(
        installer.contains("MB_ICONSTOP")
            && installer
                .matches("Abort \"Microsoft Visual C++ Redistributable")
                .count()
                == 3,
        "a failed Runtime install or post-install version check must abort OpenPencil installation"
    );
}

#[test]
fn windows_cli_installer_services_vc_runtime_fail_closed() {
    let installer = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/install-op.ps1"
    ))
    .expect("Windows CLI installer is readable");
    assert!(
        installer.contains("14.51.36247.0")
            && installer.contains("https://aka.ms/vs/18/release/14.51.36247/VC_redist.x64.exe")
            && installer
                .contains("843068991daaa1f73ad9f6239bce4d0f6a07a51f18c37ea2a867e9beca71295c"),
        "the CLI installer must bind the reviewed VC++ Runtime version, URL, and digest"
    );
    assert!(
        installer.contains("function Get-InstalledVcRedistVersion")
            && installer.contains("[Microsoft.Win32.RegistryView]::Registry64")
            && installer.contains(
                r#"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\$RuntimeArch"#,
            )
            && installer.contains("$env:PROCESSOR_ARCHITEW6432")
            && installer.contains("$env:PROCESSOR_ARCHITECTURE")
            && installer.contains(r#"$VcRuntimeArch = "arm64""#)
            && installer.contains(r#"$VcRuntimeArch = "x64""#),
        "the CLI installer must query the native-architecture Runtime key through the 64-bit registry view"
    );
    let service_start = installer
        .find("function Install-VcRedistIfRequired")
        .expect("CLI installer should define Runtime servicing");
    let service_end = installer[service_start..]
        .find("function Resolve-Version")
        .map(|offset| service_start + offset)
        .expect("Runtime servicing should be isolated before release resolution");
    let service = &installer[service_start..service_end];
    assert_eq!(
        service.matches("Get-InstalledVcRedistVersion").count(),
        2,
        "Runtime servicing must check the installed version both before and after installation"
    );
    assert!(
        service.contains("$InstallerItem.PSIsContainer")
            && service.contains("[System.IO.FileAttributes]::ReparsePoint")
            && service.contains("$InstallerItem.Length -le 0")
            && service.contains("Get-AuthenticodeSignature")
            && service.contains("[System.Management.Automation.SignatureStatus]::Valid")
            && service.contains("GetNameInfo")
            && service.contains("X509NameType]::SimpleName")
            && service.contains("$SignerName -cne \"Microsoft Corporation\"")
            && service.contains("O=Microsoft Corporation"),
        "the downloaded Runtime must be a regular file with a valid Microsoft Authenticode signer"
    );
    assert!(
        service.contains("Start-Process")
            && service.contains("-Verb RunAs -Wait -PassThru")
            && service.contains("/install /passive /norestart /log")
            && service.contains("@(0, 3010, 1638)"),
        "the Runtime installer must run elevated/passively and accept only reviewed success statuses"
    );
    assert!(
        service.contains("$null -eq $InstalledVersion -or $InstalledVersion -lt $VcRedistVersion")
            && service.contains("throw \"install-op: Visual C++ runtime"),
        "the CLI install must fail closed when the post-install Runtime version is insufficient"
    );
    let service_call = installer
        .find("$VcRedistRebootRequired = Install-VcRedistIfRequired")
        .expect("CLI install should service the Runtime");
    let archive_download = installer
        .find("Invoke-WebRequest -Uri $Url -OutFile $Archive")
        .expect("CLI install should download its release archive");
    assert!(
        service_call < archive_download,
        "the VC++ Runtime prerequisite must be ready before installing the CLI archive"
    );
    let reboot_branch = installer
        .rfind("if ($VcRedistRebootRequired)")
        .expect("CLI installer should branch on Runtime reboot status");
    let reboot_branch = &installer[reboot_branch..];
    assert!(
        reboot_branch
            .find("} else {")
            .zip(reboot_branch.find("& $Target --version"))
            .is_some_and(|(otherwise, verify)| otherwise < verify),
        "a 3010 Runtime result must not execute op before Windows restarts"
    );
}

#[test]
fn release_workflow_distinguishes_published_npm_packages_from_check_failures() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    assert!(
        workflow.contains("already published; skipping"),
        "release workflow should skip npm packages that are already published"
    );
    assert!(
        workflow.contains("E404") && workflow.contains("failed to check npm package"),
        "release workflow should publish only on npm 404 and fail on other npm view errors"
    );
}

#[test]
fn release_workflow_downloads_only_openpencil_release_artifacts() {
    let workflow = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust-release.yml"
    ))
    .expect("rust-release workflow is readable");

    assert!(
        workflow.contains("pattern: openpencil-*"),
        "release workflow should not download Docker build metadata artifacts into release assets"
    );
}
