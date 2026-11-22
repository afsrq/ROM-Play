//! The Nintendo Switch, played with Ryubing, the maintained continuation of Ryujinx, started as
//! its own program.
//!
//! Ryubing publishes no checksums and its Mac app has no Apple developer signature, so ROM Play
//! installs only the build below for each system, checked against its SHA-256. A newer one comes
//! with an update to ROM Play.

use crate::external::{Pinned, Running};
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "switch";
pub const KEYS: &str = "the Switch keys (prod.keys), dumped from your own Switch";
const KEY_FILES: [&str; 2] = ["prod.keys", "title.keys"];

const VERSION: &str = "1.3.3";

/// One system's download of the pinned version, and where the program is inside it.
struct Build {
    url: &'static str,
    file: &'static str,
    sha256: &'static str,
    exe: &'static str,
}

macro_rules! build {
    ($file:literal, $sha256:literal, $exe:literal) => {
        Some(Build {
            url: concat!(
                "https://git.ryujinx.app/projects/Ryubing/releases/download/1.3.3/",
                $file
            ),
            file: $file,
            sha256: $sha256,
            exe: $exe,
        })
    };
}

#[cfg(target_os = "macos")]
const BUILD: Option<Build> = build!(
    "ryujinx-1.3.3-macos_universal.app.tar.gz",
    "e4818bb84c98e0d3120691821e90772099e46101273d3f145ffdb10eee2c0dbb",
    "Ryujinx.app/Contents/MacOS/Ryujinx"
);
#[cfg(all(windows, target_arch = "x86_64"))]
const BUILD: Option<Build> = build!(
    "ryujinx-1.3.3-win_x64.zip",
    "baa93f48b012eefeca339a78a39844599ccd194c109093f354a4b9d1e03a056e",
    "publish/Ryujinx.exe"
);
// Ryujinx.sh starts Ryubing with the settings it needs on Linux.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const BUILD: Option<Build> = build!(
    "ryujinx-1.3.3-linux_x64.tar.gz",
    "19b67b222726f28d11846e9b7cbaed113ea03c2be016408662ffb27c558bd28c",
    "publish/Ryujinx.sh"
);
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const BUILD: Option<Build> = build!(
    "ryujinx-1.3.3-linux_arm64.tar.gz",
    "0485b611583c401733d706ec39db40999f08ba4e22f6f38b0d94602a8016d295",
    "publish/Ryujinx.sh"
);
#[cfg(not(any(
    target_os = "macos",
    all(windows, target_arch = "x86_64"),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
const BUILD: Option<Build> = None;

/// Whether Ryubing makes a build for this system.
pub fn available() -> bool {
    BUILD.is_some()
}

fn exe_in(dir: &Path) -> PathBuf {
    dir.join(BUILD.as_ref().map_or("Ryujinx", |b| b.exe))
}

pub fn emulator(dir: PathBuf) -> Pinned {
    Pinned {
        name: "Ryubing",
        dir,
        version: VERSION,
        url: BUILD.as_ref().map_or("", |b| b.url),
        file: BUILD.as_ref().map_or("", |b| b.file),
        sha256: BUILD.as_ref().map_or("", |b| b.sha256),
        exe_in,
    }
}

/// Ryubing's settings, keys, firmware and saves, kept apart from any Ryujinx installed outside
/// ROM Play, which it's told with --root-data-dir.
pub struct Home(pub PathBuf);

impl Home {
    fn keys_dir(&self) -> PathBuf {
        self.0.join("system")
    }

    pub fn firmware_installed(&self) -> bool {
        std::fs::read_dir(self.0.join("bis/system/Contents/registered"))
            .is_ok_and(|mut entries| entries.next().is_some())
    }

    /// Copies the keys from RomM's Switch firmware where Ryubing looks for them.
    pub fn take_keys(&self, from: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(self.keys_dir())?;
        for name in KEY_FILES {
            if let Some(found) = find_named(from, name) {
                std::fs::copy(found, self.keys_dir().join(name))?;
            }
        }
        Ok(())
    }
}

fn find_named(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
}

pub fn has_keys(dir: &Path) -> bool {
    find_named(dir, "prod.keys").is_some()
}

/// The Switch firmware in RomM's Switch firmware: a .zip of it.
pub fn find_firmware(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
        .collect();
    found.sort();
    found.pop()
}

const GAME_TYPES: [&str; 5] = ["nsp", "xci", "nca", "nro", "nso"];

fn is_game(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        GAME_TYPES.contains(&e.as_str())
    })
}

/// What Ryubing boots: a game's .nsp or .xci, or a homebrew .nro.
pub fn boot_path(rom: &Path) -> Result<PathBuf, String> {
    if is_game(rom) {
        return Ok(rom.to_path_buf());
    }
    if rom.is_dir() {
        return find_boot(rom)
            .ok_or_else(|| tr::tr!("This Switch game's folder has no .nsp or .xci in it"));
    }
    let kind = rom
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    Err(tr::tr!(
        "ROM Play plays Switch games stored as an .nsp or .xci, or a zip or 7z of one, and this one is a {kind} file.",
        kind
    ))
}

pub fn is_archive(rom: &Path) -> bool {
    rom.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("7z") || e.eq_ignore_ascii_case("zip"))
}

