//! Appearance: the theme catalogue, customization constraints and the applied
//! configuration, persisted in SQLite (`settings` table, key `appearance`).
//!
//! The Rust side owns *what is applied and persisted*; the React side owns the
//! token tables and the isolated preview. Both share the theme ids defined
//! here, and the defaults each theme implies for the ambient effects.

use serde::{Deserialize, Serialize};

/// Database key under which the applied appearance is stored.
pub const APPEARANCE_KEY: &str = "appearance";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Whether the theme is a light theme (affects default surface choices).
    pub light: bool,
}

/// The ten themes from the design handoff, in gallery order.
pub const THEMES: &[ThemeDef] = &[
    ThemeDef {
        id: "nova",
        name: "Nova Neon",
        description: "Signature near-black glass with electric cyan and violet energy.",
        light: false,
    },
    ThemeDef {
        id: "darcula",
        name: "Darcula",
        description: "Charcoal IDE surfaces with muted lavender accents — quiet for long sessions.",
        light: false,
    },
    ThemeDef {
        id: "midnight",
        name: "Midnight",
        description: "Deep navy blues with a calm, premium nighttime atmosphere.",
        light: false,
    },
    ThemeDef {
        id: "oled",
        name: "OLED Black",
        description: "Near-pure black with restrained highlights for OLED displays.",
        light: false,
    },
    ThemeDef {
        id: "light",
        name: "Light",
        description: "Clean white and soft gray surfaces with crisp blue accents.",
        light: true,
    },
    ThemeDef {
        id: "nord",
        name: "Nord",
        description: "Nordic blue-grays, icy accents, and soft balanced contrast.",
        light: false,
    },
    ThemeDef {
        id: "dracula",
        name: "Dracula",
        description: "Rich violet nights with pink and cyan sparks.",
        light: false,
    },
    ThemeDef {
        id: "soldark",
        name: "Solarized Dark",
        description: "Warm, low-glare dark with teal and earthy gold.",
        light: false,
    },
    ThemeDef {
        id: "sollight",
        name: "Solarized Light",
        description: "Warm cream light with balanced teal and gold accents.",
        light: true,
    },
    ThemeDef {
        id: "system",
        name: "System Default",
        description: "Follows the Windows app theme automatically.",
        light: false,
    },
];

/// Curated accent swatches offered next to the custom color picker.
pub const ACCENTS: &[&str] = &[
    "#3fe0ff", "#a78bff", "#5f9bff", "#4beaaf", "#ffc861", "#ff6ad5", "#ff6b8a",
];

pub fn theme(id: &str) -> Option<ThemeDef> {
    THEMES.iter().copied().find(|t| t.id == id)
}

/// Themes that can be resolved to a concrete token set (everything but `system`).
pub fn is_concrete(id: &str) -> bool {
    id != "system" && theme(id).is_some()
}

/// Resolve `system` to a concrete theme using the Windows app theme.
pub fn resolve(id: &str, system_is_dark: bool) -> &'static str {
    match id {
        "system" => {
            if system_is_dark {
                "nova"
            } else {
                "light"
            }
        }
        other => theme(other).map(|t| t.id).unwrap_or("nova"),
    }
}

pub fn theme_name(id: &str) -> &'static str {
    theme(id).map(|t| t.name).unwrap_or("Nova Neon")
}

// ---------------------------------------------------------------------------
// Appearance configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Comfortable,
    Compact,
    Spacious,
}

