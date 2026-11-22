//! Adding games from this computer, outside RomM, and playing the Mac apps among them.

use super::{game::playable, on_ui, with_controller, Controller};
use crate::local_games::{self, Identified, ICON_KEY};
use crate::play;
use crate::store::{GameDetail, NewLocalGame};
use slint::{ModelRc, VecModel};
use std::path::{Path, PathBuf};
use tr::tr;

/// The Add a game dialog while it's open: the picked game, and the lists its menus show.
pub(super) struct AddGame {
    found: Identified,
    /// Slug and name of each console offered.
    consoles: Vec<(String, String)>,
    /// RomM's games on the chosen console, likeliest first, after "None".
    candidates: Vec<(i64, String)>,
}

fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<slint::SharedString> {
    ModelRc::new(VecModel::from(
        items.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
}

impl Controller {
    /// The + button: picks an app or ROM file with the system's file picker, then asks which
    /// console it belongs to and which RomM game it is.
    pub(super) fn add_local_game(&self) {
        self.shared.rt.spawn(async {
            let picked = tokio::task::spawn_blocking(local_games::pick_file)
                .await
                .ok()
                .flatten();
            let Some(path) = picked else { return };
            let found = tokio::task::spawn_blocking(move || local_games::identify(&path)).await;
            if let Ok(found) = found {
                on_ui(move |c| c.open_add_game(found));
            }
        });
    }

    fn open_add_game(&self, found: Identified) {
        let Some(ui) = self.ui() else { return };
        let consoles: Vec<(String, String)> = self
            .shared
            .store
            .lock()
            .unwrap()
            .local_consoles()
            .into_iter()
            .filter(|(slug, _)| found.is_app || playable(slug))
            .collect();
        let console = if found.is_app {
            consoles.iter().position(|(slug, _)| slug == "mac")
        } else {
            None
        };
        ui.set_add_game_file(
            found
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
        );
        ui.set_add_game_is_app(found.is_app);
        ui.set_add_game_name(found.title.clone().into());
        ui.set_add_game_consoles(strings(consoles.iter().map(|(_, name)| name.clone())));
        ui.set_add_game_console(console.map_or(-1, |i| i as i32));
        ui.set_add_game_error(slint::SharedString::new());
        *self.add_game.borrow_mut() = Some(AddGame {
            found,
            consoles,
            candidates: Vec::new(),
        });
        self.offer_matches(console);
        ui.set_add_game_open(true);
    }

    /// Lists RomM's games on the chosen console, choosing the likeliest when it's clearly the same.
    fn offer_matches(&self, console: Option<usize>) {
        let Some(ui) = self.ui() else { return };
        let mut adding = self.add_game.borrow_mut();
        let Some(adding) = adding.as_mut() else {
            return;
        };
        let slug = console
            .and_then(|i| adding.consoles.get(i))
            .map(|(s, _)| s.clone());
        let candidates = slug
            .map(|slug| self.shared.store.lock().unwrap().match_candidates(&slug))
            .unwrap_or_default();
        let title = ui.get_add_game_name().to_string();
        adding.candidates = local_games::rank_matches(&title, candidates);
        let best = local_games::best_match(&title, &adding.candidates);
        ui.set_add_game_matches(strings(
            std::iter::once(tr!("None")).chain(adding.candidates.iter().map(|(_, t)| t.clone())),
        ));
        ui.set_add_game_match(best.map_or(0, |i| i as i32 + 1));
    }

    pub(super) fn add_game_console_chosen(&self, index: i32) {
        self.offer_matches(usize::try_from(index).ok());
    }

    pub(super) fn add_game_cancelled(&self) {
        self.add_game.borrow_mut().take();
        if let Some(ui) = self.ui() {
            ui.set_add_game_open(false);
        }
    }

    pub(super) fn add_game_confirmed(&self) {
        let Some(ui) = self.ui() else { return };
        let Some(adding) = self.add_game.borrow_mut().take() else {
            return;
        };
        let title = ui.get_add_game_name().trim().to_string();
        let console = usize::try_from(ui.get_add_game_console())
            .ok()
            .and_then(|i| adding.consoles.get(i).cloned());
        let Some((slug, console_name)) = console.filter(|_| !title.is_empty()) else {
            ui.set_add_game_error(tr!("Choose a name and a console.").into());
            *self.add_game.borrow_mut() = Some(adding);
            return;
        };
        let matched = usize::try_from(ui.get_add_game_match() - 1)
            .ok()
            .and_then(|i| adding.candidates.get(i))
            .map(|(id, _)| *id);
        let icon = (adding.found.is_app && matched.is_none())
            .then(|| local_games::app_icon_png(&adding.found.path))
            .flatten();
        let path = adding.found.path.to_string_lossy().into_owned();
        let added_at = now_rfc3339();
        let id = self
            .shared
            .store
            .lock()
            .unwrap()
            .add_local_game(&NewLocalGame {
                title: &title,
                platform_slug: &slug,
                platform_name: &console_name,
                path: &path,
                size_bytes: adding.found.size_bytes,
                added_at: Some(&added_at),
                cover: icon.as_ref().map(|_| ICON_KEY),
                match_id: matched,
            });
        if let Some(png) = icon {
            if let Err(e) = self.shared.covers.store_own(id, ICON_KEY, &png) {
                tracing::warn!("keeping the icon of {title}: {e}");
            }
        }
        tracing::info!(id, title, console = slug, "added a game from this computer");
        ui.set_add_game_open(false);
        self.reload_sidebar();
        self.open_game(id);
    }

    /// Puts back the icons of Mac apps added from this computer, which are their only covers,
    /// after the image cache is cleared.
    pub(super) fn restore_local_icons(&self) {
        let apps = self
            .shared
            .store
            .lock()
            .unwrap()
            .local_games_with_icons(ICON_KEY);
        for (id, path) in apps {
            if let Some(png) = local_games::app_icon_png(Path::new(&path)) {
                let _ = self.shared.covers.store_own(id, ICON_KEY, &png);
            }
        }
    }

    /// Remove from ROM Play, on a local game's page: only its place in the library goes.
    pub(super) fn remove_local_game(&self) {
        let Some(detail) = self.current_game() else {
            return;
        };
        if !local_games::is_local(detail.id) {
            return;
        }
        if self.running.borrow().is_some() || self.preparing.get() {
            return self.game_status(detail.id, tr!("Close the running game first."));
        }
        self.shared
            .store
            .lock()
            .unwrap()
            .remove_local_game(detail.id);
        self.reload_sidebar();
        self.back_to_library();
    }

    /// An app plays as itself: ROM Play opens it, and the game counts as running until it quits.
    pub(super) fn play_local_app(&self, detail: GameDetail, app: PathBuf) {
        let id = detail.id;
        let name = detail.title.clone();
        let on_exit = move |code: Option<i32>, log: Vec<String>| {
            let _ = slint::invoke_from_event_loop(move || {
                with_controller(|c| c.external_closed(id, &name, code, log));
            });
        };
        match crate::external::launch(open_app(&app), "app", on_exit) {
            Ok(running) => {
                *self.running.borrow_mut() = Some(play::RunningGame::External(running));
                self.refresh_play_button();
                self.game_status(id, String::new());
                self.record_play(id);
                *self.playing.borrow_mut() = Some(detail);
            }
            Err(e) => self.game_status(
                id,
                tr!("Could not open {name}: {e}", name = detail.title, e),
            ),
        }
    }
}

/// Opens an app as a new instance and waits until it quits, which is when the game ends. Other
/// systems run the program itself, from its own folder, where ports look for their files.
fn open_app(app: &Path) -> std::process::Command {
    if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("/usr/bin/open");
        command.args(["-W", "-n"]).arg(app);
        return command;
    }
    let mut command = std::process::Command::new(app);
    if let Some(dir) = app.parent() {
        command.current_dir(dir);
    }
    command
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_written_the_way_romm_writes_them() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_775), (2026, 11, 18));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "a leap day");
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_app_opens_as_a_new_instance_and_is_waited_for() {
        let command = open_app(Path::new("/Applications/Zelda 64.app"));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-W", "-n", "/Applications/Zelda 64.app"]);
    }
}
