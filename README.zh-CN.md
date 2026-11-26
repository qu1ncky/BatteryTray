<div align="center">

<img src="docs/icon.png" alt="" width="88">

# BatteryTray

**无线耳机、鼠标、键盘和手柄的电量——<br>显示在 Windows 任务栏和 Mac 菜单栏中。**

[![最新版本](https://img.shields.io/github/v/release/qu1ncky/BatteryTray)](https://github.com/qu1ncky/BatteryTray/releases/latest)
[![下载量](https://img.shields.io/github/downloads/qu1ncky/BatteryTray/total)](https://github.com/qu1ncky/BatteryTray/releases)
[![许可证：MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![Windows 10 / 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6)
![macOS 13+](https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20Silicon%20%7C%20Intel-000000)

[English](README.md) · [Русский](README.ru.md) · **中文**

<a href="https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray.exe"><img src="docs/download-windows-zh.svg" alt="下载 Windows 版 BatteryTray" height="56"></a>&nbsp;&nbsp;<a href="https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray-macOS.dmg"><img src="docs/download-macos-zh.svg" alt="下载 macOS 版 BatteryTray" height="56"></a>

<sub>免费开源 · 无需账号和厂商软件 · <a href="#下载和安装">如何安装</a> · <a href="https://github.com/qu1ncky/BatteryTray/releases">更新说明</a></sub>

<img src="docs/readme/hero-zh.png" alt="Windows 和 Mac 上的 BatteryTray：Razer Barracuda X 耳机 37%，Attack Shark R3 鼠标正在充电，Rainy 75 键盘约 15%" width="720">

</div>

- **一个程序管理所有设备。** 通过 2.4 GHz 接收器读取 Razer、Logitech、SteelSeries、HyperX、Corsair、ASUS ROG 等数十个品牌的游戏鼠标、键盘和耳机，还支持 Xbox 和 DualSense 手柄、AirPods，以及系统能读到电量的任何蓝牙设备。[支持的设备](docs/zh/devices.md)
- **始终可见。** 显示在 Windows 时钟旁和 Mac 菜单栏中；设备电量不足时会用警示色显示。
- **不会错过的提醒**：在游戏或通话中途没电之前提醒你。勿扰模式不会隐藏它。
- **游戏中也能看：** 在“设置 → 悬浮窗”中启用后，按 <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>V</kbd>（Mac 上为 <kbd>⌃⌥V</kbd>），所有设备的电量会显示五秒。[全部功能](docs/zh/features.md)

<p align="center">
  <img src="docs/readme/menubar-zh.gif" alt="Mac 菜单栏：耳机、鼠标和键盘及其电量；键盘电量下降，先变橙色再变红色，然后重新充电" width="830"><br>
  <sub>Mac 菜单栏</sub>
</p>
<p align="center">
  <img src="docs/readme/taskbar-zh.gif" alt="Windows 任务栏：时钟旁图标左侧显示耳机、鼠标和键盘的电量；键盘电量下降，先变橙色再变红色，然后重新充电" width="830"><br>
  <sub>Windows 任务栏（设置 → 常规 → 任务栏）</sub>
</p>

<table>
<tr>
<td width="50%"><img src="docs/readme/reminder-zh.gif" alt="低电量提醒：AirPods Pro 18%，Razer Barracuda X 14%，以及“10 分钟后提醒”和“充电前不再提醒”按钮；底部的进度条走完后提醒自动关闭" width="100%"></td>
<td width="50%"><img src="docs/readme/overlay-zh.png" alt="游戏画面上的悬浮窗：耳机 37%，Attack Shark R3 鼠标正在充电，VXE R1 鼠标 82%；按 Ctrl+Alt+V 显示" width="100%"></td>
</tr>
<tr>
<td align="center"><sub>提醒：稍后提醒、充电前不再提醒，或让它自动关闭</sub></td>
<td align="center"><sub>悬浮窗：显示在游戏和全屏应用之上</sub></td>
</tr>
</table>

## 下载和安装

<table>
<tr>
<td width="50%" valign="top">

**Windows 10 和 11**

1. 下载 [`BatteryTray.exe`](https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray.exe) 并运行。
2. 如果 Windows 提示未知应用：点击 **更多信息** → **仍要运行**。
3. 电量会显示在时钟旁（Windows 11 可能把它收进 **^** 里，把它拖到任务栏上即可）。点击设备列表上方的 **启用**，即可开机自动启动。

<img src="docs/readme/install-windows-zh.png" alt="Microsoft Defender SmartScreen：先点击“更多信息”，再点击“仍要运行”" width="100%">

也可以用 `winget install qu1ncky.BatteryTray`，或 [Scoop](https://scoop.sh)：`scoop bucket add batterytray https://github.com/qu1ncky/BatteryTray`，然后 `scoop install batterytray`。

</td>
<td width="50%" valign="top">

**macOS 13 或更高版本**，Apple 芯片和 Intel

1. 打开 [`BatteryTray-macOS.dmg`](https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray-macOS.dmg)，把 **BatteryTray** 拖到 **应用程序**。
2. 打开它。第一次打开时 macOS 会阻止：前往 **系统设置 → 隐私与安全性 → 仍要打开**。
3. 允许蓝牙访问，电量就会显示在菜单栏中。点击设备列表上方的 **启用**，即可登录时打开。

<img src="docs/readme/install-macos-zh.png" alt="系统设置，隐私与安全性：已阻止“BatteryTray”以保护Mac，仍要打开" width="100%">

权限：蓝牙；只有连接键盘时才需要“输入监控”（macOS 要求所有打开键盘的应用都具备此权限；BatteryTray 只读取电量）。不需要其他权限。

</td>
</tr>
</table>

## 更多

[支持的设备](docs/zh/devices.md) · [全部功能](docs/zh/features.md) · [常见问题](docs/zh/faq.md) · [工作原理](docs/zh/how-it-works.md) · [从源码构建](docs/development.md) · [更新说明](https://github.com/qu1ncky/BatteryTray/releases)

没有找到你的设备，或遇到问题？请[提交 issue](https://github.com/qu1ncky/BatteryTray/issues) 并附上日志：Windows 上为 `%LOCALAPPDATA%\BatteryTray\log.txt`，Mac 上为 `~/Library/Application Support/BatteryTray/log.txt`。如需添加新设备，还请在设备连接时附上[这条命令](docs/zh/devices.md#设备不在列表中)的输出，它会显示设备接收器的 USB ID。

## 许可证

[MIT](LICENSE)。第三方代码、协议来源和 Slint 署名见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。本项目与 Apple、Razer、Logitech、SteelSeries、HyperX、Corsair、ASUS、Sony、Microsoft、Valve、ATTACK SHARK 及文中提到的其他厂商无关；产品名称是其各自所有者的商标。
