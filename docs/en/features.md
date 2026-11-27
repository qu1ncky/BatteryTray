# BatteryTray features

**English** · [Русский](../ru/features.md) · [中文](../zh/features.md)

[BatteryTray](../../README.md) is a free, open-source app for Windows 10/11 and macOS 13+ that shows the battery level of wireless headsets, mice, keyboards, game controllers, AirPods and other Bluetooth devices next to the clock and reminds you before a battery runs out. One small app for all your devices instead of Razer Synapse, Logitech G HUB, SteelSeries GG, HyperX NGENUITY, Corsair iCUE or ASUS Armoury Crate: no account, no vendor software, it only reads the battery. Which devices: [devices.md](devices.md).

## The level always in sight

- **Windows tray:** a number for the device that most needs charging: green, yellow below its reminder level, red below its critical level, blue while charging. Hover it to see every device. Windows 11 hides new tray icons under the **^** arrow: drag the icon onto the taskbar to keep it in sight.
- **Windows taskbar:** turn on **Settings → General → Taskbar** and the first devices of your list (three by default, 1 to 5) sit on the taskbar left of the tray icons, each as its icon and level, in orange or red only when it runs low. A device running low always gets a place. Click it for the window, right-click for the tray menu.
- **Mac menu bar:** the first devices of your list, three by default (**Settings → General → Menu bar**: 1 to 5), each as its icon and level, monochrome until a device runs low, then orange or red. A device running low that didn't fit takes the last place. Click it for every device's level and the commands.

## Every device in one window

- Connected devices as cards with their percentage and charging state; the ones that are off with their last known level and when it was seen.
- Each device's **⋯** menu renames it, pins it to the top, hides it under "Hidden" (an eye button brings it back) or gives it reminder levels of its own.
- Drag a card to change the order; the menu bar, the taskbar and the overlay follow it.
- New devices show up by themselves within seconds of being connected; "Refresh battery levels" looks right away.

## Reminders

- **Its own window, not a toast:** always on top in the corner of the screen, so Do Not Disturb doesn't hide it, and it never takes the focus from a game.
- **"Remind me in N min"** or **"Don't remind me until it's charged"**. Left unanswered, it closes after 10 seconds while you're at the computer (a line along its bottom edge shows the time left; 1 to 15 seconds in **Settings → Reminders**, or it stays until you answer) and comes back after the repeat interval, 10 minutes by default. While you're away from the computer it waits for you.
- **Two levels:** the reminder level (20% by default) and the critical level (10%), where it comes back even if snoozed. Each device can have levels of its own, set in its **⋯** menu: the headset that drains fastest can remind you at 40% while the mouse waits for 15%.
- **A sound for each reminder** (**Settings → Reminders → Sounds**): the low battery reminder, the critical one and "Charging complete" each play the system's sound, another of the system's sounds, your own WAV or MP3 file (up to 10 MB, the first 10 seconds) or nothing.
- **"Charging complete"** when a device that can tell finishes charging with the cable still in.

## Overlay over games

Turned on in **Settings → Overlay**, a shortcut (**Ctrl+Alt+V** on Windows and **⌃⌥V** on a Mac, or one of your own) shows the connected devices for five seconds on the screen you're working on, over games and full-screen apps, without taking the focus or the mouse. Press it again to restart the timer, or choose to show it until pressed again. Pick a corner of the screen and preview it before turning it on. Devices of the same kind are shown with their names.

## Widgets on the Mac desktop

On macOS 14 or later: right-click the desktop → **Edit Widgets…** → BatteryTray.

- **Charge Next** (small) shows the device that needs charging most, large.
- **Rings** (small and medium: four devices, large: six) and **List** (large, six) show the first devices of your list, and a device running low always gets a place. A device that isn't connected shows its last level, faint, and how long ago it was seen.
- Each also comes as **· Color**, every level in its color; the others color a level only when it runs low, as the menu bar does.
- When BatteryTray quits they say so, and a click opens it.

## Honest numbers

When a device can't report its level while charging, you see a charging sign instead of a stale or made-up number. Levels estimated from a battery's voltage are marked with "≈".

## Light and dark, three languages

- Follows the system's theme, or pick light or dark in **Settings → General**. The overlay stays dark over games; on Windows the tray number and its menu follow the taskbar.
- English, Russian and Chinese. The app follows the system's language and switches at once in **Settings → General → Language**. Translations live in [`src/loc.rs`](../../src/loc.rs); corrections and new languages are welcome.

## Updates

Half a minute after start and then every 6 hours the app asks the project's update server which [release](https://github.com/qu1ncky/BatteryTray/releases/latest) is the latest; when the server doesn't answer, and once a day anyway, it asks GitHub itself. With automatic updates (the default) it downloads the new version from GitHub, checks its SHA-256 against the digest GitHub publishes and the version inside it, and swaps itself only while its windows are closed. Without them, it tells you once and the **Update** button installs it. Everything is in **Settings → General → Updates**: status, what's new, "Check now" and the automatic updates switch. On a Mac the new app must also be signed by the same certificate as the running one.

The update check is also how we know how many people use BatteryTray. It carries the app's version, the system with its version, the type of processor and a random number the app made up on its first check, so that each copy is counted once. The server keeps these and the country the check came from, never the address; nothing about your devices, your settings or how you use the app is sent. Its code is in [`server/`](../../server).

## Small and private

A single ~10 MB exe on Windows, an ~11 MB app on a Mac, about 8 MB of memory. No account, no ads; the update check is all it sends ([Updates](#updates)), and it never changes anything on your devices: it only asks them for their battery.

## Windows and macOS

The same app on both: the device list, the reminders, the overlay and every supported device come from one shared core, and each system gets the windows its users expect.

| | Windows | macOS |
|---|---|---|
| The level | a number in the tray; optionally up to five devices on the taskbar | up to five devices in the menu bar, with a menu of every device; desktop widgets (macOS 14 or later) |
| Settings | in the main window (gear) | their own window, **Settings… (⌘,)**: General, Reminders, Overlay |
| Low battery reminder | always-on-top window, bottom right | floating panel, top right |
| "Charging complete", "a new version" | Windows notifications | macOS notifications |
| Starts at sign-in | **Enable** on the banner above the list (the Startup setting), or the Task Scheduler task of a full install | **Enable** on the banner: **Open at login** |
| Bluetooth devices | the level Windows shows in Settings → Bluetooth & devices, read fresh from a Bluetooth LE device; AirPods with each earbud and the case; Magic Keyboard; Xbox and Switch controllers | the levels macOS knows: AirPods with each earbud and the case, Magic devices, headsets, controllers; any Bluetooth LE device with a Battery Service; Logitech MX over Bluetooth |
| Phones, tablets, UPS | Android on a cable (File transfer mode); a UPS on USB | iPhone and iPad on a cable |
| Xbox Wireless Adapter | yes | no: macOS has no driver for it |
| Permissions | admin rights only when a third-party driver locks a receiver ("Grant rights" above the list) | Bluetooth; Input Monitoring only for keyboards |
| Processor | x64 | one app for Apple Silicon and Intel |

More: [FAQ](faq.md) · [How it works](how-it-works.md) · [Download](../../README.md#download-and-install)
