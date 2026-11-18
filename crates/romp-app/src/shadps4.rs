//! The PlayStation 4, played with shadPS4 started as its own program.
//!
//! shadPS4 itself has no window of its own besides the game: ROM Play is its library, and the
//! shadPS4 QtLauncher, opened on the same folder, is where its settings are changed.

use crate::external::{Asset, Emulator, Running};
use std::path::{Path, PathBuf};

/// RomM's own slug, and IGDB's, which a library matched only by IGDB can carry.
const PLATFORMS: [&str; 2] = ["ps4", "ps4--1"];
/// The latest stable release. The daily builds are pre-releases, which this skips.
pub const RELEASES: &str = "https://api.github.com/repos/shadps4-emu/shadPS4/releases/latest";
/// QtLauncher only publishes pre-releases, so ROM Play takes the newest from the list.
pub const LAUNCHER_RELEASES: &str =
    "https://api.github.com/repos/shadps4-emu/shadps4-qtlauncher/releases?per_page=10";
/// shadPS4 keeps its settings, saves and shader cache in a folder named `user` in the folder
/// it starts in, when there is one, instead of the system-wide one.
const PORTABLE_DIR: &str = "user";

pub fn is_platform(slug: &str) -> bool {
    PLATFORMS.contains(&slug)
}

/// The start of the release's file name and the program inside it, for this computer. shadPS4
/// doesn't support Intel Macs.
fn build() -> Option<(&'static str, &'static str)> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(("shadps4-macos-sdl-", "shadps4"))
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some(("shadps4-win64-sdl-", "shadPS4.exe"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(("shadps4-linux-sdl-", "Shadps4-sdl.AppImage"))
    } else {
        None
    }
}

/// QtLauncher's macOS download is the only one ROM Play knows the layout of so far.
fn launcher_build() -> Option<&'static str> {
    cfg!(all(target_os = "macos", target_arch = "aarch64")).then_some("shadPS4QtLauncher-macos-qt-")
}

pub fn launcher_available() -> bool {
    available() && launcher_build().is_some()
}

fn launcher_asset(assets: &[Asset]) -> Option<&Asset> {
    let prefix = launcher_build()?;
    assets
        .iter()
        .find(|a| a.name.starts_with(prefix) && a.name.ends_with(".zip"))
}

fn launcher_exe_in(dir: &Path, _asset: &str) -> PathBuf {
    dir.join("shadPS4QtLauncher.app/Contents/MacOS/shadPS4QtLauncher")
}

/// QtLauncher's zip holds a tar of the app, which keeps the app's links and permissions.
fn unpack_inner_tar(dir: &Path) -> Result<(), String> {
    let tars: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.to_string_lossy().to_ascii_lowercase();
            name.ends_with(".tar.gz") || name.ends_with(".tgz") || name.ends_with(".tar")
        })
        .collect();
    for tar in tars {
        let status = std::process::Command::new("tar")
            .arg("-xf")
            .arg(&tar)
            .arg("-C")
            .arg(dir)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("unpacking {} failed ({status})", tar.display()));
        }
        let _ = std::fs::remove_file(&tar);
    }
    Ok(())
}

pub fn launcher(dir: PathBuf) -> Emulator {
    Emulator {
        name: "shadPS4 QtLauncher",
        dir,
        pick: launcher_asset,
        exe_in: launcher_exe_in,
        finish: unpack_inner_tar,
    }
}

/// The name ROM Play's own shadPS4 goes by in QtLauncher's list of builds.
const LAUNCHER_VERSION_NAME: &str = "ROM Play";

/// QtLauncher's list of shadPS4 builds, with ROM Play's replacing any older entry of its own.
fn versions_json(existing: Option<&str>, exe: &Path) -> String {
    let mut list: Vec<serde_json::Value> = existing
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default();
    list.retain(|v| v.get("name").and_then(|n| n.as_str()) != Some(LAUNCHER_VERSION_NAME));
    list.push(serde_json::json!({
        "name": LAUNCHER_VERSION_NAME,
        "path": exe.to_string_lossy(),
        "date": "",
        "codename": "ROM Play",
        // QtLauncher's "Custom" kind, for a build it didn't download itself.
        "type": 2,
    }));
    serde_json::to_string_pretty(&list).expect("versions json")
}

