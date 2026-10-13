//! Windows skill: open applications, files and folders, take screenshots.
//! All launches go through predefined safe operations (ShellExecute with the
//! "open" verb) — never through a shell.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tauri::{AppHandle, Runtime};

use crate::commands::SkillOutcome;
use crate::error::{NovaError, NovaResult};
use crate::util::{normalize_text, title_similarity};
use crate::windows::{
    is_blocked_executable, known_folder, launch_packaged_app, screenshots_dir, shell_execute,
};
use crate::windows::startmenu;

/// A resolved application: what to launch, plus the name Nova should say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedApp {
    pub target: LaunchTarget,
    /// The matched name (e.g. "Google Chrome") — never the raw spoken phrase.
    pub label: String,
}

impl ResolvedApp {
    fn new(target: LaunchTarget, label: impl Into<String>) -> Self {
        ResolvedApp {
            target,
            label: label.into(),
        }
    }
}

/// How Nova will start an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// A command line, URL or shortcut handled by ShellExecute.
    Path(String),
    /// A packaged (Store) app, launched through its App User Model ID.
    Aumid(String),
}

impl LaunchTarget {
    /// Human-readable description for the activity log.
    pub fn describe(&self) -> String {
        match self {
            LaunchTarget::Path(path) => path.clone(),
            LaunchTarget::Aumid(aumid) => format!("packaged app {aumid}"),
        }
    }

    /// Launch the application.
    pub fn launch(&self) -> NovaResult<()> {
        match self {
            LaunchTarget::Path(path) => shell_execute(path),
            LaunchTarget::Aumid(aumid) => launch_packaged_app(aumid),
        }
    }
}

/// Curated packaged (Microsoft Store) apps. They have no Start-Menu shortcut and
/// no usable executable path, so they are launched by App User Model ID. Nova
/// checks that the package is actually installed first, and reports an honest
/// failure when it is not.
const PACKAGED_APPS: &[(&str, &str, &str)] = &[
    // (alias, package family name, app id)
    ("microsoft to do", "Microsoft.Todos_8wekyb3d8bbwe", "App"),
    ("to do", "Microsoft.Todos_8wekyb3d8bbwe", "App"),
    ("todo app", "Microsoft.Todos_8wekyb3d8bbwe", "App"),
    ("to do app", "Microsoft.Todos_8wekyb3d8bbwe", "App"),
    ("whatsapp", "5319275A.WhatsAppDesktop_cv1g1gvanyjgm", "App"),
    ("photos", "Microsoft.Windows.Photos_8wekyb3d8bbwe", "App"),
    ("copilot", "Microsoft.Copilot_8wekyb3d8bbwe", "App"),
    ("mail", "microsoft.windowscommunicationsapps_8wekyb3d8bbwe", "microsoft.windowslive.mail"),
    (
        "calendar",
        "microsoft.windowscommunicationsapps_8wekyb3d8bbwe",
        "microsoft.windowslive.calendar",
    ),
    ("xbox", "Microsoft.GamingApp_8wekyb3d8bbwe", "Microsoft.Xbox.App"),
    ("netflix", "4DF9E0F8.Netflix_mcm4njqhnhss8", "Netflix.App"),
    ("microsoft 365", "Microsoft.MicrosoftOfficeHub_8wekyb3d8bbwe", "Microsoft.MicrosoftOfficeHub"),
];

