//! Tells an actual game from everything else you do on the PC, for the Discord
//! status. It never guesses from "some window is in front": a program counts as a
//! game only if one of these is true.
//!
//! 1. It is a Steam game (native, or run through Proton), and Steam's own
//!    install records give the game's proper name.
//! 2. It lives in a game library folder (Steam, Heroic, Lutris, GOG, Bottles,
//!    itch), and the folder it sits in gives the game's name.
//! 3. It is a well-known game by its program name.
//! 4. It is full screen and is not a browser, video player or similar.
//!
//! A game installed somewhere unusual and run in a window or borderless is
//! missed rather than risk showing a browser as a game.

use crate::lin::Foreground;
use std::path::{Path, PathBuf};

/// Folders under which launchers install games, lower case. The next folder
/// down is the game's name.
const LIBRARIES: &[&str] = &[
    "/steamapps/common/",
    "/heroic/",
    "/games/heroic/",
    "/gog games/",
    "/epic games/",
    "/lutris/games/",
    "/itch/apps/",
    "/bottles/bottles/",
];

/// Folders inside those libraries that are a launcher's own program, not a game.
const NOT_GAMES: &[&str] = &[
    "launcher",
    "steamlinuxruntime",
    "steamlinuxruntime_soldier",
    "steamlinuxruntime_sniper",
    "proton",
    "proton - experimental",
    "steamworks shared",
    "steam controller configs",
    "prefixes",
    "tools",
    "runners",
];

/// Well-known games by program name (lower case, no `.exe`), and the name to show.
const KNOWN: &[(&str, &str)] = &[
    ("valorant-win64-shipping", "VALORANT"),
    ("league of legends", "League of Legends"),
    ("fortniteclient-win64-shipping", "Fortnite"),
    ("minecraft-launcher", "Minecraft"),
    ("robloxplayerbeta", "Roblox"),
    ("cs2", "Counter-Strike 2"),
    ("csgo", "Counter-Strike"),
    ("dota2", "Dota 2"),
    ("overwatch", "Overwatch 2"),
    ("r5apex", "Apex Legends"),
    ("r5apex_dx12", "Apex Legends"),
    ("rainbowsix", "Rainbow Six Siege"),
    ("rainbowsix_vulkan", "Rainbow Six Siege"),
    ("rainbowsix_be", "Rainbow Six Siege"),
    ("gta5", "Grand Theft Auto V"),
    ("rdr2", "Red Dead Redemption 2"),
    ("rocketleague", "Rocket League"),
    ("eldenring", "Elden Ring"),
    ("cyberpunk2077", "Cyberpunk 2077"),
    ("witcher3", "The Witcher 3"),
    ("wow", "World of Warcraft"),
    ("destiny2", "Destiny 2"),
    ("tslgame", "PUBG"),
    ("cod", "Call of Duty"),
    ("modernwarfare", "Call of Duty"),
    ("hogwartslegacy", "Hogwarts Legacy"),
    ("starfield", "Starfield"),
    ("terraria", "Terraria"),
    ("stardew valley", "Stardew Valley"),
    ("factorio", "Factorio"),
    ("among us", "Among Us"),
    ("helldivers2", "Helldivers 2"),
    ("palworld-win64-shipping", "Palworld"),
    ("bg3", "Baldur's Gate 3"),
    ("bg3_dx11", "Baldur's Gate 3"),
    ("deadlock", "Deadlock"),
    ("marvel-win64-shipping", "Marvel Rivals"),
];

/// Programs that are never a game, even in full screen (lower case, matched
/// against the window class and the program name).
const NEVER: &[&str] = &[
    "firefox",
    "librewolf",
    "zen",
    "zen-browser",
    "waterfox",
    "chromium",
    "google-chrome",
    "chrome",
    "brave-browser",
    "brave",
    "vivaldi",
    "vivaldi-stable",
    "opera",
    "microsoft-edge",
    "discord",
    "vesktop",
    "spotify",
    "code",
    "code-oss",
    "cursor",
    "claude",
    "obs",
    "com.obsproject.studio",
    "vlc",
    "mpv",
    "celluloid",
    "totem",
    "org.gnome.totem",
    "zoom",
    "slack",
    "teams",
    "steam",
    "steamwebhelper",
    "heroic",
    "lutris",
    "bottles",
    "kitty",
    "alacritty",
    "foot",
    "wezterm",
    "konsole",
    "gnome-terminal",
    "org.gnome.nautilus",
    "nautilus",
    "dolphin",
    "thunar",
    "scd",
    "sc-gui",
    "simpleclip",
    "libreoffice",
    "evince",
    "okular",
];

