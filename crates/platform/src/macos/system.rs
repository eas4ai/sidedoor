use super::*;

/// Whether the running app is a bundle, which login items require.
fn is_bundled() -> bool {
    NSBundle::mainBundle()
        .bundlePath()
        .to_string()
        .ends_with(".app")
}

pub fn login_item() -> LoginItem {
    if !is_bundled() {
        return LoginItem::Unavailable;
    }
    // SAFETY: querying the main app's own service has no preconditions.
    let status = unsafe { SMAppService::mainAppService().status() };
    match status {
        SMAppServiceStatus::Enabled => LoginItem::On,
        SMAppServiceStatus::RequiresApproval => LoginItem::NeedsApproval,
        _ => LoginItem::Off,
    }
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    if !is_bundled() {
        return Err("only an app bundle can launch at login".into());
    }
    // SAFETY: registering the main app as its own login item is the
    // documented use of `mainAppService`.
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|err| err.localizedDescription().to_string())?;
    if login_item() == LoginItem::NeedsApproval {
        // SAFETY: opening System Settings has no preconditions.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
    Ok(())
}