/// The game file nearest the top of `dir`.
pub fn find_boot(dir: &Path) -> Option<PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(next) = dirs.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if is_game(&path) {
                found.push(path);
            }
        }
    }
    found
        .into_iter()
        .min_by_key(|p| (p.components().count(), p.clone()))
}

pub fn unpack_game(archive: &Path) -> Result<PathBuf, String> {
    crate::external::unpack_game(archive, find_boot, || {
        tr::tr!("The archive has no Switch game in it")
    })
}

/// What Ryubing is started with: a game, or the first time, the firmware to install.
pub enum Start<'a> {
    Game { boot: &'a Path, fullscreen: bool },
    InstallFirmware(&'a Path),
}

pub fn command(exe: &Path, home: &Home, start: &Start) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command
        .arg("--root-data-dir")
        .arg(&home.0)
        // ROM Play keeps Ryubing to the build it checked.
        .arg("--hide-updates");
    match start {
        Start::Game { boot, fullscreen } => {
            if *fullscreen {
                command.arg("--fullscreen");
            }
            command.arg(boot);
        }
        Start::InstallFirmware(firmware) => {
            command.arg("--install-firmware").arg(firmware);
        }
    }
    command
}

pub fn launch(
    exe: &Path,
    home: &Home,
    start: &Start,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> std::io::Result<Running> {
    // Ryubing falls back to its usual folder when the one it's given doesn't exist.
    std::fs::create_dir_all(&home.0)?;
    crate::external::launch(command(exe, home, start), "ryujinx", on_exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_boot_from_their_nsp_or_xci() {
        assert!(boot_path(Path::new("/g/Zelda.nsp")).is_ok());
        assert!(boot_path(Path::new("/g/Mario.XCI")).is_ok());
        let err = boot_path(Path::new("/g/Zelda.nsz")).unwrap_err();
        assert!(err.contains(".nsz"), "{err}");
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Game/Updates")).unwrap();
        std::fs::write(dir.path().join("Game/Updates/u.nsp"), b"").unwrap();
        std::fs::write(dir.path().join("Game/base.xci"), b"").unwrap();
        assert_eq!(
            boot_path(dir.path()).unwrap(),
            dir.path().join("Game/base.xci")
        );
    }

    #[test]
    fn keys_from_romm_go_where_ryubing_looks() {
        let dir = tempfile::tempdir().unwrap();
        let romm = dir.path().join("romm");
        std::fs::create_dir_all(&romm).unwrap();
        assert!(!has_keys(&romm));
        std::fs::write(romm.join("PROD.KEYS"), b"p").unwrap();
        std::fs::write(romm.join("title.keys"), b"t").unwrap();
        std::fs::write(romm.join("Firmware 19.0.1.zip"), b"z").unwrap();
        assert!(has_keys(&romm));
        assert_eq!(find_firmware(&romm), Some(romm.join("Firmware 19.0.1.zip")));
        let home = Home(dir.path().join("ryujinx"));
        home.take_keys(&romm).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("ryujinx/system/prod.keys")).unwrap(),
            b"p"
        );
        assert_eq!(
            std::fs::read(dir.path().join("ryujinx/system/title.keys")).unwrap(),
            b"t"
        );
        assert!(!home.firmware_installed());
        std::fs::create_dir_all(
            dir.path()
                .join("ryujinx/bis/system/Contents/registered/a.nca"),
        )
        .unwrap();
        assert!(home.firmware_installed());
    }

    #[test]
    fn starts_with_romps_own_folder_and_the_game() {
        let home = Home(PathBuf::from("/data/ryujinx"));
        let exe = Path::new("/cores/ryujinx/1.3.3/Ryujinx.app/Contents/MacOS/Ryujinx");
        let game = command(
            exe,
            &home,
            &Start::Game {
                boot: Path::new("/g/Zelda.nsp"),
                fullscreen: true,
            },
        );
        let args: Vec<_> = game.get_args().collect();
        assert_eq!(
            args,
            [
                "--root-data-dir",
                "/data/ryujinx",
                "--hide-updates",
                "--fullscreen",
                "/g/Zelda.nsp"
            ]
        );
        let firmware = command(exe, &home, &Start::InstallFirmware(Path::new("/f/fw.zip")));
        let args: Vec<_> = firmware.get_args().collect();
        assert_eq!(
            args,
            [
                "--root-data-dir",
                "/data/ryujinx",
                "--hide-updates",
                "--install-firmware",
                "/f/fw.zip"
            ]
        );
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// The pinned build still downloads, matches its checksum and unpacks to the app.
    /// `cargo test -p romp-app pinned_ryubing -- --ignored`
    #[tokio::test]
    #[ignore = "downloads Ryubing"]
    async fn pinned_ryubing_installs() {
        let dir = tempfile::tempdir().unwrap();
        let exe = emulator(dir.path().to_path_buf())
            .ensure(&reqwest::Client::new(), false, |_| {})
            .await
            .unwrap()
            .exe();
        assert!(exe.is_file(), "{}", exe.display());
        if !cfg!(target_os = "macos") {
            return;
        }
        let app = exe.ancestors().nth(3).unwrap();
        let signed = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep"])
            .arg(app)
            .status()
            .unwrap();
        assert!(signed.success());
    }
}
