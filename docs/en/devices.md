# Supported devices

**English** · [Русский](../ru/devices.md) · [中文](../zh/devices.md)

[BatteryTray](../../README.md) shows the battery level of wireless headsets, mice, keyboards, game controllers, AirPods and other Bluetooth devices on Windows 10/11 and macOS 13+, next to the clock on Windows and in the Mac menu bar, and reminds you before a battery runs out. It reads gaming devices through their own 2.4 GHz receivers, with the protocol their makers' software uses, so Razer Synapse, Logitech G HUB, SteelSeries GG, HyperX NGENUITY, Corsair iCUE and ASUS Armoury Crate aren't needed.

Every device below works on both Windows and macOS (Apple Silicon and Intel), except the rows marked *Windows only* or *macOS*.

**Status**

- ✅ **Tested**: confirmed on the real device, by the developers or by a user.
- **Supported**: read with the device's own protocol, as its maker's software reads it (see [How it works](how-it-works.md)), so it is expected to work as the tested ones do. If yours doesn't, [open an issue](https://github.com/qu1ncky/BatteryTray/issues) and attach the log.

Contents: [Headsets](#headsets) · [Mice](#mice) · [Keyboards](#keyboards) · [Controllers](#controllers) · [Bluetooth, Apple, phones and more](#bluetooth-apple-phones-and-more) · [Notes on specific devices](#notes-on-specific-devices) · [Device not listed?](#device-not-listed)

## Headsets

| Device | Connection | Status |
|---|---|---|
| **Razer Barracuda X** (2022) | USB-C 2.4 GHz dongle `1532:0550` | ✅ Tested |
| Razer Barracuda X (2022) on the dongle `1532:0552` | USB-C 2.4 GHz dongle | Supported |
| **Razer** BlackShark V2 HyperSpeed, BlackShark V2 Pro (2020 and 2023), BlackShark V3, BlackShark V3 X HyperSpeed, Kraken V4 / V4 Pro, Kraken Kitty V3 Pro, Barracuda Pro | receiver or cable | Supported |
| **Logitech G** G533, G535, G633 / G635 / G733 / G933 / G935, G PRO / PRO X Wireless, G PRO X 2 LIGHTSPEED, G PRO X 3 LIGHTSPEED, G522 LIGHTSPEED, G930 | receiver | Supported |
| **ASTRO** A50 (Gen 4, Gen 5), A50 X, A20 X | base station, transmitter or cable | Supported |
| **SteelSeries Arctis Nova 5** | receiver `1038:2232` | ✅ Tested |
| **SteelSeries Arctis** 1, 7, 9, Pro Wireless, 7+, Nova 3P, Nova 7 (7X, 7P, Gen 2), Nova Pro Wireless, Nova Pro Omni, Nova Elite, GameBuds | receiver or base station | Supported |
| **HyperX Cloud III Wireless** | receiver | ✅ Tested |
| **HyperX** Cloud Alpha Wireless, Cloud Alpha 2 Wireless, Cloud II Wireless, Cloud II Core Wireless, Cloud III S Wireless, Cloud Flight, Cloud Flight S, Cloud Stinger 2 Wireless, Cloud Stinger Core Wireless (PlayStation and DTS receivers) | receiver | Supported |
| **Corsair HS80 RGB Wireless** | receiver | ✅ Tested |
| **Corsair** Void series, HS70, Void v2 / Virtuoso MAX / HS80 MAX Wireless, Virtuoso, Virtuoso XT / SE | receiver or cable | Supported |
| **ASUS ROG** Pelta, Delta II, Cetra True Wireless SpeedNova, Cetra Open Wireless, Strix Go 2.4 | 2.4 GHz dongle | Supported |
| **Audeze** Maxwell / Maxwell 2, AKG N9 Hybrid, Skullcandy PLYR / Crusher PLYR 720, **Alienware** AW920H, **EPOS** H3PRO Hybrid, Fractal Design Scape, MOONDROP GM01 Pro | dongle or cable | Supported |
| **JBL Quantum** 350 / 360 / 520 / 610 / 650 / 810 / 850 / 910 / 950 Wireless | dongle or cable | Supported |
| **Jabra** headsets on a Link 380 / 390 adapter, Evolve2 65 Flex and Evolve3 75 on the cable; **Poly** Voyager UC headsets on the BT600 adapter | USB adapter or cable | Supported |
| **Sony** INZONE H5, INZONE H9 II, INZONE Buds, PULSE 3D; **MCHOSE** X9, V9 Pro, V9 Pro 2; FIFINE HT6; VGN OMNISOUND; Lenovo Wireless VoIP | receiver | Supported |
| Bluetooth headphones and headsets | Bluetooth | see [Bluetooth](#bluetooth-apple-phones-and-more) |

## Mice

| Device | Connection | Status |
|---|---|---|
| **Razer** Viper Ultimate / V2 Pro / V3 Pro / V4 Pro / Mini SE, DeathAdder V2 Pro / V3 Pro / V3 HyperSpeed / V4 Pro, Basilisk Ultimate / V3 Pro / V3 Pro 35K / V4 Pro / V4 HyperSpeed / X HyperSpeed, Naga Pro / V2 Pro / V3 Pro, Cobra Pro / HyperSpeed, Pro Click (Mini, V2), Lancehead, Mamba, Orochi V2, Atheris and more, about 45 models | HyperSpeed receiver or cable | Supported |
| **Logitech** mice and other HID++ devices: MX Master, MX Anywhere, MX Ergo, G Lightspeed series, M series and more | Unifying, Bolt, Lightspeed or Nano receiver, or cable | Supported |
| **SteelSeries** Aerox 3 / 5 / 9 Wireless, Aerox 3 Wireless Gen 2, Prime Wireless, Prime Mini Wireless, Rival 3 Wireless (and Gen 2), Rival 650 Wireless | receiver or cable | Supported |
| **HyperX** Pulsefire Haste 2 Wireless / Core / Mini, Pulsefire Haste Wireless, Pulsefire Dart | receiver or cable | Supported |
| **Corsair** Dark Core RGB Pro / Pro SE, Scimitar Elite Wireless, Ironclaw, Harpoon, Sabre RGB Pro, M65 Ultra, M75 / M75 Air, M55, Nightsabre, Darkstar, Katar Pro Wireless | SLIPSTREAM or own receiver, or cable | Supported |
| **Corsair** Sabre v2 Pro Wireless (Ultralight, MG, CF) | receiver or cable | Supported |
| **ASUS ROG** Chakram / Chakram X, Gladius II / III Wireless (and AimPoint), Harpe Ace (Aim Lab, Extreme, Mini), Harpe II Ace, Keris Wireless (and AimPoint), Keris II Ace / Origin, Pugio II, Spatha X, Strix Impact II / III Wireless, Strix Carry; TUF Gaming M4 Wireless, TX Gaming Mini and others | own receiver, ROG Omni Receiver, SpeedNova 8K receiver or cable | Supported |
| **Attack Shark R3** | 2.4 GHz receiver `3151:402D` | ✅ Tested |
| **AJAZZ AJ179 Pro** | 2.4 GHz receiver `3151:402D` | ✅ Tested |
| Other mice on the Rongyuan platform: **Attack Shark**, Akko, MonsGeek and others, 88 receiver models | 2.4 GHz receiver | Supported |
| **Attack Shark V3 Pro** | 2.4 GHz receiver `1D57:FA60`, charging dock | ✅ Tested |
| Other mice on the Xenta receivers: **Attack Shark** X11, X11 SE, X11 Pro, X3, X3 Max, X3 Pro, X1, X6 / R1, V3, V6, X8 Pro, X8 SE, X8 Plus, G3 Pro; **FURYCUBE** F1, F1 Pro, F3, G3, G9, G9 PRO, G11, G13S; **Delux** M600 Pro / M900 Pro / M900 Mini; **Inphic** IN6 / IN6 SE, IN9 / IN9 Ultra / In9 Pro, IN10 / IN10 MAX, INX; **MAMBASNAKE** M3 | 2.4 GHz receiver `1D57:FA60` or `1D57:FA65`, or cable | Supported |
| **VXE / VGN** R1 PRO / PRO MAX, R1 SE / SE+, Dragonfly R1, Dragonfly F2 | 2.4 GHz receiver or cable | Supported |
| **ATK A9 Ultra Max 2.0** | 2.4 GHz receiver `373B:11FE` | Supported |
| Other **ATK** mice on its 1K, 8K and NearLink receivers (X1, A9, F1, U2 series and others), ATK ZERO, VXE R1 NK | 2.4 GHz receiver; X1 Pro Max, A9 Plus, A9 Ultimate, A9 Mini+, F1 Ultimate 2.0 also on the cable | Supported |
| Mice with CompX's protocol: **Lamzu** Atlantis / Thorn, **Scyrox** V8 / V8S / V6, **Pulsar** X2 V2 and the CrazyLight 8K dongle, **G-Wolves** HTX Ultra and its 8K receiver, Kysona M600, Hitscan Hyperlight, Cougar Revenger Pro, Dareu A950 Wing / EM901X, HECATE G5M PRO, CHERRY XTRFY, GravaStar, Redragon M916 Pro / M991, **Endgame Gear** XM2we / OP1we | 2.4 GHz receiver or cable | Supported |
| Mice on the **Areson** receivers `25A7:FA7C` (Redragon M693, Fantech Helios Go XD5 and others under their own names) and `25A7:FA07` / `FA08` (Redragon M915RGB-WL, M913) | 2.4 GHz receiver or cable | Supported |
| **Ardor Renegade** | 2.4 GHz receiver `25A7:FA4A` | ✅ Tested |
| **AJAZZ T520 Max** | 2.4 GHz receiver `A8A5:2266` or cable `A8A4:2266` | ✅ Tested |
| Mice on the receiver `A8A5:2255`: **AJAZZ** AJ139 V2 MC, K-Snake X11, Noir M2-NEX, MCHOSE G7 Pro; **MCHOSE G3 V2** (`3837:4245`) | 2.4 GHz receiver or cable | Supported |
| **Glorious** Model O / O- / D / D- Wireless, Model O PRO, Series One PRO, Model O 2 PRO / D 2 PRO (and the 4K/8KHz Editions), Series 2 PRO | 2.4 GHz receiver or cable | Supported |
| **LAMZU** mice of the Aurora generation: Maya X, Maya, Inca, Tachi, Paro, Thorn, Atlantis OG Champion, Atlantis Mini, Orcus and others | 2.4 GHz receiver (1K, 8K) or cable | Supported |
| **WLmouse** Beast X (Pro, Mini, Max, V2), Beast G, Beast Miao, Huan, Strider, Ying, Sword X; **G-Wolves** Fenrir, HTX, HTS, HSK, HTR, HT-S2 series; Arbiter Studio Akitsu; Attack Shark R5 Ultra / R6 / R8; CRDRAKO KO-ONE; LunaFury LUNA33 / TYPE33 | 2.4 GHz receiver or cable | Supported |
| **MCHOSE** A5 Pro / Pro Max, L7, M7, A7, K7 and their V2 / V3 / Pro / Ultra versions, A5 V2 / V3, K5, R7 Ultra, V7, G3 V3 | 2.4 GHz receiver or cable | Supported |
| **Keychron** M1 – M7 on the Link receiver, the 4K mice on the 4K Link, the 8K mice (M5 8K, M6 8K and others) on the Ultra-Link 8K, Nape Pro | 2.4 GHz receiver or cable | Supported |
| **Pulsar** X2 V3, X2 V3 eS, and the Xlite V4 / X3 on the same dongles | 2.4 GHz dongle or cable | Supported |
| **VAXEE / ZYGEN** XE, XE-S, XE V2, AX, E1, NP-01, NP-01S; **Ninjutso** Sora V2, Ten / Ten Air; Sprime PM1; **Endgame Gear** OP1w 4k / XM2w 4k; **Cooler Master** MM712 / MM731; **ZOWIE** EC1-CW / EC2-CW / EC3-CW; ROCCAT Kone Pro Air; **Finalmouse** UltralightX | 2.4 GHz receiver or cable | Supported |
| **Rapoo** VT gaming mice (VT0 – VT9 lines, about 70 models); **Incott** G23, G23 V2, G24, Ghero, Zero 29 / 39 on the 8K receiver | 2.4 GHz receiver or cable | Supported |
| Machenike L7, Darmoshark M3 / M3S / M3S Pro, MSI VERSA 300 ELITE WIRELESS and CLUTCH GM31 LIGHTWEIGHT WIRELESS, AJAZZ AJ179, AULA SC580 and X9 Ultra / V9 Max+, Akko V9 Dash Pro / Dash V9 Ultra, Delux M800 Pro, DAREU A980 Pro, Royal Kludge MG6 Pro, Niceboy MW400, mice on Sinowealth's receiver `258A:002F` (Redragon M602-KS and others) | 2.4 GHz receiver or cable | Supported |

## Keyboards

| Device | Connection | Status |
|---|---|---|
| **Attack Shark X82 PRO HE** | 2.4 GHz receiver `3151:5026` | ✅ Tested |
| **Razer** BlackWidow V3 Pro / V3 Mini / V4 Mini / V4 TKL HyperSpeed, BlackWidow V4 Low-Profile (and TKL) HyperSpeed, DeathStalker V2 Pro (and TKL), Pro Type Ultra / Ergo, Joro | receiver or cable | Supported |
| **Logitech** MX Keys, K series, G Lightspeed keyboards and other HID++ keyboards | Unifying, Bolt or Lightspeed receiver, or cable | Supported |
| **Corsair** K57, K65 Plus, K70 Core TKL, K70 Pro Mini, K100 Air, MAKR 75 | SLIPSTREAM or own receiver, or cable | Supported |
| **ASUS ROG** Azoth, Azoth Extreme, Azoth X, Falchion, Falchion RX Low Profile, Strix Scope II 96 Wireless, Strix Morph 96 Wireless | own receiver, ROG Omni Receiver or cable | Supported |
| Other keyboards on the Rongyuan platform: **Attack Shark**, Akko, MonsGeek and others | 2.4 GHz receiver | Supported |
| Keyboards on CompX's "BY" receivers: **MCHOSE** (K99 V2, G75 Pro, X75, G87, K87 and others), **AULA** (F75, F75 Max, F87 Pro, F99), **VGN** (V98 Pro V3, V87 V2, N75 V2, V108), Redragon Horus Pro | 2.4 GHz receiver (`3554:FA09`, `3554:FB40`, `41E4:2001`, `258A:024F` and others) | Supported |
| Keyboards on SONiX dongles: **Attack Shark X85**, EWEADN X87, **AJAZZ** AK820 Pro / MAX / Max Plus, EPOMAKER EA75, **AULA** F87 Pro V2 / F99 MAX / F108 Pro and others | 2.4 GHz receiver (`0C45:FEFE`, `0C45:FDFD`, `0C45:FEFC`, `05AC:024F`) | Supported |
| **CHERRY** MX 3.0S Wireless, MX 2.0S Wireless / WL Pro, PIXIU 75; **ATK** V75X, Z87; **VGN / VXE** S99, V98 Pro, N75 Pro, VXE75, V87 on the receiver `320F:5088`; AJAZZ AK992; Attack Shark X98; URX Core75; MCHOSE K87 V3; **Alienware** Pro Wireless Gaming Keyboard | 2.4 GHz receiver or cable | Supported |
| **Apple Magic Keyboard** (2015, 2021, USB-C; with Touch ID and with Numeric Keypad), *Windows only* (macOS shows it itself) | Bluetooth | Supported |

## Controllers

| Device | Connection | Status |
|---|---|---|
| **Xbox Wireless Controller** over Bluetooth (the level Windows shows) | Bluetooth | ✅ Tested |
| **Xbox controllers** on the Xbox Wireless Adapter, *Windows only* (macOS has no driver for the adapter) | adapter | Supported |
| **Xbox controllers** on Bluetooth Classic, as their original firmware connects them: Xbox One S (`045E:02E0`, `02FD`), Elite Series 2 (`0B05`), *Windows only* | Bluetooth | Supported |
| **Sony DualSense**, DualSense Edge, DualShock 4 | cable, Sony adapter; Bluetooth once a game or Steam has switched the controller to full reports | Supported |
| **Nintendo Switch** Pro Controller, Joy-Con, *Windows only* (macOS shows them itself) | Bluetooth, once a game or Steam has switched the controller to full reports | Supported |
| **Steam Controller** (2026) on its Puck, Steam Controller (2015) on its wireless adapter | receiver | Supported |
| **8BitDo** Ultimate 2 Wireless, **Flydigi** Vader 5 Pro / Apex 5 / Apex 6, **GameSir** Nova 2 / Nova 2 Lite / Super Nova / Tarantula Pro, BEITONG KP40D | 2.4 GHz receiver | Supported |
| NVIDIA SHIELD Controller (2017) | cable | Supported |
| Game controllers macOS knows: Xbox, Switch Pro, MFi and others, *macOS* | Bluetooth | Supported |

## Bluetooth, Apple, phones and more

| Device | Connection | Status |
|---|---|---|
| **Any Bluetooth device whose level Windows or macOS shows**: AirPods with each earbud and the case, Magic Mouse, Magic Keyboard, Magic Trackpad, Bluetooth headphones and headsets, many mice and keyboards | Bluetooth | ✅ Tested |
| **AirPods and Beats** with each earbud and the case, on Windows | Bluetooth | Supported |
| **Bluetooth LE devices** with the standard Battery Service: many mice and keyboards (Logitech MX, keyboards with ZMK and both halves of a split one), trackers | Bluetooth LE | Supported |
| **Logitech MX** mice and keyboards connected directly over Bluetooth, *macOS* (Windows shows them itself) | Bluetooth LE | Supported |
| **iPhone and iPad**, *macOS* | USB cable; the device must trust the Mac | ✅ Tested |
| **Android phones and tablets**, *Windows only* | USB cable in "File transfer" mode | Supported |
| **UPS** that Windows shows as a battery, *Windows only* | USB | Supported |
| Any USB device that reports the standard HID "Battery Strength" | receiver or cable | Supported |

## Notes on specific devices

- **Razer Barracuda X** sends its own percentage through the dongle whenever it changes by 1%. Until it has sent one (right after the app starts), the level shows as "—", "level not known yet". While charging there is no percentage at all — the headset's number only creeps towards 100% at a fixed pace — so you see that it's charging, when it's almost full and when it's done.
- **Attack Shark / Rongyuan mice** on their cable leave the receiver and report no level over the cable (the vendor's own driver doesn't show one either), so they show as charging, and the receiver reports the real level once the cable is out.
- **Attack Shark X82 PRO HE** keyboard stays on its receiver while it charges on the cable, but reports a level 20–30% too high then, and the real one again once the cable is out. Most likely the keyboard estimates its level from the battery voltage, which the charger raises. Nothing tells that the keyboard is on the cable: the receiver's reply stays the same, no USB device appears, and the vendor's own driver shows the same inflated level. So the number is shown as it comes ([#5](https://github.com/qu1ncky/BatteryTray/issues/5)).
- **AJAZZ T520 Max** reports 100% while it charges, before its battery is full, so while charging it shows the charging sign without a number.
- **Mice on the Xenta receiver** (`1D57:FA60`: Attack Shark, FURYCUBE, Delux, Inphic, MAMBASNAKE) send their level by themselves every 2 seconds; BatteryTray only listens and sends them nothing. On the cable or the charging dock the number they send stays frozen, so you see a charging sign instead, and the real level is back a few seconds after the mouse leaves the charger. The model is told by a code in these messages: a mouse whose code isn't known yet is listed without a level and its messages go to the log, so please post the `Xenta` lines from `%LOCALAPPDATA%\BatteryTray\log.txt` in [Issues](https://github.com/qu1ncky/BatteryTray/issues) to get it added.
- **Levels in steps.** Some devices report only a few levels, so their percentage moves in steps: the older ASUS ROG mice by 25%, mice on the Areson receivers `FA07` / `FA08` and the Ardor Renegade by 10%, VAXEE mice by 5%, Flydigi controllers in steps of their own; Xbox controllers know 4 levels (on the Xbox Wireless Adapter and on Bluetooth Classic) and Switch controllers 5. The Finalmouse level is an estimate, shown with "≈", as is any level estimated from a battery's voltage.
- **Keyboards on SONiX dongles** don't say whether they are charging, so they show no charging sign.
- **Receivers that several brands share.** Many cheap 2.4 GHz receivers carry the same USB IDs under unrelated products. A device on such IDs is listed once it has answered for the first time, so that nobody gets a keyboard they don't own sitting in the list as "asleep"; until then it is asked less and less often. If yours is in the table and doesn't appear, wake it up and give it a minute, and if it still doesn't, post `log.txt` in [Issues](https://github.com/qu1ncky/BatteryTray/issues).
- **Keyboards** are asked every 5 minutes at most (some every 35), because a request can keep a keyboard from falling asleep.
- **Razer BlackShark V3** is asked once every five minutes: asked more often, the headset drops its 2.4 GHz link. So its level can be a few minutes old.
- **ASUS ROG Omni Receiver**: a mouse and a keyboard on one receiver are listed separately.
- **ASTRO A50** base stations must have their switch on PC, and can't be read while ASTRO Command Center is open.
- **Sony PULSE 3D** and **Incott** mice only report when something changes, so they appear a little after they are switched on or moved.
- **Xbox controllers on Bluetooth Classic** appear once the controller has sent its battery report.
- **Bluetooth** devices are listed only while connected: Windows keeps a stale percentage after they disconnect. On Windows a Bluetooth LE device's level is read fresh from the device. On a Mac a Bluetooth device appears once it is connected and macOS has its level; Bluetooth LE devices macOS doesn't show itself need the Bluetooth permission.
- **iPhone and iPad** (Mac) are read on the cable only, and only once they trust this Mac — BatteryTray never asks to pair, so if yours isn't listed, connect it once in Finder and tap "Trust".
- **On a Mac, keyboards and mice with keyboard keys** can only be read with the **Input Monitoring** permission (macOS asks it of every app that opens such a device). Such a device says "needs the Input Monitoring permission" until you allow it: click "Allow" on the banner above the list, or turn BatteryTray on in System Settings → Privacy & Security → Input Monitoring. BatteryTray reads only the battery level, never what you type.

## Device not listed?

New devices show up within seconds of being connected; "Refresh battery levels" looks right away. If yours doesn't appear, [open an issue](https://github.com/qu1ncky/BatteryTray/issues) with what the command below prints while the device is connected (its receiver's USB IDs and name), and the log.

**Windows**, in PowerShell:

```powershell
Get-PnpDevice -PresentOnly | ? InstanceId -match '^USB\\VID_\w{4}&PID_\w{4}\\' | % { "{0}  {1}" -f $_.InstanceId.Substring(4,17), (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_BusReportedDeviceDesc).Data }
```

The log is `%LOCALAPPDATA%\BatteryTray\log.txt`. A diagnostic poll helps too: `"C:\Program Files\BatteryTray\BatteryTray.exe" --dump "%USERPROFILE%\Desktop\battery.txt"`.

**macOS**, in Terminal:

```sh
ioreg -p IOUSB -l -w0 | awk -F' = ' '/"idVendor"/{v=$2} /"idProduct"/{p=$2} /"USB Product Name"|"kUSBProductString"/{if(n=="")n=$2} /^[ |]*}[ ]*$/{if(v!=""){gsub(/"/,"",n);printf "%04X:%04X  %s\n",v,p,n}; v=p=n=""}'
```

The log is `~/Library/Application Support/BatteryTray/log.txt`. A diagnostic poll helps too: `/Applications/BatteryTray.app/Contents/MacOS/BatteryTray --dump ~/Desktop/battery.txt`.

How a new device gets added: [How it works](how-it-works.md#adding-a-device).
