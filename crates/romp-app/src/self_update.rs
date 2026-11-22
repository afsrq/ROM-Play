//! Installs a newer ROM Play over the one running: downloaded and checked in the background, then
//! put in place when ROM Play restarts or quits. On a Mac it replaces the app, on Windows the
//! files in ROM Play's folder, and on Linux the AppImage. A .deb or .rpm is left to the package
//! manager.

use crate::updates::Release;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A newer ROM Play, downloaded and checked, waiting to replace the running one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub version: String,
    pub staged: PathBuf,
    pub app: PathBuf,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

pub fn set_pending(pending: Pending) {
    *PENDING.lock().unwrap() = Some(pending);
}

pub fn pending() -> Option<Pending> {
    PENDING.lock().unwrap().clone()
}

pub fn take_pending() -> Option<Pending> {
    PENDING.lock().unwrap().take()
}

/// Installs a waiting update however ROM Play ends. Quitting from the menu or Dock on macOS
/// ends the process from inside the event loop, so code after it never runs, but exit handlers do.
pub fn install_when_exiting() {
    #[cfg(unix)]
    {
        extern "C" fn at_exit() {
            install_pending_now();
        }
        // SAFETY: registers a plain function with no arguments, as atexit expects.
        unsafe {
            libc::atexit(at_exit);
        }
    }
}

/// Installs the waiting update, if there is one, and says how it went.
pub fn install_pending_now() {
    let Some(pending) = take_pending() else {
        return;
    };
    match install(&pending) {
        Ok(()) => tracing::info!(version = pending.version, "installed the ROM Play update"),
        Err(e) => tracing::warn!("installing the ROM Play update: {e}"),
    }
}

/// Whether this computer can update ROM Play in place. Elsewhere the update banner links to
/// the download instead.
pub fn supported() -> bool {
    cfg!(any(target_os = "macos", windows)) || appimage().is_some()
}

/// The AppImage this ROM Play runs from, which its runtime names.
fn appimage() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// What an update replaces: the `.app` on a Mac, the folder of the downloaded zip on Windows, and
/// the AppImage on Linux. `None` for a build run straight from where it was built.
pub fn running_app() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        app_containing(&std::env::current_exe().ok()?)
    } else if cfg!(windows) {
        release_folder(&std::env::current_exe().ok()?)
    } else {
        appimage()
    }
}

/// The folder a downloaded Windows ROM Play runs from, which has its license beside it, unlike a
/// build's own folder.
fn release_folder(exe: &Path) -> Option<PathBuf> {
    let folder = exe.parent()?;
    (folder.join("LICENSE").is_file() && folder.join(RUNNER).is_file())
        .then(|| folder.to_path_buf())
}

const RUNNER: &str = if cfg!(windows) {
    "romp-runner.exe"
} else {
    "romp-runner"
};
const ROMP: &str = if cfg!(windows) { "romp.exe" } else { "romp" };

/// What an update has to be to replace this ROM Play: on a Mac, an app with its bundle
/// identifier. Elsewhere the download's checksum and what's in it are enough.
pub fn identity(app: &Path) -> Option<String> {
    if cfg!(target_os = "macos") {
        bundle_id(app)
    } else {
        Some(String::new())
    }
}

fn app_containing(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    is_bundle.then(|| app.to_path_buf())
}

/// Whether the folder the app is in, such as Applications, lets this user replace it. On
/// Windows the files are replaced inside ROM Play's own folder.
pub fn can_replace(app: &Path) -> bool {
    let folder = if cfg!(windows) {
        Some(app)
    } else {
        app.parent()
    };
    let Some(folder) = folder else {
        return false;
    };
    let probe = folder.join(format!(".rom-play-update-{}", std::process::id()));
    let writable = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    writable
}

fn info_value(app: &Path, key: &str) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The bundle identifier of an app, which an update must share with the app it replaces.
pub fn bundle_id(app: &Path) -> Option<String> {
    info_value(app, "CFBundleIdentifier")
}

/// An update must be ROM Play, at the version GitHub said, and intact as signed.
fn check_app(app: &Path, bundle: &str, version: &str) -> Result<(), String> {
    if bundle_id(app).as_deref() != Some(bundle) {
        return Err("the download isn't ROM Play".into());
    }
    let found = info_value(app, "CFBundleShortVersionString").unwrap_or_default();
    if found != version {
        return Err(format!("the download is version {found}, not {version}"));
    }
    let signed = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()
        .map_err(|e| e.to_string())?;
    if !signed.status.success() {
        return Err(format!(
            "the download's signature doesn't check out: {}",
            String::from_utf8_lossy(&signed.stderr).trim()
        ));
    }
    Ok(())
}