/// Curated fallbacks for common applications, used when the Start Menu index
/// has no entry (system tools typically don't create shortcuts).
const APP_ALIASES: &[(&str, &str)] = &[
    ("notepad", "notepad.exe"),
    ("calculator", "calc.exe"),
    ("calc", "calc.exe"),
    ("paint", "mspaint.exe"),
    ("explorer", "explorer.exe"),
    ("file explorer", "explorer.exe"),
    ("task manager", "taskmgr.exe"),
    ("settings", "ms-settings:"),
    ("windows settings", "ms-settings:"),
    ("snipping tool", "ms-screenclip:"),
    ("microsoft store", "ms-windows-store:"),
    ("edge", "msedge.exe"),
    ("microsoft edge", "msedge.exe"),
    ("chrome", "chrome.exe"),
    ("google chrome", "chrome.exe"),
    ("firefox", "firefox.exe"),
    ("spotify", "spotify.exe"),
    ("vscode", "code.exe"),
    ("vs code", "code.exe"),
    ("visual studio code", "code.exe"),
    ("discord", "discord.exe"),
    ("slack", "slack.exe"),
    ("teams", "teams.exe"),
    ("zoom", "zoom.exe"),
    ("steam", "steam.exe"),
    ("terminal", "wt.exe"),
    ("windows terminal", "wt.exe"),
    ("word", "winword.exe"),
    ("excel", "excel.exe"),
    ("powerpoint", "powerpnt.exe"),
    ("outlook", "outlook.exe"),
    ("vlc", "vlc.exe"),
    ("whatsapp", "whatsapp.exe"),
    ("telegram", "telegram.exe"),
];

fn normalize_app_name(name: &str) -> String {
    let mut normalized = normalize_text(name);
    for prefix in ["the ", "my "] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.to_string();
        }
    }
    for suffix in [" app", " application", " application please"] {
        if let Some(stripped) = normalized.strip_suffix(suffix) {
            normalized = stripped.to_string();
        }
    }
    normalized.trim().to_string()
}

/// Resolve an application name to something Nova can launch.
pub fn resolve_application(name: &str) -> Option<ResolvedApp> {
    let normalized = normalize_app_name(name);
    if normalized.is_empty() {
        return None;
    }

    // 1) Packaged (Store) apps. Activation reports a real error when the
    //    package is missing, so there is no need to guess here.
    if let Some((alias, family, app_id)) = PACKAGED_APPS
        .iter()
        .find(|(alias, _, _)| *alias == normalized.as_str())
    {
        return Some(ResolvedApp::new(
            LaunchTarget::Aumid(format!("{family}!{app_id}")),
            display_app_name(alias),
        ));
    }

    // 2) Curated aliases for system tools and well-known apps.
    if let Some((alias, target)) = APP_ALIASES
        .iter()
        .find(|(alias, _)| *alias == normalized.as_str())
    {
        return Some(ResolvedApp::new(
            LaunchTarget::Path((*target).to_string()),
            display_app_name(alias),
        ));
    }

    // 3) Start Menu shortcuts (covers most user-installed applications).
    if let Some(path) = startmenu::search(&normalized) {
        let label = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&normalized)
            .to_string();
        return Some(ResolvedApp::new(
            LaunchTarget::Path(path.display().to_string()),
            label,
        ));
    }

    // 4) Alias entries by containment ("open the spotify app please"). The
    //    longest matching alias wins, so "google chrome" beats "chrome", and
    //    Nova reports the matched name — not the whole sentence.
    if let Some((alias, target)) = APP_ALIASES
        .iter()
        .filter(|(alias, _)| alias.len() >= 4 && normalized.contains(*alias))
        .max_by_key(|(alias, _)| alias.len())
    {
        return Some(ResolvedApp::new(
            LaunchTarget::Path((*target).to_string()),
            display_app_name(alias),
        ));
    }

    // 5) Packaged apps by containment ("open the microsoft to do app").
    if let Some((alias, family, app_id)) = PACKAGED_APPS
        .iter()
        .filter(|(alias, _, _)| alias.len() >= 4 && normalized.contains(*alias))
        .max_by_key(|(alias, _, _)| alias.len())
    {
        return Some(ResolvedApp::new(
            LaunchTarget::Aumid(format!("{family}!{app_id}")),
            display_app_name(alias),
        ));
    }

    None
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub fn open_application<R: Runtime>(_app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let name = params
        .get("application")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(NovaError::InvalidCommand("no application name given".into()));
    }

    match resolve_application(&name) {
        Some(resolved) => {
            resolved.target.launch()?;
            Ok(SkillOutcome {
                success: true,
                response: format!("Opening {}.", resolved.label),
                data: json!({
                    "application": resolved.label,
                    "requested": name,
                    "target": resolved.target.describe()
                }),
            })
        }
        None => {
            // Speech recognition slips ("Microsoft Studio" for "Microsoft To
            // Do") should not end in a dead end: offer the closest names Nova
            // knows and let the user pick one.
            let suggestions = suggest_applications(&name);
            if suggestions.is_empty() {
                return Ok(SkillOutcome {
                    success: false,
                    response: format!("I couldn't find {} on this PC.", name),
                    data: json!({ "application": name, "found": false }),
                });
            }
            Ok(SkillOutcome {
                success: false,
                response: format!(
                    "I couldn't find {}. Did you mean {}?",
                    name,
                    suggestions
                        .iter()
                        .map(|candidate| format!("'{candidate}'"))
                        .collect::<Vec<_>>()
                        .join(" or ")
                ),
                data: json!({
                    "application": name,
                    "found": false,
                    "requiresClarification": true,
                    "candidates": suggestions,
                }),
            })
        }
    }
}