/// Sets one key in a Qt settings (INI) file, keeping everything else in it.
fn set_ini(existing: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{section}]");
    let line = format!(
        "{key}=\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut done = false;
    for raw in existing.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            if in_section && !done {
                out.push(line.clone());
                done = true;
            }
            in_section = trimmed == header;
        } else if in_section
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        {
            if !done {
                out.push(line.clone());
                done = true;
            }
            continue;
        }
        out.push(raw.to_string());
    }
    if !done {
        if !in_section {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(header);
        }
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

pub fn available() -> bool {
    build().is_some()
}

fn release_asset(assets: &[Asset]) -> Option<&Asset> {
    let (prefix, _) = build()?;
    assets
        .iter()
        .find(|a| a.name.starts_with(prefix) && a.name.ends_with(".zip"))
}

fn exe_in(dir: &Path, _asset: &str) -> PathBuf {
    let (_, exe) = build().unwrap_or(("", "shadps4"));
    dir.join(exe)
}

pub fn emulator(dir: PathBuf) -> Emulator {
    Emulator {
        name: "shadPS4",
        dir,
        pick: release_asset,
        exe_in,
        finish: crate::external::nothing_more,
    }
}

/// shadPS4's settings, saves and caches, kept apart from any shadPS4 installed outside ROM Play.
pub struct Home(pub PathBuf);

impl Home {
    pub fn user_dir(&self) -> PathBuf {
        self.0.join(PORTABLE_DIR)
    }

    /// The `user` folder must exist before shadPS4 starts, or it falls back to its system-wide one.
    pub fn prepare(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.user_dir())
    }

    /// QtLauncher keeps its own settings in a `launcher` folder beside `user` in the folder it
    /// starts in, the same way shadPS4 does.
    fn launcher_dir(&self) -> PathBuf {
        self.0.join("launcher")
    }

    /// Makes ROM Play's shadPS4 the build QtLauncher uses, so both share one set of settings.
    pub fn point_launcher_at(&self, shadps4: &Path) -> std::io::Result<()> {
        self.prepare()?;
        let dir = self.launcher_dir();
        std::fs::create_dir_all(&dir)?;
        let versions = dir.join("versions.json");
        let existing = std::fs::read_to_string(&versions).ok();
        std::fs::write(&versions, versions_json(existing.as_deref(), shadps4))?;
        let ini = dir.join("qt_ui.ini");
        let existing = std::fs::read_to_string(&ini).unwrap_or_default();
        let exe = shadps4.to_string_lossy();
        std::fs::write(
            &ini,
            set_ini(&existing, "version_manager", "versionSelected", &exe),
        )
    }

    /// Opens QtLauncher on ROM Play's shadPS4 folder. It isn't waited for: it's an app of its own.
    pub fn open_launcher(&self, launcher: &Path) -> std::io::Result<()> {
        std::process::Command::new(launcher)
            .current_dir(&self.0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

/// Apple Silicon Macs run shadPS4, built for Intel, through Rosetta 2.
pub fn rosetta_missing() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && !Path::new("/Library/Apple/usr/libexec/oah/libRosettaRuntime").exists()
}

fn is_eboot(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("eboot.bin"))
}

/// The game folder's eboot.bin nearest the top of `dir`.
pub fn find_boot(dir: &Path) -> Option<PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(next) = dirs.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if is_eboot(&path) {
                found.push(path);
            }
        }
    }
    found
        .into_iter()
        .min_by_key(|p| (p.components().count(), p.clone()))
}

/// What shadPS4 boots: the eboot.bin of an installed game folder.
pub fn boot_path(rom: &Path) -> Result<PathBuf, String> {
    if is_eboot(rom) {
        return Ok(rom.to_path_buf());
    }
    if rom.is_dir() {
        return find_boot(rom)
            .ok_or_else(|| tr::tr!("This PS4 game folder has no eboot.bin in it"));
    }
    let kind = rom
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    Err(tr::tr!(
        "ROM Play plays PS4 games stored as an installed game folder with an eboot.bin, or a zip or 7z of one, and this one is a {kind} file.",
        kind
    ))
}

pub fn is_archive(rom: &Path) -> bool {
    rom.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("7z") || e.eq_ignore_ascii_case("zip"))
}

pub fn unpack_game(archive: &Path) -> Result<PathBuf, String> {
    crate::external::unpack_game(archive, find_boot, || {
        tr::tr!("The archive has no PS4 game folder with an eboot.bin in it")
    })
}

