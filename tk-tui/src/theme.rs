//! Dopamine colours for the ticket pane.
//!
//! Values come from `~/.config/tk/theme.json`, which chezmoi generates from
//! `.chezmoidata/palette.toml` — the same palette that drives the Neovim
//! colorscheme, the WezTerm schemes and the zellij theme. Every field falls back
//! to a compiled-in dopamine-dark value, so a `cargo install` of this crate
//! still looks right with no dotfiles applied.
//!
//! Deliberately no background colours on body text: the pane inherits the
//! terminal's, so WezTerm's `window_background_opacity` shows through the same
//! way it does in Neovim. Only genuinely-highlighted cells (the selected comment
//! header) paint a background.

use ratatui::style::Color;
use serde_json::Value;
use std::sync::OnceLock;

pub struct Theme {
    /// body text
    pub fg: Color,
    /// comments, timestamps, hints — the quiet stuff
    pub dim: Color,
    /// ticket key
    pub key: Color,
    /// workflow status, and the footer status flash
    pub status: Color,
    /// comment author
    pub author: Color,
    /// fenced code blocks
    pub code: Color,
    /// inline `code` spans
    pub code_inline: Color,
    /// links and @mentions
    pub link: Color,
    /// horizontal rules
    pub rule: Color,
    /// focused borders and the compose caret
    pub accent: Color,
    /// background of the selected comment header
    pub selection: Color,
    /// text drawn on top of `selection`
    pub inverted: Color,
    /// an unticked checkbox glyph
    pub checkbox: Color,
    /// a ticked checkbox, and the struck-through text beside it
    pub done: Color,
    /// the gutter mark on an item whose write hasn't landed yet
    pub pending: Color,
}

/// dopamine-dark, used when theme.json is absent or a key is missing.
const FALLBACK: [(&str, &str); 15] = [
    ("fg", "#D7DBDF"),
    ("comment", "#90959B"),
    ("constant", "#CC5E87"),
    ("special", "#E6BA7E"),
    ("tag", "#5CC1AE"),
    ("string", "#DFE891"),
    ("special_inline", "#E6BA7E"),
    ("link", "#5CC1AE"),
    ("guide_normal", "#3E4852"),
    ("accent", "#E6A46A"),
    ("selection_bg", "#515F6E"),
    ("bg", "#2B343D"),
    ("ui", "#8A9299"),
    ("vcs_added", "#C4D27A"),
    ("warning", "#E6A676"),
];

fn fallback(key: &str) -> Color {
    let hex = FALLBACK
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or("#D7DBDF");
    parse_hex(hex).unwrap_or(Color::Reset)
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(Color::Rgb(
        (n >> 16) as u8,
        ((n >> 8) & 0xFF) as u8,
        (n & 0xFF) as u8,
    ))
}

/// Look up a palette role, falling back to the compiled-in value.
fn role(vars: Option<&Value>, name: &str, fallback_key: &str) -> Color {
    vars.and_then(|v| v.get(name))
        .and_then(Value::as_str)
        .and_then(parse_hex)
        .unwrap_or_else(|| fallback(fallback_key))
}

fn config_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    base.join("tk/theme.json")
}

/// Which variant to render in. Precedence:
///   1. `TK_THEME_VARIANT` (dark | light | mirage) — explicit override
///   2. the macOS system appearance, so the pane follows light/dark the way
///      WezTerm and Neovim already do
///   3. `active` from theme.json (chezmoi's `[theme] variant`)
///   4. dark
fn pick_variant(doc: &Value) -> String {
    if let Ok(v) = std::env::var("TK_THEME_VARIANT") {
        if !v.trim().is_empty() {
            return v.trim().to_string();
        }
    }
    if let Some(v) = macos_appearance() {
        return v;
    }
    doc.get("active")
        .and_then(Value::as_str)
        .unwrap_or("dark")
        .to_string()
}

/// `defaults read -g AppleInterfaceStyle` prints "Dark" in dark mode and exits
/// non-zero in light mode. Returns None off macOS, or if the command is missing.
fn macos_appearance() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;
    if !out.status.success() {
        return Some("light".to_string());
    }
    if String::from_utf8_lossy(&out.stdout).contains("Dark") {
        Some("dark".to_string())
    } else {
        Some("light".to_string())
    }
}

impl Theme {
    fn load() -> Theme {
        let doc: Value = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);

        let variant = pick_variant(&doc);
        Theme::from_doc(&doc, &variant)
    }

    /// Resolve a theme from a parsed theme.json. Missing keys — or a missing
    /// variant, or `Value::Null` entirely — fall back to dopamine-dark.
    fn from_doc(doc: &Value, variant: &str) -> Theme {
        let vars = doc.get("variants").and_then(|v| v.get(variant));

        Theme {
            fg: role(vars, "fg", "fg"),
            dim: role(vars, "comment", "comment"),
            key: role(vars, "constant", "constant"),
            status: role(vars, "special", "special"),
            author: role(vars, "tag", "tag"),
            code: role(vars, "string", "string"),
            code_inline: role(vars, "special", "special_inline"),
            link: role(vars, "tag", "link"),
            rule: role(vars, "guide_normal", "guide_normal"),
            accent: role(vars, "accent", "accent"),
            selection: role(vars, "selection_bg", "selection_bg"),
            inverted: role(vars, "bg", "bg"),
            checkbox: role(vars, "ui", "ui"),
            done: role(vars, "vcs_added", "vcs_added"),
            pending: role(vars, "warning", "warning"),
        }
    }
}

