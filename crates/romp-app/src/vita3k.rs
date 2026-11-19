//! The PlayStation Vita, played with Vita3K, started as its own program.
//!
//! Vita3K publishes each build to one rolling GitHub release, with a checksum for every download.
//! ROM Play keeps all of Vita3K's files, the Vita's own storage included, in its own folder, which
//! Vita3K finds as its portable folder beside the app. A game is installed into that storage the
//! first time it's played, then started from there.

use crate::external::{Asset, Emulator, Running};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "psvita";
pub const FIRMWARE: &str = "the PS Vita system software (PSVUPDAT.PUP)";
pub const RELEASES: &str = "https://api.github.com/repos/Vita3K/Vita3K/releases/tags/continuous";

fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64-latest.dmg")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-latest.dmg")
    } else {
        None
    }
}

pub fn available() -> bool {
    asset_name().is_some()
}

fn pick(assets: &[Asset]) -> Option<&Asset> {
    let name = asset_name()?;
    assets.iter().find(|a| a.name == name)
}

fn exe_in(dir: &Path, _asset: &str) -> PathBuf {
    dir.join("Vita3K.app/Contents/MacOS/Vita3K")
}

pub fn emulator(dir: PathBuf) -> Emulator {
    Emulator {
        name: "Vita3K",
        dir,
        pick,
        exe_in,
        finish: crate::external::nothing_more,
    }
}

/// Vita3K's settings, logs and the Vita's storage, kept apart from any Vita3K installed outside
/// ROM Play.
pub struct Home(pub PathBuf);

impl Home {
    fn storage(&self) -> PathBuf {
        self.0.join("fs")
    }

    /// Whether the Vita's system software is in its storage, as Vita3K itself checks.
    pub fn firmware_installed(&self) -> bool {
        std::fs::read_dir(self.storage().join("vs0")).is_ok_and(|mut e| e.next().is_some())
    }

    pub fn has_game(&self, title_id: &str) -> bool {
        is_title_id(title_id) && self.storage().join("ux0/app").join(title_id).is_dir()
    }

    /// Vita3K uses a folder named portable beside its app, when there is one, for all its files.
    /// That folder is a link to this one, so updating Vita3K keeps them.
    pub fn link_from(&self, exe: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.0)?;
        let Some(version_dir) = exe.ancestors().nth(4) else {
            return Err(std::io::Error::other("Vita3K isn't inside its app"));
        };
        let portable = version_dir.join("portable");
        if std::fs::read_link(&portable).is_ok_and(|to| to == self.0) {
            return Ok(());
        }
        if portable.is_symlink() || portable.is_file() {
            std::fs::remove_file(&portable)?;
        } else if portable.is_dir() {
            std::fs::remove_dir_all(&portable)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&self.0, &portable)?;
        Ok(())
    }

    /// Installs RomM's Vita system software, and its font package when there is one, into the
    /// Vita's storage, without opening a window.
    pub fn install_firmware(&self, exe: &Path, pups: &[PathBuf]) -> Result<(), String> {
        self.link_from(exe).map_err(|e| e.to_string())?;
        for pup in pups {
            let output = std::process::Command::new(exe)
                .arg("--firmware")
                .arg(pup)
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| tr::tr!("Could not start {name}: {e}", name = "Vita3K", e))?;
            if !output.status.success() {
                tracing::warn!(
                    "Vita3K firmware install of {} failed: {}",
                    pup.display(),
                    String::from_utf8_lossy(&output.stdout)
                );
            }
        }
        if self.firmware_installed() {
            Ok(())
        } else {
            Err(tr::tr!(
                "Vita3K could not install the PS Vita system software."
            ))
        }
    }
}

/// RomM's Vita firmware: the system software and, when there, the font package, both .pup files.
pub fn find_firmware(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pup")))
        .collect();
    found.sort();
    found
}

/// A Vita title ID, such as PCSE00001 or a homebrew's VITASHELL: nine capital letters or
/// digits, and so a safe folder name.
fn is_title_id(id: &str) -> bool {
    id.len() == 9
        && id
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn is_named(path: &Path, extensions: &[&str]) -> bool {
    path.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        extensions.contains(&e.as_str())
    })
}