/// Closest known application names for a request Nova could not resolve.
/// Only Nova's own tables are searched — never the file system.
pub fn suggest_applications(requested: &str) -> Vec<String> {
    const THRESHOLD: f32 = 0.45;

    let mut scored: Vec<(f32, &str)> = Vec::new();
    for (alias, _, _) in PACKAGED_APPS {
        scored.push((title_similarity(requested, alias), alias));
    }
    for (alias, _) in APP_ALIASES {
        scored.push((title_similarity(requested, alias), alias));
    }

    scored.retain(|(score, _)| *score >= THRESHOLD);
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut out: Vec<String> = Vec::new();
    for (_, alias) in scored {
        let display = display_app_name(alias);
        if !out.contains(&display) {
            out.push(display);
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// Turn a table alias into something worth showing to a person.
fn display_app_name(alias: &str) -> String {
    match alias {
        "microsoft to do" => "Microsoft To Do".to_string(),
        "microsoft 365" => "Microsoft 365 Copilot".to_string(),
        "vs code" | "vscode" => "Visual Studio Code".to_string(),
        "xbox" => "Xbox".to_string(),
        other => other
            .split(' ')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

pub fn open_folder<R: Runtime>(_app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let folder = params
        .get("folder")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if folder.is_empty() {
        return Err(NovaError::InvalidCommand("no folder given".into()));
    }

    let mut normalized = normalize_text(&folder);
    for prefix in ["the ", "my "] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.to_string();
        }
    }
    normalized = normalized.trim().to_string();

    let resolved: Option<PathBuf> = if let Some(dir) = known_folder(&normalized) {
        Some(dir)
    } else {
        let direct = PathBuf::from(&folder);
        if direct.is_dir() {
            Some(direct)
        } else {
            None
        }
    };

    match resolved {
        Some(path) => {
            shell_execute(&path.display().to_string())?;
            Ok(SkillOutcome {
                success: true,
                response: format!("Opening {}.", display_folder_name(&folder, &path)),
                data: json!({ "folder": folder, "path": path.display().to_string() }),
            })
        }
        None => Ok(SkillOutcome {
            success: false,
            response: format!("I couldn't find a folder called '{}'.", folder),
            data: json!({ "folder": folder, "found": false }),
        }),
    }
}

fn display_folder_name(requested: &str, path: &Path) -> String {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if !name.is_empty() {
            return name.to_string();
        }
    }
    requested.to_string()
}

pub fn open_file<R: Runtime>(_app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let raw = params
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if raw.is_empty() {
        return Err(NovaError::InvalidCommand("no file path given".into()));
    }

    let resolved = resolve_file_path(&raw);
    let Some(path) = resolved else {
        return Ok(SkillOutcome {
            success: false,
            response: format!("I couldn't find a file at '{}'.", raw),
            data: json!({ "path": raw, "found": false }),
        });
    };

    if is_blocked_executable(&path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_uppercase();
        return Ok(SkillOutcome {
            success: false,
            response: format!(
                "For safety, I can't open executable or script files ({}). \
                 You can open it yourself from File Explorer.",
                ext
            ),
            data: json!({ "path": path.display().to_string(), "blocked": true }),
        });
    }

    shell_execute(&path.display().to_string())?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&raw)
        .to_string();
    Ok(SkillOutcome {
        success: true,
        response: format!("Opening {}.", file_name),
        data: json!({ "path": path.display().to_string() }),
    })
}

/// Resolve a user-supplied file path: absolute paths are used as-is, bare
/// paths are additionally tried inside common folders.
fn resolve_file_path(raw: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(raw);
    if candidate.is_file() {
        return Some(candidate);
    }
    if candidate.is_absolute() {
        return None;
    }
    // Try "Downloads/<name>", "Documents/<name>", "Desktop/<name>".
    for base in [
        dirs::download_dir(),
        dirs::document_dir(),
        dirs::desktop_dir(),
    ]
    .into_iter()
    .flatten()
    {
        let joined = base.join(raw);
        if joined.is_file() {
            return Some(joined);
        }
    }
    None
}

pub fn take_screenshot<R: Runtime>(_app: &AppHandle<R>, _params: &Value) -> NovaResult<SkillOutcome> {
    let dir = screenshots_dir();
    std::fs::create_dir_all(&dir)?;

    let monitors = xcap::Monitor::all()
        .map_err(|e| NovaError::Other(format!("could not enumerate monitors: {e}")))?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| NovaError::Other("no monitor found".into()))?;

    let image = monitor
        .capture_image()
        .map_err(|e| NovaError::Other(format!("screenshot capture failed: {e}")))?;

    let file_name = format!("nova-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let path = dir.join(&file_name);
    image
        .save(&path)
        .map_err(|e| NovaError::Other(format!("could not save screenshot: {e}")))?;

    Ok(SkillOutcome {
        success: true,
        response: format!("Screenshot saved to {}.", path.display()),
        data: json!({ "path": path.display().to_string() }),
    })
}

/// Shut the PC down after a short, user-visible countdown.
///
/// The argument vector is fixed by Nova (nothing from the transcript is
/// interpolated) and the command is gated behind the confirmation dialog, so
/// this stays a bounded, allowlisted operation rather than shell access.
pub fn shutdown_pc<R: Runtime>(_app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let delay = params
        .get("delaySeconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(10)
        .clamp(5, 60);

    #[cfg(windows)]
    {
        std::process::Command::new("shutdown")
            .args(["/s", "/t", &delay.to_string()])
            .spawn()
            .map_err(|e| NovaError::Other(format!("could not start the shutdown countdown: {e}")))?;
        Ok(SkillOutcome {
            success: true,
            response: format!(
                "Shutting down in {delay} seconds. Say “cancel the shutdown” if you change your mind."
            ),
            data: json!({ "delaySeconds": delay }),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = delay;
        Ok(SkillOutcome {
            success: false,
            response: "Shutting down this PC isn’t supported on this platform.".into(),
            data: json!({ "unsupportedPlatform": true }),
        })
    }
}

/// Cancel a shutdown that is still counting down (`shutdown /a`).
pub fn cancel_shutdown<R: Runtime>(
    _app: &AppHandle<R>,
    _params: &Value,
) -> NovaResult<SkillOutcome> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("shutdown")
            .arg("/a")
            .output()
            .map_err(|e| NovaError::Other(format!("could not cancel the shutdown: {e}")))?;
        if !output.status.success() {
            return Ok(SkillOutcome {
                success: false,
                response: "There was no shutdown countdown to cancel.".into(),
                data: json!({ "cancelled": false }),
            });
        }
        Ok(SkillOutcome {
            success: true,
            response: "Cancelled the shutdown.".into(),
            data: json!({ "cancelled": true }),
        })
    }
    #[cfg(not(windows))]
    {
        Ok(SkillOutcome {
            success: false,
            response: "Shutting down this PC isn’t supported on this platform.".into(),
            data: json!({ "unsupportedPlatform": true }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_aliases() {
        // Classic aliases resolve to a ShellExecute target.
        assert_eq!(
            resolve_application("calculator").map(|app| app.target),
            Some(LaunchTarget::Path("calc.exe".to_string()))
        );
        assert_eq!(
            resolve_application("the calculator app").map(|app| app.target),
            Some(LaunchTarget::Path("calc.exe".to_string()))
        );
        // Notepad is a Store app on current Windows: either the packaged app or
        // the classic executable is a correct answer.
        assert!(resolve_application("notepad").is_some());
    }

    #[test]
    fn reports_the_matched_app_not_the_whole_sentence() {
        // Regression: "open google chrome and search for X" used to answer
        // "Opening google chrome and search for the most best supercar."
        let resolved = resolve_application("google chrome and search for supercars").unwrap();
        assert_eq!(resolved.label, "Google Chrome");
    }

    #[test]
    fn packaged_apps_resolve_by_app_user_model_id() {
        assert_eq!(
            resolve_application("Microsoft to do app").map(|app| app.target),
            Some(LaunchTarget::Aumid(
                "Microsoft.Todos_8wekyb3d8bbwe!App".to_string()
            ))
        );
        assert_eq!(
            resolve_application("photos").map(|app| app.target),
            Some(LaunchTarget::Aumid(
                "Microsoft.Windows.Photos_8wekyb3d8bbwe!App".to_string()
            ))
        );
    }

    #[test]
    fn package_ids_are_well_formed() {
        // The packaged table is Nova's own data; a malformed entry would make
        // the shell host fail, so it is checked here rather than at runtime.
        for (alias, family, app_id) in PACKAGED_APPS {
            assert!(!alias.is_empty());
            assert!(family.contains('_'), "{family} is not a package family name");
            assert!(!app_id.is_empty());
        }
        assert!(PACKAGED_APPS
            .iter()
            .any(|(alias, family, app_id)| *alias == "microsoft to do"
                && *family == "Microsoft.Todos_8wekyb3d8bbwe"
                && *app_id == "App"));
    }

    #[test]
    fn normalize_app_name_strips_noise() {
        assert_eq!(normalize_app_name("The Spotify app"), "spotify");
        assert_eq!(normalize_app_name("  my   Notepad  "), "notepad");
    }

    #[test]
    #[ignore = "launches a real Store app; run with --ignored on a desktop session"]
    fn packaged_app_activation_reports_a_real_result() {
        // Microsoft To Do ships with current Windows. If it were missing, this
        // must return an error instead of silently doing nothing.
        let result = launch_packaged_app("Microsoft.Todos_8wekyb3d8bbwe!App");
        assert!(result.is_ok(), "activation failed: {:?}", result.err());
    }

    #[test]
    fn suggests_the_closest_app_when_speech_is_misheard() {
        // Regression: "open Microsoft Studio app" (a misheard "To Do") used to
        // end in a flat refusal.
        let suggestions = suggest_applications("microsoft studio");
        assert!(
            suggestions.contains(&"Microsoft To Do".to_string()),
            "expected Microsoft To Do in {suggestions:?}"
        );

        // Unrelated names get no guesses at all.
        assert!(suggest_applications("snorkelling gear").is_empty());
    }

    #[test]
    fn application_targets_are_never_command_lines() {
        // The launcher only ever produces ShellExecute targets, never a shell.
        for name in ["notepad", "spotify", "calculator"] {
            if let Some(resolved) = resolve_application(name) {
                let text = resolved.target.describe();
                assert!(!text.contains('&'));
                assert!(!text.contains('|'));
            }
        }
    }
}