/// The name of a Steam game from its install record, for a window class of
/// `steam_app_<id>` (what Proton games and most native Steam games report).
fn steam_name(class: &str) -> Option<String> {
    let id: u64 = class.strip_prefix("steam_app_")?.parse().ok()?;
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    let mut libraries: Vec<PathBuf> = vec![
        home.join(".local/share/Steam/steamapps"),
        home.join(".steam/steam/steamapps"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps"),
    ];
    // Extra libraries are listed in libraryfolders.vdf.
    for root in libraries.clone() {
        if let Ok(vdf) = std::fs::read_to_string(root.join("libraryfolders.vdf")) {
            for line in vdf.lines() {
                if let Some(p) = vdf_value(line, "path") {
                    libraries.push(PathBuf::from(p).join("steamapps"));
                }
            }
        }
    }
    libraries.iter().find_map(|lib| {
        let acf = std::fs::read_to_string(lib.join(format!("appmanifest_{id}.acf"))).ok()?;
        acf.lines().find_map(|l| vdf_value(l, "name"))
    })
}

/// The value of `"key"  "value"` on one line of a Valve KeyValues file.
fn vdf_value(line: &str, key: &str) -> Option<String> {
    let parts: Vec<&str> = line.split('"').collect();
    // ["\t", "key", "\t\t", "value", ""]
    (parts.len() >= 4 && parts[1] == key).then(|| parts[3].to_string())
}

/// What to call the program in front if it is a game, else `None`.
pub fn detect(fg: &Foreground) -> Option<String> {
    let class = fg.class.to_ascii_lowercase();
    let stem = fg
        .exe
        .as_deref()
        .and_then(|e| e.file_stem())
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if NEVER.contains(&class.as_str()) || NEVER.contains(&stem.as_str()) {
        return None;
    }
    if let Some(name) = steam_name(&class) {
        return Some(name);
    }
    if let Some(name) = fg.exe.as_deref().and_then(library_name) {
        return Some(name);
    }
    if let Some((_, name)) = KNOWN.iter().find(|(s, _)| *s == stem || *s == class) {
        return Some((*name).to_string());
    }
    if fg.fullscreen {
        let name = tidy(if class.is_empty() { &stem } else { &class });
        return (!name.is_empty()).then_some(name);
    }
    None
}

/// The game folder name if `exe` sits in a launcher's library, e.g.
/// `.../steamapps/common/Counter-Strike 2/game/bin/linuxsteamrt64/cs2` is
/// "Counter-Strike 2".
fn library_name(exe: &Path) -> Option<String> {
    let full = exe.to_str()?;
    let lower = full.to_ascii_lowercase();
    for lib in LIBRARIES {
        let Some(at) = lower.find(lib) else { continue };
        let rest = &full[at + lib.len()..];
        let folder = rest.split('/').next()?.trim();
        // A file straight in the library folder has no game folder.
        if folder.is_empty() || !rest.contains('/') {
            continue;
        }
        if NOT_GAMES.contains(&folder.to_ascii_lowercase().as_str()) {
            return None;
        }
        return Some(folder.to_string());
    }
    None
}

/// A readable name from a program name: "some-game-win64-shipping" becomes
/// "Some Game".
fn tidy(stem: &str) -> String {
    let mut s = stem.to_string();
    for suffix in [
        "-win64-shipping",
        "-win64",
        "-linux-shipping",
        "_x64",
        "_dx12",
        "_dx11",
        "_vulkan",
        "-shipping",
    ] {
        if let Some(t) = s.strip_suffix(suffix) {
            s = t.to_string();
        }
    }
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(class: &str, exe: &str, fullscreen: bool) -> Foreground {
        Foreground {
            class: class.into(),
            exe: (!exe.is_empty()).then(|| PathBuf::from(exe)),
            fullscreen,
        }
    }

    #[test]
    fn library_games_use_their_folder_name() {
        let f = fg(
            "cs2",
            "/home/me/.local/share/Steam/steamapps/common/Counter-Strike Global Offensive/game/bin/linuxsteamrt64/cs2",
            false,
        );
        assert_eq!(
            detect(&f).as_deref(),
            Some("Counter-Strike Global Offensive")
        );
    }

    #[test]
    fn other_launchers_work_and_their_runtimes_do_not() {
        assert_eq!(
            detect(&fg("x", "/home/me/Games/Heroic/Fortnite/Binaries/x", false)).as_deref(),
            Some("Fortnite")
        );
        assert!(detect(&fg(
            "x",
            "/home/me/.local/share/Steam/steamapps/common/SteamLinuxRuntime_sniper/run",
            false
        ))
        .is_none());
        // A file straight in the library folder is not a game.
        assert!(detect(&fg(
            "x",
            "/home/me/.local/share/Steam/steamapps/common/setup",
            false
        ))
        .is_none());
    }

    #[test]
    fn well_known_games_are_recognised_anywhere() {
        assert_eq!(
            detect(&fg("dota2", "/opt/stuff/dota2", false)).as_deref(),
            Some("Dota 2")
        );
        assert_eq!(
            detect(&fg("cs2", "", false)).as_deref(),
            Some("Counter-Strike 2")
        );
    }

    #[test]
    fn everyday_programs_are_never_games() {
        for (class, exe) in [
            ("firefox", "/usr/lib/firefox/firefox"),
            ("discord", "/opt/discord/Discord"),
            ("code", "/usr/bin/code"),
            ("steam", "/usr/lib/steam/steam"),
            ("kitty", "/usr/bin/kitty"),
        ] {
            assert!(detect(&fg(class, exe, false)).is_none(), "{class}");
            // Not even when full screen (a browser video, a presentation).
            assert!(
                detect(&fg(class, exe, true)).is_none(),
                "{class} full screen"
            );
        }
    }

    #[test]
    fn unknown_programs_need_full_screen() {
        assert!(detect(&fg("somegame", "/opt/somegame", false)).is_none());
        assert_eq!(
            detect(&fg("somegame", "/opt/somegame", true)).as_deref(),
            Some("Somegame")
        );
        assert_eq!(tidy("my_cool-game_x64"), "My Cool Game");
    }

    #[test]
    fn steam_records_are_read() {
        assert_eq!(
            vdf_value("\t\"name\"\t\t\"Hades II\"", "name").as_deref(),
            Some("Hades II")
        );
        assert!(vdf_value("\t\"appid\"\t\t\"1\"", "name").is_none());
    }
}
