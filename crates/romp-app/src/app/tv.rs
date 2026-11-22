//! TV mode: ROM Play full screen, sized for a TV across the room and played with a controller.
//! It shows the same library, game pages and downloads as the window, laid out bigger, with the
//! lists along the top instead of in a sidebar.

use super::{with_controller, Controller, SCREEN_GAME, SCREEN_LIBRARY, SCREEN_SETTINGS};
use crate::input::{A, B, DOWN, START, UP};
use crate::SidebarEntry;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::time::Duration;

/// The menu Start opens: leave TV mode, Settings, and quit.
const MENU_LEAVE: i32 = 0;
const MENU_SETTINGS: i32 = 1;
const MENU_QUIT: i32 = 2;
const MENU_ITEMS: i32 = 3;

impl Controller {
    pub(super) fn tv(&self) -> bool {
        self.tv.get()
    }

    pub(super) fn toggle_tv(&self) {
        if self.tv.get() {
            self.leave_tv();
        } else {
            self.enter_tv();
        }
    }

    pub(super) fn enter_tv(&self) {
        let Some(ui) = self.ui() else { return };
        if self.tv.get()
            || !matches!(
                ui.get_screen(),
                SCREEN_LIBRARY | SCREEN_GAME | SCREEN_SETTINGS
            )
        {
            return;
        }
        if ui.get_screen() == SCREEN_SETTINGS {
            self.back_to_library();
        }
        self.tv.set(true);
        self.tv_was_fullscreen.set(ui.window().is_fullscreen());
        ui.global::<crate::Palette>()
            .set_color_scheme(slint::language::ColorScheme::Dark);
        ui.set_tv_menu_open(false);
        ui.set_tv_mode(true);
        ui.window().set_fullscreen(true);
        self.show_tv_tab();
        self.tick_tv_clock();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_secs(5), || {
            with_controller(|c| c.tick_tv_clock());
        });
        *self.tv_clock.borrow_mut() = Some(timer);
        let games = self.library.borrow().games.len();
        let card = self.nav_card.get().filter(|i| *i < games);
        self.focus_card(card.or((games > 0).then_some(0)));
        tracing::info!("entered TV mode");
    }

    pub(super) fn leave_tv(&self) {
        let Some(ui) = self.ui() else { return };
        if !self.tv.replace(false) {
            return;
        }
        self.tv_clock.borrow_mut().take();
        ui.set_tv_menu_open(false);
        ui.set_tv_mode(false);
        if !self.tv_was_fullscreen.get() {
            ui.window().set_fullscreen(false);
        }
        ui.global::<crate::Palette>()
            .set_color_scheme(self.prefs.get().color_scheme());
        // The window's grid scrolls itself to the game chosen in TV mode.
        self.focus_card(self.nav_card.get());
    }

    /// The lists along the top: every console and collection, whatever the sidebar folds away.
    pub(super) fn set_tv_tabs(&self, tabs: Vec<SidebarEntry>) {
        *self.tv_tab_keys.borrow_mut() = tabs.iter().map(|t| t.key.to_string()).collect();
        if let Some(ui) = self.ui() {
            ui.set_tv_tabs(ModelRc::new(VecModel::from(tabs)));
        }
        self.show_tv_tab();
    }

    pub(super) fn show_tv_tab(&self) {
        let Some(ui) = self.ui() else { return };
        let selected = self.selected.borrow().clone();
        let index = self
            .tv_tab_keys
            .borrow()
            .iter()
            .position(|k| *k == selected)
            .unwrap_or(0);
        ui.set_tv_tab(index as i32);
    }

    /// LB and RB in TV mode: the list before or after, round and round.
    pub(super) fn step_tv_tab(&self, step: i32) {
        let keys = self.tv_tab_keys.borrow().clone();
        if keys.is_empty() {
            return;
        }
        let current = keys
            .iter()
            .position(|k| *k == *self.selected.borrow())
            .unwrap_or(0) as i32;
        let next = (current + step).rem_euclid(keys.len() as i32) as usize;
        self.nav_card.set(None);
        self.select(keys[next].clone());
        self.focus_card(Some(0));
    }

    fn tick_tv_clock(&self) {
        if let Some(ui) = self.ui() {
            let text = local_time().map_or_else(String::new, |(hour, minute)| {
                clock_text(hour, minute, twelve_hour_clock())
            });
            ui.set_tv_clock(text.into());
        }
    }

    pub(super) fn open_tv_menu(&self) {
        if let Some(ui) = self.ui() {
            ui.set_tv_menu_index(MENU_LEAVE);
            ui.set_tv_menu_open(true);
        }
    }

    pub(super) fn close_tv_menu(&self) {
        if let Some(ui) = self.ui() {
            ui.set_tv_menu_open(false);
        }
    }

    /// A button while the menu is open. Returns whether the menu took it.
    pub(super) fn tv_menu_button(&self, button: u32) -> bool {
        let Some(ui) = self.ui() else { return false };
        if !ui.get_tv_menu_open() {
            return false;
        }
        let index = ui.get_tv_menu_index();
        match button {
            UP => ui.set_tv_menu_index((index - 1).rem_euclid(MENU_ITEMS)),
            DOWN => ui.set_tv_menu_index((index + 1).rem_euclid(MENU_ITEMS)),
            A => self.tv_menu_chosen(index),
            B | START => self.close_tv_menu(),
            _ => {}
        }
        true
    }

    pub(super) fn tv_menu_chosen(&self, index: i32) {
        self.close_tv_menu();
        match index {
            MENU_LEAVE => self.leave_tv(),
            MENU_SETTINGS => {
                self.leave_tv();
                self.open_settings();
            }
            MENU_QUIT => {
                let _ = slint::quit_event_loop();
            }
            _ => {}
        }
    }

    /// The keyboard in TV mode works like a controller: the arrows move, Return chooses, Escape
    /// goes back, and Space opens the menu.
    pub(super) fn tv_key(&self, key: &str) {
        if let Some(button) = key_button(key) {
            self.nav_button(button);
        }
    }
}

