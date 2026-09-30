use super::{Owner, Prepared};
use std::{
    fs::{self, File},
    process::{Command, Stdio},
};

pub(super) fn launch(update: Prepared) -> Result<(), String> {
    let installation = &update.release.installation;
    // A symlinked portable install needs its real directory replaced, not a link.
    let root = installation
        .root
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if root.parent().is_none() {
        return Err("The app installation folder is invalid.".into());
    }
    if matches!(installation.owner, Owner::MacBundle | Owner::Portable) {
        let probe = tempfile::Builder::new().prefix(".sidedoor-write-test-").tempdir_in(root.parent().unwrap())
            .map_err(|_| "The installation folder isn't writable. Move Sidedoor to a folder you own before updating.")?;
        drop(probe);
    }
    let windows = installation.os == "windows";
    let helper = update
        .staging
        .path()
        .join(if windows { "install.ps1" } else { "install.sh" });
    fs::write(
        &helper,
        if windows {
            include_str!("helpers/install.ps1")
        } else {
            include_str!("helpers/install.sh")
        },
    )
    .map_err(|e| e.to_string())?;
    let log_dir = update
        .staging
        .path()
        .parent()
        .ok_or("Missing update cache.")?
        .to_path_buf();
    let receipt = update.staging.path().join("started-version");
    let ready = update.staging.path().join("helper-ready");
    let error = log_dir.join("last-update-error.txt");
    let log = File::create(log_dir.join("last-update.log")).map_err(|e| e.to_string())?;
    let kind = match installation.owner {
        Owner::MacBundle => "macos",
        Owner::Portable => "portable",
        Owner::Debian => "deb",
        Owner::Rpm => "rpm",
        Owner::WindowsMsi => "msi",
        Owner::WindowsSetup => "setup",
    };
    let mut command = Command::new(if windows { "powershell.exe" } else { "/bin/sh" });
    if windows {
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
    }
    command
        .arg(&helper)
        .arg(std::process::id().to_string())
        .arg(&root)
        .arg(&update.payload)
        .arg(&installation.executable)
        .arg(&receipt)
        .arg(&update.release.version)
        .arg(kind)
        .arg(&error)
        .arg(&update.release.asset.sha256)
        .arg(update.staging.path().join(&update.release.asset.name))
        .arg(&ready);
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW; elevation UI is owned by the installer.
    }
    let mut helper_process = command
        .spawn()
        .map_err(|_| "Couldn't start the update helper. The current app is unchanged.")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready.is_file() {
        if helper_process
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
            || std::time::Instant::now() >= deadline
        {
            let _ = helper_process.kill();
            let _ = helper_process.wait();
            return Err(
                "The update helper couldn't initialize. Sidedoor will keep running.".into(),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    // The helper owns cleanup after handoff. Dropping Prepared earlier cleans
    // failed/cancelled downloads, while quitting must not delete a live helper.
    let _ = update.staging.keep();
    Ok(())
}
