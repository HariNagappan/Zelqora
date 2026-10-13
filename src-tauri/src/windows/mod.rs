//! Windows platform integration: shell launching, known folders, screenshot
//! directory, and executable blocking.

pub mod startmenu;

use std::path::{Path, PathBuf};

use crate::error::{NovaError, NovaResult};

/// Open a file/folder/URL with the Windows shell (ShellExecuteW, "open" verb).
/// This is the ONLY way Nova launches anything — never a shell command line.
pub fn shell_execute(target: &str) -> NovaResult<()> {
    shell_execute_with_args(target, "")
}

/// Same, with a fixed argument string. Used for the two Windows shell hosts
/// Nova needs (`explorer.exe shell:AppsFolder\<app id>`); the arguments always
/// come from Nova's own tables, never from a transcript.
pub fn shell_execute_with_args(target: &str, args: &str) -> NovaResult<()> {
    use ::windows::core::PCWSTR;
    use ::windows::Win32::UI::Shell::ShellExecuteW;
    use ::windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let operation = to_wide("open");
    let file = to_wide(target);
    let parameters = if args.is_empty() {
        None
    } else {
        Some(to_wide(args))
    };

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(file.as_ptr()),
            parameters
                .as_ref()
                .map(|value| PCWSTR(value.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecute returns a value <= 32 on failure.
    let code = result.0 as isize;
    if code <= 32 {
        return Err(NovaError::Other(format!(
            "ShellExecute failed (code {code}) while opening '{target}'"
        )));
    }
    Ok(())
}

/// Launch a packaged (Microsoft Store) app by its Application User Model ID,
/// e.g. `Microsoft.Todos_8wekyb3d8bbwe!App`.
///
/// This uses the same activation API the Start menu uses, so a missing or
/// broken package produces a real error that Nova can report instead of a
/// silent no-op.
pub fn launch_packaged_app(aumid: &str) -> NovaResult<()> {
    use ::windows::core::HSTRING;
    use ::windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
    };
    use ::windows::Win32::UI::Shell::{
        ApplicationActivationManager, IApplicationActivationManager, ACTIVATEOPTIONS,
    };

    unsafe {
        // Idempotent per thread; a different apartment model is fine because the
        // activation manager is created in its own server process.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let manager: IApplicationActivationManager =
            CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
                .map_err(|e| {
                    NovaError::Other(format!("could not reach the Windows app activation service: {e}"))
                })?;
        let process_id = manager
            .ActivateApplication(
                &HSTRING::from(aumid),
                &HSTRING::from(""),
                ACTIVATEOPTIONS(0),
            )
            .map_err(|e| NovaError::Other(format!("could not start {aumid}: {e}")))?;
        if process_id == 0 {
            return Err(NovaError::Other(format!(
                "{aumid} did not start (Windows reported no process)"
            )));
        }
    }
    Ok(())
}

/// Resolve a known folder name (already normalized) to a path.
pub fn known_folder(normalized: &str) -> Option<PathBuf> {
    let key = normalized.trim();
    match key {
        "downloads" | "download" | "download folder" => dirs::download_dir(),
        "documents" | "document" | "docs" | "document folder" => dirs::document_dir(),
        "desktop" => dirs::desktop_dir(),
        "pictures" | "picture" | "photos" | "photo" => dirs::picture_dir(),
        "music" | "songs" => dirs::audio_dir(),
        "videos" | "video" => dirs::video_dir(),
        "home" | "user" | "profile" | "home folder" => dirs::home_dir(),
        _ => None,
    }
}

/// The folder where Nova saves screenshots: `<Pictures>/Nova Screenshots`.
pub fn screenshots_dir() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("Nova Screenshots")
}

/// Extensions Nova refuses to launch from voice commands (executables/scripts).
const BLOCKED_EXTENSIONS: &[&str] = &[
    "exe", "msi", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "scr",
    "reg", "hta", "cpl", "jar", "com", "pif",
];

pub fn is_blocked_executable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            BLOCKED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_scripts_and_executables() {
        assert!(is_blocked_executable(Path::new("C:\\evil.exe")));
        assert!(is_blocked_executable(Path::new("C:\\evil.PS1")));
        assert!(is_blocked_executable(Path::new("C:\\evil.bat")));
        assert!(!is_blocked_executable(Path::new("C:\\report.pdf")));
        assert!(!is_blocked_executable(Path::new("C:\\notes.txt")));
        assert!(!is_blocked_executable(Path::new("C:\\no-extension")));
    }

    #[test]
    fn known_folders_resolve() {
        assert!(known_folder("downloads").is_some());
        assert!(known_folder("documents").is_some());
        assert!(known_folder("nonsense-folder-xyz").is_none());
    }
}
