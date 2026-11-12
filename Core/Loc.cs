using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;

namespace BatteryTray.Core
{
    /// <summary>
    /// The app's texts in Russian, English and Chinese. The language follows Windows (Russian for a Russian
    /// Windows, Chinese for a Chinese one, English otherwise) unless chosen in the settings, and switches live:
    /// XAML binds to <see cref="Instance"/>[key] through <c>{l:T key}</c>, code asks <see cref="T"/> each time it
    /// builds a text. Device names and log lines aren't translated.
    /// </summary>
    public sealed class Loc : INotifyPropertyChanged
    {
        public const string Auto = "auto";
        public static readonly string[] Languages = { "ru", "en", "zh" };

        public static Loc Instance { get; } = new();
        public static string Current { get; private set; } = "en";
        public static CultureInfo Culture { get; private set; } = CultureInfo.GetCultureInfo("en-US");

        /// <summary>Raised after the language changed, for texts that code builds.</summary>
        public static event Action? Changed;
        public event PropertyChangedEventHandler? PropertyChanged;

        Loc()
        {
        }

        /// <summary>The language for a setting: "ru", "en" or "zh" as chosen, anything else follows Windows.</summary>
        public static string Resolve(string? setting)
        {
            if (setting != null && Array.IndexOf(Languages, setting) >= 0) return setting;
            return CultureInfo.CurrentUICulture.TwoLetterISOLanguageName switch
            {
                "ru" => "ru",
                "zh" => "zh",
                _ => "en",
            };
        }

        public static void Set(string language)
        {
            if (Array.IndexOf(Languages, language) < 0) language = "en";
            if (language == Current) return;
            Current = language;
            Culture = CultureInfo.GetCultureInfo(language switch { "ru" => "ru-RU", "zh" => "zh-CN", _ => "en-US" });
            Instance.PropertyChanged?.Invoke(Instance, new PropertyChangedEventArgs("Item[]"));
            Changed?.Invoke();
        }

        public string this[string key] => T(key);

        public static string T(string key)
        {
            if (!Strings.TryGetValue(key, out var s)) return key;
            return Current switch { "ru" => s.Ru, "zh" => s.Zh, _ => s.En };
        }

        public static string T(string key, params object?[] args)
        {
            return string.Format(Culture, T(key), args);
        }