fn key_button(key: &str) -> Option<u32> {
    use crate::input::{LEFT, RIGHT};
    let c = key.chars().next()?;
    Some(match c {
        '\u{F700}' => UP,
        '\u{F701}' => DOWN,
        '\u{F702}' => LEFT,
        '\u{F703}' => RIGHT,
        '\n' | '\r' => A,
        '\u{1b}' | '\u{8}' | '\u{7f}' => B,
        ' ' => START,
        _ => return None,
    })
}

/// The hour and minute here and now.
fn local_time() -> Option<(u32, u32)> {
    #[cfg(unix)]
    {
        // SAFETY: localtime_r only reads the time given and writes the struct it's lent.
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&now, &mut tm).is_null() {
                return None;
            }
            Some((tm.tm_hour as u32, tm.tm_min as u32))
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Where clocks read 3:05 PM rather than 15:05.
fn twelve_hour_clock() -> bool {
    const TWELVE_HOUR: [&str; 9] = ["US", "CA", "AU", "NZ", "IN", "PH", "PK", "EG", "SA"];
    sys_locale::get_locale().is_some_and(|locale| {
        locale
            .split(['-', '_'])
            .nth(1)
            .is_some_and(|region| TWELVE_HOUR.contains(&region.to_ascii_uppercase().as_str()))
    })
}

fn clock_text(hour: u32, minute: u32, twelve: bool) -> String {
    if !twelve {
        return format!("{hour:02}:{minute:02}");
    }
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let hour = match hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour}:{minute:02} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_the_way_clocks_do_here() {
        assert_eq!(clock_text(15, 5, true), "3:05 PM");
        assert_eq!(clock_text(0, 30, true), "12:30 AM");
        assert_eq!(clock_text(12, 0, true), "12:00 PM");
        assert_eq!(clock_text(9, 7, false), "09:07");
        assert_eq!(clock_text(23, 59, false), "23:59");
    }

    #[test]
    fn the_keyboard_works_like_a_controller() {
        assert_eq!(key_button("\u{F700}"), Some(UP));
        assert_eq!(key_button("\n"), Some(A));
        assert_eq!(key_button("\u{1b}"), Some(B));
        assert_eq!(key_button(" "), Some(START));
        assert_eq!(key_button("q"), None);
        assert_eq!(key_button(""), None);
    }
}