fn find_app(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "app") && p.is_dir())
}

/// Downloads a release's zip into `dir`, checks it against the digest GitHub published, unpacks
/// it and checks the app inside, returning where it is.
pub async fn stage(
    http: &reqwest::Client,
    release: &Release,
    dir: &Path,
    bundle: &str,
) -> Result<PathBuf, String> {
    let url = release
        .download
        .as_deref()
        .ok_or("the release has no download for this computer")?;
    let digest = release
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .ok_or("the release has no checksum to check the download against")?;
    let bytes = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    if !crate::cores::sha256_hex(&bytes).eq_ignore_ascii_case(digest) {
        return Err("the download didn't match its checksum".into());
    }
    let dir = dir.to_path_buf();
    let version = release.version.clone();
    let bundle = bundle.to_string();
    tokio::task::spawn_blocking(move || unpack_and_check(&bytes, &dir, &bundle, &version))
        .await
        .map_err(|e| e.to_string())?
}

fn unpack_and_check(
    bytes: &[u8],
    dir: &Path,
    bundle: &str,
    version: &str,
) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        unpack_app(bytes, dir, bundle, version)
    } else if cfg!(windows) {
        unpack_folder(bytes, dir)
    } else {
        stage_appimage(bytes, dir)
    }
}

fn unpack_app(bytes: &[u8], dir: &Path, bundle: &str, version: &str) -> Result<PathBuf, String> {
    let zip = dir.join("update.zip");
    std::fs::write(&zip, bytes).map_err(|e| e.to_string())?;
    // ditto keeps the app's links, permissions and signature intact.
    let status = std::process::Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(dir)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip);
    if !status.success() {
        return Err(format!("unpacking the update failed ({status})"));
    }
    let app = find_app(dir).ok_or("the download has no app in it")?;
    check_app(&app, bundle, version)?;
    Ok(app)
}

/// The Windows zip holds one folder with ROM Play's programs in it.
fn unpack_folder(bytes: &[u8], dir: &Path) -> Result<PathBuf, String> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .and_then(|mut archive| archive.extract(dir))
        .map_err(|e| format!("unpacking the update failed: {e}"))?;
    let has_programs = |d: &Path| d.join(ROMP).is_file() && d.join(RUNNER).is_file();
    if has_programs(dir) {
        return Ok(dir.to_path_buf());
    }
    std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| has_programs(p))
        .ok_or_else(|| "the download has no ROM Play in it".to_string())
}

fn stage_appimage(bytes: &[u8], dir: &Path) -> Result<PathBuf, String> {
    if !bytes.starts_with(b"\x7fELF") {
        return Err("the download isn't an AppImage".into());
    }
    let file = dir.join("ROM-Play.AppImage");
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(file)
}

/// Puts the staged update where the running ROM Play is. What it replaces is kept until the new
/// one is in, so a failed update leaves ROM Play as it was.
pub fn install(pending: &Pending) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        install_app(pending)?;
    } else if cfg!(windows) {
        install_files(&pending.staged, &pending.app)?;
    } else {
        install_appimage(&pending.staged, &pending.app)?;
    }
    if let Some(dir) = pending.staged.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(())
}

