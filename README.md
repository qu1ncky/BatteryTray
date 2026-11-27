<div align="center">

<img src="docs/icon.png" alt="" width="88">

# BatteryTray

**The battery level of your wireless headset, mouse, keyboard and controller —<br>on the Windows taskbar and in the Mac menu bar.**

[![Latest release](https://img.shields.io/github/v/release/qu1ncky/BatteryTray)](https://github.com/qu1ncky/BatteryTray/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/qu1ncky/BatteryTray/total)](https://github.com/qu1ncky/BatteryTray/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![Windows 10 / 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6)
![macOS 13+](https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20Silicon%20%7C%20Intel-000000)

**English** · [Русский](README.ru.md) · [中文](README.zh-CN.md)

<a href="https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray.exe"><img src="docs/download-windows-en.svg" alt="Download BatteryTray for Windows" height="56"></a>&nbsp;&nbsp;<a href="https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray-macOS.dmg"><img src="docs/download-macos-en.svg" alt="Download BatteryTray for macOS" height="56"></a>

<sub>Free and open source · no account, no vendor software · <a href="#download-and-install">how to install</a> · <a href="https://github.com/qu1ncky/BatteryTray/releases">release notes</a></sub>

<img src="docs/readme/hero-en.png" alt="BatteryTray on Windows and on a Mac: a Razer Barracuda X headset at 37%, an Attack Shark R3 mouse charging and a Rainy 75 keyboard at about 15%" width="720">

</div>

- **One app for all your devices.** Gaming mice, keyboards and headsets from Razer, Logitech, SteelSeries, HyperX, Corsair, ASUS ROG and dozens of other brands through their 2.4 GHz receivers, plus Xbox and DualSense controllers, AirPods and any Bluetooth device whose level the system knows. [Supported devices](docs/en/devices.md)
- **Always in sight.** Next to the clock on Windows and in the Mac menu bar, in a warning color when a device runs low.
- **A reminder you won't miss**, before a battery dies mid-game or mid-call. Do Not Disturb doesn't hide it.
- **Over games:** turn on the overlay in Settings → Overlay, and <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>V</kbd> (<kbd>⌃⌥V</kbd> on a Mac) shows every device's level for five seconds. [All features](docs/en/features.md)

<p align="center">
  <img src="docs/readme/menubar-en.gif" alt="The Mac menu bar: a headset, a mouse and a keyboard with their levels; the keyboard runs down, turns orange, then red, and charges back" width="830"><br>
  <sub>The Mac menu bar</sub>
</p>
<p align="center">
  <img src="docs/readme/taskbar-en.gif" alt="The Windows taskbar: a headset, a mouse and a keyboard with their levels left of the tray icons; the keyboard runs down, turns orange, then red, and charges back" width="830"><br>
  <sub>The Windows taskbar (Settings → General → Taskbar)</sub>
</p>

<table>
<tr>
<td width="50%"><img src="docs/readme/reminder-en.png" alt="The low battery reminder for AirPods Pro at 18% and a Razer Barracuda X at 14%, with Remind me in 10 min and Don't remind me until it's charged; the line along its bottom edge shows the time until it closes by itself" width="100%"></td>
<td width="50%"><img src="docs/readme/overlay-en.png" alt="The overlay over a game: a headset at 37%, an Attack Shark R3 mouse charging and a VXE R1 mouse at 82%, shown by Ctrl+Alt+V" width="100%"></td>
</tr>
<tr>
<td align="center"><sub>The reminder: snooze it, mute it until the device is charged, or let it close by itself</sub></td>
<td align="center"><sub>The overlay, over games and full-screen apps</sub></td>
</tr>
</table>

## Download and install

<table>
<tr>
<td width="50%" valign="top">

**Windows 10 and 11**

1. Download [`BatteryTray.exe`](https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray.exe) and run it.
2. If Windows warns about an unknown app: **More info** → **Run anyway**.
3. The level appears next to the clock (Windows 11 may put it under **^**: drag it out). **Enable** above the device list starts it with Windows.

<img src="docs/readme/install-windows-en.png" alt="Microsoft Defender SmartScreen: click More info, then Run anyway" width="100%">

Or `winget install qu1ncky.BatteryTray`, or [Scoop](https://scoop.sh): `scoop bucket add batterytray https://github.com/qu1ncky/BatteryTray`, then `scoop install batterytray`.

</td>
<td width="50%" valign="top">

**macOS 13 or later**, Apple Silicon and Intel

1. Open [`BatteryTray-macOS.dmg`](https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray-macOS.dmg) and drag **BatteryTray** to **Applications**.
2. Open it. The first time macOS blocks it: **System Settings → Privacy & Security → Open Anyway**.
3. Click **Allow** for Bluetooth. The levels appear in the menu bar. **Enable** above the device list opens it at login.

<img src="docs/readme/install-macos-en.png" alt="System Settings, Privacy & Security: “BatteryTray” was blocked to protect your Mac, Open Anyway" width="100%">

Permissions: Bluetooth, and Input Monitoring only when a keyboard is connected (macOS asks it of every app that opens one; BatteryTray reads only its battery). Nothing else.

</td>
</tr>
</table>

## More

[Supported devices](docs/en/devices.md) · [All features](docs/en/features.md) · [FAQ](docs/en/faq.md) · [How it works](docs/en/how-it-works.md) · [Build from source](docs/development.md) · [Release notes](https://github.com/qu1ncky/BatteryTray/releases)

A device is missing or something is wrong? [Open an issue](https://github.com/qu1ncky/BatteryTray/issues) and attach the log: `%LOCALAPPDATA%\BatteryTray\log.txt` on Windows, `~/Library/Application Support/BatteryTray/log.txt` on a Mac. To get a new device added, also attach what [this command](docs/en/devices.md#device-not-listed) prints while it is connected: its receiver's USB IDs.

## License

[MIT](LICENSE). Third-party code, protocol sources and the Slint attribution: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Not affiliated with Apple, Razer, Logitech, SteelSeries, HyperX, Corsair, ASUS, Sony, Microsoft, Valve, ATTACK SHARK or any other maker named here; product names are trademarks of their owners.