/// The process-wide theme, resolved on first use.
pub fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(Theme::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb(r, g, b)
    }

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!(parse_hex("#2B343D"), Some(rgb(0x2B, 0x34, 0x3D)));
        assert_eq!(parse_hex("#ffffff"), Some(rgb(255, 255, 255)));
        assert_eq!(parse_hex("#000000"), Some(rgb(0, 0, 0)));
    }

    #[test]
    fn rejects_malformed_hex() {
        for bad in ["2B343D", "#2B343", "#2B343DD", "#nothex", "", "#"] {
            assert_eq!(parse_hex(bad), None, "should reject {bad:?}");
        }
    }

    /// A real slice of the chezmoi-generated file.
    fn doc() -> Value {
        serde_json::json!({
            "active": "dark",
            "variants": {
                "dark":  { "fg": "#D7DBDF", "accent": "#E6A46A", "constant": "#CC5E87" },
                "light": { "fg": "#5C6166", "accent": "#CC8052", "constant": "#DB5E8C" }
            }
        })
    }

    #[test]
    fn reads_the_requested_variant() {
        let dark = Theme::from_doc(&doc(), "dark");
        assert_eq!(dark.fg, rgb(0xD7, 0xDB, 0xDF));
        assert_eq!(dark.accent, rgb(0xE6, 0xA4, 0x6A));
        assert_eq!(dark.key, rgb(0xCC, 0x5E, 0x87));

        let light = Theme::from_doc(&doc(), "light");
        assert_eq!(light.fg, rgb(0x5C, 0x61, 0x66));
        assert_eq!(light.accent, rgb(0xCC, 0x80, 0x52));
        assert_eq!(light.key, rgb(0xDB, 0x5E, 0x8C));
    }

    #[test]
    fn falls_back_per_key_when_role_absent() {
        // `doc` has no "comment"/"tag", so those fall back to dopamine-dark…
        let t = Theme::from_doc(&doc(), "light");
        assert_eq!(t.dim, rgb(0x90, 0x95, 0x9B), "dim should fall back");
        assert_eq!(t.author, rgb(0x5C, 0xC1, 0xAE), "author should fall back");
        // …while keys that ARE present still come from the requested variant.
        assert_eq!(t.fg, rgb(0x5C, 0x61, 0x66));
    }

    #[test]
    fn falls_back_wholesale_for_missing_variant_or_empty_doc() {
        let dark_fg = rgb(0xD7, 0xDB, 0xDF);
        assert_eq!(Theme::from_doc(&doc(), "nonesuch").fg, dark_fg);
        assert_eq!(Theme::from_doc(&Value::Null, "dark").fg, dark_fg);
        // every fallback key must resolve to a real colour, never Color::Reset
        let t = Theme::from_doc(&Value::Null, "dark");
        for (name, c) in [
            ("fg", t.fg), ("dim", t.dim), ("key", t.key), ("status", t.status),
            ("author", t.author), ("code", t.code), ("code_inline", t.code_inline),
            ("link", t.link), ("rule", t.rule), ("accent", t.accent),
            ("selection", t.selection), ("inverted", t.inverted),
            ("checkbox", t.checkbox), ("done", t.done), ("pending", t.pending),
        ] {
            assert!(matches!(c, Color::Rgb(..)), "{name} fell through to {c:?}");
        }
    }

    /// The palette roles `Theme::from_doc` looks up. If chezmoi's palette.toml
    /// renames one of these, the theme silently falls back to dopamine-dark —
    /// so assert the contract against the real generated file.
    const REQUIRED_ROLES: [&str; 13] = [
        "fg", "comment", "constant", "special", "tag", "string", "guide_normal", "accent",
        "selection_bg", "bg", "ui", "vcs_added", "warning",
    ];

    #[test]
    fn generated_theme_json_satisfies_the_roles_we_look_up() {
        let path = config_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: {} not present (dotfiles not applied)", path.display());
            return;
        };
        let doc: Value = serde_json::from_str(&raw).expect("theme.json must be valid JSON");

        let variants = doc
            .get("variants")
            .and_then(Value::as_object)
            .expect("theme.json needs a `variants` object");
        for expected in ["dark", "light", "mirage"] {
            assert!(variants.contains_key(expected), "missing variant {expected}");
        }
        for (name, vars) in variants {
            for role_name in REQUIRED_ROLES {
                let v = vars.get(role_name).and_then(Value::as_str);
                let v = v.unwrap_or_else(|| panic!("variant {name} is missing role {role_name}"));
                assert!(
                    parse_hex(v).is_some(),
                    "variant {name} role {role_name} is not #RRGGBB: {v}"
                );
            }
        }

        // and the resolved theme must differ between light and dark
        let dark = Theme::from_doc(&doc, "dark");
        let light = Theme::from_doc(&doc, "light");
        assert_ne!(dark.fg, light.fg, "light and dark should not resolve alike");
    }

    #[test]
    fn env_override_wins_over_doc_active() {
        // SAFETY: single-threaded test, no other thread reads the environment.
        unsafe { std::env::set_var("TK_THEME_VARIANT", "light") };
        assert_eq!(pick_variant(&doc()), "light");
        unsafe { std::env::set_var("TK_THEME_VARIANT", "  ") };
        // blank is ignored, so we fall through to appearance/active
        assert_ne!(pick_variant(&doc()), "  ");
        unsafe { std::env::remove_var("TK_THEME_VARIANT") };
    }
}
