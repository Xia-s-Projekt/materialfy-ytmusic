//! System interface preferences.

/// The system asks for fewer animations. Reads the Settings bridge → Settings.systemReducedMotion → Theme.reducedMotion
/// when Settings.animations == "system". Windows — SPI_GETCLIENTAREAANIMATION == FALSE; Linux — GNOME
/// `enable-animations` == false; macOS — "Reduce motion". If it cannot be determined — false.
pub fn reduced_motion() -> bool {
    imp::reduced_motion()
}

/// The taskbar theme (for the tray icon): Some(true) — dark. Only Windows knows it
/// (HKCU\…\Themes\Personalize, SystemUsesLightTheme); on other OSes — None, QML takes Qt's theme.
pub fn taskbar_dark() -> Option<bool> {
    imp::taskbar_dark()
}

#[cfg(target_os = "windows")]
mod imp {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};

    pub fn taskbar_dark() -> Option<bool> {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .ok()?;
        // no value — Windows 10 before 1903: the taskbar is dark
        Some(key.get_value::<u32, _>("SystemUsesLightTheme").unwrap_or(0) == 0)
    }

    pub fn reduced_motion() -> bool {
        let mut enabled: i32 = 1;
        // SAFETY: pvParam points to a BOOL, as SPI_GETCLIENTAREAANIMATION requires
        let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0) };
        ok != 0 && enabled == 0
    }
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn taskbar_dark() -> Option<bool> {
        None
    }

    pub fn reduced_motion() -> bool {
        super::command_output("gsettings", &["get", "org.gnome.desktop.interface", "enable-animations"])
            .is_some_and(|out| out == "false")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn taskbar_dark() -> Option<bool> {
        None
    }

    pub fn reduced_motion() -> bool {
        super::command_output("defaults", &["read", "com.apple.universalaccess", "reduceMotion"])
            .is_some_and(|out| out == "1")
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn taskbar_dark() -> Option<bool> {
        None
    }

    pub fn reduced_motion() -> bool {
        false
    }
}

/// A command's output with edge whitespace stripped; None — no command or an error code.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn answers_without_panic() {
        // the value depends on the machine — we only check that the call works
        let _ = super::reduced_motion();
        let _ = super::taskbar_dark();
    }
}
