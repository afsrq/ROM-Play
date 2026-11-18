use super::{on_ui, Controller};
use crate::self_update::{self, Pending};
use crate::updates::{self, Platform, State, STATE_KEY};
use std::time::Duration;
use tr::tr;

/// How often a ROM Play left open looks again; `State::due` still keeps it to once a day.
const RECHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Controller {
    fn update_state(&self) -> State {
        State::from_json(self.shared.store.lock().unwrap().get(STATE_KEY).as_deref())
    }

    fn save_update_state(&self, state: &State) {
        self.shared
            .store
            .lock()
            .unwrap()
            .set(STATE_KEY, &state.to_json());
    }

    fn set_update_status(&self, text: String) {
        if let Some(ui) = self.ui() {
            ui.set_settings_update_status(text.into());
        }
    }

    /// Asks GitHub for the newest release, at most once a day, and tells about it.
    pub(super) fn check_for_updates(&self) {
        self.show_update();
        self.prepare_update();
        let state = self.update_state();
        if !self.prefs.get().check_updates || !state.due(now()) {
            return;
        }
        self.ask_github(state, false);
    }

    /// The Check now button in Settings: asks GitHub right away, whatever the setting.
    pub(super) fn check_updates_now(&self) {
        self.set_update_status(tr!("Checking…"));
        self.ask_github(self.update_state(), true);
    }

    fn ask_github(&self, state: State, by_hand: bool) {
        let http = self.shared.http.clone();
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            match updates::latest(&http, updates::RELEASES, Platform::current()).await {
                Ok(latest) => {
                    tracing::info!(version = latest.version, "newest ROM Play release");
                    let state = State {
                        checked_at: now(),
                        latest: Some(latest),
                        // Checking by hand offers a version skipped before.
                        skipped: if by_hand { None } else { state.skipped },
                    };
                    store.lock().unwrap().set(STATE_KEY, &state.to_json());
                    on_ui(move |c| {
                        if by_hand {
                            c.update_dismissed.set(false);
                        }
                        c.show_update();
                        c.prepare_update();
                    });
                }
                Err(e) => {
                    tracing::warn!("checking for a newer ROM Play: {e}");
                    if by_hand {
                        on_ui(move |c| {
                            c.set_update_status(tr!("Could not reach GitHub to check ({e})", e));
                        });
                    }
                }
            }
        });
    }

    /// Looks again every few hours while ROM Play stays open.
    pub(super) fn recheck_for_updates_now_and_then(&self) {
        self.shared.rt.spawn(async {
            loop {
                tokio::time::sleep(RECHECK_EVERY).await;
                on_ui(|c| c.check_for_updates());
            }
        });
    }

    /// Downloads and checks a newer ROM Play in the background, so it's ready to install when
    /// ROM Play restarts. Where it can't replace itself, the banner links to the download.
    fn prepare_update(&self) {
        if !self_update::supported() || !self.prefs.get().check_updates {
            return;
        }
        let state = self.update_state();
        let Some(release) = state.offer(&updates::current_version()).cloned() else {
            return;
        };
        let ready = self_update::pending().is_some_and(|p| p.version == release.version);
        if ready || self.update_staging.get() {
            return;
        }
        let Some(app) = self_update::running_app() else {
            return;
        };
        let Some(bundle) = self_update::bundle_id(&app) else {
            return;
        };
        if !self_update::can_replace(&app) {
            tracing::info!("ROM Play can't replace itself in {}", app.display());
            return;
        }
        self.update_staging.set(true);
        self.show_update();
        let http = self.shared.http.clone();
        let dir = crate::paths::data_dir()
            .join("updates")
            .join(&release.version);
        self.shared.rt.spawn(async move {
            let staged = self_update::stage(&http, &release, &dir, &bundle).await;
            on_ui(move |c| {
                c.update_staging.set(false);
                match staged {
                    Ok(staged) => {
                        tracing::info!(version = release.version, "ROM Play update ready");
                        self_update::set_pending(Pending {
                            version: release.version,
                            staged,
                            app,
                        });
                    }
                    Err(e) => tracing::warn!("downloading the ROM Play update: {e}"),
                }
                c.show_update();
            });
        });
    }

    pub(super) fn show_update(&self) {
        let Some(ui) = self.ui() else { return };
        let state = self.update_state();
        let current = updates::current_version();
        let on = self.prefs.get().check_updates;
        let available = state.available(&current).filter(|_| on);
        ui.set_update_version(
            available
                .map(|r| r.version.clone())
                .unwrap_or_default()
                .into(),
        );
        ui.set_update_homebrew(updates::homebrew());
        let ready = self_update::pending()
            .filter(|p| available.is_some_and(|r| r.version == p.version))
            .is_some();
        ui.set_update_ready(ready);
        ui.set_update_downloading(self.update_staging.get());
        let offered = state.offer(&current).is_some() && on;
        ui.set_update_shown(offered && !self.update_dismissed.get());
        let status = match state.offer(&current) {
            Some(r) if ready => tr!(
                "ROM Play {} is ready. Restart ROM Play to update.",
                r.version
            ),
            Some(r) if self.update_staging.get() => tr!("Downloading ROM Play {}…", r.version),
            Some(r) => tr!("ROM Play {} is available.", r.version),
            None if state.latest.is_some() => tr!("ROM Play is up to date."),
            None => String::new(),
        };
        ui.set_settings_update_status(status.into());
    }

    /// Restart to update: puts the downloaded version in place and opens it.
    pub(super) fn restart_to_update(&self) {
        if self.running.borrow().is_some() || self.preparing.get() {
            return;
        }
        let Some(pending) = self_update::take_pending() else {
            return;
        };
        match self_update::install(&pending) {
            Ok(()) => {
                tracing::info!(version = pending.version, "installed the ROM Play update");
                if let Err(e) = self_update::relaunch_after_exit(&pending.app) {
                    tracing::warn!("reopening ROM Play after the update: {e}");
                }
                let _ = slint::quit_event_loop();
            }
            Err(e) => {
                tracing::warn!("installing the ROM Play update: {e}");
                self.show_update();
                self.set_update_status(tr!(
                    "The update couldn't be installed ({e}). Download it from GitHub instead.",
                    e
                ));
                self.download_update();
            }
        }
    }

    pub(super) fn download_update(&self) {
        let state = self.update_state();
        let Some(offer) = state.available(&updates::current_version()) else {
            return;
        };
        let url = offer.download.as_deref().unwrap_or(&offer.page);
        if let Err(e) = open::that_detached(url) {
            tracing::warn!("opening {url}: {e}");
        }
    }

    pub(super) fn open_update_notes(&self) {
        let state = self.update_state();
        if let Some(offer) = state.available(&updates::current_version()) {
            if let Err(e) = open::that_detached(&offer.page) {
                tracing::warn!("opening {}: {e}", offer.page);
            }
        }
    }

    pub(super) fn skip_update(&self) {
        let mut state = self.update_state();
        state.skipped = state.latest.as_ref().map(|r| r.version.clone());
        self.save_update_state(&state);
        self_update::take_pending();
        self.show_update();
    }

    pub(super) fn dismiss_update(&self) {
        self.update_dismissed.set(true);
        self.show_update();
    }
}
