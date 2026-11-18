//! Installs a newer ROM Play over the one running, on macOS: downloaded and checked in the
//! background, then put in place when ROM Play restarts or quits.

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
    cfg!(target_os = "macos")
}

/// The `.app` this ROM Play runs from, or `None` for a build run straight from its folder.
pub fn running_app() -> Option<PathBuf> {
    app_containing(&std::env::current_exe().ok()?)
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

/// Whether the folder the app is in, such as Applications, lets this user replace it.
pub fn can_replace(app: &Path) -> bool {
    let Some(folder) = app.parent() else {
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

/// Puts the staged app where the running one is. The running one is moved aside first and put
/// back if the copy fails, so a failed update leaves ROM Play as it was.
pub fn install(pending: &Pending) -> Result<(), String> {
    let app = &pending.app;
    let mut old = app.as_os_str().to_os_string();
    old.push(".old");
    let old = PathBuf::from(old);
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
    if let Some(dir) = pending.staged.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(())
}

/// Opens the app again once this process has exited, so the new version starts fresh.
pub fn relaunch_after_exit(app: &Path) -> std::io::Result<()> {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(app)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
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