fn install_app(pending: &Pending) -> Result<(), String> {
    let app = &pending.app;
    let old = with_suffix(app, ".old");
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(app, &old).map_err(|e| format!("moving the old app aside: {e}"))?;
    let copied = std::process::Command::new("/usr/bin/ditto")
        .arg(&pending.staged)
        .arg(app)
        .status();
    if !matches!(copied, Ok(s) if s.success()) {
        let _ = std::fs::remove_dir_all(app);
        let _ = std::fs::rename(&old, app);
        return Err(format!(
            "copying the new app into place failed ({copied:?})"
        ));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The new AppImage is written beside the old one, then renamed over it in one step, which Linux
/// allows while the old one runs.
fn install_appimage(staged: &Path, appimage: &Path) -> Result<(), String> {
    let next = with_suffix(appimage, ".new");
    std::fs::copy(staged, &next).map_err(|e| format!("copying the new AppImage: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(appimage).map_or(0o755, |m| m.permissions().mode() | 0o111);
        std::fs::set_permissions(&next, std::fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&next, appimage).map_err(|e| {
        let _ = std::fs::remove_file(&next);
        format!("putting the new AppImage in place: {e}")
    })
}

/// Replaces ROM Play's files one by one. Windows won't delete a program that's running, but it
/// will rename one, so each old file is renamed aside first, then removed, or left for the next
/// start to remove when it's still running.
fn install_files(staged: &Path, folder: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    list_files(staged, staged, &mut files).map_err(|e| e.to_string())?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut placed: Vec<PathBuf> = Vec::new();
    let result = files
        .iter()
        .try_for_each(|relative| -> std::io::Result<()> {
            let target = folder.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if target.exists() {
                let aside = with_suffix(&target, OLD);
                let _ = std::fs::remove_file(&aside);
                std::fs::rename(&target, &aside)?;
                moved.push((target.clone(), aside));
            }
            std::fs::copy(staged.join(relative), &target)?;
            placed.push(target);
            Ok(())
        });
    if let Err(e) = result {
        for file in placed {
            let _ = std::fs::remove_file(file);
        }
        for (target, aside) in moved {
            let _ = std::fs::rename(aside, target);
        }
        return Err(format!("putting the new files in place: {e}"));
    }
    for (_, aside) in moved {
        let _ = std::fs::remove_file(aside);
    }
    Ok(())
}

const OLD: &str = ".old";

fn list_files(root: &Path, dir: &Path, into: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            list_files(root, &path, into)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            into.push(relative.to_path_buf());
        }
    }
    Ok(())
}

/// Removes what an update on Windows set aside while it was still running.
pub fn clean_up_after_update() {
    if !cfg!(windows) {
        return;
    }
    let Some(folder) = running_app() else { return };
    let mut files = Vec::new();
    if list_files(&folder, &folder, &mut files).is_ok() {
        for file in files {
            if file.to_string_lossy().ends_with(OLD) {
                let _ = std::fs::remove_file(folder.join(file));
            }
        }
    }
}

/// Opens the app again once this process has exited, so the new version starts fresh.
pub fn relaunch_after_exit(app: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = format!(
            "Wait-Process -Id {} -ErrorAction SilentlyContinue; Start-Process -FilePath $args[0]",
            std::process::id()
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .arg(app.join(ROMP))
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let open = if cfg!(target_os = "macos") {
            r#"exec /usr/bin/open "$2""#
        } else {
            r#"exec "$2""#
        };
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(
                r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; {open}"#
            ))
            .arg("sh")
            .arg(std::process::id().to_string())
            .arg(app)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_app_a_program_runs_from() {
        assert_eq!(
            app_containing(Path::new("/Applications/ROM Play.app/Contents/MacOS/romp")),
            Some(PathBuf::from("/Applications/ROM Play.app"))
        );
        assert_eq!(
            app_containing(Path::new("/Users/me/target/release/romp")),
            None
        );
    }

    #[test]
    fn new_files_replace_old_ones_and_a_failure_puts_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("ROM Play");
        let staged = dir.path().join("staged");
        std::fs::create_dir_all(folder.join("shaders")).unwrap();
        std::fs::create_dir_all(staged.join("shaders")).unwrap();
        std::fs::write(folder.join(ROMP), b"old").unwrap();
        std::fs::write(folder.join("shaders/a.glsl"), b"old").unwrap();
        std::fs::write(folder.join("mine.txt"), b"kept").unwrap();
        std::fs::write(staged.join(ROMP), b"new").unwrap();
        std::fs::write(staged.join("shaders/a.glsl"), b"new").unwrap();
        std::fs::write(staged.join("shaders/b.glsl"), b"new").unwrap();
        install_files(&staged, &folder).unwrap();
        assert_eq!(std::fs::read(folder.join(ROMP)).unwrap(), b"new");
        assert_eq!(
            std::fs::read(folder.join("shaders/a.glsl")).unwrap(),
            b"new"
        );
        assert_eq!(
            std::fs::read(folder.join("shaders/b.glsl")).unwrap(),
            b"new"
        );
        assert_eq!(std::fs::read(folder.join("mine.txt")).unwrap(), b"kept");
        assert!(!with_suffix(&folder.join(ROMP), OLD).exists());

        // A file where a folder should go stops the update partway, and everything goes back.
        std::fs::write(staged.join(ROMP), b"newer").unwrap();
        std::fs::create_dir_all(staged.join("mine.txt")).unwrap();
        std::fs::write(staged.join("mine.txt/x"), b"x").unwrap();
        assert!(install_files(&staged, &folder).is_err());
        assert_eq!(std::fs::read(folder.join(ROMP)).unwrap(), b"new");
        assert_eq!(std::fs::read(folder.join("mine.txt")).unwrap(), b"kept");
    }

    #[cfg(unix)]
    #[test]
    fn a_new_appimage_replaces_the_old_one_and_can_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let appimage = dir.path().join("ROM-Play.AppImage");
        std::fs::write(&appimage, b"\x7fELF old").unwrap();
        std::fs::set_permissions(&appimage, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(stage_appimage(b"not a program", &dir.path().join("s")).is_err());
        std::fs::create_dir_all(dir.path().join("s")).unwrap();
        let staged = stage_appimage(b"\x7fELF new", &dir.path().join("s")).unwrap();
        install_appimage(&staged, &appimage).unwrap();
        assert_eq!(std::fs::read(&appimage).unwrap(), b"\x7fELF new");
        let mode = std::fs::metadata(&appimage).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
        assert!(!with_suffix(&appimage, ".new").exists());
    }

    #[test]
    fn a_downloaded_windows_rom_play_is_told_from_a_build() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(ROMP);
        std::fs::write(&exe, b"").unwrap();
        std::fs::write(dir.path().join(RUNNER), b"").unwrap();
        assert_eq!(
            release_folder(&exe),
            None,
            "a build has no license beside it"
        );
        std::fs::write(dir.path().join("LICENSE"), b"").unwrap();
        assert_eq!(release_folder(&exe), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn a_folder_this_user_can_write_to_can_hold_an_update() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_replace(&dir.path().join("ROM Play.app")));
        assert!(!can_replace(Path::new("/System/ROM Play.app")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_swaps_the_app_and_keeps_the_old_one_when_it_fails() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Applications/ROM Play.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/version"), b"old").unwrap();
        let staged = dir.path().join("updates/1.0.0/ROM Play.app");
        std::fs::create_dir_all(staged.join("Contents")).unwrap();
        std::fs::write(staged.join("Contents/version"), b"new").unwrap();
        let pending = Pending {
            version: "1.0.0".into(),
            staged: staged.clone(),
            app: app.clone(),
        };
        install(&pending).unwrap();
        assert_eq!(std::fs::read(app.join("Contents/version")).unwrap(), b"new");
        assert!(!dir.path().join("Applications/ROM Play.app.old").exists());
        assert!(
            !dir.path().join("updates/1.0.0").exists(),
            "the staged copy is cleaned up"
        );

        let missing = Pending {
            staged: dir.path().join("updates/2.0.0/ROM Play.app"),
            ..pending
        };
        assert!(install(&missing).is_err());
        assert_eq!(
            std::fs::read(app.join("Contents/version")).unwrap(),
            b"new",
            "a failed update leaves the app as it was"
        );
    }

    #[test]
    fn only_one_update_waits_at_a_time() {
        let pending = Pending {
            version: "9.9.9".into(),
            staged: PathBuf::from("/s"),
            app: PathBuf::from("/a"),
        };
        set_pending(pending.clone());
        assert_eq!(super::pending(), Some(pending.clone()));
        assert_eq!(take_pending(), Some(pending));
        assert_eq!(take_pending(), None);
    }

    /// Downloads the real 0.23.1 release and checks it the way an update is checked.
    /// `cargo test -p romp-app real_release_stages -- --ignored`
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "downloads ROM Play from GitHub"]
    async fn real_release_stages_and_installs() {
        let http = reqwest::Client::builder()
            .user_agent("ROM Play tests")
            .build()
            .unwrap();
        let release = crate::updates::latest(
            &http,
            "https://api.github.com/repos/afsrq/ROM-Play/releases/tags/v0.23.1",
            crate::updates::Platform::MacOs,
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let staged = stage(
            &http,
            &release,
            &dir.path().join("updates/0.23.1"),
            "io.github.afsrq.romplay",
        )
        .await
        .unwrap();
        let app = dir.path().join("Applications/ROM Play.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        let pending = Pending {
            version: release.version.clone(),
            staged,
            app: app.clone(),
        };
        install(&pending).unwrap();
        assert_eq!(
            info_value(&app, "CFBundleShortVersionString").as_deref(),
            Some("0.23.1")
        );
        check_app(&app, "io.github.afsrq.romplay", "0.23.1").unwrap();
        assert!(
            stage(&http, &release, &dir.path().join("x"), "com.example.other")
                .await
                .is_err(),
            "another app's id is refused"
        );
    }
}
