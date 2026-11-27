# BatteryTray 常见问题

[English](../en/faq.md) · [Русский](../ru/faq.md) · **中文**

[BatteryTray](../../README.zh-CN.md) 是一款适用于 Windows 10/11 和 macOS 13+ 的免费开源程序，在时钟旁显示无线耳机、鼠标、键盘、手柄、AirPods 和其他蓝牙设备的电量，并在电池耗尽前提醒你。[下载](../../README.zh-CN.md#下载和安装) · [支持的设备](devices.md) · [功能](features.md)

## 查看电量

**有没有能在任务栏或菜单栏显示无线鼠标、键盘或耳机电量的程序？**
有：BatteryTray，免费开源，适用于 Windows 10/11 和 macOS 13+。它在 Windows 时钟旁显示最需要充电的设备的电量（也可以在任务栏上显示多个设备），在 Mac 菜单栏中显示你的设备的电量，在一个窗口里列出所有设备，并在电池耗尽前提醒你。它通过接收器读取游戏鼠标、键盘和耳机（Razer、Logitech、SteelSeries、HyperX、Corsair、ASUS、Attack Shark 等众多品牌），也能读取蓝牙设备，无需厂商软件。

**如何在 Windows 11 上不打开“设置”就看到蓝牙设备的电量？**
BatteryTray 会把它显示在时钟旁：Windows 知道电量的所有蓝牙设备都会显示，低功耗蓝牙设备的电量直接从设备读取最新值，AirPods 的左右耳机和充电盒分别显示。

**如何在 Windows 上查看 AirPods 的电量？**
BatteryTray 像 iPhone 一样，从 AirPods 自己发出的蓝牙消息中读取左右耳机和充电盒的电量，并显示在时钟旁。

**能显示蓝牙耳机的电量吗？**
能，前提是系统本身知道它的电量：在 Windows 上，“设置 → 蓝牙和其他设备”中能看到百分比；在 Mac 上，蓝牙菜单中能看到百分比。

**有没有能在 Mac 菜单栏显示所有设备电量的程序？**
有：macOS 版 BatteryTray 可以显示 AirPods（左右耳机和充电盒分别显示）、妙控鼠标、妙控键盘、妙控板、蓝牙耳机和鼠标、手柄、通过数据线连接的 iPhone 或 iPad，以及通过接收器连接的游戏鼠标、键盘和耳机，显示在菜单栏和一个窗口中。

**玩游戏时怎么看耳机电量？**
在“设置 → 悬浮窗”中启用悬浮窗，然后在游戏中按 Ctrl+Alt+V（Mac 上为 ⌃⌥V）：所有已连接设备的电量会显示五秒，不会抢走焦点。快捷键也可以在那里修改。

**能在 Mac 上显示 iPhone 或 iPad 的电量吗？**
能，只要它通过数据线连接并信任这台 Mac（第一次连接时访达会询问）：电量和是否在充电会显示在列表和菜单栏中。

**能在 Windows 上显示 Android 手机的电量吗？**
能，只要它通过数据线连接并处于“文件传输”模式（在“仅充电”模式下 Windows 不会把它识别为设备）。

## 无需厂商软件

**如何在电脑上不装 Razer Synapse 查看 Razer Barracuda X 的电量？**
安装 BatteryTray：它通过耳机的 USB-C 2.4 GHz 接收器读取电量，不需要蓝牙，也不需要 Synapse。

**如何不用 G HUB 或 Options+ 查看 Logitech 鼠标或键盘的电量？**
BatteryTray 通过 Unifying、Bolt 或 Lightspeed 接收器，用 Logitech 的 HID++ 协议询问设备。

**如何在 Mac 上不装厂商软件查看 Razer、Logitech、SteelSeries 或 Corsair 设备的电量？**
和 Windows 上一样：BatteryTray 通过 USB 用设备自己的协议与它的接收器通信。

**需要卸载 Synapse、G HUB、iCUE 等厂商软件吗？**
不需要，可以保留。目前已知的唯一例外：ASTRO Command Center 打开时无法读取 ASTRO A50。

## 提醒

**如何为无线鼠标或耳机设置低电量提醒？**
当设备电量低于其提醒值（默认 20%，也可以为每个设备单独设置）时，BatteryTray 会显示提醒窗口，到达严重值时会再次提醒。勿扰模式不会隐藏它，它也不会抢走游戏的焦点。如果没有回应，它会在 10 秒后关闭，并在重复间隔后再次出现；你不在电脑前时，它会一直等你。

**每种提醒可以有自己的提示音吗？**
可以，在“设置 → 提醒 → 提示音”中：低电量提醒、严重低电量提醒和“充电完成”可以分别使用系统提示音、系统的其他声音、你自己的 WAV 或 MP3 文件，或者不发声。

**为什么设备充电时没有百分比？**
有些设备插线时不报告电量，或者报告的数字与实际电量无关。这时 BatteryTray 会显示正在充电，而不是过时或编造的数字。

## 安装和安全

**为什么 Windows 提示这是未知应用？**
exe 没有使用付费的代码签名证书，所以 Microsoft Defender SmartScreen 还不认识它。点击 **更多信息**，再点击 **仍要运行**。源代码是公开的，每个版本都由它构建。

**为什么 macOS 提示无法验证此应用？**
该应用使用项目自己的证书签名，而不是 Apple 付费的 Developer ID，也没有经过公证。所以第一次打开时 macOS 会阻止它：打开 **系统设置 → 隐私与安全性**，点击 **仍要打开**。每个版本的证书都相同，所以你授予的权限在更新后仍然保留。

**安全吗？它会发送什么？**
BatteryTray 只向设备询问电量，从不更改它们的设置。它没有账号，也没有遥测；它唯一会连接的地方是 GitHub，用来检查新版本。更新会用 GitHub 公布的 SHA-256 进行校验，在 Mac 上还会校验应用的签名。

**为什么 Mac 版要求“输入监控”权限？**
macOS 只允许具有此权限的应用打开键盘或带键盘按键的鼠标，即使只是读取电量也一样。BatteryTray 只在连接了这类设备时才请求该权限，并且只读取电量，从不读取你输入的内容。除此之外，Mac 版只请求蓝牙权限：不需要辅助功能，也不需要屏幕录制。

**为什么 Windows 版会要求管理员权限？**
通常不需要，除非有第三方驱动把接收器锁定为只有管理员才能访问：例如某些廉价键盘附带的 G-SPY 驱动（`vkbms.inf`）会占用所有 usage 为 `FF00:0001` 的 HID 集合，包括 Razer 的接收器。这时设备列表上方会有一行提示，点击 **授予权限** 即可正式安装程序，只需确认一次 UAC：程序会通过具有最高权限的计划任务启动（每次登录不再弹出确认），并放在 Program Files 中，没有管理员权限就无法替换它的 exe。

**支持 Apple 芯片（M1–M4）和 Intel 的 Mac 吗？**
支持，macOS 版是同时支持两者的通用应用，需要 macOS 13 Ventura 或更高版本。

**它如何更新？**
自动从本仓库在 GitHub 上的版本更新：启动半分钟后检查一次，之后每 6 小时检查一次，校验下载的文件，并且只在窗口关闭时替换自己。如果在“设置 → 常规 → 更新”中关闭自动更新，程序只会提示你有新版本。

**可以用 winget 或 Scoop 安装吗？**
可以：`winget install qu1ncky.BatteryTray`，或 `scoop bucket add batterytray https://github.com/qu1ncky/BatteryTray` 和 `scoop install batterytray`。如需完整安装，请从 [Releases](https://github.com/qu1ncky/BatteryTray/releases/latest) 下载 `BatteryTray-…-win-x64.zip`，解压后运行 `install.cmd`：程序会安装到 `C:\Program Files\BatteryTray`，并在登录时通过名为“BatteryTray”的任务计划程序任务启动。

**设置和日志在哪里？如何卸载？**
Windows：`%LOCALAPPDATA%\BatteryTray` 中有 `settings.json`、`state.json`（上次已知的电量）和 `log.txt`。卸载时，在“设置 → 常规”中关闭 **开机自动启动**，从托盘菜单退出程序并删除 exe；完整安装后请运行 zip 中的 `uninstall.cmd`。Mac：`~/Library/Application Support/BatteryTray`，登录项为 `~/Library/LaunchAgents/io.github.qu1ncky.BatteryTray.plist`；关闭 **登录时打开**，从应用菜单退出，然后把它移到废纸篓。如果手动编辑把 `settings.json` 改坏了，程序会把它保存为 `settings.json.bak`，并使用默认设置重新开始。

## 遇到问题

**我的设备不在列表中。**
新设备连接后几秒内就会出现，“刷新电量”会立即查找。如果仍然没有，请参阅[设备不在列表中？](devices.md#设备不在列表中)：Windows 和 macOS 各有一条命令可以显示设备接收器的 USB ID 和名称，把输出连同日志发到 [Issues](https://github.com/qu1ncky/BatteryTray/issues) 即可。

**在哪里报告问题？**
在 [Issues](https://github.com/qu1ncky/BatteryTray/issues) 中，并附上日志：Windows 上为 `%LOCALAPPDATA%\BatteryTray\log.txt`，Mac 上为 `~/Library/Application Support/BatteryTray/log.txt`。
