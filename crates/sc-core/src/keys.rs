//! Names for the keys a shortcut can use, shared by the daemon (which turns a
//! shortcut string into a compositor bind) and the app (which turns the key you
//! press into a string). A shortcut is written `ALT+F10`: any of `CTRL`, `ALT`,
//! `SHIFT` and `SUPER`, then one key.

/// Keys with a fixed friendly name and the X keysym that compositors know it by.
/// Letters, digits and F1 to F24 are handled by rule, not listed here.
const NAMED: &[(&str, &str)] = &[
    ("BACKSPACE", "BackSpace"),
    ("TAB", "Tab"),
    ("ENTER", "Return"),
    ("PAUSE", "Pause"),
    ("CAPSLOCK", "Caps_Lock"),
    ("ESCAPE", "Escape"),
    ("SPACE", "space"),
    ("PAGEUP", "Prior"),
    ("PAGEDOWN", "Next"),
    ("END", "End"),
    ("HOME", "Home"),
    ("LEFT", "Left"),
    ("UP", "Up"),
    ("RIGHT", "Right"),
    ("DOWN", "Down"),
    ("PRINT", "Print"),
    ("INSERT", "Insert"),
    ("DELETE", "Delete"),
    ("APPS", "Menu"),
    ("NUM0", "KP_0"),
    ("NUM1", "KP_1"),
    ("NUM2", "KP_2"),
    ("NUM3", "KP_3"),
    ("NUM4", "KP_4"),
    ("NUM5", "KP_5"),
    ("NUM6", "KP_6"),
    ("NUM7", "KP_7"),
    ("NUM8", "KP_8"),
    ("NUM9", "KP_9"),
    ("NUMMULTIPLY", "KP_Multiply"),
    ("NUMADD", "KP_Add"),
    ("NUMSUBTRACT", "KP_Subtract"),
    ("NUMDECIMAL", "KP_Decimal"),
    ("NUMDIVIDE", "KP_Divide"),
    ("NUMLOCK", "Num_Lock"),
    ("SCROLLLOCK", "Scroll_Lock"),
    ("VOLUMEMUTE", "XF86AudioMute"),
    ("VOLUMEDOWN", "XF86AudioLowerVolume"),
    ("VOLUMEUP", "XF86AudioRaiseVolume"),
    ("MEDIANEXT", "XF86AudioNext"),
    ("MEDIAPREV", "XF86AudioPrev"),
    ("MEDIASTOP", "XF86AudioStop"),
    ("MEDIAPLAY", "XF86AudioPlay"),
    (";", "semicolon"),
    ("=", "equal"),
    (",", "comma"),
    ("-", "minus"),
    (".", "period"),
    ("/", "slash"),
    ("`", "grave"),
    ("[", "bracketleft"),
    ("\\", "backslash"),
    ("]", "bracketright"),
    ("'", "apostrophe"),
];

/// Other spellings that mean the same key.
const ALIASES: &[(&str, &str)] = &[
    ("PRINTSCREEN", "PRINT"),
    ("PRTSC", "PRINT"),
    ("INS", "INSERT"),
    ("DEL", "DELETE"),
    ("PGUP", "PAGEUP"),
    ("PGDN", "PAGEDOWN"),
    ("RETURN", "ENTER"),
    ("ESC", "ESCAPE"),
    ("BACKTICK", "`"),
    ("GRAVE", "`"),
    ("TILDE", "`"),
    ("PLUS", "="),
    ("EQUALS", "="),
    ("OPENBRACKET", "["),
    ("CLOSEBRACKET", "]"),
    ("MINUS", "-"),
    ("COMMA", ","),
    ("PERIOD", "."),
    ("SLASH", "/"),
    ("SEMICOLON", ";"),
    ("QUOTE", "'"),
    ("BACKSLASH", "\\"),
];

/// The canonical name for a key written any way (any case, an alias), or `None`
/// if it isn't a key SimpleClip knows.
pub fn canonical_name(name: &str) -> Option<String> {
    let k = name.trim().to_ascii_uppercase();
    if let Some(n) = k.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&n).then(|| format!("F{n}"));
    }
    if k.len() == 1 {
        let c = k.as_bytes()[0];
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(k);
        }
    }
    if let Some((_, canon)) = ALIASES.iter().find(|(a, _)| *a == k) {
        return Some((*canon).to_string());
    }
    NAMED
        .iter()
        .find(|(n, _)| *n == k)
        .map(|(n, _)| (*n).to_string())
}

