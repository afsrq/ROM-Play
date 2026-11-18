//! Games on this computer that aren't in RomM: Mac apps, such as recompiled ports, and ROM files,
//! which ROM Play plays with its own emulators. Each goes under the console it belongs to, and can
//! borrow its cover and details from the same game in RomM.

use std::path::{Path, PathBuf};

/// Games added from this computer have negative ids, so RomM never hears of them.
pub fn is_local(id: i64) -> bool {
    id < 0
}

/// What the cover cache knows a game's own icon by, when it came from its app.
pub const ICON_KEY: &str = "local:icon";

/// A game picked on this computer, worked out from its file before it's added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identified {
    pub path: PathBuf,
    pub title: String,
    pub is_app: bool,
    pub size_bytes: i64,
}

/// Asks for a game with the system's file picker. `None` when it's cancelled.
pub fn pick_file() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(
            r#"POSIX path of (choose file with prompt "Choose a game to add to ROM Play: a Mac app, or a ROM file")"#,
        )
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // The picker ends a folder, such as an app, with a slash.
    let path = path.trim_end_matches('/');
    (!path.is_empty()).then(|| PathBuf::from(path))
}

pub fn is_app(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        && path.is_dir()
}

/// Reads a picked game's name and size: an app's own name, or a ROM's file name without its
/// extension and tags such as (USA).
pub fn identify(path: &Path) -> Identified {
    let is_app = is_app(path);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = if is_app {
        app_name(path).unwrap_or_else(|| crate::versions::stem(&file_name).to_string())
    } else {
        rom_title(&file_name)
    };
    Identified {
        path: path.to_path_buf(),
        title,
        is_app,
        size_bytes: size_of(path) as i64,
    }
}

fn info_value(app: &Path, key: &str) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !value.is_empty()).then_some(value)
}

fn app_name(app: &Path) -> Option<String> {
    info_value(app, "CFBundleDisplayName").or_else(|| info_value(app, "CFBundleName"))
}

/// A ROM's name from its file: `Super Mario 64 (USA) [!].z64` is `Super Mario 64`.
pub fn rom_title(file_name: &str) -> String {
    let stem = crate::versions::stem(file_name);
    let cut = stem.find(['(', '[']).unwrap_or(stem.len());
    let title = stem[..cut].trim().replace('_', " ");
    if title.is_empty() {
        stem.to_string()
    } else {
        title
    }
}

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| size_of(&e.path()))
        .sum()
}

/// An app's icon as a PNG, from the .icns it names, through macOS's own image tool.
pub fn app_icon_png(app: &Path) -> Option<Vec<u8>> {
    let name = info_value(app, "CFBundleIconFile").unwrap_or_else(|| "AppIcon".into());
    let file = if Path::new(&name).extension().is_some() {
        name
    } else {
        format!("{name}.icns")
    };
    let icns = app.join("Contents/Resources").join(file);
    if !icns.is_file() {
        return None;
    }
    let png = std::env::temp_dir().join(format!("rom-play-icon-{}.png", std::process::id()));
    let status = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "--resampleHeightWidthMax", "512"])
        .arg(&icns)
        .arg("--out")
        .arg(&png)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    let bytes = status.success().then(|| std::fs::read(&png).ok()).flatten();
    let _ = std::fs::remove_file(&png);
    bytes
}

/// Words that say a game is a port rather than which game it is.
const NOISE: [&str; 9] = [
    "the",
    "recomp",
    "recompiled",
    "port",
    "pc",
    "mac",
    "edition",
    "remastered",
    "of",
];

fn words(title: &str) -> Vec<String> {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !NOISE.contains(w))
        .map(str::to_string)
        .collect()
}

/// How alike two titles are, from 0 to 1, by the words they share.
fn likeness(a: &str, b: &str) -> f32 {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|w| b.contains(w)).count();
    shared as f32 / a.len().max(b.len()) as f32
}

/// RomM's games on the console, the likeliest match for `title` first, then by name.
pub fn rank_matches(title: &str, mut candidates: Vec<(i64, String)>) -> Vec<(i64, String)> {
    candidates.sort_by(|(_, x), (_, y)| {
        likeness(title, y)
            .partial_cmp(&likeness(title, x))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.to_lowercase().cmp(&y.to_lowercase()))
    });
    candidates
}

/// The likeliest match is chosen for you only when it's clearly the same game.
pub fn best_match(title: &str, ranked: &[(i64, String)]) -> Option<usize> {
    let (_, first) = ranked.first()?;
    (likeness(title, first) >= 0.6).then_some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_names_lose_their_tags_and_extension() {
        assert_eq!(rom_title("Super Mario 64 (USA) [!].z64"), "Super Mario 64");
        assert_eq!(rom_title("Sonic_the_Hedgehog.md"), "Sonic the Hedgehog");
        assert_eq!(rom_title("(Proto).bin"), "(Proto)");
    }

    #[test]
    fn a_ports_likeliest_match_comes_first() {
        let ranked = rank_matches(
            "Zelda64 Recompiled",
            vec![
                (1, "Banjo-Kazooie".into()),
                (2, "The Legend of Zelda: Majora's Mask".into()),
                (3, "Zelda64".into()),
            ],
        );
        assert_eq!(ranked[0].0, 3);
        assert_eq!(best_match("Zelda64 Recompiled", &ranked), Some(0));
        let none = rank_matches("Tetris", vec![(1, "Banjo-Kazooie".into())]);
        assert_eq!(
            best_match("Tetris", &none),
            None,
            "an unlike game isn't picked for you"
        );
        assert_eq!(best_match("Tetris", &[]), None);
    }

    #[test]
    fn a_rom_is_identified_by_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("Tetris (World).gb");
        std::fs::write(&rom, [0u8; 32]).unwrap();
        let found = identify(&rom);
        assert_eq!(found.title, "Tetris");
        assert!(!found.is_app);
        assert_eq!(found.size_bytes, 32);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_app_is_identified_by_its_own_name_and_icon() {
        let app = Path::new("/System/Applications/Calculator.app");
        if !app.exists() {
            return;
        }
        let found = identify(app);
        assert!(found.is_app);
        assert_eq!(found.title, "Calculator");
        assert!(found.size_bytes > 0);
        let png = app_icon_png(app).expect("Calculator has an icon");
        assert!(png.starts_with(b"\x89PNG"));
    }
}