impl Default for Density {
    fn default() -> Self {
        Density::Comfortable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intensity {
    /// Ambient motion kept to a minimum; interaction feedback stays on.
    Minimal,
    Balanced,
    Immersive,
}

impl Default for Intensity {
    fn default() -> Self {
        Intensity::Balanced
    }
}

impl Intensity {
    /// Multiplier applied to the shared motion variable (`--M`).
    pub fn motion(self) -> f32 {
        match self {
            Intensity::Minimal => 0.65,
            Intensity::Balanced => 1.0,
            Intensity::Immersive => 1.3,
        }
    }

    /// Speed multiplier for the ambient canvas.
    pub fn ambient_speed(self) -> f32 {
        match self {
            Intensity::Minimal => 0.7,
            Intensity::Balanced => 1.0,
            Intensity::Immersive => 1.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Effects {
    /// Drifting luminous particles.
    pub particles: bool,
    /// Neural network lines, perspective grid and travelling pulses.
    pub network: bool,
    /// Depth-scaled parallax between background layers.
    pub parallax: bool,
    /// The environment energizes while Nova is listening.
    pub voice: bool,
    /// Glow intensity in percent (0..150).
    pub glow: u8,
    /// Panel opacity in percent (60..100).
    pub transparency: u8,
    pub intensity: Intensity,
}

impl Default for Effects {
    fn default() -> Self {
        Effects {
            particles: true,
            network: true,
            parallax: true,
            voice: true,
            glow: 100,
            transparency: 100,
            intensity: Intensity::Balanced,
        }
    }
}

impl Effects {
    /// Defaults implied by a theme (mirrors the design's per-theme presets).
    pub fn for_theme(id: &str) -> Effects {
        let (particles, network, glow, intensity) = match resolve(id, true) {
            "darcula" => (false, false, 25, Intensity::Minimal),
            "midnight" => (true, true, 60, Intensity::Balanced),
            "oled" => (false, true, 20, Intensity::Minimal),
            "light" => (true, true, 35, Intensity::Balanced),
            "nord" => (true, true, 35, Intensity::Balanced),
            "dracula" => (true, true, 70, Intensity::Balanced),
            "soldark" => (false, true, 30, Intensity::Balanced),
            "sollight" => (false, false, 25, Intensity::Minimal),
            _ => (true, true, 100, Intensity::Balanced),
        };
        Effects {
            particles,
            network,
            parallax: true,
            voice: true,
            glow,
            transparency: 100,
            intensity,
        }
    }

    fn normalize(&mut self) {
        self.glow = self.glow.min(150);
        self.transparency = self.transparency.clamp(60, 100);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Theme id, one of [`THEMES`].
    pub theme: String,
    /// Custom accent (`#rrggbb`) or `None` to use the theme's own accent.
    pub accent: Option<String>,
    pub density: Density,
    pub effects: Effects,
    /// True once the user changed accent/density/effects away from the
    /// theme presets — surfaced as a "Customized" tag.
    pub customized: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: "nova".into(),
            accent: None,
            density: Density::Comfortable,
            effects: Effects::default(),
            customized: false,
        }
    }
}

impl Appearance {
    /// Clamp/repair a configuration coming from the UI or the database.
    pub fn normalized(mut self) -> Appearance {
        if theme(&self.theme).is_none() {
            self.theme = "nova".into();
        }
        self.accent = self.accent.as_deref().and_then(normalize_hex);
        self.effects.normalize();
        self
    }

    /// Apply a theme, adopting that theme's effect presets (the design's
    /// Apply behaviour) and dropping a custom accent.
    pub fn apply_theme(mut self, id: &str) -> Appearance {
        let id = theme(id).map(|t| t.id).unwrap_or("nova");
        self.theme = id.to_string();
        self.accent = None;
        self.effects = Effects::for_theme(id);
        self.customized = false;
        self
    }

    /// Reset accent/density/effects for the current theme.
    pub fn reset_customization(mut self) -> Appearance {
        let theme_id = self.theme.clone();
        self.accent = None;
        self.density = Density::Comfortable;
        self.effects = Effects::for_theme(&theme_id);
        self.customized = false;
        self
    }
}

/// Validate `#rgb`/`#rrggbb` and return a lowercase `#rrggbb`.
pub fn normalize_hex(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let expanded = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };
    if !expanded.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

/// Relative luminance (0..1) of an `#rrggbb` color, per WCAG.
pub fn luminance(hex: &str) -> f32 {
    let normalized = normalize_hex(hex).unwrap_or_else(|| "#3fe0ff".into());
    let raw = u32::from_str_radix(&normalized[1..], 16).unwrap_or(0x3fe0ff);
    let channel = |value: u32| {
        let c = value as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = channel((raw >> 16) & 0xff);
    let g = channel((raw >> 8) & 0xff);
    let b = channel(raw & 0xff);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Foreground color that stays readable on top of `accent`.
pub fn on_accent(accent: &str) -> &'static str {
    if luminance(accent) > 0.45 {
        "#08131a"
    } else {
        "#ffffff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_the_ten_designed_themes() {
        let ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            vec![
                "nova", "darcula", "midnight", "oled", "light", "nord", "dracula", "soldark",
                "sollight", "system"
            ]
        );
        // Darcula and Dracula must stay distinct themes.
        assert_ne!(theme("darcula").unwrap().name, theme("dracula").unwrap().name);
        assert_ne!(Effects::for_theme("darcula"), Effects::for_theme("dracula"));
    }

    #[test]
    fn system_resolves_to_a_concrete_theme() {
        assert_eq!(resolve("system", true), "nova");
        assert_eq!(resolve("system", false), "light");
        assert_eq!(resolve("nord", true), "nord");
        assert_eq!(resolve("nonsense", true), "nova");
    }

    #[test]
    fn normalization_repairs_bad_input() {
        let fixed = Appearance {
            theme: "not-a-theme".into(),
            accent: Some("nope".into()),
            effects: Effects {
                glow: 250,
                transparency: 10,
                ..Default::default()
            },
            ..Default::default()
        }
        .normalized();
        assert_eq!(fixed.theme, "nova");
        assert!(fixed.accent.is_none());
        assert_eq!(fixed.effects.glow, 150);
        assert_eq!(fixed.effects.transparency, 60);

        let hexed = Appearance {
            accent: Some("#ABC".into()),
            ..Default::default()
        }
        .normalized();
        assert_eq!(hexed.accent.as_deref(), Some("#aabbcc"));
    }

    #[test]
    fn applying_a_theme_restores_its_presets() {
        let customized = Appearance::default()
            .apply_theme("dracula")
            .normalized();
        let tweaked = Appearance {
            accent: Some("#ff0000".into()),
            density: Density::Compact,
            customized: true,
            ..customized.clone()
        };
        let applied = tweaked.clone().apply_theme("darcula");
        assert_eq!(applied.theme, "darcula");
        assert!(applied.accent.is_none(), "theme switch drops the custom accent");
        assert_eq!(applied.effects, Effects::for_theme("darcula"));
        assert!(!applied.customized);

        let reset = tweaked.clone().reset_customization();
        assert_eq!(reset.theme, "dracula");
        assert!(!reset.customized);
        assert_eq!(reset.density, Density::Comfortable);
        assert!(reset.accent.is_none());
    }

    #[test]
    fn density_and_intensity_match_the_design_controls() {
        assert_eq!(Intensity::Minimal.motion(), 0.65);
        assert_eq!(Intensity::Balanced.motion(), 1.0);
        assert_eq!(Intensity::Immersive.motion(), 1.3);
        assert!(Intensity::Minimal.ambient_speed() < Intensity::Immersive.ambient_speed());
    }

    #[test]
    fn accent_contrast_picks_a_readable_foreground() {
        assert_eq!(on_accent("#3fe0ff"), "#08131a");
        assert_eq!(on_accent("#1b1f24"), "#ffffff");
        assert!(luminance("#ffffff") > luminance("#000000"));
        assert!(ACCENTS
            .iter()
            .all(|accent| normalize_hex(accent).is_some()));
    }
}