/// What Vita3K installs a game from: a .vpk or .zip, which it reads itself, or a game's folder.
pub fn content_path(rom: &Path) -> Result<PathBuf, String> {
    if rom.is_file() && is_named(rom, &["vpk", "zip"]) {
        return Ok(rom.to_path_buf());
    }
    if rom.is_dir() {
        return find_boot(rom)
            .ok_or_else(|| tr::tr!("This PS Vita game's folder has no sce_sys/param.sfo in it"));
    }
    let kind = rom
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    Err(tr::tr!(
        "ROM Play plays PS Vita games stored as a .vpk, a .zip or a game folder, and this one is a {kind} file.",
        kind
    ))
}

/// Archives Vita3K can't read itself, which ROM Play unpacks first.
pub fn is_archive(rom: &Path) -> bool {
    is_named(rom, &["7z"])
}

/// The game folder nearest the top of `dir`: the one holding sce_sys/param.sfo.
pub fn find_boot(dir: &Path) -> Option<PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(next) = dirs.pop() {
        if next.join("sce_sys/param.sfo").is_file() {
            found.push(next);
            continue;
        }
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                dirs.push(path);
            }
        }
    }
    found
        .into_iter()
        .min_by_key(|p| (p.components().count(), p.clone()))
}

pub fn unpack_game(archive: &Path) -> Result<PathBuf, String> {
    crate::external::unpack_game(archive, find_boot, || {
        tr::tr!("The archive has no PS Vita game in it")
    })
}

/// The game's title ID, read from its param.sfo, which says where Vita3K installs it.
pub fn title_id(content: &Path) -> Option<String> {
    let sfo = if content.is_dir() {
        std::fs::read(content.join("sce_sys/param.sfo")).ok()?
    } else {
        sfo_in_archive(content)?
    };
    let game = sfo_value(&sfo, "CATEGORY").is_none_or(|c| c == "gd");
    let id = sfo_value(&sfo, "TITLE_ID")?;
    (game && is_title_id(&id)).then_some(id)
}

/// The param.sfo of the game nearest the top of a .vpk or .zip.
fn sfo_in_archive(archive: &Path) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).ok()?).ok()?;
    let name = zip
        .file_names()
        .filter_map(|n| n.ok().map(std::borrow::Cow::into_owned))
        .filter(|n| {
            let lower = n.to_ascii_lowercase();
            lower == "sce_sys/param.sfo" || lower.ends_with("/sce_sys/param.sfo")
        })
        .min_by_key(|n| (n.matches('/').count(), n.clone()))?;
    let mut entry = zip.by_name(&name).ok()?;
    let mut bytes = Vec::new();
    entry.by_ref().take(1 << 20).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// A text value from a param.sfo, the table of a Vita game's name, ID and kind.
pub fn sfo_value(sfo: &[u8], key: &str) -> Option<String> {
    if sfo.get(..4)? != b"\0PSF" {
        return None;
    }
    let keys = u32_at(sfo, 8)?;
    let data = u32_at(sfo, 12)?;
    let count = u32_at(sfo, 16)?;
    for i in 0..count.min(256) {
        let entry = 20 + i * 16;
        let key_at = keys + u16_at(sfo, entry)?;
        let len = u32_at(sfo, entry + 4)?;
        let data_at = data + u32_at(sfo, entry + 12)?;
        let name = sfo.get(key_at..)?;
        let name = &name[..name.iter().position(|&b| b == 0)?];
        if name != key.as_bytes() {
            continue;
        }
        let value = sfo.get(data_at..data_at.checked_add(len)?)?;
        let value = value.split(|&b| b == 0).next().unwrap_or_default();
        return Some(String::from_utf8_lossy(value).trim().to_string());
    }
    None
}

/// What Vita3K is started with: a game already in the Vita's storage, or one to install first.
pub enum Start<'a> {
    Installed(&'a str),
    Install(&'a Path),
}

pub fn command(exe: &Path, start: &Start, fullscreen: bool) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    // A plain window that plays the game and closes with it, and settings ROM Play passes here
    // stay out of Vita3K's own.
    command.args(["--frontend", "sdl", "--keep-config"]);
    if fullscreen {
        command.arg("--fullscreen");
    }
    match start {
        Start::Installed(title_id) => {
            command.arg("--installed-path").arg(title_id);
        }
        Start::Install(content) => {
            command.arg(content);
        }
    }
    command
}

