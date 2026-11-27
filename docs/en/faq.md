# BatteryTray FAQ

**English** · [Русский](../ru/faq.md) · [中文](../zh/faq.md)

[BatteryTray](../../README.md) is a free, open-source app for Windows 10/11 and macOS 13+ that shows the battery level of wireless headsets, mice, keyboards, game controllers, AirPods and other Bluetooth devices next to the clock, and reminds you before a battery runs out. [Download](../../README.md#download-and-install) · [Supported devices](devices.md) · [Features](features.md)

## Seeing the battery

**Is there an app that shows the battery level of my wireless mouse, keyboard or headset on the taskbar or in the menu bar?**
Yes: BatteryTray, free and open source, for Windows 10/11 and macOS 13+. It shows the level of the device that most needs charging next to the clock on Windows (or several devices on the taskbar), your devices' levels in the Mac menu bar, every device in one window, and reminds you before a battery runs out. It reads gaming mice, keyboards and headsets through their receivers (Razer, Logitech, SteelSeries, HyperX, Corsair, ASUS, Attack Shark and many more) and Bluetooth devices, with no vendor software.

**How do I see the battery of my Bluetooth devices on Windows 11 without opening Settings?**
BatteryTray puts it next to the clock: every Bluetooth device whose level Windows knows, read fresh from Bluetooth LE devices, with AirPods' earbuds and case each on their own.

**How do I see my AirPods battery on Windows?**
BatteryTray reads the level of each earbud and of the case from the AirPods' own Bluetooth messages, as an iPhone does, and shows it next to the clock.

**Can it show my Bluetooth headphones' battery?**
Yes, if the system itself knows it: on Windows, when Settings → Bluetooth & devices shows a percentage; on a Mac, when the Bluetooth menu does.

**Is there a Mac menu bar app that shows the battery of all my devices?**
Yes: BatteryTray for macOS shows AirPods (each earbud and the case), Magic Mouse, Magic Keyboard, Magic Trackpad, Bluetooth headsets and mice, game controllers, an iPhone or iPad on a cable, and gaming mice, keyboards and headsets on their receivers, in the menu bar and in one window.

**How do I see my headset's battery while gaming?**
Turn on the overlay in Settings → Overlay, then press Ctrl+Alt+V (⌃⌥V on a Mac) in the game: every connected device's level shows for five seconds, without taking the focus. The shortcut can be changed there too.

**Can it show my iPhone or iPad battery on a Mac?**
Yes, while it is on a cable and trusts the Mac (Finder asks for that the first time you connect it): its level and whether it charges, in the list and the menu bar.

**Can it show my Android phone's battery on Windows?**
Yes, while it is on the cable in "File transfer" mode (in "Charging only" Windows doesn't see it as a device).

## Without vendor software

**How do I see my Razer Barracuda X battery on PC without Razer Synapse?**
Install BatteryTray: it reads the headset through its USB-C 2.4 GHz dongle, no Bluetooth or Synapse needed.

**How do I check my Logitech mouse or keyboard battery without G HUB or Options+?**
BatteryTray asks the device over Logitech's HID++ protocol through the Unifying, Bolt or Lightspeed receiver.

**How do I check my Razer, Logitech, SteelSeries or Corsair battery on a Mac without their software?**
The same way as on Windows: BatteryTray talks to the device's receiver over USB with the device's own protocol.

**Do I have to uninstall Synapse, G HUB, iCUE or the other vendor apps?**
No, they can stay. The one exception known so far: an ASTRO A50 can't be read while ASTRO Command Center is open.

## Reminders

**How do I get a low battery warning for my wireless mouse or headset?**
BatteryTray shows a reminder window when a device drops below its level (20% by default, or a level of its own for each device) and again at the critical level. Do Not Disturb doesn't hide it, and it doesn't take the focus from a game. Left unanswered, it closes after 10 seconds and comes back after the repeat interval; while you're away from the computer it waits for you.

**Can each reminder have its own sound?**
Yes, in Settings → Reminders → Sounds: the low battery reminder, the critical one and "Charging complete" each play the system's sound, another of the system's sounds, your own WAV or MP3 file, or nothing.

**Why is there no percentage while my device charges?**
Some devices don't report their level on the cable, or report a number that has nothing to do with the real charge. BatteryTray then shows that it's charging rather than a stale or made-up number.

## Installing and safety

**Why does Windows warn about an unknown app?**
The exe isn't signed with a paid code signing certificate, so Microsoft Defender SmartScreen doesn't know it yet. Click **More info**, then **Run anyway**. The source is public and every release is built from it.

**Why does macOS say it can't verify the app?**
The app is signed with the project's own certificate, not with Apple's paid Developer ID, and isn't notarized. So the first time macOS blocks it: open **System Settings → Privacy & Security** and click **Open Anyway**. The certificate is the same for every version, so the permissions you give stay after updates.

**Is it safe? What does it send?**
BatteryTray only asks devices for their battery and never changes their settings. It has no account and no telemetry; the only place it connects to is GitHub, to look for a new version. Updates are checked against the SHA-256 that GitHub publishes, and on a Mac also against the app's signature.

**Why does the Mac app ask for Input Monitoring?**
macOS lets an app open a keyboard, or a mouse that also has keyboard keys, only with that permission, even to read its battery. BatteryTray asks for it only when such a device is connected, and only reads the battery, never what you type. Besides it, the Mac app asks only for Bluetooth: no Accessibility, no Screen Recording.

**Why does it ask for administrator rights on Windows?**
It doesn't, unless a third-party driver locks a receiver to administrators: for example the G-SPY driver (`vkbms.inf`) that comes with some cheap keyboards takes every HID collection with usage `FF00:0001`, the Razer dongle included. Then a line above the device list says so, and **Grant rights** installs the app properly, with one UAC prompt: it starts through a scheduled task with highest privileges (no prompt at each sign-in) and lives in Program Files, where its exe can't be swapped without admin rights.

**Does it work on Apple Silicon (M1–M4) and Intel Macs?**
Yes, the macOS app is one universal app for both, macOS 13 Ventura or later.

**How does it update?**
By itself, from this repository's releases on GitHub: it checks half a minute after start and then every 6 hours, verifies the download and swaps itself only while its windows are closed. Turn that off in Settings → General → Updates, and the app only tells you about a new version.

**Can I install it with winget or Scoop?**
Yes: `winget install qu1ncky.BatteryTray`, or `scoop bucket add batterytray https://github.com/qu1ncky/BatteryTray` and `scoop install batterytray`. For a full install, download `BatteryTray-…-win-x64.zip` from [Releases](https://github.com/qu1ncky/BatteryTray/releases/latest), unzip it and run `install.cmd`: it installs to `C:\Program Files\BatteryTray` and starts at sign-in through a Task Scheduler task named "BatteryTray".

**Where are the settings and the log? How do I uninstall it?**
Windows: `%LOCALAPPDATA%\BatteryTray` holds `settings.json`, `state.json` (last known levels) and `log.txt`. To uninstall, turn off **Start with Windows** in Settings → General, quit the app from its tray menu and delete the exe; after a full install, run `uninstall.cmd` from the zip. Mac: `~/Library/Application Support/BatteryTray`, and the login item is `~/Library/LaunchAgents/io.github.qu1ncky.BatteryTray.plist`; turn **Open at login** off, quit the app from its menu and move it to the Trash. If a hand edit breaks `settings.json`, the app keeps it as `settings.json.bak` and starts from the defaults.

## Something doesn't work

**My device isn't listed.**
New devices show up within seconds of being connected, and "Refresh battery levels" looks right away. If yours still isn't there, see [Device not listed?](devices.md#device-not-listed): a command for Windows and one for macOS that show its receiver's USB IDs and name; post their output in [Issues](https://github.com/qu1ncky/BatteryTray/issues) with the log.

**Where do I report a problem?**
In [Issues](https://github.com/qu1ncky/BatteryTray/issues), with the log: `%LOCALAPPDATA%\BatteryTray\log.txt` on Windows, `~/Library/Application Support/BatteryTray/log.txt` on a Mac.