        static readonly Dictionary<string, (string Ru, string En, string Zh)> Strings = new()
        {
            // Window
            ["app.title"] = ("Заряд устройств", "Device Battery", "设备电量"),
            ["tab.devices"] = ("Устройства", "Devices", "设备"),
            ["tab.settings"] = ("Настройки", "Settings", "设置"),
            ["refresh"] = ("Опросить устройства сейчас", "Check devices now", "立即刷新设备"),
            ["none.online"] = ("Сейчас ни одно устройство не на связи", "No devices connected right now", "当前没有已连接的设备"),
            ["offline.header"] = ("Не на связи", "Not connected", "未连接"),
            ["updated.at"] = ("Обновлено в {0:HH:mm:ss}", "Updated at {0:HH:mm:ss}", "更新于 {0:HH:mm:ss}"),
            ["polling"] = ("Опрашиваю устройства…", "Checking devices…", "正在检测设备…"),

            // Device cards
            ["state.charging"] = ("Заряжается", "Charging", "充电中"),
            ["state.full"] = ("Заряжено", "Charged", "已充满"),
            ["state.offline"] = ("Нет связи", "Not connected", "未连接"),
            ["last.seen"] = ("последний раз {0}%, {1}", "last: {0}%, {1}", "上次 {0}%，{1}"),
            ["ago.now"] = ("только что", "just now", "刚刚"),
            ["ago.min"] = ("{0} мин назад", "{0} min ago", "{0} 分钟前"),
            ["ago.hours"] = ("{0} ч назад", "{0} h ago", "{0} 小时前"),
            ["ago.date"] = ("{0:dd.MM} в {0:HH:mm}", "{0:MMM d}, {0:HH:mm}", "{0:M月d日} {0:HH:mm}"),

            // Settings
            ["lang.header"] = ("Язык · Language", "Language", "语言 · Language"),
            ["lang.auto"] = ("Как в системе", "System", "跟随系统"),
            ["alerts.header"] = ("Напоминания", "Reminders", "提醒"),
            ["alerts.low"] = ("Напоминать, когда заряд ниже", "Remind me when the battery is below", "电量低于此值时提醒"),
            ["alerts.critical"] = ("Критический уровень", "Critical level", "严重低电量"),
            ["alerts.critical.hint"] = ("На нём напоминание появится снова, даже если вы его отложили",
                "At this level the reminder comes back even if you snoozed it", "达到此电量时，即使已推迟也会再次提醒"),
            ["alerts.repeat"] = ("Повторять напоминание каждые", "Repeat the reminder every", "重复提醒间隔"),
            ["minutes"] = ("{0} мин", "{0} min", "{0} 分钟"),
            ["alerts.sound"] = ("Звуковой сигнал", "Sound", "提示音"),
            ["charged.toggle"] = ("Сообщать, когда зарядка завершена", "Notify when charging is complete", "充电完成时通知"),
            ["charged.hint"] = ("Уведомление Windows. Работает с устройствами, которые сами сообщают, что зарядились",
                "A Windows notification, for devices that report a finished charge", "Windows 通知，适用于会报告充电完成的设备"),
            ["alerts.test"] = ("Показать пример напоминания", "Show a sample reminder", "显示提醒示例"),
            ["devices.header"] = ("Устройства", "Devices", "设备"),
            ["device.name.tip"] = ("Своё название; пусто — как называет себя устройство",
                "Your own name for it; leave empty for the device's own name", "自定义名称；留空则使用设备自带名称"),
            ["device.visible"] = ("Показывать в списке", "Show in the list", "在列表中显示"),
            ["device.alerts"] = ("Напоминать о разряде", "Low battery reminders", "低电量提醒"),
            ["device.thresholds"] = ("Свои уровни для напоминаний", "Custom reminder levels", "自定义提醒电量"),
            ["device.thresholds.tip"] = ("Выключено — действуют уровни из «Напоминаний»; включено — уровни ниже, только для этого устройства",
                "Off: the levels under Reminders apply; on: the levels below, for this device only",
                "关闭时使用“提醒”中的设置；开启后，下方的值只对此设备生效"),
            ["updates.header"] = ("Обновления", "Updates", "更新"),
            ["updates.check"] = ("Проверить сейчас", "Check now", "立即检查"),
            ["updates.auto"] = ("Устанавливать обновления автоматически", "Install updates automatically", "自动安装更新"),
            ["updates.auto.hint"] = ("Новая версия ставится, пока окна программы закрыты, и программа сама перезапускается.",
                "A new version installs while the app's windows are closed, then the app restarts by itself.",
                "新版本会在程序窗口关闭时安装，之后程序自动重启。"),
            ["system.header"] = ("Система", "System", "系统"),
            ["autostart"] = ("Запускать при входе в Windows", "Start with Windows", "开机自动启动"),
            ["open.folder"] = ("Открыть папку с логом и настройками", "Open the log and settings folder", "打开日志和设置文件夹"),
            ["elev.admin"] = ("Программа работает с правами администратора и видит все устройства. При входе в Windows права выдаются без запроса.",
                "Running as administrator, so every device is visible. The rights are granted at sign-in without a prompt.",
                "程序以管理员权限运行，可以看到所有设备。登录 Windows 时会自动获得该权限，无需确认。"),
            ["elev.installed"] = ("Программа запущена без прав администратора. Если у устройства написано «нужны права администратора», откройте программу из меню «Пуск».",
                "Running without administrator rights. If a device says \"needs administrator rights\", open the app from the Start menu.",
                "程序未以管理员权限运行。如果设备显示“需要管理员权限”，请从开始菜单打开本程序。"),
            ["elev.portable"] = ("Это переносная версия без прав администратора. Если у устройства написано «нужны права администратора», установите программу через install.cmd.",
                "This is the portable copy, without administrator rights. If a device says \"needs administrator rights\", install the app with install.cmd.",
                "这是便携版，没有管理员权限。如果设备显示“需要管理员权限”，请通过 install.cmd 安装本程序。"),

            // Battery overlay
            ["overlay.header"] = ("Заряд поверх окон", "Battery overlay", "电量悬浮窗"),
            ["overlay.enabled"] = ("Включить горячую клавишу", "Enable overlay shortcut", "启用悬浮窗快捷键"),
            ["overlay.shortcut"] = ("Горячая клавиша", "Shortcut", "快捷键"),
            ["overlay.record.hint"] = ("Нажмите в поле и введите сочетание. Для обычных клавиш используйте Ctrl или Alt. F-клавиши можно использовать отдельно (кроме F12).",
                "Focus the field and press a shortcut. Use Ctrl or Alt with ordinary keys. Function keys can be used alone, except F12.",
                "点击输入框并按下快捷键。普通按键需搭配 Ctrl 或 Alt。功能键可单独使用（F12 除外）。"),
            ["overlay.reset"] = ("Сбросить", "Reset", "重置"),
            ["overlay.mode"] = ("При нажатии горячей клавиши", "When the shortcut is pressed", "按下快捷键时"),
            ["overlay.timed"] = ("Показать на 5 секунд", "Show for 5 seconds", "显示 5 秒"),
            ["overlay.toggle"] = ("Показать / скрыть повторным нажатием", "Show / hide with the same shortcut", "用同一快捷键显示／隐藏"),
            ["overlay.corner"] = ("Угол экрана", "Screen corner", "屏幕角落"),
            ["overlay.top.left"] = ("Сверху слева", "Top left", "左上"),
            ["overlay.top.right"] = ("Сверху справа", "Top right", "右上"),
            ["overlay.bottom.left"] = ("Снизу слева", "Bottom left", "左下"),
            ["overlay.bottom.right"] = ("Снизу справа", "Bottom right", "右下"),
            ["overlay.names"] = ("Всегда показывать названия", "Always show names", "始终显示名称"),
            ["overlay.preview"] = ("Показать на 5 секунд", "Preview for 5 seconds", "预览 5 秒"),
            ["overlay.compatibility"] = ("На экране активного приложения. Не перехватывает мышь и фокус. Работает на рабочем столе и в играх без рамки; эксклюзивный полный экран не поддерживается.",
                "Appears on the active app's monitor without taking focus or mouse input. Supports desktop and borderless games; exclusive fullscreen is not supported.",
                "显示在当前应用所在的屏幕上，不抢占焦点或鼠标操作。支持桌面和无边框游戏；不支持独占全屏。"),
            ["overlay.ready"] = ("Горячая клавиша включена", "Shortcut is ready", "快捷键已就绪"),
            ["overlay.disabled"] = ("Горячая клавиша выключена. Предпросмотр доступен.", "Shortcut is disabled. Preview is still available.", "快捷键已禁用，仍可预览。"),
            ["overlay.unavailable"] = ("Не удалось зарегистрировать горячую клавишу. Выберите другую или выключите и включите её снова.",
                "The shortcut could not be registered. Choose another, or disable and enable it to retry.",
                "无法注册快捷键。请选择其他组合，或关闭后重新启用以重试。"),
            ["overlay.conflict"] = ("Не удалось зарегистрировать сочетание. Предыдущее сохранено; попробуйте другое.",
                "That shortcut is unavailable. The previous shortcut is kept; try another.",
                "该快捷键不可用，已保留原快捷键；请尝试其他组合。"),
            ["overlay.invalid"] = ("Выберите другое сочетание. Для обычных клавиш нужен Ctrl или Alt. Win, F12, Ctrl+C, Ctrl+V, Ctrl+X и Alt+F4 зарезервированы.",
                "Choose another shortcut. Ordinary keys require Ctrl or Alt. Windows keys, F12, Ctrl+C, Ctrl+V, Ctrl+X, and Alt+F4 are reserved.",
                "请选择其他快捷键。普通按键需搭配 Ctrl 或 Alt。Windows 键、F12、Ctrl+C、Ctrl+V、Ctrl+X 和 Alt+F4 为保留快捷键。"),

            // Updates
            ["upd.checking"] = ("Проверяю, нет ли новой версии…", "Checking for a new version…", "正在检查新版本…"),
            ["upd.uptodate"] = ("Установлена последняя версия {0} · проверено в {1:HH:mm}",
                "You have the latest version {0} · checked at {1:HH:mm}", "已是最新版本 {0} · 检查于 {1:HH:mm}"),
            ["upd.available"] = ("Доступна версия {0}, у вас {1}", "Version {0} is available, you have {1}", "有新版本 {0}，当前版本 {1}"),
            ["upd.downloading"] = ("Загружаю версию {0}: {1}%", "Downloading version {0}: {1}%", "正在下载版本 {0}：{1}%"),
            ["upd.installing"] = ("Устанавливаю версию {0}…", "Installing version {0}…", "正在安装版本 {0}…"),
            ["upd.failed"] = ("Не получилось: {0}", "That didn't work: {0}", "失败：{0}"),
            ["upd.idle"] = ("Версия {0}. Проверка обновлений начнётся через полминуты после запуска.",
                "Version {0}. The update check starts half a minute after launch.", "版本 {0}。启动半分钟后开始检查更新。"),
            ["upd.install"] = ("Обновить до {0}", "Update to {0}", "更新到 {0}"),
            ["upd.banner"] = ("Доступна версия {0}", "Version {0} is available", "有新版本 {0}"),
            ["upd.banner.button"] = ("Обновить", "Update", "更新"),
            ["upd.err.size"] = ("размер файла {0} вместо {1}", "file size {0} instead of {1}", "文件大小为 {0}，应为 {1}"),
            ["upd.err.hash"] = ("контрольная сумма не совпала", "checksum mismatch", "校验和不匹配"),
            ["upd.err.version"] = ("в релизе {0} лежит exe версии {1}", "release {0} contains an exe of version {1}", "版本 {0} 中的程序版本为 {1}"),
            ["upd.err.cancelled"] = ("обновление отменено или не удалось, подробности в логе",
                "the update was cancelled or failed, see the log", "更新已取消或失败，详情见日志"),

            // Tray and notifications
            ["tray.open"] = ("Открыть", "Open", "打开"),
            ["tray.exit"] = ("Выход", "Exit", "退出"),
            ["tray.nodata"] = ("Заряд устройств: нет данных", "Device Battery: no data", "设备电量：暂无数据"),
            ["tray.charging"] = ("{0}: заряжается ⚡", "{0}: charging ⚡", "{0}：充电中 ⚡"),
            ["notice.updated.title"] = ("BatteryTray обновлён", "BatteryTray updated", "BatteryTray 已更新"),
            ["notice.updated.text"] = ("Установлена версия {0}.", "Version {0} is installed.", "已安装版本 {0}。"),
            ["notice.charged.title"] = ("Зарядка завершена", "Charging complete", "充电完成"),
            ["notice.charged.text"] = ("{0} — {1}%. Кабель можно отключать.", "{0} — {1}%. You can unplug it.", "{0} — {1}%。可以拔掉充电线了。"),
            ["notice.available.title"] = ("Доступна версия BatteryTray {0}", "BatteryTray {0} is available", "BatteryTray {0} 可用"),
            ["notice.available.text"] = ("Откройте BatteryTray → Настройки → Обновления, чтобы установить.",
                "Open BatteryTray → Settings → Updates to install it.", "打开 BatteryTray → 设置 → 更新 进行安装。"),

            // Low battery reminder
            ["alert.title"] = ("Пора зарядить", "Time to charge", "该充电了"),
            ["alert.title.many"] = ("Пора зарядить устройства", "Time to charge your devices", "设备该充电了"),
            ["alert.title.critical"] = ("Почти разрядилось!", "Almost empty!", "电量即将耗尽！"),
            ["alert.snooze"] = ("Напомнить через {0} мин", "Remind me in {0} min", "{0} 分钟后提醒"),
            ["alert.snooze.tip"] = ("Напомнить позже", "Remind me later", "稍后提醒"),
            ["alert.mute"] = ("Не напоминать до зарядки", "Don't remind me until it's charged", "充电前不再提醒"),
            ["alert.hint.critical"] = ("Осталось совсем немного — поставьте на зарядку, чтобы не отключилось в самый неподходящий момент.",
                "Very little left: plug it in before it dies at the worst moment.", "电量所剩无几，请及时充电，以免在关键时刻断电。"),
            ["alert.hint"] = ("Окно не закроется само, пока вы не выберете, когда напомнить снова.",
                "This window stays until you choose when to be reminded again.", "在您选择下次提醒时间之前，此窗口不会自动关闭。"),

            // What the providers report
            ["via.24"] = ("2.4 ГГц", "2.4 GHz", "2.4 GHz"),
            ["via.cable"] = ("кабель", "cable", "有线"),
            ["via.adapter"] = ("адаптер", "adapter", "适配器"),
            ["via.receiver"] = ("USB-приёмник", "USB receiver", "USB 接收器"),
            ["name.mouse"] = ("Мышь", "Mouse", "鼠标"),
            ["name.keyboard"] = ("Клавиатура", "Keyboard", "键盘"),
            ["name.xbox"] = ("Геймпад Xbox", "Xbox controller", "Xbox 手柄"),
            ["d.needsAdmin"] = ("нужны права администратора", "needs administrator rights", "需要管理员权限"),
            ["d.error"] = ("ошибка: {0}", "error: {0}", "错误：{0}"),
            ["d.asleep"] = ("спит или выключено", "asleep or off", "休眠或已关闭"),
            ["d.offOrAway"] = ("выключено или далеко", "off or out of range", "已关闭或超出范围"),
            ["d.receiverSilent"] = ("приёмник не отвечает", "the receiver isn't responding", "接收器无响应"),
            ["d.noLink"] = ("не удалось связаться с устройством", "couldn't reach the device", "无法连接设备"),
            ["d.readFailed"] = ("заряд не прочитался", "couldn't read the battery", "无法读取电量"),
            ["d.levelUnknown"] = ("заряд пока неизвестен", "level not known yet", "暂未获取电量"),
            ["d.cableHidden"] = ("процент будет виден без кабеля", "the level shows once unplugged", "拔掉线后显示电量"),
            ["d.chargerHidden"] = ("процент будет виден, когда мышь снимут с зарядки", "the level shows once it's off the charger",
                "离开充电器后显示电量"),
            ["d.wasBefore"] = ("до зарядки было {0}%", "{0}% before charging", "充电前为 {0}%"),
            ["d.almostFull"] = ("почти заряжено", "almost full", "即将充满"),
            ["d.unplugOk"] = ("можно отключать кабель", "you can unplug it", "可以拔掉充电线了"),
            ["d.settling"] = ("уточняется после зарядки", "settling after charging", "充电后校准中"),
            ["d.estimate"] = ("примерно, по напряжению", "approximate, from the voltage", "按电压估算"),
            ["d.busy"] = ("нет доступа: устройство занято другой программой", "no access: another program is using it", "无法访问：设备被其他程序占用"),
            ["d.noController"] = ("геймпад не подключён к адаптеру", "no controller on the adapter", "适配器未连接手柄"),
            ["d.batteryError"] = ("геймпад сообщает об ошибке батареи", "the controller reports a battery error", "手柄报告电池故障"),
            ["d.waitFullMode"] = ("заряд появится, когда геймпадом начнёт пользоваться игра или Steam",
                "the level shows once a game or Steam uses the controller", "游戏或 Steam 使用手柄后才会显示电量"),
            ["d.fourLevels"] = ("примерно: геймпад сообщает только 4 уровня", "approximate: the controller reports only 4 levels",
                "约值：手柄仅报告 4 个等级"),
            ["d.tenSteps"] = ("примерно: мышь сообщает заряд шагами по 10%", "approximate: the mouse reports the level in 10% steps",
                "约值：鼠标以 10% 为单位报告电量"),
            ["d.unknownModel"] = ("заряд этой модели программа пока не читает — её поможет добавить лог",
                "this model's level can't be read yet; its log can help add it", "暂时无法读取此型号的电量，日志可帮助添加支持"),
        };
    }
}