pub fn launch(
    exe: &Path,
    home: &Home,
    start: &Start,
    fullscreen: bool,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> std::io::Result<Running> {
    home.link_from(exe)?;
    crate::external::launch(command(exe, start, fullscreen), "vita3k", on_exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A param.sfo holding the given text values, laid out the way the Vita's are.
    fn sfo(values: &[(&str, &str)]) -> Vec<u8> {
        let keys_at = 20 + values.len() * 16;
        let mut keys = Vec::new();
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for (key, value) in values {
            let key_offset = keys.len() as u16;
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);
            let data_offset = data.len() as u32;
            let len = value.len() as u32 + 1;
            data.extend_from_slice(value.as_bytes());
            data.push(0);
            while data.len() % 4 != 0 {
                data.push(0);
            }
            entries.extend_from_slice(&key_offset.to_le_bytes());
            entries.extend_from_slice(&0x0204u16.to_le_bytes());
            entries.extend_from_slice(&len.to_le_bytes());
            entries.extend_from_slice(&len.next_multiple_of(4).to_le_bytes());
            entries.extend_from_slice(&data_offset.to_le_bytes());
        }
        while keys.len() % 4 != 0 {
            keys.push(0);
        }
        let mut out = b"\0PSF".to_vec();
        out.extend_from_slice(&0x0101u32.to_le_bytes());
        out.extend_from_slice(&(keys_at as u32).to_le_bytes());
        out.extend_from_slice(&((keys_at + keys.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(values.len() as u32).to_le_bytes());
        out.extend(entries);
        out.extend(keys);
        out.extend(data);
        out
    }

    fn zip_of(files: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new().suffix(".vpk").tempfile().unwrap();
        let mut zip = zip::ZipWriter::new(file.reopen().unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        file
    }

    #[test]
    fn a_param_sfo_gives_the_games_title_id() {
        let game = sfo(&[
            ("CATEGORY", "gd"),
            ("TITLE", "Gravity Rush"),
            ("TITLE_ID", "PCSA00011"),
        ]);
        assert_eq!(sfo_value(&game, "TITLE").as_deref(), Some("Gravity Rush"));
        assert_eq!(sfo_value(&game, "TITLE_ID").as_deref(), Some("PCSA00011"));
        assert_eq!(sfo_value(&game, "APP_VER"), None);
        assert_eq!(sfo_value(b"not an sfo", "TITLE_ID"), None);
        assert_eq!(sfo_value(&game[..30], "TITLE_ID"), None, "cut short");
    }

    #[test]
    fn a_vpk_or_folder_says_which_game_it_installs() {
        let game = sfo(&[("CATEGORY", "gd"), ("TITLE_ID", "PCSE00001")]);
        let patch = sfo(&[("CATEGORY", "gp"), ("TITLE_ID", "PCSE00001")]);
        let vpk = zip_of(&[("eboot.bin", b"e"), ("sce_sys/param.sfo", &game)]);
        assert_eq!(title_id(vpk.path()).as_deref(), Some("PCSE00001"));
        let bundle = zip_of(&[
            (
                "app/PCSE00002/sce_sys/param.sfo",
                &sfo(&[("TITLE_ID", "PCSE00002")]),
            ),
            ("patch/PCSE00002/sce_sys/param.sfo/x", b""),
        ]);
        assert_eq!(title_id(bundle.path()).as_deref(), Some("PCSE00002"));
        let only_patch = zip_of(&[("sce_sys/param.sfo", &patch)]);
        assert_eq!(title_id(only_patch.path()), None, "a patch isn't started");
        let odd = zip_of(&[("sce_sys/param.sfo", &sfo(&[("TITLE_ID", "../../x")]))]);
        assert_eq!(title_id(odd.path()), None, "not a folder name to trust");

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Game/PCSB00003/sce_sys")).unwrap();
        std::fs::write(
            dir.path().join("Game/PCSB00003/sce_sys/param.sfo"),
            sfo(&[("CATEGORY", "gd"), ("TITLE_ID", "PCSB00003")]),
        )
        .unwrap();
        let folder = content_path(dir.path()).unwrap();
        assert_eq!(folder, dir.path().join("Game/PCSB00003"));
        assert_eq!(title_id(&folder).as_deref(), Some("PCSB00003"));
        assert_eq!(content_path(vpk.path()).unwrap(), vpk.path());
        let err = content_path(Path::new("/g/Game.pkg")).unwrap_err();
        assert!(err.contains(".pkg"), "{err}");
    }

    #[test]
    fn the_vitas_storage_says_whats_installed() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home(dir.path().join("vita3k"));
        assert!(!home.firmware_installed());
        assert!(!home.has_game("PCSE00001"));
        std::fs::create_dir_all(dir.path().join("vita3k/fs/vs0/sys")).unwrap();
        std::fs::create_dir_all(dir.path().join("vita3k/fs/ux0/app/PCSE00001")).unwrap();
        assert!(home.firmware_installed());
        assert!(home.has_game("PCSE00001"));
        assert!(!home.has_game(".."));
        assert!(!home.has_game("pcse00001"));
        std::fs::create_dir_all(dir.path().join("vita3k/fs/ux0/app/VITASHELL")).unwrap();
        assert!(
            home.has_game("VITASHELL"),
            "homebrew has its own kind of ID"
        );
    }

    #[cfg(unix)]
    #[test]
    fn vita3k_finds_rom_plays_folder_as_its_portable_one() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir
            .path()
            .join("cores/vita3k/v1/Vita3K.app/Contents/MacOS/Vita3K");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(dir.path().join("cores/vita3k/v1/portable/fs")).unwrap();
        let home = Home(dir.path().join("data/vita3k"));
        home.link_from(&exe).unwrap();
        let portable = dir.path().join("cores/vita3k/v1/portable");
        assert_eq!(std::fs::read_link(&portable).unwrap(), home.0);
        home.link_from(&exe).unwrap();
        assert!(home.0.is_dir());
    }

    #[test]
    fn games_start_in_a_plain_window() {
        let exe = Path::new("/cores/vita3k/Vita3K");
        let args = |start: &Start, fullscreen| -> Vec<String> {
            command(exe, start, fullscreen)
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args(&Start::Installed("PCSE00001"), true),
            [
                "--frontend",
                "sdl",
                "--keep-config",
                "--fullscreen",
                "--installed-path",
                "PCSE00001"
            ]
        );
        assert_eq!(
            args(&Start::Install(Path::new("/g/Game.vpk")), false),
            ["--frontend", "sdl", "--keep-config", "/g/Game.vpk"]
        );
    }

    #[test]
    fn firmware_is_every_pup_from_romm() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_firmware(dir.path()).is_empty());
        std::fs::write(dir.path().join("PSVUPDAT.PUP"), b"s").unwrap();
        std::fs::write(dir.path().join("PSP2UPDAT.PUP"), b"f").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"n").unwrap();
        assert_eq!(
            find_firmware(dir.path()),
            [
                dir.path().join("PSP2UPDAT.PUP"),
                dir.path().join("PSVUPDAT.PUP")
            ]
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod real {
    use super::*;

    /// The rolling release still installs from its disk image, and starts.
    /// `cargo test -p romp-app real_vita3k -- --ignored`
    #[tokio::test]
    #[ignore = "downloads Vita3K"]
    async fn real_vita3k_installs_and_starts() {
        let dir = tempfile::tempdir().unwrap();
        let http = reqwest::Client::builder()
            .user_agent("ROM Play tests")
            .build()
            .unwrap();
        let exe = emulator(dir.path().join("cores"))
            .ensure(&http, RELEASES, false, |_| {})
            .await
            .unwrap()
            .exe();
        assert!(exe.is_file(), "{}", exe.display());
        let home = Home(dir.path().join("data"));
        home.link_from(&exe).unwrap();
        let version = std::process::Command::new(&exe)
            .arg("--version")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&version.stdout).contains("Vita3K"));
        assert!(
            home.0.join("config.yml").is_file(),
            "Vita3K wrote its settings to ROM Play's folder"
        );
        let again = emulator(dir.path().join("cores"))
            .ensure(&http, RELEASES, false, |_| panic!("already current"))
            .await
            .unwrap();
        assert!(matches!(again, crate::external::Ready::Current(_)));
    }
}
