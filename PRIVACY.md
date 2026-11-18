# Privacy

ROM Play is a desktop app. It has no servers of its own, no accounts of its own, no analytics, no crash reporting and no ads. Nothing about you or how you use ROM Play is sent to its developers.

## What ROM Play sends, and where

ROM Play only talks to these services, and only to do what you ask of it:

| Service | When | What it receives |
|---|---|---|
| Ryubing (git.ryujinx.app) | When ROM Play installs the Switch emulator | A request for that build |
| Your RomM server | While you use your library | Your sign-in, the requests for your library, covers and games, your saves, save states and screenshots, play sessions and play time, and a device name made of "ROM Play on" and your computer's name |
| RetroAchievements (retroachievements.org) | Only after you sign in to it in Settings | Your username, a sign-in token, which game you're playing (by the game file's hash), the achievements you unlock, leaderboard scores and what you're doing in the game ("rich presence") |
| libretro buildbot (buildbot.libretro.com) | When ROM Play installs or updates an emulator, or system files one needs | A request for that file |
| GitHub (github.com) | Once a day, or when you choose Check now, to check for a newer ROM Play and, on a Mac, download it, unless you turn this off in Settings. When ROM Play installs or updates the PS2 emulator on macOS, xemu, RPCS3, shadPS4 or the shadPS4 QtLauncher, which it checks for before each game of theirs starts | A request for that release |

Every request also carries your IP address and a short description of the app, such as `Romp/0.9.0 (macOS)`, as any internet request does. Each of these services handles that under its own privacy policy: [RetroAchievements](https://retroachievements.org/privacy), [GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement), and for your RomM server, whoever runs it.

Emulators run in a sandbox. On macOS it also stops them reaching the internet. On Windows and Linux it limits which files they can change, but not their network access. Achievement requests from a game go through the app, which only sends them to RetroAchievements. xemu, RPCS3 and shadPS4 run as their own programs, outside ROM Play's sandbox.

## What ROM Play keeps on your computer

- **Games you add from this Mac** with the + button, and their play time and saves, stay on this computer. ROM Play doesn't tell RomM about them.
- **Sign-in tokens** for your RomM server and RetroAchievements, in your system's keychain or credential store. ROM Play never stores your passwords.
- **A log of what ROM Play did**, for the last five launches, in the `logs` folder inside the folder below. It can include your server's address, game names and folder paths, never your sign-in tokens or passwords, and ROM Play never sends it anywhere. About ROM Play → Show log opens it, so you can attach it to a bug report if you choose to.
- **Your library list, downloaded games, saves, save states, screenshots and settings**, in `~/Library/Application Support/Romp` on macOS, `%LOCALAPPDATA%\Romp` on Windows and `~/.local/share/Romp` on Linux.

They stay until you remove them. Signing out of RomM in Settings removes its token and the library list. Signing out of RetroAchievements removes its token. Deleting the folder above removes everything else.

## Your rights

ROM Play's developers hold no data about you, so there's nothing for them to show, correct or delete under the GDPR or similar laws. For data held by your RomM server, ask whoever runs it. For RetroAchievements, see its privacy policy.

## Changes

Changes to this policy are part of ROM Play's source history, and each release's notes mention them.

## Contact

Questions about privacy can go to [ROM Play's issues on GitHub](https://github.com/afsrq/ROM-Play/issues).
