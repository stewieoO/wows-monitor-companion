use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use std::{env, error::Error};

const APP_NAME: &str = "WoWs Monitor Companion";

fn launcher() -> Result<AutoLaunch, Box<dyn Error>> {
    let executable = env::current_exe()?;
    let executable = executable.to_string_lossy();
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(APP_NAME)
        .set_app_path(&executable)
        .set_args(&["--minimized"]);

    #[cfg(target_os = "windows")]
    builder.set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser);

    #[cfg(target_os = "linux")]
    builder.set_linux_launch_mode(auto_launch::LinuxLaunchMode::XdgAutostart);

    Ok(builder.build()?)
}

pub fn is_enabled() -> bool {
    match launcher() {
        Ok(launcher) => launcher.is_enabled().unwrap_or(false),
        Err(_) => false,
    }
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let launcher = launcher().map_err(|error| error.to_string())?;
    if enabled {
        launcher.enable().map_err(|error| error.to_string())
    } else {
        launcher.disable().map_err(|error| error.to_string())
    }
}