/// The X keysym name for a key name, which is what Hyprland binds and the
/// GlobalShortcuts portal take (`F10` stays `F10`, `ENTER` becomes `Return`).
pub fn keysym(name: &str) -> Option<String> {
    let canon = canonical_name(name)?;
    if canon.len() == 1 && canon.as_bytes()[0].is_ascii_alphabetic() {
        return Some(canon.to_ascii_lowercase());
    }
    NAMED
        .iter()
        .find(|(n, _)| *n == canon)
        .map(|(_, sym)| (*sym).to_string())
        .or(Some(canon))
}

/// A parsed shortcut: modifiers in a fixed order, and the key's canonical name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
    pub key: String,
}

impl Shortcut {
    /// Parse "CTRL+ALT+F10" style text. Exactly one non-modifier key is required.
    pub fn parse(spec: &str) -> Option<Self> {
        let mut s = Self {
            ctrl: false,
            alt: false,
            shift: false,
            sup: false,
            key: String::new(),
        };
        let mut key = None;
        for part in spec.split('+').map(|p| p.trim()) {
            match part.to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => s.ctrl = true,
                "ALT" => s.alt = true,
                "SHIFT" => s.shift = true,
                "SUPER" | "WIN" | "META" | "CMD" | "MOD" => s.sup = true,
                "" => return None,
                _ => {
                    if key.replace(canonical_name(part)?).is_some() {
                        return None; // two non-modifier keys
                    }
                }
            }
        }
        s.key = key?;
        Some(s)
    }

    /// Hyprland's modifier list, e.g. `ALT SHIFT`.
    pub fn hypr_mods(&self) -> String {
        let mut m = Vec::new();
        if self.sup {
            m.push("SUPER");
        }
        if self.ctrl {
            m.push("CTRL");
        }
        if self.alt {
            m.push("ALT");
        }
        if self.shift {
            m.push("SHIFT");
        }
        m.join(" ")
    }

    /// The XDG shortcuts spec trigger, e.g. `CTRL+ALT+F10`, as the
    /// GlobalShortcuts portal wants it.
    pub fn portal_trigger(&self) -> String {
        let mut parts = Vec::new();
        if self.sup {
            parts.push("LOGO".to_string());
        }
        if self.ctrl {
            parts.push("CTRL".to_string());
        }
        if self.alt {
            parts.push("ALT".to_string());
        }
        if self.shift {
            parts.push("SHIFT".to_string());
        }
        parts.push(keysym(&self.key).unwrap_or_else(|| self.key.clone()));
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_canonical() {
        assert_eq!(canonical_name("f10").as_deref(), Some("F10"));
        assert_eq!(canonical_name("backtick").as_deref(), Some("`"));
        assert_eq!(canonical_name("Return").as_deref(), Some("ENTER"));
        assert_eq!(canonical_name("a").as_deref(), Some("A"));
        assert_eq!(canonical_name("7").as_deref(), Some("7"));
        assert!(canonical_name("F25").is_none());
        assert!(canonical_name("NOPE").is_none());
        assert!(canonical_name("").is_none());
    }

    #[test]
    fn keysyms_match_what_compositors_expect() {
        assert_eq!(keysym("F10").as_deref(), Some("F10"));
        assert_eq!(keysym("`").as_deref(), Some("grave"));
        assert_eq!(keysym("enter").as_deref(), Some("Return"));
        assert_eq!(keysym("g").as_deref(), Some("g"));
        assert_eq!(keysym("5").as_deref(), Some("5"));
        assert_eq!(keysym("pageup").as_deref(), Some("Prior"));
    }

    #[test]
    fn shortcuts_parse() {
        let s = Shortcut::parse("ctrl+Alt+f10").unwrap();
        assert!(s.ctrl && s.alt && !s.shift && !s.sup);
        assert_eq!(s.key, "F10");
        assert_eq!(s.hypr_mods(), "CTRL ALT");
        assert_eq!(s.portal_trigger(), "CTRL+ALT+F10");
        assert_eq!(Shortcut::parse("WIN+`").unwrap().hypr_mods(), "SUPER");
        assert!(Shortcut::parse("ALT+F9+F10").is_none());
        assert!(Shortcut::parse("ALT+").is_none());
        assert!(Shortcut::parse("ALT").is_none());
        assert!(Shortcut::parse("").is_none());
    }
}
