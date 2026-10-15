# Third-party notices

## razer-barracuda-x-battery

The Razer Barracuda X protocol code (`Providers/RazerBarracudaProvider.cs`), the HID helpers (`Hid/Hid.cs`) and the tray icon renderer (`UI/TrayIcon.cs`) are adapted from [razer-barracuda-x-battery](https://github.com/mehdibouchami/razer-barracuda-x-battery) by Mehdi Bouchami, used under the MIT License:

```
MIT License

Copyright (c) 2026 Mehdi Bouchami

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## rivalcfg

The SteelSeries battery commands (`Providers/SteelSeriesProvider.cs`) follow [rivalcfg](https://github.com/flozz/rivalcfg) by Fabien Loison, licensed under the WTFPL.

## ATTACK SHARK web driver

The Rongyuan receiver status command and the list of receiver IDs (`Providers/RongyuanDongleProvider.cs`) were found by reading the official ATTACK SHARK web driver (qmk.top) for interoperability; no code from it is included.

## Protocol references (facts only, no code copied)

The following projects document the device protocols that the providers below implement independently. Only protocol facts (command bytes, report layouts, device IDs) were taken from them; none of their code is included, so their licenses don't apply to this project's code.

- `Providers/RazerPeripheralProvider.cs` — report layout, commands, per-device transaction IDs and waits from [OpenRazer](https://github.com/openrazer/openrazer) (GPL-2.0).
- `Providers/LogitechProvider.cs` — HID++ requests, battery features and voltage curve from [Solaar](https://github.com/pwr-Solaar/Solaar) (GPL-2.0), the Linux driver `drivers/hid/hid-logitech-hidpp.c` (GPL-2.0) and [LGSTrayBattery](https://github.com/andyvorld/LGSTrayBattery); the 2012 Logitech HID++ 2.0 draft specification.
- `Providers/HeadsetProvider.cs` — headset requests and responses from [HeadsetControl](https://github.com/Sapd/HeadsetControl) (GPL-3.0).
- `Providers/PlayStationProvider.cs` — DualSense / DualShock 4 battery bytes from the Linux kernel driver `drivers/hid/hid-playstation.c` (GPL-2.0).
- `Providers/XInputProvider.cs` — Bluetooth product IDs of Xbox controllers as listed by [SDL](https://github.com/libsdl-org/SDL) (zlib).