/// GitHub's build artifacts lose the executable bit, so a release's program can arrive without it.
fn make_executable(exe: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(exe)?.permissions().mode();
        if mode & 0o111 != 0o111 {
            std::fs::set_permissions(exe, std::fs::Permissions::from_mode(mode | 0o755))?;
        }
    }
    let _ = exe;
    Ok(())
}

pub fn command(exe: &Path, home: &Home, boot: &Path, fullscreen: bool) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command
        .current_dir(&home.0)
        .arg("--fullscreen")
        .arg(if fullscreen { "true" } else { "false" })
        .arg("--game")
        .arg(boot);
    command
}

pub fn launch(
    exe: &Path,
    home: &Home,
    boot: &Path,
    fullscreen: bool,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> std::io::Result<Running> {
    home.prepare()?;
    make_executable(exe)?;
    crate::external::launch(command(exe, home, boot, fullscreen), "shadps4", on_exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: String::new(),
            digest: None,
        }
    }

    #[test]
    fn romm_and_igdb_slugs_are_both_the_ps4() {
        assert!(is_platform("ps4"));
        assert!(is_platform("ps4--1"));
        assert!(!is_platform("ps3"));
        assert!(!is_platform("ps5"));
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn picks_the_mac_build_from_a_release() {
        let assets = [
            asset("shadps4-linux-sdl-0.21.0.zip"),
            asset("shadps4-macos-sdl-0.21.0.zip"),
            asset("shadps4-win64-sdl-0.21.0.zip"),
        ];
        assert_eq!(
            release_asset(&assets).map(|a| a.name.as_str()),
            Some("shadps4-macos-sdl-0.21.0.zip")
        );
        assert_eq!(
            exe_in(Path::new("/x/v.0.21.0"), "ignored"),
            Path::new("/x/v.0.21.0/shadps4")
        );
    }

    #[test]
    fn boots_the_eboot_nearest_the_top_of_a_game_folder() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("CUSA00001");
        std::fs::create_dir_all(game.join("sce_module/extra")).unwrap();
        std::fs::write(game.join("sce_module/extra/eboot.bin"), b"deep").unwrap();
        std::fs::write(game.join("EBOOT.BIN"), b"top").unwrap();
        assert_eq!(boot_path(dir.path()).unwrap(), game.join("EBOOT.BIN"));
        assert_eq!(
            boot_path(&game.join("EBOOT.BIN")).unwrap(),
            game.join("EBOOT.BIN")
        );
    }

    #[test]
    fn other_files_are_explained() {
        let err = boot_path(Path::new("/games/Bloodborne.pkg")).unwrap_err();
        assert!(err.contains(".pkg"), "{err}");
        let empty = tempfile::tempdir().unwrap();
        assert!(boot_path(empty.path()).is_err());
    }

    #[test]
    fn a_zipped_game_folder_is_unpacked_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Game.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("CUSA00001/eboot.bin", options).unwrap();
            std::io::Write::write_all(&mut zip, b"elf").unwrap();
            zip.start_file("CUSA00001/sce_sys/param.sfo", options)
                .unwrap();
            zip.finish().unwrap();
        }
        let boot = unpack_game(&archive).unwrap();
        assert_eq!(boot, dir.path().join("Game/CUSA00001/eboot.bin"));
        assert!(dir
            .path()
            .join("Game/CUSA00001/sce_sys/param.sfo")
            .is_file());
        assert!(!archive.exists(), "the archive is removed");
    }

    #[test]
    fn starts_in_its_own_folder_so_it_uses_romps_settings() {
        let home = Home(PathBuf::from("/data/shadps4"));
        let command = command(
            Path::new("/cores/shadps4/v1/shadps4"),
            &home,
            Path::new("/games/CUSA00001/eboot.bin"),
            true,
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/data/shadps4")));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                "--fullscreen",
                "true",
                "--game",
                "/games/CUSA00001/eboot.bin"
            ]
        );
        assert_eq!(home.user_dir(), Path::new("/data/shadps4/user"));
    }

    #[test]
    fn qtlauncher_lists_romps_build_once_and_keeps_the_players_own() {
        let exe = Path::new("/Users/me/Library/Application Support/RomP/cores/shadps4/v2/shadps4");
        let first = versions_json(None, Path::new("/old/v1/shadps4"));
        let mine = r#"[{"name":"Mine","path":"/elsewhere/shadps4","type":2}]"#;
        let merged: serde_json::Value = serde_json::from_str(&versions_json(
            Some(&first.replace(']', &format!(",{}", &mine[1..]))),
            exe,
        ))
        .unwrap();
        let names: Vec<&str> = merged
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Mine", "ROM Play"]);
        assert_eq!(merged[1]["path"], exe.to_string_lossy().as_ref());
        // Unreadable lists start over rather than failing.
        assert!(versions_json(Some("not json"), exe).contains("ROM Play"));
    }

    #[test]
    fn the_selected_build_is_set_without_touching_other_settings() {
        let start = "[general]\ntheme=dark\n\n[version_manager]\nversionPath=/x\nversionSelected=/old\n\n[gui]\nsize=2\n";
        let out = set_ini(
            start,
            "version_manager",
            "versionSelected",
            "/new path/shadps4",
        );
        assert_eq!(
            out,
            "[general]\ntheme=dark\n\n[version_manager]\nversionPath=/x\nversionSelected=\"/new path/shadps4\"\n\n[gui]\nsize=2\n"
        );
        assert_eq!(
            set_ini("", "version_manager", "versionSelected", "/a"),
            "[version_manager]\nversionSelected=\"/a\"\n"
        );
        assert_eq!(
            set_ini(
                "[gui]\nsize=2\n",
                "version_manager",
                "versionSelected",
                "/a"
            ),
            "[gui]\nsize=2\n\n[version_manager]\nversionSelected=\"/a\"\n"
        );
        assert_eq!(
            set_ini(
                "[version_manager]\nversionPath=/x\n[gui]\n",
                "version_manager",
                "versionSelected",
                "/a"
            ),
            "[version_manager]\nversionPath=/x\nversionSelected=\"/a\"\n[gui]\n"
        );
    }

    #[test]
    fn qtlauncher_starts_in_the_same_folder_and_finds_romps_build() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home(dir.path().to_path_buf());
        home.point_launcher_at(Path::new("/cores/shadps4/v1/shadps4"))
            .unwrap();
        assert!(
            home.user_dir().is_dir(),
            "the shared settings folder exists"
        );
        let ini = std::fs::read_to_string(dir.path().join("launcher/qt_ui.ini")).unwrap();
        assert!(ini.contains("versionSelected=\"/cores/shadps4/v1/shadps4\""));
        assert!(dir.path().join("launcher/versions.json").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_tar_inside_the_download_is_unpacked_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("A.app/Contents/MacOS")).unwrap();
        std::fs::write(src.join("A.app/Contents/MacOS/A"), b"bin").unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(out.join("A-macos.tar.gz"))
            .arg("-C")
            .arg(&src)
            .arg("A.app")
            .status()
            .unwrap();
        assert!(status.success());
        unpack_inner_tar(&out).unwrap();
        assert!(out.join("A.app/Contents/MacOS/A").is_file());
        assert!(!out.join("A-macos.tar.gz").exists(), "the tar is removed");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_without_the_executable_bit_gets_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("shadps4");
        std::fs::write(&exe, b"bin").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).unwrap();
        make_executable(&exe).unwrap();
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    /// Checks the real releases still unpack the way ROM Play expects. Run it with
    /// `cargo test -p romp-app real_releases -- --ignored` after shadPS4 changes its packaging.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    #[ignore = "downloads shadPS4 and QtLauncher from GitHub"]
    async fn real_releases_install_and_point_at_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let http = reqwest::Client::builder()
            .user_agent("ROM Play tests")
            .build()
            .unwrap();
        let exe = emulator(dir.path().join("shadps4"))
            .ensure(&http, RELEASES, false, |_| {})
            .await
            .unwrap()
            .exe();
        assert!(exe.is_file(), "{}", exe.display());
        assert!(exe.with_file_name("libvulkan.dylib").is_file());
        let app = launcher(dir.path().join("launcher"))
            .ensure(&http, LAUNCHER_RELEASES, false, |_| {})
            .await
            .unwrap()
            .exe();
        assert!(app.is_file(), "{}", app.display());
        make_executable(&exe).unwrap();
        let help = std::process::Command::new(&exe)
            .arg("--help")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&help.stdout);
        assert!(text.contains("--fullscreen"), "{text}");
        assert!(text.contains("--game"), "{text}");
    }
}
